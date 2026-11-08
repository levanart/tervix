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
