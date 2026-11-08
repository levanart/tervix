use std::collections::BTreeSet;

use crate::{NavigationCommand, PaneId, Size, ViewId};

/// Resource limits for untrusted restored layouts and bounded undo snapshots.
const MAX_DEPTH: usize = 32;
const MAX_PANES: usize = 256;

/// Direction in which the two children are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// First above second.
    Horizontal,
    /// First to the left of second.
    Vertical,
}

/// First child's share in basis points, constrained to 10–90%.
/// `Layout::measure` additionally enforces logical minimum tile dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitRatio(u16);

impl SplitRatio {
    /// Equal shares for both children.
    pub const HALF: Self = Self(5_000);

    /// Validates a first-child share. Returns an error outside 1000..=9000.
    pub fn new(basis_points: u16) -> Result<Self, LayoutError> {
        if (1_000..=9_000).contains(&basis_points) {
            Ok(Self(basis_points))
        } else {
            Err(LayoutError::InvalidRatio)
        }
    }

    /// Returns the first-child share out of 10,000.
    #[must_use]
    pub const fn basis_points(self) -> u16 {
        self.0
    }
}

/// A tile and the view instance it owns. Contains no runtime session handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pane {
    /// Stable tile identity.
    pub id: PaneId,
    /// Independent view identity, retained during swaps.
    pub view: ViewId,
}

/// Recursive binary layout. Raw trees become trusted only through `Layout::new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutNode {
    /// A single view instance.
    Pane(Pane),
    /// Two recursively arranged subtrees.
    Split {
        /// Placement direction.
        axis: Axis,
        /// First child's share.
        ratio: SplitRatio,
        /// Top or left child.
        first: Box<Self>,
        /// Bottom or right child.
        second: Box<Self>,
    },
}

/// A nonempty, validated layout with reachable focus and unique pane/view IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    root: LayoutNode,
    focused: PaneId,
}

/// Typed commands shared by future keyboard, palette, menu and pointer adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutCommand {
    /// Directional action resolved using this viewport and minimum tile size.
    Navigate {
        /// Device-independent action on the active pane.
        action: NavigationCommand,
        /// Current visible logical area.
        viewport: Size,
        /// Minimum logical dimensions of every tile, including decorations.
        minimum: Size,
    },
    /// Splits a tile, retaining it as first child and focusing the new tile.
    Split {
        /// Existing tile to split.
        target: PaneId,
        /// Fresh tile and fresh view instance.
        pane: Pane,
        /// Child arrangement.
        axis: Axis,
        /// Initial share.
        ratio: SplitRatio,
    },
    /// Changes focus without changing topology.
    Focus(PaneId),
    /// Removes a tile and collapses its parent; cannot remove the final tile.
    Close(PaneId),
    /// Exchanges whole tiles, preserving both view identities and focus.
    Swap(PaneId, PaneId),
    /// Resizes the immediate parent of a tile.
    Resize {
        /// Tile whose immediate parent is resized.
        pane: PaneId,
        /// New first-child share, independent of which child contains `pane`.
        ratio: SplitRatio,
    },
}

/// Rejected commands leave the original layout unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LayoutError {
    /// Ratio exceeds policy bounds.
    #[error("split ratio must be between 1000 and 9000 basis points")]
    InvalidRatio,
    /// A tile is missing, including an unreachable focus.
    #[error("pane does not exist in this layout")]
    PaneNotFound,
    /// Pane identities must be unique.
    #[error("pane identity already exists")]
    DuplicatePane,
    /// A view instance may occupy only one tile.
    #[error("view instance already belongs to a pane")]
    DuplicateView,
    /// Every layout must retain one tile.
    #[error("cannot close the last pane")]
    LastPane,
    /// Single-tile layouts have no split boundary.
    #[error("pane has no parent split")]
    NoSplit,
    /// Prevent pathological tree depth or memory use.
    #[error("layout exceeds the depth or pane count limit")]
    ResourceLimit,
    /// Logical dimensions must be positive.
    #[error("layout dimensions must be nonzero")]
    InvalidSize,
    /// Requested minimum canvas cannot be represented.
    #[error("layout geometry exceeds the coordinate range")]
    GeometryOverflow,
    /// No pane lies in the requested direction; navigation does not wrap.
    #[error("no pane in this direction")]
    NoNeighbor,
    /// No ancestor split boundary exists on that side of the active pane.
    #[error("no resizable boundary in this direction")]
    NoBoundary,
    /// Keyboard resize step must be positive and bounded.
    #[error("resize step must be between 1 and 8000 basis points")]
    InvalidStep,
}

impl Layout {
    /// Validates a raw tree and focus. Rejects duplicate IDs and resource limits.
    pub fn new(root: LayoutNode, focused: PaneId) -> Result<Self, LayoutError> {
        let layout = Self { root, focused };
        layout.validate()?;
        Ok(layout)
    }

    /// Creates a valid one-pane layout.
    #[must_use]
    pub fn single(pane: Pane) -> Self {
        Self {
            root: LayoutNode::Pane(pane),
            focused: pane.id,
        }
    }

    /// Returns the read-only tree; mutation must use validated commands.
    #[must_use]
    pub fn root(&self) -> &LayoutNode {
        &self.root
    }

    /// Returns the active tile.
    #[must_use]
    pub fn focused(&self) -> PaneId {
        self.focused
    }

