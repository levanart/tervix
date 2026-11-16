//! Read-only native telemetry adapter. OS collection runs on the application worker.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tervix_application::monitoring::{
    Metrics, Process, ProcessId, ProcessIdentity, TelemetryError, TelemetryProvider, normalize_cpu,
};

/// Native sysinfo collector, with lazily initialized OS state and CPU baselines.
/// Construction performs no collection. Move into one `MonitoringService`.
#[derive(Default)]
pub struct NativeTelemetry {
    system: Option<System>,
    previous: Option<Instant>,
    identities: HashMap<ProcessIdentity, ProcessBaseline>,
}

#[derive(Default)]
struct ProcessBaseline {
    primed: bool,
}

impl ProcessBaseline {
    fn observe(&mut self, spaced: bool) -> bool {
        let usable = spaced && self.primed;
        self.primed |= spaced;
        usable
    }
}

fn sufficiently_spaced(previous: Option<Instant>, started: Instant) -> bool {
    previous.is_some_and(|previous| {
        started.saturating_duration_since(previous) >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL
    })
}

fn process_cpu_ready(
    identities: &mut HashMap<ProcessIdentity, ProcessBaseline>,
    id: ProcessIdentity,
    spaced: bool,
) -> bool {
    match identities.entry(id) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            // sysinfo 0.37.2 Windows creates zero counters with
            // last_update=now. The immediate update skips them;
            // the next spaced update only primes the counters.
            entry.insert(ProcessBaseline {
                primed: !cfg!(windows),
            });
            false
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => entry.get_mut().observe(spaced),
    }
}

impl TelemetryProvider for NativeTelemetry {
    fn sample(&mut self) -> Result<Metrics, TelemetryError> {
        if !sysinfo::IS_SUPPORTED_SYSTEM {
            return Err(TelemetryError::Unavailable);
        }
        // Compare refresh start with the previous refresh END: Windows resets
        // last_update even when it skips updating the delta counters. Collection
        // duration must not make a too-soon refresh look sufficiently spaced.
        let started = Instant::now();
        let warm = sufficiently_spaced(self.previous, started);
        let system = self.system.get_or_insert_with(System::new);
        system.refresh_cpu_usage();
        system.refresh_memory();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
        let now = Instant::now();
        self.previous = Some(now);
        let count = system.cpus().len();
        let mut processes: Vec<_> = system
            .processes()
            .iter()
            .map(|(pid, process)| {
                let pid = ProcessId(pid.as_u32());
                let known = process.start_time() != 0;
                // sysinfo returns zero for inaccessible process memory. Preserve N/A
                // conservatively for zero readings or unknown incarnations.
                let identity = known.then_some(ProcessIdentity {
                    pid,
                    start_time: process.start_time(),
                });
                let has_baseline =
                    identity.is_some_and(|id| process_cpu_ready(&mut self.identities, id, warm));
                Process {
                    pid,
                    identity,
                    name: process.name().to_string_lossy().into_owned(),
                    cpu_percent: (warm && has_baseline)
                        .then(|| normalize_cpu(process.cpu_usage(), count))
                        .flatten(),
                    resident_bytes: (known && process.memory() > 0).then_some(process.memory()),
                    private_bytes: None,
                    path: process
                        .exe()
                        .filter(|path| !path.as_os_str().is_empty())
                        .map(ToOwned::to_owned),
                    state: match process.status() {
                        sysinfo::ProcessStatus::Unknown(_) => None,
                        status => Some(status.to_string()),
                    },
                    uptime: known.then(|| Duration::from_secs(process.run_time())),
                }
            })
            .collect();
        processes.sort_unstable_by_key(|process| process.pid);
        self.identities.retain(|id, _| {
            system
                .process(sysinfo::Pid::from_u32(id.pid.0))
                .is_some_and(|process| process.start_time() == id.start_time)
        });
        let memory_known = system.total_memory() > 0;
        Ok(Metrics {
            cpu_percent: (warm && count > 0)
                .then(|| normalize_cpu(system.global_cpu_usage(), 1))
                .flatten(),
            cores: system
                .cpus()
                .iter()
                .map(|cpu| warm.then(|| normalize_cpu(cpu.cpu_usage(), 1)).flatten())
                .collect(),
            total_memory: memory_known.then_some(system.total_memory()),
            available_memory: memory_known.then_some(system.available_memory()),
            total_swap: memory_known.then_some(system.total_swap()),
            used_swap: memory_known.then_some(system.used_swap()),
            processes: Arc::from(processes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_uses_refresh_end_and_short_observations_do_not_prime() {
        let start = Instant::now();
        let minimum = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL;
        let mut baseline = ProcessBaseline::default();
        assert!(!sufficiently_spaced(None, start));
        // A long collection followed immediately by another is still too soon.
        let end = start + minimum * 3;
        let short = end + minimum / 2;
        assert!(!baseline.observe(sufficiently_spaced(Some(end), short)));
        // The skipped call reset sysinfo's clock. Measuring from the older end
        // would incorrectly prime this observation.
        let short_again = short + minimum / 2;
        assert!(!baseline.observe(sufficiently_spaced(Some(short), short_again)));
        let prime = short_again + minimum;
        assert!(!baseline.observe(sufficiently_spaced(Some(short_again), prime)));
        assert!(baseline.observe(sufficiently_spaced(Some(prime), prime + minimum)));
        assert!(
            !baseline.observe(false),
            "never expose a cached too-soon value"
        );
        assert!(
            baseline.observe(true),
            "a short call does not erase initialized counters"
        );
    }

    #[test]
    fn each_new_incarnation_requires_its_own_delta_baseline() {
        let mut identities = HashMap::new();
        let first = ProcessIdentity {
            pid: ProcessId(42),
            start_time: 1,
        };
        let reused = ProcessIdentity {
            start_time: 2,
            ..first
        };
        for id in [first, reused] {
            assert!(!process_cpu_ready(&mut identities, id, true));
            assert!(!process_cpu_ready(&mut identities, id, false));
            assert_eq!(process_cpu_ready(&mut identities, id, true), !cfg!(windows));
            assert!(process_cpu_ready(&mut identities, id, true));
        }
        assert!(process_cpu_ready(&mut identities, first, true));
    }
}
