//! Read-only process identities, independent of platform handles.

/// Numeric OS process identifier; alone it does not identify an incarnation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u32);

/// Process incarnation. Only construct when the OS supplies a known start time.
/// Future process actions must revalidate this identity against the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessIdentity {
    /// OS process identifier.
    pub pid: ProcessId,
    /// Seconds since Unix epoch, as reported by the OS.
    pub start_time: u64,
}