    /// Returns tiles in deterministic depth-first visual order.
    #[must_use]
    pub fn panes(&self) -> Vec<Pane> {
        let mut panes = Vec::new();
        self.root.collect(&mut panes);
        panes
    }

    /// Checks uniqueness, resource bounds and focus reachability.
    pub fn validate(&self) -> Result<(), LayoutError> {
        let mut panes = BTreeSet::new();
        let mut views = BTreeSet::new();
        self.root.validate(0, &mut panes, &mut views)?;
        if !panes.contains(&self.focused) {
            return Err(LayoutError::PaneNotFound);
        }
        Ok(())
    }

    /// Computes a new validated layout without mutating this one.
    /// Failure cannot partially apply a command. No external side effects occur.
    pub fn apply(&self, command: LayoutCommand) -> Result<Self, LayoutError> {
        let mut next = self.clone();
        match command {
            LayoutCommand::Navigate {
                action,
                viewport,
                minimum,
            } => {
                return self.navigate(action, viewport, minimum);
            }
            LayoutCommand::Focus(id) => next.focused = id,
            LayoutCommand::Split {
                target,
                pane,
                axis,
                ratio,
            } => {
                let node = next
                    .root
                    .find_mut(target)
                    .ok_or(LayoutError::PaneNotFound)?;
                *node = LayoutNode::Split {
                    axis,
                    ratio,
                    first: Box::new(node.clone()),
                    second: Box::new(LayoutNode::Pane(pane)),
                };
                next.focused = pane.id;
            }
            LayoutCommand::Close(id) => {
                next.root.find_mut(id).ok_or(LayoutError::PaneNotFound)?;
                next.root = next.root.remove(id).ok_or(LayoutError::LastPane)?;
                if next.focused == id {
                    next.focused = next.root.first_pane().id;
                }
            }
            LayoutCommand::Swap(a, b) => {
                let first = *next.root.pane(a).ok_or(LayoutError::PaneNotFound)?;
                let second = *next.root.pane(b).ok_or(LayoutError::PaneNotFound)?;
                next.root.swap(first, second);
            }
            LayoutCommand::Resize { pane, ratio } => {
                next.root.find_mut(pane).ok_or(LayoutError::PaneNotFound)?;
                if !next.root.resize_parent(pane, ratio) {
                    return Err(LayoutError::NoSplit);
                }
            }
        }
        next.validate()?;
        Ok(next)
    }
}

impl LayoutNode {
    fn validate(
        &self,
        depth: usize,
        panes: &mut BTreeSet<PaneId>,
        views: &mut BTreeSet<ViewId>,
    ) -> Result<(), LayoutError> {
        if depth > MAX_DEPTH || panes.len() >= MAX_PANES {
            return Err(LayoutError::ResourceLimit);
        }
        match self {
            Self::Pane(pane) => {
                if !panes.insert(pane.id) {
                    return Err(LayoutError::DuplicatePane);
                }
                if !views.insert(pane.view) {
                    return Err(LayoutError::DuplicateView);
                }
            }
            Self::Split { first, second, .. } => {
                first.validate(depth + 1, panes, views)?;
                second.validate(depth + 1, panes, views)?;
            }
        }
        Ok(())
    }

    fn collect(&self, panes: &mut Vec<Pane>) {
        match self {
            Self::Pane(pane) => panes.push(*pane),
            Self::Split { first, second, .. } => {
                first.collect(panes);
                second.collect(panes);
            }
        }
    }

    fn first_pane(&self) -> Pane {
        match self {
            Self::Pane(pane) => *pane,
            Self::Split { first, .. } => first.first_pane(),
        }
    }

    fn pane(&self, id: PaneId) -> Option<&Pane> {
        match self {
            Self::Pane(pane) => (pane.id == id).then_some(pane),
            Self::Split { first, second, .. } => first.pane(id).or_else(|| second.pane(id)),
        }
    }

    fn find_mut(&mut self, id: PaneId) -> Option<&mut Self> {
        match self {
            Self::Pane(pane) if pane.id == id => Some(self),
            Self::Pane(_) => None,
            Self::Split { first, second, .. } => first.find_mut(id).or_else(|| second.find_mut(id)),
        }
    }

    fn swap(&mut self, a: Pane, b: Pane) {
        match self {
            Self::Pane(pane) if pane.id == a.id => *pane = b,
            Self::Pane(pane) if pane.id == b.id => *pane = a,
            Self::Pane(_) => {}
            Self::Split { first, second, .. } => {
                first.swap(a, b);
                second.swap(a, b);
            }
        }
    }

    pub(super) fn remove(self, id: PaneId) -> Option<Self> {
        match self {
            Self::Pane(pane) => (pane.id != id).then_some(Self::Pane(pane)),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => match (first.remove(id), second.remove(id)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    axis,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(remaining), None) | (None, Some(remaining)) => Some(remaining),
                (None, None) => None,
            },
        }
    }

    fn resize_parent(&mut self, id: PaneId, share: SplitRatio) -> bool {
        match self {
            Self::Pane(_) => false,
            Self::Split {
                ratio,
                first,
                second,
                ..
            } => {
                if matches!(first.as_ref(), Self::Pane(pane) if pane.id == id)
                    || matches!(second.as_ref(), Self::Pane(pane) if pane.id == id)
                {
                    *ratio = share;
                    true
                } else {
                    first.resize_parent(id, share) || second.resize_parent(id, share)
                }
            }
        }
    }
}
