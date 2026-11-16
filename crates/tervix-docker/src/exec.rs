//! Engine-native duplex TTY. No Docker CLI and no remote process-kill fiction.
use arc_swap::ArcSwap;
use bollard::{
    Docker,
    exec::{StartExecOptions, StartExecResults},
    models::ExecConfig,
    query_parameters::ResizeExecOptions,
};
use futures_util::{FutureExt, StreamExt};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc as std_mpsc,
};
use std::time::Duration;
use tervix_application::{
    docker::*,
    terminal::{GridSize, Lifecycle, Snapshot, TerminalError, TerminalProvider, TerminalSession},
};
use tokio::{io::AsyncWriteExt, sync::mpsc};
use tokio_util::sync::CancellationToken;

/// Routes local/WSL/SSH profiles to the existing provider and exec to the shared Engine.
pub struct EngineTerminal {
    /// Local terminal adapter.
    pub local: Arc<dyn TerminalProvider>,
    /// Shared Docker service.
    pub docker: Arc<dyn DockerService>,
}
impl TerminalProvider for EngineTerminal {
    fn start(
        &self,
        profile: tervix_application::terminal::Profile,
        size: GridSize,
    ) -> Result<Box<dyn TerminalSession>, TerminalError> {
        match profile {
            tervix_application::terminal::Profile::DockerExec(options) => self
                .docker
                .exec(options, size)
                .map_err(|e| TerminalError::Startup(e.to_string())),
            other => self.local.start(other, size),
        }
    }
}

