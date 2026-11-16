//! Explicit stable-target docking shares keyboard move invariants.
mod support;
use proptest::prelude::*;
use support::{pane, split, three_panes};
use tervix_core::{Axis, Direction, LayoutCommand, LayoutError, Size};

#[allow(clippy::unwrap_used)] // Fixed positive fixture dimensions.
fn command(source: u128, target: u128, side: Direction) -> LayoutCommand {
    LayoutCommand::MovePane {
        source: pane(source).id,
        target: pane(target).id,
        side,
        viewport: Size::new(100, 100).unwrap(),
        minimum: Size::new(300, 220).unwrap(),
    }
}

#[test]
fn explicit_target_preserves_unfocused_source_and_rejects_bad_addresses() -> Result<(), LayoutError>
{
    let layout = three_panes()?;
    for side in [
        Direction::Left,
        Direction::Right,
        Direction::Up,
        Direction::Down,
    ] {
        let moved = layout.apply(command(1, 3, side))?;
        assert_eq!(moved.focused(), layout.focused());
        let measured = moved.measure(Size::new(100, 100)?, Size::new(300, 220)?)?;
        assert_eq!(measured.neighbor(pane(3).id, side)?, pane(1).id);
        assert!(
            measured
                .panes()
                .iter()
                .all(|p| p.rect.width >= 300 && p.rect.height >= 220)
        );
    }
    assert_eq!(
        layout.apply(command(1, 1, Direction::Left)),
        Err(LayoutError::SamePane)
    );
    assert_eq!(
        layout.apply(command(99, 1, Direction::Left)),
        Err(LayoutError::PaneNotFound)
    );
    assert_eq!(
        layout.apply(command(1, 99, Direction::Left)),
        Err(LayoutError::PaneNotFound)
    );
    assert_eq!(layout, three_panes()?);
    Ok(())
}

#[test]
fn docking_rejects_depth_overflow_atomically() -> Result<(), LayoutError> {
    let mut layout = three_panes()?;
    layout = layout.apply(split(1, 4, Axis::Horizontal))?;
    let mut target = 3;
    for id in 5..=34 {
        layout = layout.apply(split(target, id, Axis::Horizontal))?;
        target = id;
    }
    let before = layout.clone();
    let result = layout.apply(LayoutCommand::MovePane {
        source: pane(1).id,
        target: pane(target).id,
        side: Direction::Left,
        viewport: Size::new(100, 100)?,
        minimum: Size::new(300, 220)?,
    });
    assert_eq!(result, Err(LayoutError::ResourceLimit));
    assert_eq!(layout, before);
    Ok(())
}

#[test]
fn directional_move_reuses_explicit_docking() -> Result<(), LayoutError> {
    let layout = three_panes()?;
    let measured = layout.measure(Size::new(1000, 800)?, Size::new(80, 40)?)?;
    let target = measured.neighbor(layout.focused(), Direction::Left)?;
    assert_eq!(
        layout.apply(support::navigate(tervix_core::NavigationCommand::Move(
            Direction::Left
        ))?)?,
        layout.apply(LayoutCommand::MovePane {
            source: layout.focused(),
            target,
            side: Direction::Left,
            viewport: Size::new(1000, 800)?,
            minimum: Size::new(80, 40)?
        })?
    );
    Ok(())
}

proptest! {
    #[test]
    fn arbitrary_explicit_moves_swaps_and_rejections_preserve_all_pairs(
        actions in prop::collection::vec((0u8..8, 0u8..8, 0u8..5), 0..150)
    ) {
        let mut layout = three_panes()?;
        for id in 4..=7 { layout = layout.apply(split(1,id,Axis::Vertical))?; }
        let mut expected = layout.panes(); expected.sort_by_key(|p| p.id);
        for (a,b,side) in actions {
            let before = layout.clone();
            let action = if side == 4 { LayoutCommand::Swap(pane(a as u128).id, pane(b as u128).id) }
                else { command(a as u128,b as u128,[Direction::Left,Direction::Right,Direction::Up,Direction::Down][side as usize]) };
            match layout.apply(action) { Ok(next) => layout = next, Err(_) => prop_assert_eq!(&layout,&before) }
            layout.validate()?;
            let mut actual = layout.panes(); actual.sort_by_key(|p| p.id);
            prop_assert_eq!(&actual,&expected);
            prop_assert_eq!(layout.focused(),before.focused());
        }
    }
}
