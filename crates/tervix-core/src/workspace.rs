use std::collections::BTreeSet;

use crate::{Layout, LayoutCommand, LayoutError, TabId, WindowId, WorkspaceId};

const MAX_WINDOWS: usize = 32;
const MAX_TABS: usize = 256;

/// A complete tiling page. Its layout retains focus while another tab is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    id: TabId,
    layout: Layout,
}

impl Tab {
    /// Creates a page from an already validated layout; performs no I/O.
    #[must_use]
    pub fn new(id: TabId, layout: Layout) -> Self {
        Self { id, layout }
    }

    /// Stable page identity.
    #[must_use]
    pub fn id(&self) -> TabId {
        self.id
    }

    /// Read-only layout; changes go through workspace commands.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
}

/// Ordered, nonempty tabs presented by one future native window; no OS handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    id: WindowId,
    tabs: Vec<Tab>,
    selected: TabId,
}

impl Window {
    /// Creates a window with one selected page.
    #[must_use]
    pub fn new(id: WindowId, tab: Tab) -> Self {
        Self {
            id,
            selected: tab.id,
            tabs: vec![tab],
        }
    }

    /// Stable window identity.
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// Pages in user-defined order.
    #[must_use]
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Selected page, always present in `tabs`.
    #[must_use]
    pub fn selected(&self) -> TabId {
        self.selected
    }

    fn remove_tab(&mut self, index: usize) -> Tab {
        let removed = self.tabs.remove(index);
        if self.selected == removed.id {
            self.selected = self.tabs[index.min(self.tabs.len() - 1)].id;
        }
        removed
    }
}

/// Named composition spanning one or more windows, with workspace-wide IDs.
/// This snapshot owns configurations, never live sessions or native resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    id: WorkspaceId,
    name: String,
    windows: Vec<Window>,
    active: WindowId,
}

/// Device-independent, transactional workspace edits. IDs are allocated outside core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceCommand {
    /// Append and activate a window. All contained identities must be fresh.
    AddWindow(Window),
    /// Remove a window, except the last; caller handles resource-close policy.
    CloseWindow(WindowId),
    /// Activate a window without changing its selected tab or pane focus.
    Activate(WindowId),
    /// Append and select a page in a window; does not activate that window.
    AddTab {
        /// Owning window.
        window: WindowId,
        /// New page with fresh tab/pane/view identities.
        tab: Tab,
    },
    /// Remove a page, except the window's last page.
    CloseTab(TabId),
    /// Select a page in its owning window without activating the window.
    SelectTab(TabId),
    /// Reorder or transfer a whole page without changing pane/view identities.
    /// Cannot empty the source window; detach/empty-window policy is deferred.
    MoveTab {
        /// Page to move.
        tab: TabId,
        /// Destination window, possibly the current owner.
        window: WindowId,
        /// Insertion index in the destination after removing the page.
        index: usize,
    },
    /// Apply an existing layout command, including to an inactive tab.
    Layout {
        /// Page owning the affected layout.
        tab: TabId,
        /// Validated tiling action.
        command: LayoutCommand,
    },
}

/// Rejected workspace edits leave every window, selection and layout unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceError {
    /// A referenced window is absent.
    #[error("window does not exist")]
    WindowNotFound,
    /// A referenced page is absent.
    #[error("tab does not exist")]
    TabNotFound,
    /// Window identities must be unique in a workspace.
    #[error("duplicate window identity")]
    DuplicateWindow,
    /// Tab identities must be unique across windows.
    #[error("duplicate tab identity")]
    DuplicateTab,
    /// Workspace must retain a window.
    #[error("cannot close the last window")]
    LastWindow,
    /// Windows must retain a page, including during transfers.
    #[error("cannot remove the last tab in a window")]
    LastTab,
    /// Insertion index must be within the post-removal destination length.
    #[error("tab insertion index is out of range")]
    InvalidIndex,
    /// Workspace name cannot be empty or whitespace-only.
    #[error("workspace name must not be blank")]
    InvalidName,
    /// Bound the snapshot to 32 windows and 256 total tabs.
    #[error("workspace exceeds the window or tab limit")]
    ResourceLimit,
    /// Includes workspace-wide duplicate pane/view IDs and local layout errors.
    #[error(transparent)]
    Layout(#[from] LayoutError),
}

impl Workspace {
    /// Creates a named composition. Rejects blank names or invalid identities.
    pub fn new(id: WorkspaceId, name: String, window: Window) -> Result<Self, WorkspaceError> {
        let workspace = Self {
            id,
            name,
            active: window.id,
            windows: vec![window],
        };
        workspace.validate()?;
        Ok(workspace)
    }

    /// Stable composition identity.
    #[must_use]
    pub fn id(&self) -> WorkspaceId {
        self.id
    }

    /// User-visible name, preserved exactly as supplied.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Ordered windows. There is always at least one.
    #[must_use]
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Active window identity, always reachable.
    #[must_use]
    pub fn active(&self) -> WindowId {
        self.active
    }

