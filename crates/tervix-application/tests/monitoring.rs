//! Shared scheduler, failure recovery and presentation semantics regression tests.
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
use tervix_application::monitoring::{
    Metrics, MonitoringConfig, MonitoringService, ProcessId, ProcessIdentity, TelemetryError,
    TelemetryProvider, normalize_cpu,
};

struct Provider {
    calls: Arc<AtomicUsize>,
    signal: mpsc::SyncSender<()>,
}
impl TelemetryProvider for Provider {
    fn sample(&mut self) -> Result<Metrics, TelemetryError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self.signal.try_send(());
        match call {
            1 => Err(TelemetryError::Provider("injected read failure".into())),
            _ => Ok(Metrics {
                cpu_percent: Some(25.0),
                ..Metrics::default()
            }),
        }
    }
}

fn wait_revision(
    reader: &tervix_application::monitoring::MonitorReader,
    revision: u64,
) -> Arc<tervix_application::monitoring::Snapshot> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(frame) = reader.latest()
            && frame.revision >= revision
        {
            return frame;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "worker publication timed out"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[allow(clippy::unwrap_used)] // Fixtures fail the test if worker creation/publication fails.
fn duplicate_readers_share_one_worker_failure_retains_stale_data_and_recovery_clears_error() {
    let (sender, receiver) = mpsc::sync_channel(4);
    let calls = Arc::new(AtomicUsize::new(0));
    let service = MonitoringService::start(
        Some(Box::new(Provider {
            calls: Arc::clone(&calls),
            signal: sender,
        })),
        MonitoringConfig::new(500, 5000, 60),
    )
    .unwrap();
    service.set_visible(true);
    let mut first = service.reader();
    let mut second = service.reader();
    receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    let initial = wait_revision(&first, 1);
    assert!(Arc::ptr_eq(&initial, &second.latest().unwrap()));
    assert_eq!(initial.metrics.as_ref().unwrap().cpu_percent, Some(25.0));
    first.poll();
    second.poll();
    assert!(first.poll().is_none());
    assert!(second.poll().is_none());
    receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    let failed = wait_revision(&first, 2);
    assert!(failed.is_stale());
    assert_eq!(failed.interval, Duration::from_millis(500));
    assert!(failed.error.is_some());
    assert!(Arc::ptr_eq(
        initial.metrics.as_ref().unwrap(),
        failed.metrics.as_ref().unwrap()
    ));
    assert_eq!(failed.history.len(), 1);
    receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    let recovered = wait_revision(&second, 3);
    assert!(recovered.error.is_none());
    assert_eq!(recovered.history.len(), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    service.shutdown_and_join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
#[allow(clippy::unwrap_used)] // Thread startup/join failures must fail this test.
fn unavailable_provider_has_no_fictional_metrics_and_cancels_without_waiting_for_interval() {
    let service = MonitoringService::start(None, MonitoringConfig::default()).unwrap();
    let frame = wait_revision(&service.reader(), 1);
    assert_eq!(frame.error, Some(TelemetryError::Unavailable));
    assert!(frame.metrics.is_none());
    assert!(frame.observed_at.is_none());
    assert!(frame.is_stale());
    let start = std::time::Instant::now();
    service.shutdown_and_join().unwrap();
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn cpu_normalization_and_pid_reuse_are_explicit() {
    assert_eq!(normalize_cpu(240.0, 4), Some(60.0));
    assert_eq!(normalize_cpu(200.0, 1), Some(100.0));
    for value in [f32::NAN, f32::INFINITY, -1.0] {
        assert_eq!(normalize_cpu(value, 4), None);
    }
    assert_eq!(normalize_cpu(0.0, 0), None);
    assert_eq!(normalize_cpu(0.0, 4), Some(0.0));
    assert_ne!(
        ProcessIdentity {
            pid: ProcessId(42),
            start_time: 100
        },
        ProcessIdentity {
            pid: ProcessId(42),
            start_time: 101
        }
    );
}

struct DelayedProvider {
    started: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}

impl TelemetryProvider for DelayedProvider {
    fn sample(&mut self) -> Result<Metrics, TelemetryError> {
        self.started
            .send(())
            .map_err(|_| TelemetryError::Unavailable)?;
        self.release
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| TelemetryError::Unavailable)?;
        Ok(Metrics::default())
    }
}

#[test]
#[allow(clippy::unwrap_used)] // Synchronization fixture failures must fail the test.
fn cancellation_during_sampling_does_not_publish_late_results() {
    let (started, notification) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let service = MonitoringService::start(
        Some(Box::new(DelayedProvider {
            started,
            release: wait,
        })),
        MonitoringConfig::default(),
    )
    .unwrap();
    let reader = service.reader();
    notification.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(reader.latest().unwrap().error.is_none()); // Starting, not unavailable.
    service.cancel();
    release.send(()).unwrap();
    service.shutdown_and_join().unwrap();
    let frame = reader.latest().unwrap();
    assert_eq!(frame.revision, 0);
    assert!(frame.metrics.is_none());
}
