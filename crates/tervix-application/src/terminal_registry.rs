//! View-keyed resource reconciliation, deliberately independent of layout undo.
use crate::{
    ViewId,
    terminal::{
        GridSize, Lifecycle, Profile, Snapshot, TerminalError, TerminalProvider, TerminalSession,
    },
};
use std::{collections::HashMap, sync::Arc};

/// One view's durable in-process terminal state.
pub struct Entry {
    /// Process incarnation retained as an ended tombstone after close.
    pub session_id: Option<crate::SessionId>,
    /// Requested profile.
    pub profile: Profile,
    /// Latest immutable frame.
    pub snapshot: Snapshot,
    session: Option<Box<dyn TerminalSession>>,
    present: bool,
}

/// Application-owned session handles; UI only sees DTOs and nonblocking commands.
#[derive(Default)]
pub struct TerminalRegistry {
    provider: Option<Arc<dyn TerminalProvider>>,
    entries: HashMap<ViewId, Entry>,
}
impl TerminalRegistry {
    /// Requests cancellation and polls teardown acknowledgements without blocking.
    /// Returns true once every owned session has published a terminal lifecycle
    /// and its port has been released. Call repeatedly; do not reconcile or restart
    /// views during shutdown. A close request alone is not a teardown acknowledgement.
    pub fn shutdown(&mut self) -> bool {
        for entry in self.entries.values_mut() {
            if let Some(session) = &mut entry.session {
                session.close();
                if let Some(frame) = session.poll() {
                    entry.snapshot = frame;
                }
                if matches!(
                    entry.snapshot.lifecycle,
                    Lifecycle::Starting | Lifecycle::Running | Lifecycle::Closing
                ) {
                    continue;
                }
                entry.session = None;
            }
        }
        self.entries.values().all(|entry| entry.session.is_none())
    }

    /// Shared notifications for the currently owned session workers.
    pub fn wakeups(&self) -> Vec<Arc<std::sync::atomic::AtomicBool>> {
        self.entries
            .values()
            .filter_map(|e| e.session.as_ref().map(|s| s.wakeup()))
            .collect()
    }
    /// Installs the composition root's adapter.
    pub fn set_provider(&mut self, provider: Arc<dyn TerminalProvider>) {
        self.provider = Some(provider);
    }
    /// Releases entries no longer reachable from any live layout or bounded undo.
    /// Call after reconciliation; undo-reachable closed entries retain identity only.
    pub fn retain_reachable(&mut self, reachable: &std::collections::HashSet<ViewId>) {
        self.entries.retain(|id, _| reachable.contains(id));
    }
    /// Closes absent views, creates unseen views, and never restarts ended views on undo.
    pub fn reconcile(&mut self, views: impl IntoIterator<Item = (ViewId, Profile)>) {
        let visible: HashMap<_, _> = views.into_iter().collect();
        for (id, entry) in &mut self.entries {
            if !visible.contains_key(id) && entry.present {
                if let Some(session) = entry.session.take() {
                    session.close();
                }
                entry.snapshot.lifecycle = Lifecycle::Closed;
                entry.snapshot.cells = Default::default();
                entry.snapshot.cursor = None;
                entry.present = false;
            }
        }
        if self.provider.is_none() {
            return;
        }
        for (id, profile) in visible {
            if !self.entries.contains_key(&id)
                || self.entries.get(&id).is_some_and(|e| e.profile != profile)
            {
                self.restart(id, profile);
            }
            if let Some(entry) = self.entries.get_mut(&id) {
                entry.present = true;
            }
        }
    }
    /// Explicitly replace a session; this is never invoked by topology undo.
    pub fn restart(&mut self, id: ViewId, profile: Profile) {
        if let Some(session) = self.entries.get_mut(&id).and_then(|e| e.session.take()) {
            session.close();
        }
        let size = GridSize::new(80, 24);
        let mut snapshot = Snapshot::starting(size);
        let session = self.provider.as_ref().and_then(|provider| {
            match provider.start(profile.clone(), size) {
                Ok(session) => Some(session),
                Err(e) => {
                    snapshot.lifecycle = Lifecycle::Failed(e.to_string());
                    None
                }
            }
        });
        let session_id = session.as_ref().map(|s| s.id());
        self.entries.insert(
            id,
            Entry {
                session_id,
                profile,
                snapshot,
                session,
                present: true,
            },
        );
    }
    /// Poll immutable frames without locks or blocking I/O. Returns changed view IDs.
    pub fn poll(&mut self) -> Vec<ViewId> {
        self.entries
            .iter_mut()
            .filter_map(|(id, entry)| {
                let frame = entry.session.as_mut()?.poll()?;
                entry.snapshot = frame;
                Some(*id)
            })
            .collect()
    }
    /// Inspect visible state.
    pub fn get(&self, id: ViewId) -> Option<&Entry> {
        self.entries.get(&id)
    }
    /// Queue input to a live session.
    pub fn input(&self, id: ViewId, bytes: Vec<u8>) -> Result<(), TerminalError> {
        self.entries
            .get(&id)
            .and_then(|e| e.session.as_ref())
            .ok_or(TerminalError::Ended)?
            .input(bytes)
    }
    /// Queue a debounced resize to a live session.
    pub fn resize(&self, id: ViewId, size: GridSize) -> Result<(), TerminalError> {
        self.entries
            .get(&id)
            .and_then(|e| e.session.as_ref())
            .ok_or(TerminalError::Ended)?
            .resize(size)
    }
}
