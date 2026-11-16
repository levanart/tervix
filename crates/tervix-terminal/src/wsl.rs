use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tervix_application::wsl::{Discovery, DiscoveryError, DiscoveryState, DistributionName};

const LIMIT: usize = 65536;
const TIMEOUT: Duration = Duration::from_secs(5);

/// Bounded WSL discovery worker. Construction only creates a worker, never runs WSL.
pub struct NativeWslDiscovery {
    requests: mpsc::SyncSender<()>,
    results: mpsc::Receiver<DiscoveryState>,
    pending: bool,
    active: Arc<AtomicBool>,
}
impl NativeWslDiscovery {
    /// Start one worker with capacity-one request/result channels.
    pub fn new() -> Result<Self, std::io::Error> {
        let (requests, receiver) = mpsc::sync_channel(1);
        let (sender, results) = mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(false));
        let busy = active.clone();
        std::thread::Builder::new()
            .name("wsl-discovery".into())
            .spawn(move || {
                while receiver.recv().is_ok() {
                    discover_with(|state| {
                        let _ = sender.send(state);
                    });
                    busy.store(false, Ordering::Release);
                }
            })?;
        Ok(Self {
            requests,
            results,
            pending: false,
            active,
        })
    }
}
impl Discovery for NativeWslDiscovery {
    fn refresh(&mut self) -> bool {
        if self.pending || self.active.swap(true, Ordering::AcqRel) {
            return false;
        }
        if self.requests.try_send(()).is_err() {
            self.active.store(false, Ordering::Release);
            return false;
        }
        self.pending = true;
        true
    }
    fn poll(&mut self) -> Option<DiscoveryState> {
        match self.results.try_recv() {
            Ok(state) => {
                self.pending = false;
                Some(state)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) if self.pending => {
                self.pending = false;
                Some(DiscoveryState::Error(DiscoveryError::Io(
                    "discovery worker ended".into(),
                )))
            }
            Err(_) => None,
        }
    }
}

fn discover_state() -> DiscoveryState {
    let mut state = DiscoveryState::Unavailable;
    discover_with(|result| state = result);
    state
}

fn discover_with(publish: impl FnOnce(DiscoveryState)) {
    if !cfg!(windows) {
        publish(DiscoveryState::Unavailable);
        return;
    }
    let Some(path) = super::profiles::executable("wsl.exe") else {
        publish(DiscoveryState::Unavailable);
        return;
    };
    let mut command = Command::new(path);
    command.args(["--list", "--quiet"]);
    let mut publish = Some(publish);
    let result = run_with(&mut command, TIMEOUT, |result| {
        if let Some(publish) = publish.take() {
            publish(state_from(result.clone()));
        }
    });
    if let Some(publish) = publish {
        publish(state_from(result));
    }
}

fn state_from(result: Result<Vec<u8>, DiscoveryError>) -> DiscoveryState {
    match result.and_then(|bytes| parse(&bytes)) {
        Ok(names) => DiscoveryState::Ready(names),
        Err(error) => DiscoveryState::Error(error),
    }
}

/// Blocking compatibility helper; call only on a background worker.
pub fn discover_wsl() -> Result<Vec<String>, String> {
    match discover_state() {
        DiscoveryState::Ready(names) => Ok(names.iter().map(|n| n.as_str().to_owned()).collect()),
        DiscoveryState::Unavailable => Err("WSL is unavailable (wsl.exe not found)".into()),
        DiscoveryState::Error(error) => Err(error.to_string()),
        _ => Err("WSL discovery did not complete".into()),
    }
}

fn io(error: std::io::Error) -> DiscoveryError {
    DiscoveryError::Io(error.to_string())
}

#[cfg(all(test, windows))]
fn run(command: &mut Command, timeout: Duration) -> Result<Vec<u8>, DiscoveryError> {
    run_with(command, timeout, |_| {})
}

