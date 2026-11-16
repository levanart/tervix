//! Headless layout and terminal orchestration. OS resources stay behind I/O ports.

use std::collections::VecDeque;

pub mod docker;
pub mod monitoring;
pub mod ssh;
pub mod terminal;
pub mod terminal_registry;
pub mod wsl;

pub use tervix_core::{
    Axis, Direction, Layout, LayoutCommand, LayoutError, LayoutGeometry, LayoutNode,
    NavigationCommand, Pane, PaneGeometry, PaneId, Rect, SessionId, Size, SplitBoundary,
    SplitRatio, ViewId,
};

/// Maximum number of topology/size changes retained for undo.
pub const UNDO_LIMIT: usize = 32;

/// Maximum recently focused, still-reachable panes retained per tab.
pub const FOCUS_HISTORY_LIMIT: usize = 32;

/// Application actions, independent of input devices and GUI toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Apply a validated domain command.
    Layout(LayoutCommand),
    /// Restore the previous layout snapshot; never restarts external sessions.
    Undo,
    /// Focus the most recently active reachable pane, without topology undo.
    FocusPrevious,
}

/// Error from a rejected action; state and undo history remain unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// Domain invariant rejected the change.
    #[error(transparent)]
    Layout(#[from] LayoutError),
    /// Undo has no retained snapshots.
    #[error("no layout changes to undo")]
    NothingToUndo,
    /// No previously focused pane remains reachable.
    #[error("no previous pane to focus")]
    NoPreviousFocus,
    /// Finish or cancel the active gesture before issuing another action.
    #[error("a resize gesture is already in progress")]
    ResizeInProgress,
    /// Preview, commit and cancel require an active gesture.
    #[error("no resize gesture is in progress")]
    NoResizeInProgress,
    /// Gesture previews accept only explicit split resize commands.
    #[error("resize previews require LayoutCommand::Resize or ResizeSplit")]
    NotResizeCommand,
}

/// One tab's current layout and bounded layout-only history.
/// View/session registries remain outside history: undo restores identities,
/// not live processes. `terminal_registry` reconciles resources independently.
#[derive(Debug)]
pub struct LayoutController {
    layout: Layout,
    history: VecDeque<Layout>,
    focus_history: VecDeque<PaneId>,
    resize_origin: Option<Layout>,
}

