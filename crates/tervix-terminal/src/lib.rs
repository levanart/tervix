//! Windows ConPTY adapter. Blocking OS handles and VT state live only on workers.
mod emulator;
mod ingestion;
mod profiles;
mod wsl;
pub use wsl::{NativeWslDiscovery, discover_wsl};

/// Shared bounded VT state for non-ConPTY byte transports (e.g. Docker exec).
/// Uses the same parser, OSC policy, scrollback and snapshot rendering as local shells.
pub struct Emulator {
    term: alacritty_terminal::Term<emulator::Listener>,
    parser: alacritty_terminal::vte::ansi::Processor,
    guard: ingestion::Guard,
    size: GridSize,
}
impl Emulator {
    /// Create VT state with a bounded protocol-reply sender.
    pub fn new(size: GridSize, replies: SyncSender<Vec<u8>>) -> Self {
        Self {
            term: emulator::new(size, replies),
            parser: Default::default(),
            guard: Default::default(),
            size,
        }
    }
    /// Parse output; rejects input exceeding the combining-character budget.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.guard.check(bytes)?;
        self.parser.advance(&mut self.term, bytes);
        Ok(())
    }
    /// Resize without resetting scrollback.
    pub fn resize(&mut self, size: GridSize) {
        self.size = size;
        self.term.resize(emulator::DimensionsOf(size));
    }
    /// Immutable presentation frame.
    pub fn snapshot(&self, lifecycle: Lifecycle) -> Snapshot {
        emulator::snapshot(&self.term, self.size, lifecycle)
    }
}

use portable_pty::{PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    time::Duration,
};
use tervix_application::terminal::{
    GridSize, Lifecycle, Profile, Snapshot, TerminalError, TerminalProvider, TerminalSession,
};

/// Native Windows provider. Construction performs no I/O.
pub struct NativeTerminal;

