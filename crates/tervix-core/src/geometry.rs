//! Integer geometry with explicit overflow instead of hidden or zero-sized tiles.

use crate::{Axis, Layout, LayoutError, LayoutNode, Pane, SplitRatio};

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

/// A complete non-overlapping tiling, possibly larger than its viewport.
/// A future UI must expose scroll/pan on overflowing axes, never clip silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutGeometry {
    viewport: Size,
    canvas: Size,
    panes: Vec<PaneGeometry>,
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
        measured.allocate(
            Rect {
                x: 0,
                y: 0,
                width: canvas.width,
                height: canvas.height,
            },
            &mut panes,
        );
        Ok(LayoutGeometry {
            viewport,
            canvas,
            panes,
        })
    }
}

// Cache subtree minima once, so allocation is linear in the number of nodes.
struct MeasuredNode {
    minimum: Size,
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

    fn allocate(&self, rect: Rect, panes: &mut Vec<PaneGeometry>) {
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
                first.allocate(a, panes);
                second.allocate(b, panes);
            }
        }
    }
}
