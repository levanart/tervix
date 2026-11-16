//! Nonblocking shared Docker Engine port. UI never handles transport or credentials.

use std::{sync::Arc, time::SystemTime};

/// Full Engine container identity, never a display name or shell fragment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContainerId(String);

impl ContainerId {
    /// Accept a full 64-digit hexadecimal Engine ID; rejects names and path injection.
    pub fn new(value: String) -> Result<Self, DockerError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(DockerError::InvalidInput);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    /// Full stable identifier used for Engine requests.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Explicit local endpoint. No implicit DOCKER_HOST, context switch or TCP downgrade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// Docker Desktop host-engine pipe (`docker_engine` or `dockerDesktopLinuxEngine`).
    NamedPipe(String),
    /// Absolute Unix socket on Unix hosts. Not a Windows path into a WSL filesystem.
    UnixSocket(String),
}

impl Default for Endpoint {
    fn default() -> Self {
        #[cfg(windows)]
        {
            Self::NamedPipe("//./pipe/docker_engine".into())
        }
        #[cfg(not(windows))]
        {
            Self::UnixSocket("/var/run/docker.sock".into())
        }
    }
}

/// Discovered context metadata, never registry auth or TLS private material.
#[derive(Debug, Clone)]
pub struct Context {
    /// Display name from local Docker context metadata.
    pub name: String,
    /// Configured current context; discovery never automatically connects it.
    pub selected: bool,
    /// Supported local transport, absent for remote/insecure/invalid contexts.
    pub endpoint: Option<Endpoint>,
    /// Why this context cannot be connected by this adapter.
    pub error: Option<DockerError>,
}

/// Sanitized machine-readable failure; never includes a daemon response body or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DockerError {
    /// Bounded queue or stream budget exhausted; command was not accepted.
    #[error("Docker is busy; request was not accepted")]
    Busy,
    /// No explicit live connection.
    #[error("Docker is disconnected")]
    Disconnected,
    /// Worker or request cancelled. An interrupted mutation may have reached the daemon.
    #[error("Docker request cancelled; refresh to determine action outcome")]
    Cancelled,
    /// Deadline elapsed. An interrupted mutation may have reached the daemon.
    #[error("Docker request timed out; refresh to determine action outcome")]
    Timeout,
    /// Socket/pipe permission or Engine authorization denied.
    #[error("Docker access denied")]
    AccessDenied,
    /// Container disappeared.
    #[error("Docker container was not found")]
    NotFound,
    /// Container lifecycle conflicts with operation.
    #[error("Docker operation conflicts with container state")]
    Conflict,
    /// Invalid ID, endpoint, confirmation or request.
    #[error("invalid Docker request")]
    InvalidInput,
    /// Transport or platform not supported; do not silently downgrade security.
    #[error("Docker endpoint is unsupported on this platform")]
    Unsupported,
    /// Engine error, with sanitized HTTP status only.
    #[error("Docker Engine returned HTTP {0}")]
    Engine(u16),
    /// Malformed/incompatible response.
    #[error("incompatible Docker Engine response")]
    Protocol,
    /// Transport failed; may retry read-only discovery after backoff.
    #[error("Docker Engine is unavailable")]
    Unavailable,
}

/// Connection lifecycle. Retained rows during retry are stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial state; no Engine I/O occurs until Connect.
    Disconnected,
    /// Initial handshake in progress.
    Connecting,
    /// Version negotiation and list succeeded.
    Connected,
    /// Explicitly requested endpoint will be retried with bounded backoff.
    Retrying(DockerError),
}

/// Implemented operations, not an authorization guarantee for a specific container.
#[derive(Debug, Clone, Default)]
pub struct Capabilities {
    /// List and safe inspect.
    pub containers: bool,
    /// Logs may still be unsupported by an individual logging driver.
    pub logs: bool,
    /// Stats fields may be absent on Windows or stopped containers.
    pub stats: bool,
    /// Lifecycle actions, always explicitly submitted by user.
    pub lifecycle: bool,
}

/// Published port; missing host binding means exposed-only.
#[derive(Debug, Clone)]
pub struct Port {
    /// Container port.
    pub private: u16,
    /// Published host port.
    pub public: Option<u16>,
    /// TCP, UDP or another Engine protocol.
    pub protocol: String,
    /// Host binding address, not a connection credential.
    pub host_ip: Option<String>,
}