impl LayoutController {
    /// Starts orchestration from an already validated layout.
    #[must_use]
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            history: VecDeque::new(),
            focus_history: VecDeque::new(),
            resize_origin: None,
        }
    }

    /// Returns the current immutable domain snapshot.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Views reachable through the live layout, bounded undo, or resize cancellation.
    pub fn reachable_views(&self) -> std::collections::HashSet<ViewId> {
        std::iter::once(&self.layout)
            .chain(self.history.iter())
            .chain(self.resize_origin.iter())
            .flat_map(|layout| layout.panes().into_iter().map(|pane| pane.view))
            .collect()
    }

    /// Number of retained changes, at most `UNDO_LIMIT`.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.history.len()
    }

    /// Previously active, reachable panes, from oldest to newest. Excludes the
    /// current focus; IDs are unique. Not part of topology snapshots.
    pub fn focus_history(&self) -> impl DoubleEndedIterator<Item = PaneId> + '_ {
        self.focus_history.iter().copied()
    }

    /// Whether a transactional pointer resize is active.
    #[must_use]
    pub fn resize_active(&self) -> bool {
        self.resize_origin.is_some()
    }

    /// Begins a resize without changing the layout or undo history.
    /// Returns `ResizeInProgress` if a gesture is already active. All regular
    /// dispatches are rejected until commit/cancel, keeping split targets stable.
    pub fn begin_resize(&mut self) -> Result<(), CommandError> {
        if self.resize_active() {
            return Err(CommandError::ResizeInProgress);
        }
        self.resize_origin = Some(self.layout.clone());
        Ok(())
    }

    /// Applies a validated `LayoutCommand::Resize` or `ResizeSplit` preview,
    /// visible via `layout`. Recursive addresses remain stable during the gesture.
    /// Does not record undo or alter focus history. Any error preserves the last
    /// valid preview and leaves the gesture active. Other commands are rejected.
    pub fn preview_resize(&mut self, command: LayoutCommand) -> Result<(), CommandError> {
        if !self.resize_active() {
            return Err(CommandError::NoResizeInProgress);
        }
        if !matches!(
            command,
            LayoutCommand::Resize { .. } | LayoutCommand::ResizeSplit { .. }
        ) {
            return Err(CommandError::NotResizeCommand);
        }
        let next = self.layout.apply(command)?;
        self.layout = next;
        Ok(())
    }

    /// Ends the gesture, recording exactly one pre-gesture snapshot if changed.
    /// Unchanged gestures record nothing. Returns `NoResizeInProgress` if idle.
    pub fn commit_resize(&mut self) -> Result<(), CommandError> {
        let origin = self
            .resize_origin
            .take()
            .ok_or(CommandError::NoResizeInProgress)?;
        if origin != self.layout {
            self.record_undo(origin);
        }
        Ok(())
    }

    /// Restores the exact pre-gesture layout without changing undo/focus history.
    /// Returns `NoResizeInProgress` if idle.
    pub fn cancel_resize(&mut self) -> Result<(), CommandError> {
        let origin = self
            .resize_origin
            .take()
            .ok_or(CommandError::NoResizeInProgress)?;
        self.layout = origin;
        Ok(())
    }

    /// Validates before commit. Focus-only and no-op commands do not add history.
    /// Errors leave both the current layout and undo stack unchanged.
    /// During a resize gesture every command returns `ResizeInProgress`.
    pub fn dispatch(&mut self, command: Command) -> Result<(), CommandError> {
        if self.resize_active() {
            return Err(CommandError::ResizeInProgress);
        }
        match command {
            Command::Undo => {
                let previous = self.history.pop_back().ok_or(CommandError::NothingToUndo)?;
                self.commit(previous);
            }
            Command::FocusPrevious => {
                let previous = self
                    .focus_history
                    .back()
                    .copied()
                    .ok_or(CommandError::NoPreviousFocus)?;
                let next = self.layout.apply(LayoutCommand::Focus(previous))?;
                self.commit(next);
            }
            Command::Layout(action) => self.dispatch_layout(action)?,
        }
        Ok(())
    }

    fn dispatch_layout(&mut self, action: LayoutCommand) -> Result<(), CommandError> {
        let mut next = self.layout.apply(action)?;
        if matches!(action, LayoutCommand::Close(id) if id == self.layout.focused()) {
            let remaining = next.panes();
            if let Some(id) = self
                .focus_history
                .iter()
                .rev()
                .find(|id| remaining.iter().any(|pane| pane.id == **id))
            {
                next = next.apply(LayoutCommand::Focus(*id))?;
            }
        }
        if next == self.layout {
            return Ok(());
        }
        if records_undo(action) {
            self.record_undo(self.layout.clone());
        }
        self.commit(next);
        Ok(())
    }

    fn record_undo(&mut self, snapshot: Layout) {
        if self.history.len() == UNDO_LIMIT {
            self.history.pop_front();
        }
        self.history.push_back(snapshot);
    }

    fn commit(&mut self, next: Layout) {
        let old_focus = self.layout.focused();
        let new_focus = next.focused();
        let panes = next.panes();
        self.focus_history
            .retain(|id| *id != new_focus && panes.iter().any(|pane| pane.id == *id));
        if old_focus != new_focus && panes.iter().any(|pane| pane.id == old_focus) {
            self.focus_history.retain(|id| *id != old_focus);
            self.focus_history.push_back(old_focus);
            if self.focus_history.len() > FOCUS_HISTORY_LIMIT {
                self.focus_history.pop_front();
            }
        }
        self.layout = next;
    }
}

fn records_undo(action: LayoutCommand) -> bool {
    match action {
        LayoutCommand::Focus(_) => false,
        LayoutCommand::Split { .. }
        | LayoutCommand::MovePane { .. }
        | LayoutCommand::Close(_)
        | LayoutCommand::Swap(_, _)
        | LayoutCommand::Resize { .. }
        | LayoutCommand::ResizeSplit { .. } => true,
        LayoutCommand::Navigate { action, .. } => match action {
            NavigationCommand::Focus(_) => false,
            NavigationCommand::Swap(_)
            | NavigationCommand::Move(_)
            | NavigationCommand::Resize { .. } => true,
        },
    }
}
