//! Read-only native OS smoke test, including CPU warmup and current PID identity.
use std::time::Duration;
use tervix_application::monitoring::{ProcessId, TelemetryProvider};
use tervix_system::NativeTelemetry;

#[test]
#[allow(clippy::unwrap_used)] // Failure to sample/find our own process fails native acceptance.
fn native_read_only_sampling_warms_cpu_and_finds_own_process() {
    let mut provider = NativeTelemetry::default();
    let first = provider.sample().unwrap();
    assert!(first.cpu_percent.is_none());
    assert!(
        first
            .processes
            .iter()
            .all(|process| process.cpu_percent.is_none())
    );
    std::thread::sleep(Duration::from_millis(250));
    let next = provider.sample().unwrap();
    assert!(next.total_memory.is_some_and(|bytes| bytes > 0));
    assert!(
        next.cpu_percent
            .is_some_and(|value| (0.0..=100.0).contains(&value))
    );
    assert!(!next.cores.is_empty());
    let own = next
        .processes
        .iter()
        .find(|process| process.pid == ProcessId(std::process::id()))
        .unwrap();
    assert!(own.identity.is_some());
    assert!(own.resident_bytes.is_some_and(|bytes| bytes > 0));
    assert!(own.path.is_some());
    if cfg!(windows) {
        assert!(
            own.cpu_percent.is_none(),
            "second Windows observation only primes counters"
        );
    }
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(50));
    let third = provider.sample().unwrap();
    assert!(
        third
            .processes
            .iter()
            .find(|p| p.pid == own.pid)
            .unwrap()
            .cpu_percent
            .is_some()
    );
}

#[cfg(windows)]
#[test]
fn newly_busy_process_has_no_lifetime_cpu_then_reports_a_real_interval()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    struct Cleanup(Child);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut provider = NativeTelemetry::default();
    let _ = provider.sample()?;
    // Start after the provider is already observing. Busy work begins immediately,
    // so a valid later delta must be nonzero rather than merely within 0..100.
    let child = Cleanup(
        Command::new("powershell.exe")
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: background-only fixture.
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "while ($true) { for ($i=0; $i -lt 100000; $i++) { $x = [Math]::Sqrt($i) } }",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    let pid = ProcessId(child.0.id());
    let gap = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(100);
    for observation in 0..3 {
        std::thread::sleep(gap);
        let metrics = provider.sample()?;
        let process = metrics
            .processes
            .iter()
            .find(|p| p.pid == pid)
            .ok_or("busy child absent")?;
        assert!(process.identity.is_some());
        if observation < 2 {
            assert_eq!(
                process.cpu_percent, None,
                "new process observation {observation}"
            );
        } else {
            assert!(
                process
                    .cpu_percent
                    .is_some_and(|cpu| cpu > 0.0 && cpu <= 100.0),
                "expected actual busy interval, got {:?}",
                process.cpu_percent
            );
        }
    }
    Ok(())
}
