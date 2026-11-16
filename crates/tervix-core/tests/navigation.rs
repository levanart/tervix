//! Directional focus/move/swap/resize commands and identity preservation.

mod support;

use proptest::prelude::*;
use support::{navigate, pane, split, three_panes};
use tervix_core::{
    Axis, Direction, Layout, LayoutCommand, LayoutError, LayoutNode, NavigationCommand, Size,
    SplitRatio,
};

#[test]
fn neighbors_use_overlap_and_deterministic_tie_breaking() -> Result<(), LayoutError> {
    let layout = three_panes()?;
    let geometry = layout.measure(Size::new(1000, 800)?, Size::new(80, 40)?)?;
    for (source, direction, expected) in [
        (1, Direction::Right, 2), // Equidistant centers: first visual leaf wins.
        (2, Direction::Down, 3),
        (3, Direction::Up, 2),
        (2, Direction::Left, 1),
        (3, Direction::Left, 1),
    ] {
        assert_eq!(
            geometry.neighbor(pane(source).id, direction)?,
            pane(expected).id
        );
    }
    assert_eq!(
        geometry.neighbor(pane(1).id, Direction::Up),
        Err(LayoutError::NoNeighbor)
    );
    assert_eq!(
        geometry.neighbor(pane(3).id, Direction::Right),
        Err(LayoutError::NoNeighbor)
    );
    assert_eq!(
        geometry.neighbor(pane(99).id, Direction::Right),
        Err(LayoutError::PaneNotFound)
    );
    let grid = layout.apply(split(1, 4, Axis::Horizontal))?;
    let geometry = grid.measure(Size::new(1000, 800)?, Size::new(80, 40)?)?;
    // Diagonally touching tile 3 must not win over tile 2 to the right of 1.
    assert_eq!(geometry.neighbor(pane(1).id, Direction::Right)?, pane(2).id);
    assert_eq!(geometry.neighbor(pane(4).id, Direction::Right)?, pane(3).id);
    Ok(())
}

#[test]
fn focus_and_swap_preserve_topology_and_view_pairs() -> Result<(), LayoutError> {
    let layout = three_panes()?;
    let focused = layout.apply(navigate(NavigationCommand::Focus(Direction::Up))?)?;
    assert_eq!(focused.focused(), pane(2).id);
    assert_eq!(focused.root(), layout.root());
    let swapped = layout.apply(navigate(NavigationCommand::Swap(Direction::Left))?)?;
    assert_eq!(swapped.panes(), vec![pane(3), pane(2), pane(1)]);
    assert_eq!(swapped.focused(), pane(3).id);
    assert_eq!(
        swapped.apply(LayoutCommand::Swap(pane(1).id, pane(3).id))?,
        layout
    );
    Ok(())
}

#[test]
fn moving_places_active_pane_on_requested_side_and_collapses_old_parent() -> Result<(), LayoutError>
{
    for direction in [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ] {
        let layout = match direction {
            Direction::Down | Direction::Right => {
                three_panes()?.apply(LayoutCommand::Focus(pane(1).id))?
            }
            Direction::Up | Direction::Left => three_panes()?,
        };
        // Pane 1 has no tile below; use the top-right tile for Down.
        let layout = if direction == Direction::Down {
            layout.apply(LayoutCommand::Focus(pane(2).id))?
        } else {
            layout
        };
        let active = layout.focused();
        let geometry = layout.measure(Size::new(1000, 800)?, Size::new(80, 40)?)?;
        let target = geometry.neighbor(active, direction)?;
        let moved = layout.apply(navigate(NavigationCommand::Move(direction))?)?;
        let mut original_panes = layout.panes();
        let mut moved_panes = moved.panes();
        original_panes.sort_by_key(|p| p.id);
        moved_panes.sort_by_key(|p| p.id);
        assert_eq!(original_panes, moved_panes);
        assert_eq!(moved.focused(), active);
        let geometry = moved.measure(Size::new(1000, 800)?, Size::new(80, 40)?)?;
        assert_eq!(geometry.neighbor(target, direction)?, active);
        moved.validate()?;
    }
    Ok(())
}

