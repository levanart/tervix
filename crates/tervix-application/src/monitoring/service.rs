use super::{HistoryPoint, Snapshot, TelemetryError, TelemetryProvider};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

/// Validated scheduler policy. Intervals are clamped to 500–5000 ms;
/// history is clamped to 60–300 seconds with at most 601 points.
#[derive(Debug, Clone, Copy)]
pub struct MonitoringConfig {
    active: Duration,
    hidden: Duration,
    history: Duration,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self::new(1000, 5000, 300)
    }
}

impl MonitoringConfig {
    /// Construct a bounded configuration from milliseconds and history seconds.
    pub fn new(active_ms: u64, hidden_ms: u64, history_seconds: u64) -> Self {
        Self {
            active: Duration::from_millis(active_ms.clamp(500, 5000)),
            hidden: Duration::from_millis(hidden_ms.clamp(500, 5000)),
            history: Duration::from_secs(history_seconds.clamp(60, 300)),
        }
    }

    fn interval(self, visible: bool) -> Duration {
        if visible { self.active } else { self.hidden }
    }
}

struct Shared {
    latest: Mutex<Arc<Snapshot>>,
    visible: AtomicBool,
    cancelled: AtomicBool,
}

/// Cheap independent view of one shared provider. Cloning never starts a poller.
#[derive(Clone)]
pub struct MonitorReader {
    shared: Arc<Shared>,
    revision: u64,
}

impl MonitorReader {
    /// Latest frame without waiting for a producer lock. None means retry later.
    /// Returns the same Arc to every pane until a new publication.
    pub fn latest(&self) -> Option<Arc<Snapshot>> {
        self.shared
            .latest
            .try_lock()
            .ok()
            .map(|frame| Arc::clone(&frame))
    }

    /// Coalesced updates per reader; intermediate publications may be superseded.
    pub fn poll(&mut self) -> Option<Arc<Snapshot>> {
        let frame = self.latest()?;
        if frame.revision == self.revision {
            return None;
        }
        self.revision = frame.revision;
        Some(frame)
    }
}

/// Own exactly one service at composition root and share readers across all windows.
/// Drop cancels/wakes without joining, so it never blocks GUI on OS sampling.
pub struct MonitoringService {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl MonitoringService {
    /// Start a named background worker. None supplies an explicit unavailable provider.
    /// Only thread creation errors are returned inline; OS sampling is always off-thread.
    pub fn start(
        provider: Option<Box<dyn TelemetryProvider>>,
        config: MonitoringConfig,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            latest: Mutex::new(Arc::new(Snapshot {
                revision: 0,
                observed_at: None,
                sampled_at: None,
                interval: config.hidden,
                metrics: None,
                error: provider.is_none().then_some(TelemetryError::Unavailable),
                history: Arc::from([]),
            })),
            visible: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        });
        let state = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("tervix-monitoring".into())
            .spawn(move || run(provider, config, state))?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    /// Subscribe with an independent coalescing cursor.
    pub fn reader(&self) -> MonitorReader {
        MonitorReader {
            shared: Arc::clone(&self.shared),
            revision: u64::MAX,
        }
    }

    /// Supply aggregate visibility: true if ANY monitoring/process pane in ANY
    /// window is visible. Do not call independently with each pane's local state.
    pub fn set_visible(&self, any_visible: bool) {
        if self.shared.visible.swap(any_visible, Ordering::Relaxed) != any_visible
            && let Some(worker) = &self.worker
        {
            worker.thread().unpark();
        }
    }

    /// Request cancellation. A running provider call completes before worker exit.
    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::Relaxed);
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
    }

    /// Cancel and join on an application shutdown/background thread, never GUI.
    /// Reports provider panic as the standard thread join error.
    pub fn shutdown_and_join(mut self) -> thread::Result<()> {
        self.cancel();
        match self.worker.take() {
            Some(worker) => worker.join(),
            None => Ok(()),
        }
    }
}

impl Drop for MonitoringService {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn run(
    mut provider: Option<Box<dyn TelemetryProvider>>,
    config: MonitoringConfig,
    shared: Arc<Shared>,
) {
    let mut history: VecDeque<(Instant, HistoryPoint)> = VecDeque::new();
    let mut frame = match shared.latest.lock() {
        Ok(frame) => (**frame).clone(),
        Err(_) => return,
    };
    let mut last_attempt = None;
    loop {
        if shared.cancelled.load(Ordering::Relaxed) {
            break;
        }
        let interval = config.interval(shared.visible.load(Ordering::Relaxed));
        if let Some(last) = last_attempt {
            let due = last + interval;
            if Instant::now() < due {
                thread::park_timeout(due.saturating_duration_since(Instant::now()));
                continue;
            }
        }
        let result = match provider.as_mut() {
            Some(provider) => provider.sample(),
            None => Err(TelemetryError::Unavailable),
        };
        if shared.cancelled.load(Ordering::Relaxed) {
            break;
        }
        let now = Instant::now();
        last_attempt = Some(now);
        frame.interval = interval;
        frame.revision = frame.revision.saturating_add(1);
        match result {
            Ok(metrics) => {
                let observed_at = SystemTime::now();
                history.push_back((
                    now,
                    HistoryPoint {
                        observed_at,
                        cpu_percent: metrics.cpu_percent,
                        available_memory: metrics.available_memory,
                    },
                ));
                frame.observed_at = Some(observed_at);
                frame.sampled_at = Some(now);
                frame.metrics = Some(Arc::new(metrics));
                frame.error = None;
            }
            Err(error) => frame.error = Some(error),
        }
        prune_history(&mut history, now, config.history);
        frame.history = history.iter().map(|(_, point)| point.clone()).collect();
        let next = Arc::new(frame.clone());
        match shared.latest.lock() {
            Ok(mut latest) => *latest = next,
            Err(_) => break,
        }
    }
}

fn prune_history(
    history: &mut VecDeque<(Instant, HistoryPoint)>,
    now: Instant,
    duration: Duration,
) {
    while history
        .front()
        .is_some_and(|(time, _)| now.duration_since(*time) > duration)
        || history.len() > 601
    {
        history.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_bounds_cadence_and_retention() {
        let config = MonitoringConfig::new(0, u64::MAX, u64::MAX);
        assert_eq!(config.interval(true), Duration::from_millis(500));
        assert_eq!(config.interval(false), Duration::from_millis(5000));
        assert_eq!(config.history, Duration::from_secs(300));
        assert_eq!(
            MonitoringConfig::new(1000, 5000, 0).history,
            Duration::from_secs(60)
        );
    }

    #[test]
    fn history_uses_monotonic_age_and_hard_capacity() {
        let now = Instant::now();
        let point = HistoryPoint {
            observed_at: SystemTime::UNIX_EPOCH,
            cpu_percent: None,
            available_memory: None,
        };
        let mut history = VecDeque::from([(now - Duration::from_secs(301), point.clone())]);
        history.extend((0..700).map(|_| (now, point.clone())));
        prune_history(&mut history, now, Duration::from_secs(300));
        assert_eq!(history.len(), 601);
        assert!(history.iter().all(|(time, _)| *time == now));
    }
}
