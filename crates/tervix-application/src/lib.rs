//! Headless layout orchestration. Does not own GUI or OS resources.

use std::collections::VecDeque;

pub use tervix_core::{
    Axis, Direction, Layout, LayoutCommand, LayoutError, LayoutGeometry, LayoutNode,
    NavigationCommand, Pane, PaneGeometry, PaneId, Rect, Size, SplitRatio, ViewId,
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
}

/// One tab's current layout and bounded layout-only history.
/// View/session registries remain outside history: undo restores identities,
/// not live processes. Resource reconciliation will be added with PTY ownership.
#[derive(Debug)]
pub struct LayoutController {
    layout: Layout,
    history: VecDeque<Layout>,
    focus_history: VecDeque<PaneId>,
}

impl LayoutController {
    /// Starts orchestration from an already validated layout.
    #[must_use]
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            history: VecDeque::new(),
            focus_history: VecDeque::new(),
        }
    }

    /// Returns the current immutable domain snapshot.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
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

    /// Validates before commit. Focus-only and no-op commands do not add history.
    /// Errors leave both the current layout and undo stack unchanged.
    pub fn dispatch(&mut self, command: Command) -> Result<(), CommandError> {
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
            if self.history.len() == UNDO_LIMIT {
                self.history.pop_front();
            }
            self.history.push_back(self.layout.clone());
        }
        self.commit(next);
        Ok(())
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
        | LayoutCommand::Close(_)
        | LayoutCommand::Swap(_, _)
        | LayoutCommand::Resize { .. } => true,
        LayoutCommand::Navigate { action, .. } => match action {
            NavigationCommand::Focus(_) => false,
            NavigationCommand::Swap(_)
            | NavigationCommand::Move(_)
            | NavigationCommand::Resize { .. } => true,
        },
    }
}
