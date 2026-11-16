use crate::{
    engine::{Connector, Engine, NativeConnector},
    logs::Source,
};
use arc_swap::ArcSwap;
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};
use tervix_application::docker::*;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

const QUEUE: usize = 32;
const DEADLINE: Duration = Duration::from_secs(5);
const MAX_SOURCES: usize = 32;

type Sender<T> = oneshot::Sender<Result<T, DockerError>>;

enum Request {
    Exec(ExecOptions, Box<crate::exec::Worker>),
    Contexts(Sender<Arc<[Context]>>),
    Connect(Endpoint),
    Disconnect,
    Inspect(ContainerId, Sender<Details>),
    Stats(ContainerId, Sender<Stats>),
    Act(ContainerId, Action, Sender<()>),
    Logs(ContainerId, Sender<Box<dyn LogReader>>),
}

struct Envelope {
    epoch: u64,
    incarnation: u64,
    request: Request,
}

struct Ticket<T>(oneshot::Receiver<Result<T, DockerError>>);

impl<T: Send> Reply<T> for Ticket<T> {
    fn poll(&mut self) -> Option<Result<T, DockerError>> {
        match self.0.try_recv() {
            Ok(value) => Some(value),
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(oneshot::error::TryRecvError::Closed) => Some(Err(DockerError::Cancelled)),
        }
    }
}

/// One background Tokio runtime and shared source registry for the entire application.
/// Construction starts a worker but performs no Engine I/O. Drop requests cancellation
/// without joining or blocking the GUI. Inject as `Arc<dyn DockerService>`.
pub struct NativeDocker {
    exec_generation: Arc<AtomicU64>,
    commands: mpsc::Sender<Envelope>,
    latest: Arc<ArcSwap<Snapshot>>,
    visible: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    changed: Arc<Notify>,
    stop: CancellationToken,
}

impl NativeDocker {
    /// Start a disconnected service. Fails only if the OS worker cannot be created.
    /// Tokio initialization failure closes the queue; subsequent requests return Cancelled.
    pub fn new() -> std::io::Result<Self> {
        let (commands, incoming) = mpsc::channel(QUEUE);
        let latest = Arc::new(ArcSwap::from_pointee(Snapshot {
            incarnation: 0,
            revision: 0,
            endpoint: None,
            state: ConnectionState::Disconnected,
            capabilities: Capabilities::default(),
            observed_at: None,
            containers: Arc::from([]),
        }));
        let visible = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(0));
        let changed = Arc::new(Notify::new());
        let stop = CancellationToken::new();
        let exec_generation = Arc::new(AtomicU64::new(0));
        let actor = Actor {
            exec_tasks: Vec::new(),
            exec_generation: exec_generation.clone(),
            incoming,
            latest: latest.clone(),
            visible: visible.clone(),
            epoch: epoch.clone(),
            changed: changed.clone(),
            generation: 0,
            endpoint: None,
            engine: None,
            sources: HashMap::new(),
            stats_cache: HashMap::new(),
            context_job: None,
            source_cancel: stop.child_token(),
            failures: 0,
            due: tokio::time::Instant::now(),
            connector: Arc::new(NativeConnector),
        };
        let cancel = stop.clone();
        std::thread::Builder::new()
            .name("tervix-docker".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => {}
                        () = actor.run() => {}
                    }
                    // Give child streams an opportunity to publish their cancellation state.
                    cancel.cancel();
                    tokio::task::yield_now().await;
                });
                runtime.shutdown_timeout(Duration::from_millis(100));
            })?;
        Ok(Self {
            exec_generation,
            commands,
            latest,
            visible,
            epoch,
            changed,
            stop,
        })
    }

    fn queue(
        &self,
        request: Request,
        selection: bool,
        incarnation: Option<u64>,
    ) -> Result<(), DockerError> {
        let permit = self.commands.try_reserve().map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => DockerError::Busy,
            mpsc::error::TrySendError::Closed(_) => DockerError::Cancelled,
        })?;
        let epoch = if selection {
            self.epoch.fetch_add(1, Ordering::AcqRel) + 1
        } else {
            self.epoch.load(Ordering::Acquire)
        };
        permit.send(Envelope {
            epoch,
            incarnation: incarnation.unwrap_or_else(|| self.exec_generation()),
            request,
        });
        if selection {
            self.changed.notify_waiters();
        }
        Ok(())
    }

    fn reply<T: Send + 'static>(
        &self,
        incarnation: Option<u64>,
        request: impl FnOnce(Sender<T>) -> Request,
    ) -> Result<Box<dyn Reply<T>>, DockerError> {
        let (tx, rx) = oneshot::channel();
        self.queue(request(tx), false, incarnation)?;
        Ok(Box::new(Ticket(rx)))
    }
}