struct Session {
    id: tervix_application::SessionId,
    input: SyncSender<Vec<u8>>,
    resize: SyncSender<GridSize>,
    snapshots: Receiver<Snapshot>,
    cancelled: Arc<AtomicBool>,
    wakeup: Arc<AtomicBool>,
}
impl TerminalSession for Session {
    fn id(&self) -> tervix_application::SessionId {
        self.id
    }
    fn wakeup(&self) -> Arc<AtomicBool> {
        self.wakeup.clone()
    }
    fn poll(&mut self) -> Option<Snapshot> {
        self.snapshots.try_iter().last()
    }
    fn input(&self, bytes: Vec<u8>) -> Result<(), TerminalError> {
        if bytes.len() > 65536 {
            return Err(TerminalError::Busy);
        }
        self.input.try_send(bytes).map_err(queue_error)
    }
    fn resize(&self, size: GridSize) -> Result<(), TerminalError> {
        let size = GridSize::new(size.columns, size.rows);
        self.resize.try_send(size).map_err(queue_error)
    }
    fn close(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}
fn queue_error<T>(error: TrySendError<T>) -> TerminalError {
    match error {
        TrySendError::Full(_) => TerminalError::Busy,
        TrySendError::Disconnected(_) => TerminalError::Ended,
    }
}

impl TerminalProvider for NativeTerminal {
    fn start(
        &self,
        profile: Profile,
        size: GridSize,
    ) -> Result<Box<dyn TerminalSession>, TerminalError> {
        let size = GridSize::new(size.columns, size.rows);
        let (input, inputs) = mpsc::sync_channel(64);
        let (resize, resizes) = mpsc::sync_channel(4);
        let (frames, snapshots) = mpsc::sync_channel(2);
        let cancelled = Arc::new(AtomicBool::new(false));
        let wakeup = Arc::new(AtomicBool::new(true));
        let notify = wakeup.clone();
        let cancel = cancelled.clone();
        let replies = input.clone();
        std::thread::Builder::new()
            .name("terminal-owner".into())
            .spawn(move || {
                if let Err(error) = run(
                    profile,
                    size,
                    inputs,
                    resizes,
                    (&frames, &notify),
                    &cancel,
                    replies,
                ) {
                    let mut frame = Snapshot::starting(size);
                    frame.lifecycle = Lifecycle::Failed(error);
                    let _ = frames.send(frame);
                    notify.store(true, Ordering::Release);
                }
            })
            .map_err(|e| TerminalError::Startup(e.to_string()))?;
        Ok(Box::new(Session {
            id: tervix_application::SessionId::from_uuid(uuid::Uuid::new_v4()),
            input,
            resize,
            snapshots,
            cancelled,
            wakeup,
        }))
    }
}

fn pty_size(size: GridSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.columns,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn run(
    profile: Profile,
    mut size: GridSize,
    inputs: Receiver<Vec<u8>>,
    resizes: Receiver<GridSize>,
    (frames, notify): (&SyncSender<Snapshot>, &AtomicBool),
    cancel: &Arc<AtomicBool>,
    replies: SyncSender<Vec<u8>>,
) -> Result<(), String> {
    let command = profiles::command(profile)?;
    if cancel.load(Ordering::Acquire) {
        let mut frame = Snapshot::starting(size);
        frame.lifecycle = Lifecycle::Closed;
        let _ = frames.send(frame);
        notify.store(true, Ordering::Release);
        return Ok(());
    }
    let pair = native_pty_system()
        .openpty(pty_size(size))
        .map_err(|e| e.to_string())?;
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|e| e.to_string())?;
    drop(pair.slave);
    let (output, bytes) = mpsc::sync_channel::<Result<Vec<u8>, String>>(32);
    let read_thread = std::thread::Builder::new()
        .name("terminal-reader".into())
        .spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                let chunk = match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => Ok(buffer[..n].to_vec()),
                    Err(e) => Err(e.to_string()),
                };
                let failed = chunk.is_err();
                if output.send(chunk).is_err() || failed {
                    break;
                }
            }
        });
    let stop_writer = cancel.clone();
    let write_thread = std::thread::Builder::new()
        .name("terminal-writer".into())
        .spawn(move || {
            while !stop_writer.load(Ordering::Acquire) {
                match inputs.recv_timeout(Duration::from_millis(20)) {
                    Ok(input) => {
                        if writer
                            .write_all(&input)
                            .and_then(|_| writer.flush())
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => break,
                }
            }
        });
    if read_thread.is_err() || write_thread.is_err() {
        let _ = child.kill();
        cancel.store(true, Ordering::Release);
        return Err("Could not create terminal I/O workers".into());
    }
    let mut term = emulator::new(size, replies);
    let mut parser: alacritty_terminal::vte::ansi::Processor = Default::default();
    let mut guard = ingestion::Guard::default();
    let mut dirty = true;
    let mut lifecycle = 'owner: loop {
        if cancel.load(Ordering::Acquire) {
            let _ = child.kill();
            break Lifecycle::Closed;
        }
        if let Some(next) = resizes.try_iter().last()
            && next != size
        {
            if let Err(e) = pair.master.resize(pty_size(next)) {
                break Lifecycle::Failed(e.to_string());
            }
            size = next;
            term.resize(emulator::DimensionsOf(size));
            dirty = true;
        }
        match bytes.recv_timeout(Duration::from_millis(8)) {
            Ok(Ok(chunk)) => {
                if let Err(e) = ingest(&mut guard, &mut parser, &mut term, Ok(chunk)) {
                    break Lifecycle::Failed(e);
                }
                dirty = true;
            }
            Ok(Err(e)) => break Lifecycle::Failed(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // EOF can precede the process exit notification. Avoid spinning
                // while the owner waits for that independent lifecycle signal.
                std::thread::sleep(Duration::from_millis(8));
            }
        }
        // Cap per-iteration work so cancellation/resize cannot starve under output flood.
        match drain_output(&bytes, 31, &mut guard, &mut parser, &mut term) {
            Ok(changed) => dirty |= changed,
            Err(e) => break Lifecycle::Failed(e),
        }
        if dirty
            && frames
                .try_send(emulator::snapshot(&term, size, Lifecycle::Running))
                .is_ok()
        {
            dirty = false;
            notify.store(true, Ordering::Release);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // Consume already delivered final bytes before announcing exit.
                if let Err(e) = drain_output(&bytes, 32, &mut guard, &mut parser, &mut term) {
                    break 'owner Lifecycle::Failed(e);
                }
                break Lifecycle::Exited(status.exit_code());
            }
            Ok(None) => {}
            Err(e) => break Lifecycle::Failed(e.to_string()),
        }
    };
    // The writer observes the same cancellation flag and may close stdin while
    // this owner is between its top-of-loop check and try_wait. ConPTY can then
    // report STATUS_CONTROL_C_EXIT before we see cancellation. An explicit close
    // still owns this transition; do not misreport it as an unsolicited exit.
    if cancel.load(Ordering::Acquire) && matches!(lifecycle, Lifecycle::Exited(_)) {
        lifecycle = Lifecycle::Closed;
    }
    cancel.store(true, Ordering::Release);
    if !matches!(lifecycle, Lifecycle::Exited(_)) {
        let _ = child.kill();
    }
    let _ = child.wait();
    // ClosePseudoConsole can synchronously flush output. Keep the reader and the
    // bounded consumer draining concurrently with teardown, including final bytes
    // delivered after the child has already signalled exit.
    let closer = std::thread::Builder::new()
        .name("terminal-teardown".into())
        .spawn(move || drop(pair.master));
    while let Ok(chunk) = bytes.recv_timeout(Duration::from_secs(2)) {
        if !matches!(lifecycle, Lifecycle::Failed(_))
            && let Err(e) = ingest(&mut guard, &mut parser, &mut term, chunk)
        {
            lifecycle = Lifecycle::Failed(e);
        }
    }
    drop(bytes);
    if let Ok(thread) = closer {
        let _ = thread.join();
    }
    if let Ok(thread) = read_thread {
        let _ = thread.join();
    }
    if let Ok(thread) = write_thread {
        let _ = thread.join();
    }
    let _ = frames.send(emulator::snapshot(&term, size, lifecycle));
    notify.store(true, Ordering::Release);
    Ok(())
}

