//! Integer geometry with explicit overflow instead of hidden or zero-sized tiles.

use crate::{Axis, Layout, LayoutError, LayoutNode, Pane, PaneId, SplitRatio};

/// Positive dimensions in caller-chosen logical units (not physical DPI pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    width: u32,
    height: u32,
}

impl Size {
    /// Rejects zero width or height with `LayoutError::InvalidSize`.
    pub fn new(width: u32, height: u32) -> Result<Self, LayoutError> {
        if width == 0 || height == 0 {
            return Err(LayoutError::InvalidSize);
        }
        Ok(Self { width, height })
    }

    /// Horizontal extent.
    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Vertical extent.
    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// Half-open rectangle in layout canvas coordinates, without gaps or decorations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Horizontal extent.
    pub width: u32,
    /// Vertical extent.
    pub height: u32,
}

/// A stable tile/view pair and its allocated rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneGeometry {
    /// Unchanged identity from the domain tree.
    pub pane: Pane,
    /// Tile bounds; includes space for future headers/borders.
    pub rect: Rect,
}

/// One recursive divider, in canvas coordinates. This Copy-compatible address
/// survives ratio changes, but must be rediscovered after topology changes.
/// No permanent split IDs or heap-allocated paths are needed for pointer gestures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitBoundary {
    /// Deterministic first descendant pane used to address this split.
    pub anchor: PaneId,
    /// Zero denotes the anchor's immediate parent.
    pub ancestor_depth: u8,
    /// Entire split allocation, used to convert pointer positions to ratios.
    pub rect: Rect,
    /// Vertical divides left/right; horizontal divides top/bottom.
    pub axis: Axis,
    /// Stored first-child ratio; actual position may be minimum-clamped.
    pub ratio: SplitRatio,
    /// Absolute x (vertical) or y (horizontal) of the actual divider line.
    /// Its perpendicular span is given by `rect`; UI chooses hit-test thickness.
    pub position: u32,
    /// Recursive minimum dimensions of the first subtree.
    pub first_minimum: Size,
    /// Recursive minimum dimensions of the second subtree.
    pub second_minimum: Size,
}

/// A complete non-overlapping tiling, possibly larger than its viewport.
/// A future UI must expose scroll/pan on overflowing axes, never clip silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutGeometry {
    viewport: Size,
    canvas: Size,
    panes: Vec<PaneGeometry>,
    boundaries: Vec<SplitBoundary>,
}

impl LayoutGeometry {
    /// Requested visible area.
    #[must_use]
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// Full allocated area required to keep every tile reachable.
    #[must_use]
    pub fn canvas(&self) -> Size {
        self.canvas
    }

    /// Whether horizontal scroll/pan is required.
    #[must_use]
    pub fn overflows_x(&self) -> bool {
        self.canvas.width > self.viewport.width
    }

    /// Whether vertical scroll/pan is required.
    #[must_use]
    pub fn overflows_y(&self) -> bool {
        self.canvas.height > self.viewport.height
    }

    /// Allocations in deterministic depth-first order.
    #[must_use]
    pub fn panes(&self) -> &[PaneGeometry] {
        &self.panes
    }

    /// All recursive split dividers in deterministic preorder (outer before inner).
    /// Coordinates include overflow; apply the same scroll transform as panes.
    #[must_use]
    pub fn boundaries(&self) -> &[SplitBoundary] {
        &self.boundaries
    }
}

impl Layout {
    /// Allocates every tile at least `minimum` units, expanding the canvas when
    /// necessary. Ratios are visually clamped to subtree minima, not rewritten.
    /// Returns `GeometryOverflow` if required dimensions exceed `u32::MAX`.
    /// Does not mutate the tree, focus, identities or persisted split ratios.
    pub fn measure(&self, viewport: Size, minimum: Size) -> Result<LayoutGeometry, LayoutError> {
        let measured = MeasuredNode::new(self.root(), minimum)?;
        let canvas = Size {
            width: viewport.width.max(measured.minimum.width),
            height: viewport.height.max(measured.minimum.height),
        };
        let mut panes = Vec::new();
        let mut boundaries = Vec::new();
        measured.allocate(
            Rect {
                x: 0,
                y: 0,
                width: canvas.width,
                height: canvas.height,
            },
            &mut panes,
            &mut boundaries,
        );
        Ok(LayoutGeometry {
            viewport,
            canvas,
            panes,
            boundaries,
        })
    }

