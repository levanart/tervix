//! Device-independent directional actions over measured recursive layouts.

use crate::{
    Axis, Layout, LayoutCommand, LayoutError, LayoutGeometry, LayoutNode, Pane, PaneId, Rect, Size,
    SplitRatio,
};

/// A physical direction in a tab's logical coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Decreasing y.
    Up,
    /// Increasing y.
    Down,
    /// Decreasing x.
    Left,
    /// Increasing x.
    Right,
}

impl Direction {
    fn axis(self) -> Axis {
        match self {
            Self::Up | Self::Down => Axis::Horizontal,
            Self::Left | Self::Right => Axis::Vertical,
        }
    }

    fn is_before(self) -> bool {
        matches!(self, Self::Up | Self::Left)
    }
}

/// Actions on the active pane. All preserve pane/view identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationCommand {
    /// Focus the nearest pane in this direction; no wrap at edges.
    Focus(Direction),
    /// Swap with the nearest pane, retaining focus on the active identity.
    Swap(Direction),
    /// Remove the active leaf, collapse its old parent and split the nearest
    /// target with equal shares, placing the active leaf on the indicated side.
    Move(Direction),
    /// Push the nearest ancestor boundary on this side by a positive basis-point
    /// step. Clamps to 10–90%; viewport minima are applied during measurement.
    Resize {
        /// Boundary side to push outward.
        direction: Direction,
        /// Positive delta out of 10,000; rejected outside 1..=8000.
        step_bps: u16,
    },
}

impl LayoutGeometry {
    /// Finds a directional neighbor with positive perpendicular overlap.
    /// Ranks by forward edge gap, then perpendicular center distance, then
    /// depth-first order. Corner-only contact does not count. No edge wrapping.
    /// Returns `PaneNotFound` for an unknown source, `NoNeighbor` at an edge.
    pub fn neighbor(&self, source: PaneId, direction: Direction) -> Result<PaneId, LayoutError> {
        let rect = self
            .panes()
            .iter()
            .find(|tile| tile.pane.id == source)
            .ok_or(LayoutError::PaneNotFound)?
            .rect;
        self.panes()
            .iter()
            .filter(|tile| tile.pane.id != source)
            .filter_map(|tile| {
                neighbor_rank(rect, tile.rect, direction).map(|rank| (rank, tile.pane.id))
            })
            // min_by_key returns the first equal key, keeping tree order deterministic.
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, id)| id)
            .ok_or(LayoutError::NoNeighbor)
    }
}

fn neighbor_rank(source: Rect, target: Rect, direction: Direction) -> Option<(u32, u64)> {
    let (s_start, s_len, t_start, t_len, s_perp, s_span, t_perp, t_span) = match direction.axis() {
        Axis::Vertical => (
            source.x,
            source.width,
            target.x,
            target.width,
            source.y,
            source.height,
            target.y,
            target.height,
        ),
        Axis::Horizontal => (
            source.y,
            source.height,
            target.y,
            target.height,
            source.x,
            source.width,
            target.x,
            target.width,
        ),
    };
    if s_perp >= t_perp + t_span || t_perp >= s_perp + s_span {
        return None;
    }
    let gap = if direction.is_before() {
        s_start.checked_sub(t_start + t_len)?
    } else {
        t_start.checked_sub(s_start + s_len)?
    };
    let center = (2 * u64::from(s_perp) + u64::from(s_span))
        .abs_diff(2 * u64::from(t_perp) + u64::from(t_span));
    Some((gap, center))
}

impl Layout {
    pub(crate) fn navigate(
        &self,
        action: NavigationCommand,
        viewport: Size,
        minimum: Size,
    ) -> Result<Self, LayoutError> {
        match action {
            NavigationCommand::Focus(direction) => {
                let target = self.neighbor_target(direction, viewport, minimum)?;
                self.apply(LayoutCommand::Focus(target))
            }
            NavigationCommand::Swap(direction) => {
                let target = self.neighbor_target(direction, viewport, minimum)?;
                self.apply(LayoutCommand::Swap(self.focused(), target))
            }
            NavigationCommand::Move(direction) => {
                let target = self.neighbor_target(direction, viewport, minimum)?;
                let active = self
                    .panes()
                    .into_iter()
                    .find(|pane| pane.id == self.focused())
                    .ok_or(LayoutError::PaneNotFound)?;
                let mut root = self
                    .root()
                    .clone()
                    .remove(active.id)
                    .ok_or(LayoutError::LastPane)?;
                insert_beside(&mut root, target, active, direction);
                Self::new(root, active.id)
            }
            NavigationCommand::Resize {
                direction,
                step_bps,
            } => {
                if !(1..=8000).contains(&step_bps) {
                    return Err(LayoutError::InvalidStep);
                }
                let mut root = self.root().clone();
                if !resize_boundary(&mut root, self.focused(), direction, step_bps)? {
                    return Err(LayoutError::NoBoundary);
                }
                Self::new(root, self.focused())
            }
        }
    }

    fn neighbor_target(
        &self,
        direction: Direction,
        viewport: Size,
        minimum: Size,
    ) -> Result<PaneId, LayoutError> {
        self.measure(viewport, minimum)?
            .neighbor(self.focused(), direction)
    }
}

fn insert_beside(node: &mut LayoutNode, target: PaneId, active: Pane, direction: Direction) {
    match node {
        LayoutNode::Pane(pane) if pane.id == target => {
            let existing = Box::new(node.clone());
            let moved = Box::new(LayoutNode::Pane(active));
            let (first, second) = if direction.is_before() {
                (moved, existing)
            } else {
                (existing, moved)
            };
            *node = LayoutNode::Split {
                axis: direction.axis(),
                ratio: SplitRatio::HALF,
                first,
                second,
            };
        }
        LayoutNode::Pane(_) => {}
        LayoutNode::Split { first, second, .. } => {
            insert_beside(first, target, active, direction);
            insert_beside(second, target, active, direction);
        }
    }
}

fn resize_boundary(
    node: &mut LayoutNode,
    active: PaneId,
    direction: Direction,
    step: u16,
) -> Result<bool, LayoutError> {
    match node {
        LayoutNode::Pane(_) => Ok(false),
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            // Deepest matching ancestor wins, not simply the immediate parent.
            if resize_boundary(first, active, direction, step)?
                || resize_boundary(second, active, direction, step)?
            {
                return Ok(true);
            }
            let on_inner_side = if direction.is_before() {
                contains(second, active)
            } else {
                contains(first, active)
            };
            if *axis != direction.axis() || !on_inner_side {
                return Ok(false);
            }
            let current = ratio.basis_points();
            let proposed = if direction.is_before() {
                current.saturating_sub(step)
            } else {
                current.saturating_add(step)
            };
            *ratio = SplitRatio::new(proposed.clamp(1000, 9000))?;
            Ok(true)
        }
    }
}

fn contains(node: &LayoutNode, active: PaneId) -> bool {
    match node {
        LayoutNode::Pane(pane) => pane.id == active,
        LayoutNode::Split { first, second, .. } => {
            contains(first, active) || contains(second, active)
        }
    }
}