struct Session {
    id: tervix_application::SessionId,
    input: mpsc::Sender<Vec<u8>>,
    resize: mpsc::Sender<GridSize>,
    latest: Arc<ArcSwap<Snapshot>>,
    seen: Arc<Snapshot>,
    wake: Arc<AtomicBool>,
    cancel: CancellationToken,
}
impl TerminalSession for Session {
    fn id(&self) -> tervix_application::SessionId {
        self.id
    }
    fn wakeup(&self) -> Arc<AtomicBool> {
        self.wake.clone()
    }
    fn poll(&mut self) -> Option<Snapshot> {
        let next = self.latest.load_full();
        if Arc::ptr_eq(&next, &self.seen) {
            return None;
        }
        self.seen = next.clone();
        Some((*next).clone())
    }
    fn input(&self, bytes: Vec<u8>) -> Result<(), TerminalError> {
        if self.cancel.is_cancelled() {
            return Err(TerminalError::Ended);
        }
        if bytes.len() > 65536 {
            return Err(TerminalError::Busy);
        }
        self.input.try_send(bytes).map_err(queue_error)
    }
    fn resize(&self, size: GridSize) -> Result<(), TerminalError> {
        if self.cancel.is_cancelled() {
            return Err(TerminalError::Ended);
        }
        self.resize
            .try_send(GridSize::new(size.columns, size.rows))
            .map_err(queue_error)
    }
    fn close(&self) {
        self.latest.rcu(|old| {
            if matches!(old.lifecycle, Lifecycle::Starting | Lifecycle::Running) {
                let mut frame = (**old).clone();
                frame.lifecycle = Lifecycle::Closing;
                Arc::new(frame)
            } else {
                old.clone()
            }
        });
        self.wake.store(true, Ordering::Release);
        self.cancel.cancel();
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
fn queue_error<T>(e: mpsc::error::TrySendError<T>) -> TerminalError {
    match e {
        mpsc::error::TrySendError::Full(_) => TerminalError::Busy,
        mpsc::error::TrySendError::Closed(_) => TerminalError::Ended,
    }
}

pub(crate) struct Worker {
    input: Option<mpsc::Receiver<Vec<u8>>>,
    resize: mpsc::Receiver<GridSize>,
    latest: Arc<ArcSwap<Snapshot>>,
    wake: Arc<AtomicBool>,
    cancel: CancellationToken,
    emulator: tervix_terminal::Emulator,
    replies: std_mpsc::Receiver<Vec<u8>>,
    size: GridSize,
}
impl Drop for Worker {
    fn drop(&mut self) {
        if matches!(
            self.latest.load().lifecycle,
            Lifecycle::Starting | Lifecycle::Running | Lifecycle::Closing
        ) {
            self.publish(Lifecycle::Failed("Exec request cancelled or connection changed; detached, remote process outcome unknown".into()));
        }
    }
}
pub(crate) fn session(size: GridSize) -> (Box<dyn TerminalSession>, Worker) {
    let size = GridSize::new(size.columns, size.rows);
    let (input, inputs) = mpsc::channel(32);
    let (resize, resizes) = mpsc::channel(4);
    let latest = Arc::new(ArcSwap::from_pointee(Snapshot::starting(size)));
    let seen = latest.load_full();
    let wake = Arc::new(AtomicBool::new(true));
    let cancel = CancellationToken::new();
    let (reply, replies) = std_mpsc::sync_channel(32);
    (
        Box::new(Session {
            id: tervix_application::SessionId::from_uuid(uuid::Uuid::new_v4()),
            input,
            resize,
            latest: latest.clone(),
            seen,
            wake: wake.clone(),
            cancel: cancel.clone(),
        }),
        Worker {
            input: Some(inputs),
            resize: resizes,
            latest,
            wake,
            cancel,
            emulator: tervix_terminal::Emulator::new(size, reply),
            replies,
            size,
        },
    )
}
impl Worker {
    fn publish(&self, state: Lifecycle) {
        self.latest.store(Arc::new(self.emulator.snapshot(state)));
        self.wake.store(true, Ordering::Release);
    }
    pub(crate) async fn run(
        mut self,
        docker: Docker,
        options: ExecOptions,
        connection: CancellationToken,
    ) {
        let cancel = self.cancel.clone();
        let state = tokio::select! {
            biased;
            () = cancel.cancelled() => Lifecycle::Closed,
            () = connection.cancelled() => Lifecycle::Failed("Engine connection changed; exec detached, remote process outcome unknown".into()),
            result = self.attached(&docker, options) => match result {
                Ok(code) => Lifecycle::Exited(code),
                Err(error) => Lifecycle::Failed(error),
            },
        };
        // Dropping attach halves detaches. Engine has no exec kill/delete API.
        self.publish(state);
    }
    async fn attached(&mut self, docker: &Docker, options: ExecOptions) -> Result<u32, String> {
        let mut argv = vec![options.program];
        argv.extend(options.args);
        let created = deadline(docker.create_exec(
            options.container.as_str(),
            ExecConfig {
                attach_stdin: Some(true),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                tty: Some(true),
                privileged: Some(false),
                cmd: Some(argv),
                env: Some(vec!["TERM=xterm-256color".into()]),
                ..Default::default()
            },
        ))
        .await?;
        // Validate daemon-controlled ID before constructing further URL paths.
        if created.id.len() != 64 || !created.id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid Engine exec identity".into());
        }
        let StartExecResults::Attached { mut output, input } = deadline(docker.start_exec(
            &created.id,
            Some(StartExecOptions {
                detach: false,
                tty: true,
                output_capacity: Some(8192),
            }),
        ))
        .await?
        else {
            return Err("Engine did not attach exec TTY".into());
        };
        self.resize_remote(docker, &created.id, self.size).await?;
        self.publish(Lifecycle::Running);
        let mut tick = tokio::time::interval(Duration::from_millis(40));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut dirty = false;
        let inputs = self.input.take().ok_or("Exec input already attached")?;
        let (reply_tx, reply_rx) = mpsc::channel(32);
        // Keep the write future alive across output/tick polls: cancelling write_all
        // and retrying it can duplicate a partially written paste. Both directions
        // must progress even when the remote TTY stops reading stdin temporarily.
        {
            let writer = write_input(input, inputs, reply_rx);
            tokio::pin!(writer);
            loop {
                tokio::select! {
                    result = &mut writer => return result,
                    chunk = std::panic::AssertUnwindSafe(output.next()).catch_unwind() => match chunk {
                        Ok(Some(Ok(chunk))) => {
                            self.emulator.feed(chunk.as_ref())?;
                            dirty = true;
                            // Buffered output must not starve other sessions or cancellation.
                            tokio::task::yield_now().await;
                        }
                        Ok(Some(Err(e))) => return Err(exec_error(e)),
                        Ok(None) => break,
                        Err(_) => return Err("Incompatible Engine exec stream; detached".into()),
                    },
                    _ = tick.tick() => {
                        let mut next = None;
                        while let Ok(size) = self.resize.try_recv() { next = Some(size); }
                        if let Some(size) = next && size != self.size {
                            self.resize_remote(docker, &created.id, size).await?;
                            self.size = size;
                            self.emulator.resize(size);
                            dirty = true;
                        }
                        let replies: Vec<_> = self.replies.try_iter().take(32).collect();
                        for bytes in replies {
                            reply_tx.try_send(bytes).map_err(|_| "Exec protocol reply queue full; detached".to_string())?;
                        }
                        if dirty { self.publish(Lifecycle::Running); dirty = false; }
                    }
                }
            }
        }
        // EOF alone is not an exit code. Allow bounded propagation of Engine state.
        let eof_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        for _ in 0..20 {
            let state =
                tokio::time::timeout_at(eof_deadline, deadline(docker.inspect_exec(&created.id)))
                    .await
                    .map_err(|_| {
                        "Exec output EOF; exit inspection timed out, detached, outcome unknown"
                            .to_string()
                    })??;
            if state.running == Some(false) {
                return state
                    .exit_code
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| "Exec ended without a valid exit code".into());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("Exec output EOF while Engine still reports running; detached, outcome unknown".into())
    }
    async fn resize_remote(
        &mut self,
        docker: &Docker,
        id: &str,
        size: GridSize,
    ) -> Result<(), String> {
        deadline(docker.resize_exec(
            id,
            ResizeExecOptions {
                h: i32::from(size.rows),
                w: i32::from(size.columns),
            },
        ))
        .await
    }
}

async fn write_input(
    mut input: impl tokio::io::AsyncWrite + Unpin,
    mut inputs: mpsc::Receiver<Vec<u8>>,
    mut replies: mpsc::Receiver<Vec<u8>>,
) -> Result<u32, String> {
    loop {
        let bytes = tokio::select! {
            bytes = inputs.recv() => bytes.ok_or("Exec input closed; detached")?,
            Some(bytes) = replies.recv() => bytes,
        };
        deadline(input.write_all(&bytes)).await?;
        deadline(input.flush()).await?;
    }
}
fn exec_error(error: bollard::errors::Error) -> String {
    format!(
        "{} · exec shell may be missing or unavailable; choose /bin/sh or /bin/bash explicitly. Detached; remote outcome may be unknown",
        crate::engine::map_error(error)
    )
}
async fn deadline<T, E>(
    operation: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, String>
where
    E: Into<bollard::errors::Error>,
{
    tokio::time::timeout(Duration::from_secs(5), operation)
        .await
        .map_err(|_| "Exec I/O timed out; detached, remote outcome unknown".to_string())?
        .map_err(|e| exec_error(e.into()))
}