/// Safe all-container table row. No env, labels, command, mounts or credentials.
#[derive(Debug, Clone)]
pub struct Container {
    /// Full Engine ID.
    pub id: ContainerId,
    /// Engine display names.
    pub names: Arc<[String]>,
    /// Image reference.
    pub image: String,
    /// Machine state (running/exited/paused/etc.).
    pub state: String,
    /// Human-readable status, not parsed for lifecycle decisions.
    pub status: String,
    /// Unix creation seconds, if reported.
    pub created: Option<i64>,
    /// Bounded published/exposed ports.
    pub ports: Arc<[Port]>,
}

/// Shared coalesced frame; keep the last successful frame visibly stale on failure.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Connection incarnation; changes on endpoint selection and connection failure.
    /// Targets from an older incarnation must never be reused after reconnect.
    pub incarnation: u64,
    /// Monotonic publication counter.
    pub revision: u64,
    /// Selected endpoint, absent until explicitly connected.
    pub endpoint: Option<Endpoint>,
    /// Current connection state.
    pub state: ConnectionState,
    /// Successfully negotiated operations.
    pub capabilities: Capabilities,
    /// Last successful all-container observation.
    pub observed_at: Option<SystemTime>,
    /// Immutable rows shared by duplicate panes.
    pub containers: Arc<[Container]>,
}

/// Deliberately allowlisted inspect details, rather than arbitrary daemon JSON.
#[derive(Debug, Clone)]
pub struct Details {
    /// Identity verified against requested ID.
    pub id: ContainerId,
    /// Display name.
    pub name: String,
    /// Image ID/reference.
    pub image: String,
    /// Container platform.
    pub platform: String,
    /// Running flag if supplied by Engine.
    pub running: Option<bool>,
    /// Paused flag if supplied by Engine.
    pub paused: Option<bool>,
    /// Exit code, if known.
    pub exit_code: Option<i64>,
    /// RFC3339 start time, not a promise of process continuity.
    pub started_at: Option<String>,
    /// RFC3339 finish time.
    pub finished_at: Option<String>,
}

/// On-demand stats; no fake zeros for absent measurements.
#[derive(Debug, Clone)]
pub struct Stats {
    /// Wall-clock collection time; cached duplicate requests retain this timestamp.
    pub observed_at: SystemTime,
    /// Engine CPU convention: 100% = one logical CPU; may exceed 100%.
    pub cpu_percent: Option<f64>,
    /// Raw memory usage bytes (includes cache; not the CLI working-set subtraction).
    pub memory_bytes: Option<u64>,
    /// Memory limit bytes.
    pub memory_limit: Option<u64>,
}

/// Explicit user action; no action is retried automatically after timeout/reconnect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Start an existing container.
    Start,
    /// Graceful stop, bounded timeout; require UI confirmation before submission.
    Stop,
    /// Restart; require UI confirmation before submission.
    Restart,
    /// Suspend container processes.
    Pause,
    /// Resume container processes.
    Unpause,
    /// Remove only a stopped container, never force or delete volumes. Confirmation
    /// must contain the same full ID shown by the confirming UI.
    Remove {
        /// Full ID displayed in and accepted by the confirmation dialog.
        confirmed: ContainerId,
    },
}

/// Log transport channel. TTY merges stdout/stderr, so it is explicitly marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogChannel {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
    /// TTY combined output or Engine console stream.
    Combined,
}

/// One bounded, display-sanitized log fragment. No raw control sequences are rendered.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Monotonic source sequence for loss/gap detection.
    pub sequence: u64,
    /// Original transport channel.
    pub channel: LogChannel,
    /// Engine RFC3339 timestamp when present.
    pub timestamp: Option<String>,
    /// Plain text; may contain sensitive application data. Never trace/export automatically.
    pub text: String,
    /// Fragment exceeded entry byte limit.
    pub truncated: bool,
}

/// Stream lifecycle independent of global engine state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogState {
    /// Opening/following.
    Following,
    /// Container stopped or finite stream ended. Explicit resubscribe to reopen.
    Ended,
    /// Engine error; resubscribe explicitly. Global connection may independently retry.
    Failed(DockerError),
    /// Endpoint disconnected/replaced or service shut down.
    Cancelled,
}

/// Latest bounded ring snapshot; slow consumers coalesce without producer blocking.
#[derive(Debug, Clone)]
pub struct LogSnapshot {
    /// Publication revision, including lifecycle-only changes.
    pub revision: u64,
    /// Following/EOF/error/cancel state.
    pub state: LogState,
    /// Oldest-to-newest entries. No unbounded history exists behind this frame.
    pub entries: Arc<[LogEntry]>,
    /// Total entries evicted by byte/entry budget, explicitly visible to user.
    pub dropped_entries: u64,
    /// Input chunks truncated by entry-size or per-frame work limits. Content beyond
    /// those limits is unavailable; its number of original lines cannot be determined.
    pub truncated_chunks: u64,
}

