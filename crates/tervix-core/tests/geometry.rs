//! Logical geometry, minimum tile sizes and deterministic explicit overflow.

mod support;

use proptest::prelude::*;
use support::{navigate, pane, split, three_panes};
use tervix_core::{
    Axis, Direction, Layout, LayoutCommand, LayoutError, NavigationCommand, Rect, Size, SplitRatio,
};

#[test]
fn odd_dimensions_tile_exactly_without_rounding_gaps() -> Result<(), LayoutError> {
    let layout = three_panes()?;
    let before = layout.clone();
    let geometry = layout.measure(Size::new(1001, 801)?, Size::new(80, 40)?)?;
    let rects: Vec<_> = geometry.panes().iter().map(|tile| tile.rect).collect();
    assert_eq!(
        rects,
        vec![
            Rect {
                x: 0,
                y: 0,
                width: 500,
                height: 801
            },
            Rect {
                x: 500,
                y: 0,
                width: 501,
                height: 400
            },
            Rect {
                x: 500,
                y: 400,
                width: 501,
                height: 401
            },
        ]
    );
    assert_eq!(geometry.canvas(), geometry.viewport());
    assert!(!geometry.overflows_x());
    assert!(!geometry.overflows_y());
    assert_eq!(layout, before);
    Ok(())
}

#[test]
fn tiny_windows_expand_canvas_without_hiding_or_zeroing_panes() -> Result<(), LayoutError> {
    let layout = three_panes()?;
    let geometry = layout.measure(Size::new(1, 1)?, Size::new(80, 40)?)?;
    assert_eq!(geometry.canvas(), Size::new(160, 80)?);
    assert!(geometry.overflows_x());
    assert!(geometry.overflows_y());
    assert_eq!(
        geometry.panes().iter().map(|p| p.pane).collect::<Vec<_>>(),
        layout.panes()
    );
    assert!(
        geometry
            .panes()
            .iter()
            .all(|p| p.rect.width >= 80 && p.rect.height >= 40)
    );
    // Neighbors remain available beyond the visible viewport.
    assert_eq!(geometry.neighbor(pane(3).id, Direction::Up)?, pane(2).id);
    assert_eq!(
        layout
            .measure(Size::new(1000, 1)?, Size::new(80, 40)?)?
            .canvas(),
        Size::new(1000, 80)?
    );
    Ok(())
}

#[test]
fn subtree_minima_override_ratio_only_in_projection() -> Result<(), LayoutError> {
    let layout = three_panes()?.apply(LayoutCommand::Resize {
        pane: pane(1).id,
        ratio: SplitRatio::new(1000)?,
    })?;
    let geometry = layout.measure(Size::new(200, 100)?, Size::new(80, 40)?)?;
    assert_eq!(geometry.panes()[0].rect.width, 80);
    assert_eq!(geometry.panes()[1].rect.width, 120);
    // Larger viewport restores the requested ratio; no persisted ratio drift.
    assert_eq!(
        layout
            .measure(Size::new(2000, 100)?, Size::new(80, 40)?)?
            .panes()[0]
            .rect
            .width,
        200
    );
    Ok(())
}

#[test]
fn maximum_coordinates_are_safe_and_unrepresentable_minima_fail() -> Result<(), LayoutError> {
    assert_eq!(Size::new(0, 1), Err(LayoutError::InvalidSize));
    assert_eq!(Size::new(1, 0), Err(LayoutError::InvalidSize));
    let layout = three_panes()?;
    let geometry = layout.measure(Size::new(u32::MAX, u32::MAX)?, Size::new(1, 1)?)?;
    assert_eq!(geometry.neighbor(pane(3).id, Direction::Left)?, pane(1).id);
    assert_eq!(
        geometry
            .panes()
            .iter()
            .map(|tile| u64::from(tile.rect.width) * u64::from(tile.rect.height))
            .sum::<u64>(),
        u64::from(u32::MAX).pow(2)
    );
    assert_eq!(
        layout.measure(Size::new(1, 1)?, Size::new(u32::MAX, 1)?),
        Err(LayoutError::GeometryOverflow)
    );
    assert_eq!(
        layout.measure(Size::new(1, 1)?, Size::new(1, u32::MAX)?),
        Err(LayoutError::GeometryOverflow)
    );
    Ok(())
}