    pub(crate) fn clamp_split_ratio(
        &self,
        anchor: PaneId,
        depth: u8,
        ratio: SplitRatio,
        viewport: Size,
        minimum: Size,
    ) -> Result<SplitRatio, LayoutError> {
        self.root()
            .pane_depth(anchor)
            .ok_or(LayoutError::PaneNotFound)?;
        let target = self
            .root()
            .ancestor(anchor, depth)
            .ok_or(LayoutError::NoSplit)?;
        let geometry = self.measure(viewport, minimum)?;
        let boundary = geometry
            .boundaries()
            .iter()
            .find(|boundary| {
                self.root()
                    .ancestor(boundary.anchor, boundary.ancestor_depth)
                    .is_some_and(|node| std::ptr::eq(node, target))
            })
            .ok_or(LayoutError::NoSplit)?;
        let (total, first, second) = match boundary.axis {
            Axis::Vertical => (
                boundary.rect.width,
                boundary.first_minimum.width,
                boundary.second_minimum.width,
            ),
            Axis::Horizontal => (
                boundary.rect.height,
                boundary.first_minimum.height,
                boundary.second_minimum.height,
            ),
        };
        // floor(total * ratio / 10000) must fit both recursive minima.
        let total = u64::from(total);
        let lower = (u64::from(first) * 10_000).div_ceil(total).max(1000);
        let upper = ((total - u64::from(second) + 1) * 10_000)
            .div_ceil(total)
            .saturating_sub(1)
            .min(9000);
        // Extreme subtree imbalance or integer quantization can leave no ratio
        // inside policy bounds. Measurement still enforces minima exactly; use
        // the nearest policy-valid ratio in that case.
        let share = if lower <= upper {
            u64::from(ratio.basis_points()).clamp(lower, upper)
        } else {
            lower.clamp(1000, 9000)
        };
        SplitRatio::new(share as u16)
    }
}

// Cache subtree minima once, so allocation is linear in the number of nodes.
struct MeasuredNode {
    minimum: Size,
    anchor: PaneId,
    anchor_distance: u8,
    content: MeasuredContent,
}

enum MeasuredContent {
    Pane(Pane),
    Split {
        axis: Axis,
        ratio: SplitRatio,
        first: Box<MeasuredNode>,
        second: Box<MeasuredNode>,
    },
}

impl MeasuredNode {
    fn new(node: &LayoutNode, leaf_minimum: Size) -> Result<Self, LayoutError> {
        match node {
            LayoutNode::Pane(pane) => Ok(Self {
                minimum: leaf_minimum,
                anchor: pane.id,
                anchor_distance: 0,
                content: MeasuredContent::Pane(*pane),
            }),
            LayoutNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let first = Box::new(Self::new(first, leaf_minimum)?);
                let second = Box::new(Self::new(second, leaf_minimum)?);
                let a = first.minimum;
                let b = second.minimum;
                let add = |x: u32, y: u32| x.checked_add(y).ok_or(LayoutError::GeometryOverflow);
                let minimum = match axis {
                    Axis::Vertical => Size {
                        width: add(a.width, b.width)?,
                        height: a.height.max(b.height),
                    },
                    Axis::Horizontal => Size {
                        width: a.width.max(b.width),
                        height: add(a.height, b.height)?,
                    },
                };
                Ok(Self {
                    minimum,
                    anchor: first.anchor,
                    anchor_distance: first.anchor_distance + 1,
                    content: MeasuredContent::Split {
                        axis: *axis,
                        ratio: *ratio,
                        first,
                        second,
                    },
                })
            }
        }
    }

    fn allocate(
        &self,
        rect: Rect,
        panes: &mut Vec<PaneGeometry>,
        boundaries: &mut Vec<SplitBoundary>,
    ) {
        match &self.content {
            MeasuredContent::Pane(pane) => panes.push(PaneGeometry { pane: *pane, rect }),
            MeasuredContent::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (total, first_min, second_min) = match axis {
                    Axis::Vertical => (rect.width, first.minimum.width, second.minimum.width),
                    Axis::Horizontal => (rect.height, first.minimum.height, second.minimum.height),
                };
                // Product fits u64; result cannot exceed total (ratio <= 9000).
                let desired = (u64::from(total) * u64::from(ratio.basis_points()) / 10_000) as u32;
                let share = desired.clamp(first_min, total - second_min);
                let (a, b) = match axis {
                    Axis::Vertical => (
                        Rect {
                            width: share,
                            ..rect
                        },
                        Rect {
                            x: rect.x + share,
                            width: total - share,
                            ..rect
                        },
                    ),
                    Axis::Horizontal => (
                        Rect {
                            height: share,
                            ..rect
                        },
                        Rect {
                            y: rect.y + share,
                            height: total - share,
                            ..rect
                        },
                    ),
                };
                boundaries.push(SplitBoundary {
                    anchor: self.anchor,
                    ancestor_depth: self.anchor_distance - 1,
                    rect,
                    axis: *axis,
                    ratio: *ratio,
                    position: match axis {
                        Axis::Vertical => b.x,
                        Axis::Horizontal => b.y,
                    },
                    first_minimum: first.minimum,
                    second_minimum: second.minimum,
                });
                first.allocate(a, panes, boundaries);
                second.allocate(b, panes, boundaries);
            }
        }
    }
}