impl Drop for NativeDocker {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl DockerService for NativeDocker {
    fn exec_generation(&self) -> u64 {
        self.exec_generation.load(Ordering::Acquire)
    }
    fn exec(
        &self,
        options: ExecOptions,
        size: tervix_application::terminal::GridSize,
    ) -> Result<Box<dyn tervix_application::terminal::TerminalSession>, DockerError> {
        if options.generation != self.exec_generation()
            || options.program.is_empty()
            || options.program.len() > 1024
            || options.program.contains('\0')
            || options.args.len() > 64
            || options
                .args
                .iter()
                .any(|a| a.len() > 8192 || a.contains('\0'))
        {
            return Err(DockerError::InvalidInput);
        }
        let (session, worker) = crate::exec::session(size);
        let incarnation = options.generation;
        self.queue(
            Request::Exec(options, Box::new(worker)),
            false,
            Some(incarnation),
        )?;
        Ok(session)
    }
    fn contexts(&self) -> Result<Box<dyn Reply<Arc<[Context]>>>, DockerError> {
        self.reply(None, Request::Contexts)
    }
    fn connect(&self, endpoint: Endpoint) -> Result<(), DockerError> {
        crate::engine::validate(&endpoint)?;
        self.queue(Request::Connect(endpoint), true, None)
    }
    fn disconnect(&self) -> Result<(), DockerError> {
        self.queue(Request::Disconnect, true, None)
    }
    fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::Release);
    }
    fn snapshot(&self) -> Arc<Snapshot> {
        self.latest.load_full()
    }
    fn inspect(
        &self,
        incarnation: u64,
        id: ContainerId,
    ) -> Result<Box<dyn Reply<Details>>, DockerError> {
        self.reply(Some(incarnation), |tx| Request::Inspect(id, tx))
    }
    fn stats(
        &self,
        incarnation: u64,
        id: ContainerId,
    ) -> Result<Box<dyn Reply<Stats>>, DockerError> {
        self.reply(Some(incarnation), |tx| Request::Stats(id, tx))
    }
    fn act(
        &self,
        incarnation: u64,
        id: ContainerId,
        action: Action,
    ) -> Result<Box<dyn Reply<()>>, DockerError> {
        if matches!(&action, Action::Remove { confirmed } if confirmed != &id) {
            return Err(DockerError::InvalidInput);
        }
        self.reply(Some(incarnation), |tx| Request::Act(id, action, tx))
    }
    fn logs(
        &self,
        incarnation: u64,
        id: ContainerId,
    ) -> Result<Box<dyn Reply<Box<dyn LogReader>>>, DockerError> {
        self.reply(Some(incarnation), |tx| Request::Logs(id, tx))
    }
}

struct Actor {
    exec_tasks: Vec<tokio::task::JoinHandle<()>>,
    exec_generation: Arc<AtomicU64>,
    context_job: Option<tokio::task::JoinHandle<Result<Arc<[Context]>, DockerError>>>,
    stats_cache: HashMap<ContainerId, Stats>,
    connector: Arc<dyn Connector>,
    incoming: mpsc::Receiver<Envelope>,
    latest: Arc<ArcSwap<Snapshot>>,
    visible: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    changed: Arc<Notify>,
    generation: u64,
    endpoint: Option<Endpoint>,
    engine: Option<Arc<dyn Engine>>,
    sources: HashMap<ContainerId, Source>,
    source_cancel: CancellationToken,
    failures: u32,
    due: tokio::time::Instant,
}

impl Drop for Actor {
    fn drop(&mut self) {
        self.source_cancel.cancel();
    }
}