fn ingest(
    guard: &mut ingestion::Guard,
    parser: &mut alacritty_terminal::vte::ansi::Processor,
    term: &mut alacritty_terminal::Term<emulator::Listener>,
    chunk: Result<Vec<u8>, String>,
) -> Result<(), String> {
    let chunk = chunk?;
    guard.check(&chunk)?;
    parser.advance(term, &chunk);
    Ok(())
}

fn drain_output(
    bytes: &Receiver<Result<Vec<u8>, String>>,
    limit: usize,
    guard: &mut ingestion::Guard,
    parser: &mut alacritty_terminal::vte::ansi::Processor,
    term: &mut alacritty_terminal::Term<emulator::Listener>,
) -> Result<bool, String> {
    let mut changed = false;
    for chunk in bytes.try_iter().take(limit) {
        ingest(guard, parser, term, chunk)?;
        changed = true;
    }
    Ok(changed)
}

#[cfg(test)]
mod output_tests {
    use super::*;

    #[test]
    fn combining_budget_bounds_actual_cells_and_snapshot_across_reader_chunks() {
        let (tx, _) = mpsc::sync_channel(8);
        let size = GridSize::new(4, 2);
        let mut term = emulator::new(size, tx);
        let mut parser = Default::default();
        let mut guard = Default::default();
        assert!(ingest(&mut guard, &mut parser, &mut term, Ok(b"e".to_vec())).is_ok());
        let chunk = "\u{301}".repeat(4096).into_bytes();
        let mut accepted = 0;
        while ingest(&mut guard, &mut parser, &mut term, Ok(chunk.clone())).is_ok() {
            accepted += 4096;
            assert!(accepted <= 1_048_576);
        }
        let frame = emulator::snapshot(&term, size, Lifecycle::Running);
        assert_eq!(frame.cells[0].text.chars().count(), accepted + 1);
        assert_eq!(accepted, 1_048_576);
    }

    #[test]
    #[allow(clippy::unwrap_used)] // In-memory fixture channels cannot disconnect here.
    fn successful_output_followed_by_reader_error_is_not_discarded() {
        let (tx, _) = mpsc::sync_channel(8);
        let size = GridSize::new(20, 2);
        let mut term = emulator::new(size, tx);
        let mut parser = Default::default();
        let mut guard = Default::default();
        let (tx, rx) = mpsc::sync_channel(32);
        tx.send(Ok(b"hello".to_vec())).unwrap();
        tx.send(Err("reader failed".into())).unwrap();
        let result = drain_output(&rx, 31, &mut guard, &mut parser, &mut term);
        assert_eq!(result, Err("reader failed".into()));
        assert_eq!(
            emulator::snapshot(&term, size, Lifecycle::Running).cells[0].text,
            "h"
        );
    }
}