fn run_with(
    command: &mut Command,
    timeout: Duration,
    publish: impl FnOnce(&Result<Vec<u8>, DiscoveryError>),
) -> Result<Vec<u8>, DiscoveryError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(io)?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(DiscoveryError::Io("missing stdout".into()));
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = std::thread::Builder::new()
        .name("wsl-output".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout
                .take((LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(io)
                .and({
                    if bytes.len() > LIMIT {
                        Err(DiscoveryError::OutputLimit)
                    } else {
                        Ok(bytes)
                    }
                });
            let _ = sender.send(result);
        });
    let reader = match reader {
        Ok(reader) => reader,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io(error));
        }
    };
    let deadline = Instant::now() + timeout;
    let result = (|| {
        let status = loop {
            if Instant::now() >= deadline {
                return Err(DiscoveryError::Timeout);
            }
            match child.try_wait().map_err(io)? {
                Some(status) => break status,
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        if !status.success() {
            return Err(DiscoveryError::Exit(status.code()));
        }
        receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| DiscoveryError::Timeout)?
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    // Publish timeout before joining: an inherited pipe can outlive wsl.exe.
    // A single persistent worker joins it before accepting another request.
    publish(&result);
    let _ = reader.join();
    result
}

fn parse(bytes: &[u8]) -> Result<Vec<DistributionName>, DiscoveryError> {
    let utf16 = bytes.starts_with(&[0xff, 0xfe]) || bytes.contains(&0);
    let text = if utf16 {
        let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
        if !bytes.len().is_multiple_of(2) {
            return Err(DiscoveryError::InvalidOutput);
        }
        let words: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        String::from_utf16(&words).map_err(|_| DiscoveryError::InvalidOutput)?
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(bytes)
                .to_vec(),
        )
        .map_err(|_| DiscoveryError::InvalidOutput)?
    };
    let mut names = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let name = DistributionName::new(line)?;
        if !names.contains(&name) {
            names.push(name);
        }
        if names.len() > 256 {
            return Err(DiscoveryError::OutputLimit);
        }
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_malformed_output() -> Result<(), DiscoveryError> {
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(
            "Ubuntu\r\n開発\r\nUbuntu\r\n"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        );
        assert_eq!(
            parse(&bytes)?
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>(),
            ["Ubuntu", "開発"]
        );
        bytes.push(0);
        assert_eq!(parse(&bytes), Err(DiscoveryError::InvalidOutput));
        assert_eq!(
            parse(&[0xff, 0xfe, 0x00, 0xd8]),
            Err(DiscoveryError::InvalidOutput)
        );
        assert_eq!(parse(&[0xff]), Err(DiscoveryError::InvalidOutput));
        assert!(parse(b"\r\n")?.is_empty());
        assert_eq!(parse(b"--bad"), Err(DiscoveryError::InvalidOutput));
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn process_error_timeout_and_output_budget() {
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "exit 7"]);
        assert_eq!(
            run(&mut command, TIMEOUT),
            Err(DiscoveryError::Exit(Some(7)))
        );
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "pause"]);
        assert_eq!(
            run(&mut command, Duration::ZERO),
            Err(DiscoveryError::Timeout)
        );
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-Command", "[Console]::Write('x' * 65537)"]);
        assert_eq!(run(&mut command, TIMEOUT), Err(DiscoveryError::OutputLimit));
    }
    #[test]
    #[ignore = "explicitly probes locally installed WSL distributions"]
    fn installed_distributions() -> Result<(), std::io::Error> {
        let discovery = NativeWslDiscovery::new()?;
        let mut catalog = tervix_application::wsl::DistributionCatalog::new(Box::new(discovery));
        assert!(catalog.refresh());
        assert_eq!(catalog.state(), &DiscoveryState::Loading);
        let deadline = Instant::now() + Duration::from_secs(7);
        while !catalog.poll() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("Observed WSL: {:?}", catalog.state());
        assert!(matches!(catalog.state(), DiscoveryState::Ready(_)));
        Ok(())
    }
}
