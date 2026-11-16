//! Shared read-only monitoring port and presentation data; no adapter types escape.

mod service;
pub use service::{MonitorReader, MonitoringConfig, MonitoringService};
pub use tervix_core::{ProcessId, ProcessIdentity};

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

/// Process row. Missing values must be rendered as N/A, never fabricated zeros.
#[derive(Debug, Clone)]
pub struct Process {
    /// Numeric PID, even when incarnation cannot be determined.
    pub pid: ProcessId,
    /// PID plus known OS start time; absent for inaccessible processes.
    pub identity: Option<ProcessIdentity>,
    /// Display name, not a command line.
    pub name: String,
    /// CPU normalized to 0–100% of the whole machine; absent during warmup.
    pub cpu_percent: Option<f32>,
    /// Resident memory in bytes; absent if inaccessible.
    pub resident_bytes: Option<u64>,
    /// Private memory is not available through this provider.
    pub private_bytes: Option<u64>,
    /// Executable path, if permitted.
    pub path: Option<PathBuf>,
    /// Provider's human-readable OS state, if known.
    pub state: Option<String>,
    /// Elapsed runtime when known.
    pub uptime: Option<Duration>,
}

/// Immutable sampled machine values. Every byte count uses bytes, not KiB.
#[derive(Debug, Clone, Default)]
pub struct Metrics {
    /// Whole-machine utilization, 0–100%; absent during initial warmup.
    pub cpu_percent: Option<f32>,
    /// Per logical core, in stable provider order; empty if unsupported.
    pub cores: Arc<[Option<f32>]>,
    /// Installed physical RAM.
    pub total_memory: Option<u64>,
    /// Available physical RAM (includes reclaimable caches where reported).
    pub available_memory: Option<u64>,
    /// Total configured swap/pagefile.
    pub total_swap: Option<u64>,
    /// Used swap/pagefile.
    pub used_swap: Option<u64>,
    /// Process rows sorted by PID. Only the latest sample retains rows.
    pub processes: Arc<[Process]>,
}

/// Finite utilization normalized to the whole machine. Invalid values are absent.
pub fn normalize_cpu(value: f32, logical_cores: usize) -> Option<f32> {
    (value.is_finite() && value >= 0.0 && logical_cores > 0)
        .then(|| (value / logical_cores as f32).clamp(0.0, 100.0))
}

/// Typed provider failure; does not contain shell input or credentials.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TelemetryError {
    /// Provider not installed or unsupported on this platform.
    #[error("system telemetry is unavailable")]
    Unavailable,
    /// OS sampling failed. Retain the last successful data as stale.
    #[error("system telemetry sampling failed: {0}")]
    Provider(String),
}

/// Blocking read-only I/O boundary. Called exclusively by one scheduler worker.
pub trait TelemetryProvider: Send + 'static {
    /// Read current values. Must return in finite time; cancellation waits for this call.
    /// CPU delta providers should report None until a sufficient interval has elapsed.
    fn sample(&mut self) -> Result<Metrics, TelemetryError>;
}

/// Small history row, deliberately excluding process lists and paths.
#[derive(Debug, Clone)]
pub struct HistoryPoint {
    /// Observation wall clock, for chart labels only.
    pub observed_at: SystemTime,
    /// Whole-machine utilization.
    pub cpu_percent: Option<f32>,
    /// Available physical memory, bytes.
    pub available_memory: Option<u64>,
}

/// Coalescible shared frame. Freshness is calculated at consumption, not frozen.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Monotonic publication sequence; independent readers never consume each other's events.
    pub revision: u64,
    /// Last successful observation, or None before first success.
    pub observed_at: Option<SystemTime>,
    /// Monotonic instant of the last successful observation; use `age()` for freshness.
    pub sampled_at: Option<std::time::Instant>,
    /// Current requested sampling interval.
    pub interval: Duration,
    /// Last successful values; absent before first success.
    pub metrics: Option<Arc<Metrics>>,
    /// Last provider error, cleared on recovery.
    pub error: Option<TelemetryError>,
    /// Bounded aggregate history (default five minutes).
    pub history: Arc<[HistoryPoint]>,
}

impl Snapshot {
    /// Actual monotonic age, unaffected by wall-clock changes.
    pub fn age(&self) -> Option<Duration> {
        self.sampled_at.map(|time| time.elapsed())
    }

    /// Failed, missing, or older than twice the requested cadence.
    pub fn is_stale(&self) -> bool {
        self.error.is_some() || self.age().is_none_or(|age| age > self.interval * 2)
    }
}