    /// Checks resource limits, selections and unique window/tab/pane/view IDs.
    /// Uniqueness is scoped to this workspace; no global registry is consulted.
    pub fn validate(&self) -> Result<(), WorkspaceError> {
        if self.name.trim().is_empty() {
            return Err(WorkspaceError::InvalidName);
        }
        if self.windows.is_empty() || self.windows.len() > MAX_WINDOWS {
            return Err(WorkspaceError::ResourceLimit);
        }
        let mut windows = BTreeSet::new();
        let mut tabs = BTreeSet::new();
        let mut panes = BTreeSet::new();
        let mut views = BTreeSet::new();
        for window in &self.windows {
            if !windows.insert(window.id) {
                return Err(WorkspaceError::DuplicateWindow);
            }
            if window.tabs.is_empty() || !window.tabs.iter().any(|tab| tab.id == window.selected) {
                return Err(WorkspaceError::TabNotFound);
            }
            for tab in &window.tabs {
                if !tabs.insert(tab.id) {
                    return Err(WorkspaceError::DuplicateTab);
                }
                if tabs.len() > MAX_TABS {
                    return Err(WorkspaceError::ResourceLimit);
                }
                tab.layout.validate()?;
                for pane in tab.layout.panes() {
                    if !panes.insert(pane.id) {
                        return Err(LayoutError::DuplicatePane.into());
                    }
                    if !views.insert(pane.view) {
                        return Err(LayoutError::DuplicateView.into());
                    }
                }
            }
        }
        if !windows.contains(&self.active) {
            return Err(WorkspaceError::WindowNotFound);
        }
        Ok(())
    }

    /// Computes a fully validated candidate; failure has no partial side effects.
    /// Close commands do not terminate processes; confirmation belongs to application.
    pub fn apply(&self, command: WorkspaceCommand) -> Result<Self, WorkspaceError> {
        let mut next = self.clone();
        match command {
            WorkspaceCommand::AddWindow(window) => {
                next.active = window.id;
                next.windows.push(window);
            }
            WorkspaceCommand::CloseWindow(id) => {
                let index = next.window_index(id)?;
                if next.windows.len() == 1 {
                    return Err(WorkspaceError::LastWindow);
                }
                next.windows.remove(index);
                if next.active == id {
                    next.active = next.windows[index.min(next.windows.len() - 1)].id;
                }
            }
            WorkspaceCommand::Activate(id) => {
                next.window_index(id)?;
                next.active = id;
            }
            WorkspaceCommand::AddTab { window, tab } => {
                let index = next.window_index(window)?;
                next.windows[index].selected = tab.id;
                next.windows[index].tabs.push(tab);
            }
            WorkspaceCommand::CloseTab(id) => {
                let (window, tab) = next.tab_index(id)?;
                if next.windows[window].tabs.len() == 1 {
                    return Err(WorkspaceError::LastTab);
                }
                next.windows[window].remove_tab(tab);
            }
            WorkspaceCommand::SelectTab(id) => {
                let (window, _) = next.tab_index(id)?;
                next.windows[window].selected = id;
            }
            WorkspaceCommand::MoveTab { tab, window, index } => {
                next.move_tab(tab, window, index)?;
            }
            WorkspaceCommand::Layout { tab, command } => {
                let (window, tab) = next.tab_index(tab)?;
                let layout = &mut next.windows[window].tabs[tab].layout;
                *layout = layout.apply(command)?;
            }
        }
        next.validate()?;
        Ok(next)
    }

    fn window_index(&self, id: WindowId) -> Result<usize, WorkspaceError> {
        self.windows
            .iter()
            .position(|window| window.id == id)
            .ok_or(WorkspaceError::WindowNotFound)
    }

    fn tab_index(&self, id: TabId) -> Result<(usize, usize), WorkspaceError> {
        self.windows
            .iter()
            .enumerate()
            .find_map(|(window, value)| {
                value
                    .tabs
                    .iter()
                    .position(|tab| tab.id == id)
                    .map(|tab| (window, tab))
            })
            .ok_or(WorkspaceError::TabNotFound)
    }

    fn move_tab(
        &mut self,
        id: TabId,
        target: WindowId,
        index: usize,
    ) -> Result<(), WorkspaceError> {
        let (source, tab) = self.tab_index(id)?;
        let destination = self.window_index(target)?;
        let same_window = source == destination;
        if !same_window && self.windows[source].tabs.len() == 1 {
            return Err(WorkspaceError::LastTab);
        }
        let length = self.windows[destination].tabs.len() - usize::from(same_window);
        if index > length {
            return Err(WorkspaceError::InvalidIndex);
        }
        // A reorder preserves selection; a transfer selects the moved page at destination.
        let selected = self.windows[source].selected;
        let removed = if same_window {
            self.windows[source].tabs.remove(tab)
        } else {
            self.windows[source].remove_tab(tab)
        };
        self.windows[destination].tabs.insert(index, removed);
        self.windows[destination].selected = if same_window { selected } else { id };
        Ok(())
    }
}