proptest! {
    #[test]
    fn generated_layouts_cover_canvas_once_and_keep_minimum_sizes(
        operations in prop::collection::vec((any::<usize>(), any::<bool>(), 1000_u16..=9000), 0..40),
        width in 1_u32..10000, height in 1_u32..10000,
        min_width in 1_u32..300, min_height in 1_u32..150,
    ) {
        let mut layout = Layout::single(pane(1));
        for (index, (selection, vertical, ratio)) in operations.into_iter().enumerate() {
            let panes = layout.panes();
            let target = panes[selection % panes.len()].id;
            let ratio = SplitRatio::new(ratio).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let command = LayoutCommand::Split { target, pane: pane(index as u128 + 2),
                axis: if vertical { Axis::Vertical } else { Axis::Horizontal }, ratio };
            match layout.apply(command) {
                Ok(next) => layout = next,
                Err(error) => prop_assert_eq!(error, LayoutError::ResourceLimit),
            }
        }
        let viewport = Size::new(width, height).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let minimum = Size::new(min_width, min_height).map_err(|e| TestCaseError::fail(e.to_string()))?;
        let before = layout.clone();
        let geometry = layout.measure(viewport, minimum).map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert_eq!(&layout, &before);
        prop_assert_eq!(geometry.panes().iter().map(|p| p.pane).collect::<Vec<_>>(), layout.panes());
        let mut area = 0_u64;
        for (i, tile) in geometry.panes().iter().enumerate() {
            let a = tile.rect;
            prop_assert!(a.width >= min_width && a.height >= min_height);
            prop_assert!(a.x + a.width <= geometry.canvas().width());
            prop_assert!(a.y + a.height <= geometry.canvas().height());
            area += u64::from(a.width) * u64::from(a.height);
            for other in &geometry.panes()[i + 1..] {
                let b = other.rect;
                prop_assert!(a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y);
            }
        }
        prop_assert_eq!(area, u64::from(geometry.canvas().width()) * u64::from(geometry.canvas().height()));
    }
}

#[test]
fn movement_remeasures_geometry_without_reassigning_views() -> Result<(), LayoutError> {
    let layout = three_panes()?;
    let moved = layout.apply(navigate(NavigationCommand::Move(Direction::Left))?)?;
    let geometry = moved.measure(Size::new(1000, 800)?, Size::new(80, 40)?)?;
    assert_eq!(geometry.panes()[0].pane, pane(3));
    assert_eq!(
        geometry.panes()[0].rect,
        Rect {
            x: 0,
            y: 0,
            width: 250,
            height: 800
        }
    );
    assert_eq!(geometry.panes()[1].pane, pane(1));
    assert_eq!(geometry.panes()[2].pane, pane(2));
    // Moving is structural, unlike exchanging identities at existing rectangles.
    assert_eq!(layout.panes(), vec![pane(1), pane(2), pane(3)]);
    // Verify the fixture helper can also place another split along either axis.
    assert_eq!(moved.apply(split(2, 4, Axis::Horizontal))?.panes().len(), 4);
    Ok(())
}

#[test]
fn every_recursive_boundary_is_addressable_without_changing_other_splits() -> Result<(), LayoutError>
{
    // Both root children are splits, so no immediate-parent resize can reach root.
    let layout = three_panes()?
        .apply(split(1, 4, Axis::Horizontal))?
        .apply(split(4, 5, Axis::Vertical))?;
    let viewport = Size::new(1001, 801)?;
    let minimum = Size::new(10, 10)?;
    let geometry = layout.measure(viewport, minimum)?;
    assert_eq!(geometry.boundaries().len(), layout.panes().len() - 1);
    let root = geometry.boundaries()[0];
    assert_eq!(
        root.rect,
        Rect {
            x: 0,
            y: 0,
            width: 1001,
            height: 801
        }
    );
    assert_eq!(root.axis, Axis::Vertical);
    assert_eq!(root.position, 500);
    assert_eq!(root.anchor, pane(1).id);
    assert_eq!(root.ancestor_depth, 1);
    assert_eq!(geometry.boundaries()[2].ancestor_depth, 0);
    for (selected, boundary) in geometry.boundaries().iter().enumerate() {
        let changed = layout.apply(LayoutCommand::ResizeSplit {
            anchor: boundary.anchor,
            ancestor_depth: boundary.ancestor_depth,
            ratio: SplitRatio::new(6000)?,
            viewport,
            minimum,
        })?;
        assert_eq!(changed.panes(), layout.panes());
        assert_eq!(changed.focused(), layout.focused());
        for (index, actual) in changed
            .measure(viewport, minimum)?
            .boundaries()
            .iter()
            .enumerate()
        {
            assert_eq!(
                actual.ratio.basis_points(),
                if selected == index { 6000 } else { 5000 }
            );
        }
    }
    // An alternate anchor at a different leaf depth identifies the same root.
    let ratio = SplitRatio::new(6000)?;
    let resize = |anchor, ancestor_depth| LayoutCommand::ResizeSplit {
        anchor,
        ancestor_depth,
        ratio,
        viewport,
        minimum,
    };
    assert_eq!(
        layout.apply(resize(pane(5).id, 2))?,
        layout.apply(resize(root.anchor, root.ancestor_depth))?,
    );
    Ok(())
}

