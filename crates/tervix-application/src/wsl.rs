//! WSL selector DTOs and a nonblocking discovery port; no operating-system handles.

/// Validated distribution name, kept verbatim and passed as one argv element.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DistributionName(String);

impl DistributionName {
    /// Reject empty, oversized, option-like or control-containing names.
    pub fn new(name: impl Into<String>) -> Result<Self, DiscoveryError> {
        let name = name.into();
        if name.is_empty()
            || name.len() > 1024
            || name.starts_with('-')
            || name.trim() != name
            || name.chars().any(char::is_control)
        {
            return Err(DiscoveryError::InvalidOutput);
        }
        Ok(Self(name))
    }
    /// Original Unicode name, without localization-dependent annotations.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Select this distribution using the existing terminal profile API.
    pub fn profile(&self) -> crate::terminal::Profile {
        crate::terminal::Profile::Wsl(Some(self.0.clone()))
    }
}

/// Machine-readable failures; localized command output is never parsed as status.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiscoveryError {
    /// Process creation, reading or waiting failed.
    #[error("WSL discovery I/O failed: {0}")]
    Io(String),
    /// WSL did not finish within its deadline.
    #[error("WSL discovery timed out")]
    Timeout,
    /// Nonzero exit, including missing WSL components; message language is irrelevant.
    #[error("WSL discovery exited with code {0:?}")]
    Exit(Option<i32>),
    /// Output exceeds the 64 KiB byte budget.
    #[error("WSL discovery exceeded 64 KiB")]
    OutputLimit,
    /// Malformed Unicode or an invalid distribution name.
    #[error("WSL returned malformed distribution output")]
    InvalidOutput,
}

/// Selector state. An empty Ready list means WSL ran successfully but has no distros.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DiscoveryState {
    /// No explicit request yet.
    #[default]
    Idle,
    /// Background request pending.
    Loading,
    /// Successfully discovered distribution names.
    Ready(Vec<DistributionName>),
    /// wsl.exe is absent or this platform is unsupported.
    Unavailable,
    /// Failure with retry available.
    Error(DiscoveryError),
}

/// One bounded background discovery service, injected by the composition root.
pub trait Discovery: Send {
    /// Queue a refresh; false means a request is already pending or worker ended.
    fn refresh(&mut self) -> bool;
    /// Consume a completed state immediately, without blocking.
    fn poll(&mut self) -> Option<DiscoveryState>;
}

/// Application-owned selector model; repeated refreshes coalesce while Loading.
pub struct DistributionCatalog {
    discovery: Box<dyn Discovery>,
    state: DiscoveryState,
}
impl DistributionCatalog {
    /// Construct without starting discovery or performing OS I/O.
    pub fn new(discovery: Box<dyn Discovery>) -> Self {
        Self {
            discovery,
            state: DiscoveryState::Idle,
        }
    }
    /// Start an explicit refresh; existing terminal selections are unaffected.
    pub fn refresh(&mut self) -> bool {
        if self.state == DiscoveryState::Loading || !self.discovery.refresh() {
            return false;
        }
        self.state = DiscoveryState::Loading;
        true
    }
    /// Poll from an asynchronous UI task/tick; returns whether state changed.
    pub fn poll(&mut self) -> bool {
        if let Some(state) = self.discovery.poll() {
            self.state = state;
            true
        } else {
            false
        }
    }
    /// Current selector presentation state.
    pub fn state(&self) -> &DiscoveryState {
        &self.state
    }
}

/// Explicit Linux launch settings. Windows host cwd is deliberately not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Chosen distribution; None preserves WSL's system default.
    pub distribution: Option<DistributionName>,
    /// Absolute Linux cwd (or `~`); never implicitly translate a Windows path.
    pub linux_cwd: Option<String>,
    /// Direct Linux executable and arguments; empty uses the configured default shell.
    pub argv: Vec<String>,
}
impl Launch {
    /// Validate cwd and argv before any process starts.
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        if self.linux_cwd.as_ref().is_some_and(|p| {
            p.len() > 32768 || !(p.starts_with('/') || p == "~") || p.contains(['\0', '\r', '\n'])
        }) || self.argv.iter().any(|a| a.contains('\0'))
            || self.argv.first().is_some_and(|a| a.is_empty())
            || self.argv.len() > 256
            || self.argv.iter().map(String::len).sum::<usize>() > 32768
        {
            return Err(DiscoveryError::InvalidOutput);
        }
        Ok(())
    }
    /// Profile carrying these explicit settings, validated again by the adapter.
    pub fn profile(self) -> Result<crate::terminal::Profile, DiscoveryError> {
        self.validate()?;
        Ok(crate::terminal::Profile::WslLaunch(self))
    }
}