impl Actor {
    async fn run(mut self) {
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                command = self.incoming.recv() => match command {
                    Some(command) => self.handle(command).await,
                    None => break,
                },
                _ = tick.tick() => {
                    self.exec_tasks.retain(|task| !task.is_finished());
                    self.sources.retain(|_, source| !source.task.is_finished());
                    if self.endpoint.is_some() && tokio::time::Instant::now() >= self.due {
                        self.refresh().await;
                    }
                }
            }
        }
        self.source_cancel.cancel();
    }

    fn publish(&self, state: ConnectionState, rows: Option<Arc<[Container]>>) {
        let old = self.latest.load_full();
        let connected = state == ConnectionState::Connected;
        self.latest.store(Arc::new(Snapshot {
            incarnation: self.exec_generation.load(Ordering::Acquire),
            revision: old.revision.saturating_add(1),
            endpoint: self.endpoint.clone(),
            state,
            capabilities: Capabilities {
                containers: connected,
                logs: connected,
                stats: connected,
                lifecycle: connected,
            },
            observed_at: if self.endpoint.is_none() {
                None
            } else if rows.is_some() {
                Some(SystemTime::now())
            } else {
                old.observed_at
            },
            containers: rows.unwrap_or_else(|| old.containers.clone()),
        }));
    }

    fn clear(&mut self) {
        self.exec_generation.fetch_add(1, Ordering::AcqRel);
        self.source_cancel.cancel();
        self.source_cancel = CancellationToken::new();
        self.sources.clear();
        self.stats_cache.clear();
        self.engine = None;
        self.endpoint = None;
        self.failures = 0;
        self.publish(ConnectionState::Disconnected, Some(Arc::from([])));
    }

    async fn handle(&mut self, envelope: Envelope) {
        // Endpoint generation prevents queued actions from hitting a newly selected daemon.
        if envelope.epoch != self.epoch.load(Ordering::Acquire) {
            return;
        }
        // A retry may replace the connection without changing the selected endpoint.
        // Do not dispatch work queued against the failed connection on its replacement.
        if !matches!(
            &envelope.request,
            Request::Connect(_) | Request::Disconnect | Request::Contexts(_)
        ) && envelope.incarnation != self.exec_generation.load(Ordering::Acquire)
        {
            return;
        }
        match envelope.request {
            Request::Exec(options, worker) => {
                if envelope.epoch != self.generation
                    || options.generation != self.exec_generation.load(Ordering::Acquire)
                    || self.endpoint.as_ref() != Some(&options.endpoint)
                    || self.engine.is_none()
                {
                    return;
                }
                self.exec_tasks.retain(|task| !task.is_finished());
                if self.exec_tasks.len() < 32
                    && let Some(client) = self.engine.as_ref().and_then(|e| e.exec_client())
                {
                    self.exec_tasks.push(tokio::spawn(worker.run(
                        client,
                        options,
                        self.source_cancel.child_token(),
                    )));
                }
            }
            Request::Contexts(reply) => {
                if reply.is_closed() {
                    return;
                }
                // At most one filesystem task, including after a request timeout.
                let task = self
                    .context_job
                    .get_or_insert_with(|| tokio::task::spawn_blocking(crate::contexts::discover));
                let result = bounded(
                    async { task.await.map_err(|_| DockerError::Unavailable)? },
                    &self.epoch,
                    envelope.epoch,
                    &self.changed,
                    DEADLINE,
                )
                .await;
                if !matches!(result, Err(DockerError::Timeout | DockerError::Cancelled)) {
                    self.context_job = None;
                }
                let _ = reply.send(result);
            }
            Request::Connect(endpoint) => {
                self.generation = envelope.epoch;
                self.clear();
                self.endpoint = Some(endpoint);
                self.publish(ConnectionState::Connecting, None);
                self.refresh().await;
            }
            Request::Disconnect => {
                self.generation = envelope.epoch;
                self.clear();
            }
            Request::Inspect(id, reply) => {
                if envelope.epoch != self.generation || reply.is_closed() {
                    return;
                }
                let result = match &self.engine {
                    Some(engine) => self.bounded(engine.inspect(id)).await,
                    None => Err(DockerError::Disconnected),
                };
                let _ = reply.send(result);
            }
            Request::Stats(id, reply) => {
                if envelope.epoch != self.generation || reply.is_closed() {
                    return;
                }
                self.stats_cache.retain(|_, sample| {
                    sample
                        .observed_at
                        .elapsed()
                        .is_ok_and(|age| age < Duration::from_secs(1))
                });
                if let Some(sample) = self.stats_cache.get(&id) {
                    let _ = reply.send(Ok(sample.clone()));
                    return;
                }
                let result = match &self.engine {
                    Some(engine) => self.bounded(engine.stats(id.clone())).await,
                    None => Err(DockerError::Disconnected),
                };
                if let Ok(sample) = &result
                    && self.stats_cache.len() < 256
                {
                    self.stats_cache.insert(id, sample.clone());
                }
                let _ = reply.send(result);
            }
            Request::Act(id, action, reply) => {
                // A closed completion mailbox is cancellation before dispatch, not user intent.
                if envelope.epoch != self.generation || reply.is_closed() {
                    return;
                }
                let result = match &self.engine {
                    Some(engine) => self.bounded(engine.act(id, action)).await,
                    None => Err(DockerError::Disconnected),
                };
                let _ = reply.send(result);
                self.due = tokio::time::Instant::now();
            }
            Request::Logs(id, reply) => {
                if envelope.epoch != self.generation || reply.is_closed() {
                    return;
                }
                let result = self.subscribe(id);
                let _ = reply.send(result);
            }
        }
    }

    fn subscribe(&mut self, id: ContainerId) -> Result<Box<dyn LogReader>, DockerError> {
        let engine = self.engine.clone().ok_or(DockerError::Disconnected)?;
        self.sources.retain(|_, source| !source.task.is_finished());
        if let Some(source) = self.sources.get(&id) {
            return Ok(source.reader());
        }
        if self.sources.len() >= MAX_SOURCES {
            return Err(DockerError::Busy);
        }
        let source = Source::start(engine, id.clone(), self.source_cancel.child_token());
        let reader = source.reader();
        self.sources.insert(id, source);
        Ok(reader)
    }

    async fn bounded<T>(
        &self,
        operation: impl Future<Output = Result<T, DockerError>>,
    ) -> Result<T, DockerError> {
        bounded(
            operation,
            &self.epoch,
            self.generation,
            &self.changed,
            DEADLINE,
        )
        .await
    }

    async fn refresh(&mut self) {
        if self.generation != self.epoch.load(Ordering::Acquire) {
            return;
        }
        let result = async {
            if self.engine.is_none() {
                let endpoint = self.endpoint.clone().ok_or(DockerError::Disconnected)?;
                self.engine = Some(self.bounded(self.connector.connect(endpoint)).await?);
            }
            let engine = self.engine.as_ref().ok_or(DockerError::Disconnected)?;
            self.bounded(engine.list()).await
        }
        .await;
        match result {
            Ok(rows) => {
                self.failures = 0;
                self.publish(ConnectionState::Connected, Some(rows));
                self.due = tokio::time::Instant::now()
                    + Duration::from_secs(if self.visible.load(Ordering::Acquire) {
                        1
                    } else {
                        5
                    });
            }
            Err(DockerError::Cancelled) => {}
            Err(error) => {
                self.exec_generation.fetch_add(1, Ordering::AcqRel);
                self.engine = None;
                self.stats_cache.clear();
                self.source_cancel.cancel();
                self.source_cancel = CancellationToken::new();
                self.sources.clear();
                self.failures = self.failures.saturating_add(1);
                self.publish(ConnectionState::Retrying(error), None);
                self.due = tokio::time::Instant::now() + backoff(self.failures);
            }
        }
    }
}

async fn bounded<T>(
    operation: impl Future<Output = Result<T, DockerError>>,
    epoch: &AtomicU64,
    generation: u64,
    changed: &Notify,
    deadline: Duration,
) -> Result<T, DockerError> {
    let superseded = async {
        loop {
            let notified = changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if epoch.load(Ordering::Acquire) != generation {
                return;
            }
            notified.await;
            // Notification can arrive after the actor already began this very generation.
            // Only a different generation cancels; do not reset the operation's deadline.
        }
    };
    tokio::select! {
        biased;
        () = superseded => Err(DockerError::Cancelled),
        result = tokio::time::timeout(deadline, operation) => result.unwrap_or(Err(DockerError::Timeout)),
    }
}

fn backoff(failures: u32) -> Duration {
    let base = (1u64 << failures.min(5)).min(30) * 1000;
    // Wall-clock jitter avoids synchronized reconnect across app instances; never for IDs/security.
    let jitter = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|time| u64::from(time.subsec_nanos()) % 251)
        .unwrap_or(0);
    Duration::from_millis(base + jitter)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Test fixture/assertion failures must fail the test.
mod tests;