#[test]
fn explicit_resize_clamps_recursive_minima_in_both_axes() -> Result<(), LayoutError> {
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let layout = Layout::single(pane(1))
            .apply(split(1, 2, axis))?
            .apply(split(1, 3, axis))?
            .apply(split(2, 4, axis))?;
        let viewport = Size::new(500, 500)?;
        let minimum = Size::new(100, 100)?;
        for (requested, expected) in [(1000, 4000), (9000, 6019)] {
            let changed = layout.apply(LayoutCommand::ResizeSplit {
                anchor: pane(3).id,
                ancestor_depth: 1,
                ratio: SplitRatio::new(requested)?,
                viewport,
                minimum,
            })?;
            let measured = changed.measure(viewport, minimum)?;
            let root = measured.boundaries()[0];
            assert_eq!(root.ratio.basis_points(), expected);
            assert_eq!(root.position, if requested == 1000 { 200 } else { 300 });
            assert!(
                measured
                    .panes()
                    .iter()
                    .all(|p| p.rect.width >= 100 && p.rect.height >= 100)
            );
        }
    }
    Ok(())
}

#[test]
fn invalid_split_addresses_are_transactional_and_overflow_remains_explicit()
-> Result<(), LayoutError> {
    let layout = three_panes()?;
    let before = layout.clone();
    let viewport = Size::new(1, 1)?;
    let command = |anchor, ancestor_depth, minimum| LayoutCommand::ResizeSplit {
        anchor,
        ancestor_depth,
        ratio: SplitRatio::HALF,
        viewport,
        minimum,
    };
    let minimum = Size::new(80, 40)?;
    for (anchor, depth, error) in [
        (pane(99).id, 0, LayoutError::PaneNotFound),
        (pane(1).id, 1, LayoutError::NoSplit),
        (pane(3).id, u8::MAX, LayoutError::NoSplit),
    ] {
        assert_eq!(layout.apply(command(anchor, depth, minimum)), Err(error));
        assert_eq!(layout, before);
    }
    assert_eq!(
        Layout::single(pane(1)).apply(command(pane(1).id, 0, minimum)),
        Err(LayoutError::NoSplit)
    );
    assert_eq!(
        layout.apply(command(pane(1).id, 0, Size::new(u32::MAX, 1)?)),
        Err(LayoutError::GeometryOverflow)
    );
    let changed = layout.apply(command(pane(1).id, 0, minimum))?;
    let geometry = changed.measure(Size::new(1, 1)?, minimum)?;
    assert!(geometry.overflows_x() && geometry.overflows_y());
    assert_eq!(geometry.boundaries()[0].position, 80);
    assert_eq!(geometry.boundaries()[1].position, 40);
    Ok(())
}

#[test]
fn unrepresentable_minimum_ratio_still_projects_exact_minima() -> Result<(), LayoutError> {
    // One third cannot be expressed in basis points at this canvas resolution.
    let layout = Layout::single(pane(1))
        .apply(split(1, 2, Axis::Vertical))?
        .apply(split(2, 3, Axis::Vertical))?;
    let viewport = Size::new(1, 1)?;
    let minimum = Size::new(10000, 1)?;
    let changed = layout.apply(LayoutCommand::ResizeSplit {
        anchor: pane(1).id,
        ancestor_depth: 0,
        ratio: SplitRatio::new(9000)?,
        viewport,
        minimum,
    })?;
    let geometry = changed.measure(viewport, minimum)?;
    assert_eq!(geometry.boundaries()[0].ratio.basis_points(), 3334);
    assert_eq!(geometry.boundaries()[0].position, 10000);
    assert!(geometry.panes().iter().all(|p| p.rect.width == 10000));

    // Even the largest supported coordinate range safely clamps and measures.
    let huge = Size::new(u32::MAX, u32::MAX)?;
    let changed = layout.apply(LayoutCommand::ResizeSplit {
        anchor: pane(3).id,
        ancestor_depth: 1,
        ratio: SplitRatio::new(9000)?,
        viewport: huge,
        minimum: Size::new(1, 1)?,
    })?;
    assert_eq!(changed.measure(huge, Size::new(1, 1)?)?.canvas(), huge);
    Ok(())
}
