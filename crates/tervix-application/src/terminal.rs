//! Nonblocking terminal port and immutable presentation data. No OS handles escape here.

use std::sync::Arc;

/// Requested shell, launched with an argument vector rather than shell interpolation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Profile {
    /// Discover PowerShell 7, Windows PowerShell, then cmd.
    #[default]
    Default,
    /// Discover an installed PowerShell.
    PowerShell,
    /// Windows command interpreter.
    Cmd,
    /// WSL default or an explicitly selected distribution.
    Wsl(Option<String>),
    /// Explicit distribution, Linux cwd and direct command arguments.
    WslLaunch(crate::wsl::Launch),
    /// Native OpenSSH interactive connection, explicitly started by the user.
    Ssh(crate::ssh::Connection),
    /// Engine-native interactive TTY, bound to a connection incarnation.
    DockerExec(crate::docker::ExecOptions),
}

/// Terminal lifecycle, including explicit closure independent of layout undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lifecycle {
    /// Background startup is pending.
    Starting,
    /// Child is running.
    Running,
    /// Transport teardown requested; terminal acknowledgement is still pending.
    Closing,
    /// Child exited with a process exit code.
    Exited(u32),
    /// Startup or I/O failed.
    Failed(String),
    /// Explicitly closed. Undo must not resurrect a process.
    Closed,
}

/// Grid dimensions bounded at the application boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    /// Number of columns (2..=500).
    pub columns: u16,
    /// Number of rows (1..=200).
    pub rows: u16,
}

impl GridSize {
    /// Clamps measured dimensions to the memory budget.
    pub fn new(columns: u16, rows: u16) -> Self {
        Self {
            columns: columns.clamp(2, 500),
            rows: rows.clamp(1, 200),
        }
    }
}

/// One rendered cell; wide spacers are omitted by the adapter.
#[derive(Debug, Clone)]
pub struct Cell {
    /// Zero-based column.
    pub column: u16,
    /// Zero-based row.
    pub row: u16,
    /// Unicode base character plus combining characters.
    pub text: String,
    /// RGB foreground.
    pub foreground: [u8; 3],
    /// RGB background.
    pub background: [u8; 3],
}

/// Coalescible immutable frame; raw PTY output is never discarded.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Current lifecycle.
    pub lifecycle: Lifecycle,
    /// Actual grid dimensions.
    pub size: GridSize,
    /// Visible cells only; scrollback stays bounded in the adapter.
    pub cells: Arc<[Cell]>,
    /// Visible cursor column and row.
    pub cursor: Option<(u16, u16)>,
    /// Application cursor mode for arrow encoding.
    pub application_cursor: bool,
    /// Bracketed paste negotiated by the application.
    pub bracketed_paste: bool,
}

impl Snapshot {
    /// Empty startup frame.
    pub fn starting(size: GridSize) -> Self {
        Self {
            lifecycle: Lifecycle::Starting,
            size,
            cells: Arc::from([]),
            cursor: None,
            application_cursor: false,
            bracketed_paste: false,
        }
    }
}

/// Rejected nonblocking command. Presentation must report that input was not accepted.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TerminalError {
    /// Bounded writer/control queue is full.
    #[error("terminal busy; input was not accepted")]
    Busy,
    /// Worker no longer accepts commands.
    #[error("terminal has ended; restart explicitly")]
    Ended,
    /// OS worker could not be started.
    #[error("terminal startup failed: {0}")]
    Startup(String),
}

/// Session operations must return immediately, including Drop/shutdown.
pub trait TerminalSession: Send {
    /// Stable identity of this process incarnation, distinct from its pane/view.
    fn id(&self) -> crate::SessionId;
    /// Coalesced notification set after a frame becomes available; consumed off the GUI thread.
    fn wakeup(&self) -> Arc<std::sync::atomic::AtomicBool>;
    /// Consume the newest available frame without blocking.
    fn poll(&mut self) -> Option<Snapshot>;
    /// Queue at most 64 KiB of input, without logging its contents.
    fn input(&self, bytes: Vec<u8>) -> Result<(), TerminalError>;
    /// Queue a measured, debounced grid size.
    fn resize(&self, size: GridSize) -> Result<(), TerminalError>;
    /// Cancel startup/close transport on its worker. Local providers terminate their
    /// child; Docker exec detaches only (Engine has no exec kill API).
    fn close(&self);
}

/// Adapter factory; creation queues background work and never performs OS I/O inline.
pub trait TerminalProvider: Send + Sync {
    /// Start a fresh session.
    fn start(
        &self,
        profile: Profile,
        size: GridSize,
    ) -> Result<Box<dyn TerminalSession>, TerminalError>;
}