/// Local presentation filter; does not spawn another Engine stream.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    /// Match a channel, or both when absent.
    pub channel: Option<LogChannel>,
    /// Literal case-sensitive substring, not a regex or shell expression.
    pub contains: String,
}

impl LogFilter {
    /// Filter one entry; timestamp remains separate from display text.
    pub fn matches(&self, entry: &LogEntry) -> bool {
        self.channel.is_none_or(|channel| entry.channel == channel)
            && entry.text.contains(&self.contains)
    }
}

/// Bounded asynchronous reply mailbox. Polling is immediate; never wait on the GUI.
pub trait Reply<T>: Send {
    /// Consume completion once. `None` means pending; poll only until the first result.
    fn poll(&mut self) -> Option<Result<T, DockerError>>;
}

/// Independent pane reader sharing a per-container source. Dropping the last reader
/// cancels that stream. Pausing freezes this reader, not collection or other readers;
/// resume jumps to the latest bounded frame and its explicit eviction count.
pub trait LogReader: Send {
    /// Latest frame; while paused returns the last frame seen by this reader.
    fn latest(&mut self) -> Arc<LogSnapshot>;
    /// Freeze/resume local presentation; source memory remains bounded.
    fn set_paused(&mut self, paused: bool);
}

/// Shared adapter service, injected once from composition root as `Arc<dyn DockerService>`.
/// All methods are immediate, bounded and perform no OS I/O on the caller thread.
/// Container operations take the `Snapshot::incarnation` that supplied their target.
/// A mismatch cancels the request, including across automatic same-endpoint retries;
/// adapters must validate this at dispatch, not replace it with the current value.
pub trait DockerService: Send + Sync {
    /// Connection incarnation; old targets and exec configurations must never follow reconnects.
    fn exec_generation(&self) -> u64 {
        0
    }
    /// Queue an interactive TTY. Closing detaches transport, never kills the container
    /// or promises termination of the exec process. No automatic retry.
    fn exec(
        &self,
        _options: ExecOptions,
        _size: crate::terminal::GridSize,
    ) -> Result<Box<dyn crate::terminal::TerminalSession>, DockerError> {
        Err(DockerError::Unsupported)
    }
    /// Discover bounded local context metadata in the background, without invoking Docker CLI
    /// or resolving credentials. Unsupported remote contexts are shown with a reason.
    fn contexts(&self) -> Result<Box<dyn Reply<Arc<[Context]>>>, DockerError>;
    /// Queue explicit endpoint selection and version negotiation. Starts read-only retries.
    fn connect(&self, endpoint: Endpoint) -> Result<(), DockerError>;
    /// Queue disconnect; cancels log sources and clears endpoint-specific state.
    fn disconnect(&self) -> Result<(), DockerError>;
    /// Set aggregate visibility across windows (1s visible, 5s hidden list polling).
    fn set_visible(&self, visible: bool);
    /// Read the newest immutable connection/list frame.
    fn snapshot(&self) -> Arc<Snapshot>;
    /// Request allowlisted inspect. No credentials/env/commands in result.
    fn inspect(
        &self,
        incarnation: u64,
        id: ContainerId,
    ) -> Result<Box<dyn Reply<Details>>, DockerError>;
    /// Request fresh stats only on demand; callers may share the returned DTO.
    fn stats(
        &self,
        incarnation: u64,
        id: ContainerId,
    ) -> Result<Box<dyn Reply<Stats>>, DockerError>;
    /// Queue one explicit user operation. Confirmation belongs at presentation boundary.
    fn act(
        &self,
        incarnation: u64,
        id: ContainerId,
        action: Action,
    ) -> Result<Box<dyn Reply<()>>, DockerError>;
    /// Subscribe to one shared bounded source per ID (200 initial tail fragments).
    fn logs(
        &self,
        incarnation: u64,
        id: ContainerId,
    ) -> Result<Box<dyn Reply<Box<dyn LogReader>>>, DockerError>;
}

/// Explicit Engine exec identity and direct argv (no shell interpolation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOptions {
    /// Selected endpoint, checked again on dispatch.
    pub endpoint: Endpoint,
    /// Connection incarnation captured when the action was selected.
    pub generation: u64,
    /// Full immutable container identity.
    pub container: ContainerId,
    /// Program executed directly; UI defaults to /bin/sh, optionally /bin/bash.
    pub program: String,
    /// Direct arguments, never concatenated into a shell command.
    pub args: Vec<String>,
}