#[test]
fn directional_resize_finds_ancestor_boundary_and_clamps() -> Result<(), LayoutError> {
    let layout = three_panes()?;
    let resized = layout.apply(navigate(NavigationCommand::Resize {
        direction: Direction::Left,
        step_bps: 500,
    })?)?;
    let expected = LayoutNode::Split {
        axis: Axis::Vertical,
        ratio: SplitRatio::new(4500)?,
        first: Box::new(LayoutNode::Pane(pane(1))),
        second: Box::new(LayoutNode::Split {
            axis: Axis::Horizontal,
            ratio: SplitRatio::HALF,
            first: Box::new(LayoutNode::Pane(pane(2))),
            second: Box::new(LayoutNode::Pane(pane(3))),
        }),
    };
    assert_eq!(resized.root(), &expected);
    let up = layout.apply(navigate(NavigationCommand::Resize {
        direction: Direction::Up,
        step_bps: 8000,
    })?)?;
    let geometry = up.measure(Size::new(1000, 800)?, Size::new(1, 1)?)?;
    assert_eq!(geometry.panes()[1].rect.height, 80);
    assert_eq!(
        up.apply(navigate(NavigationCommand::Resize {
            direction: Direction::Up,
            step_bps: 8000
        })?)?,
        up
    );
    // From pane 1, pushing Right grows the root first share, not any nested split.
    let right = layout
        .apply(LayoutCommand::Focus(pane(1).id))?
        .apply(navigate(NavigationCommand::Resize {
            direction: Direction::Right,
            step_bps: 8000,
        })?)?;
    assert_eq!(
        right
            .measure(Size::new(1000, 800)?, Size::new(1, 1)?)?
            .panes()[0]
            .rect
            .width,
        900
    );
    let down = layout
        .apply(LayoutCommand::Focus(pane(2).id))?
        .apply(navigate(NavigationCommand::Resize {
            direction: Direction::Down,
            step_bps: 500,
        })?)?;
    assert_eq!(
        down.measure(Size::new(1000, 800)?, Size::new(1, 1)?)?
            .panes()[1]
            .rect
            .height,
        440
    );
    Ok(())
}

#[test]
fn edge_actions_and_invalid_steps_are_rejected_atomically() -> Result<(), LayoutError> {
    let layout = three_panes()?;
    for action in [
        NavigationCommand::Focus(Direction::Right),
        NavigationCommand::Swap(Direction::Right),
        NavigationCommand::Move(Direction::Right),
    ] {
        assert_eq!(
            layout.apply(navigate(action)?),
            Err(LayoutError::NoNeighbor)
        );
    }
    assert_eq!(
        layout.apply(navigate(NavigationCommand::Resize {
            direction: Direction::Right,
            step_bps: 500
        })?),
        Err(LayoutError::NoBoundary)
    );
    for step_bps in [0, 8001, u16::MAX] {
        assert_eq!(
            layout.apply(navigate(NavigationCommand::Resize {
                direction: Direction::Left,
                step_bps
            })?),
            Err(LayoutError::InvalidStep)
        );
    }
    let single = Layout::single(pane(1));
    assert_eq!(
        single.apply(navigate(NavigationCommand::Move(Direction::Up))?),
        Err(LayoutError::NoNeighbor)
    );
    assert_eq!(
        single.apply(navigate(NavigationCommand::Resize {
            direction: Direction::Up,
            step_bps: 100
        })?),
        Err(LayoutError::NoBoundary)
    );
    assert_eq!(layout, three_panes()?);
    Ok(())
}

#[test]
fn move_exceeding_depth_limit_rejects_whole_transaction() -> Result<(), LayoutError> {
    let mut layout = Layout::single(pane(1)).apply(split(1, 2, Axis::Vertical))?;
    for id in 3..=33 {
        layout = layout.apply(split(2, id, Axis::Vertical))?;
    }
    // Source 34 sits in a separate left subtree. Removing it does not reduce
    // the target leaf 2's depth of 32 on the right.
    layout = layout.apply(split(1, 34, Axis::Vertical))?;
    let before = layout.clone();
    assert_eq!(
        layout.apply(navigate(NavigationCommand::Move(Direction::Right))?),
        Err(LayoutError::ResourceLimit)
    );
    assert_eq!(layout, before);
    assert_eq!(layout.panes().len(), 34);
    Ok(())
}

proptest! {
    #[test]
    fn directional_sequences_never_lose_or_duplicate_view_instances(
        operations in prop::collection::vec((0_u8..4, 0_u8..4), 0..150)
    ) {
        let mut layout = three_panes().map_err(|e| TestCaseError::fail(e.to_string()))?;
        for (operation, direction) in operations {
            let direction = match direction { 0 => Direction::Up, 1 => Direction::Down, 2 => Direction::Left, _ => Direction::Right };
            let action = match operation {
                0 => NavigationCommand::Focus(direction),
                1 => NavigationCommand::Swap(direction),
                2 => NavigationCommand::Move(direction),
                _ => NavigationCommand::Resize { direction, step_bps: 1000 },
            };
            let command = navigate(action).map_err(|e| TestCaseError::fail(e.to_string()))?;
            match layout.apply(command) {
                Ok(next) => layout = next,
                Err(error) => prop_assert!(matches!(error, LayoutError::NoNeighbor | LayoutError::NoBoundary)),
            }
            let mut pairs = layout.panes();
            pairs.sort_by_key(|p| p.id);
            prop_assert_eq!(pairs, vec![pane(1), pane(2), pane(3)]);
            prop_assert!(layout.validate().is_ok());
        }
    }
}
