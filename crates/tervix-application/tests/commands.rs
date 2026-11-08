//! Transactional dispatch and bounded snapshot history contracts.

use proptest::prelude::*;
use tervix_application::{
    Axis, Command, CommandError, Direction, FOCUS_HISTORY_LIMIT, Layout, LayoutCommand,
    LayoutController, NavigationCommand, Pane, PaneId, Size, SplitRatio, UNDO_LIMIT, ViewId,
};
use uuid::Uuid;

fn pane(value: u128) -> Pane {
    Pane {
        id: PaneId::from_uuid(Uuid::from_u128(value)),
        view: ViewId::from_uuid(Uuid::from_u128(value + 1000)),
    }
}

fn split() -> Command {
    Command::Layout(LayoutCommand::Split {
        target: pane(1).id,
        pane: pane(2),
        axis: Axis::Vertical,
        ratio: SplitRatio::HALF,
    })
}

#[test]
fn undo_restores_exact_snapshot_and_failed_actions_preserve_history() -> Result<(), CommandError> {
    let initial = Layout::single(pane(1));
    let mut controller = LayoutController::new(initial.clone());
    assert_eq!(
        controller.dispatch(Command::Undo),
        Err(CommandError::NothingToUndo)
    );
    controller.dispatch(split())?;
    let before = controller.layout().clone();
    assert!(controller.dispatch(split()).is_err());
    assert_eq!(controller.layout(), &before);
    assert_eq!(controller.undo_depth(), 1);
    controller.dispatch(Command::Layout(LayoutCommand::Focus(pane(1).id)))?;
    assert_eq!(controller.undo_depth(), 1);
    controller.dispatch(Command::Undo)?;
    assert_eq!(controller.layout(), &initial);
    assert_eq!(controller.undo_depth(), 0);
    Ok(())
}

#[test]
fn history_is_bounded_and_noops_do_not_consume_it() -> Result<(), CommandError> {
    let mut controller = LayoutController::new(Layout::single(pane(1)));
    controller.dispatch(split())?;
    controller.dispatch(Command::Layout(LayoutCommand::Swap(pane(1).id, pane(1).id)))?;
    assert_eq!(controller.undo_depth(), 1);
    for _ in 0..100 {
        controller.dispatch(Command::Layout(LayoutCommand::Swap(pane(1).id, pane(2).id)))?;
    }
    assert_eq!(controller.undo_depth(), UNDO_LIMIT);
    for _ in 0..UNDO_LIMIT {
        controller.dispatch(Command::Undo)?;
    }
    assert_eq!(
        controller.dispatch(Command::Undo),
        Err(CommandError::NothingToUndo)
    );
    Ok(())
}

fn navigation(action: NavigationCommand) -> Result<Command, CommandError> {
    Ok(Command::Layout(LayoutCommand::Navigate {
        action,
        viewport: Size::new(1000, 800)?,
        minimum: Size::new(80, 40)?,
    }))
}

#[test]
fn directional_focus_and_previous_focus_do_not_consume_undo() -> Result<(), CommandError> {
    let mut controller = LayoutController::new(Layout::single(pane(1)));
    assert_eq!(
        controller.dispatch(Command::FocusPrevious),
        Err(CommandError::NoPreviousFocus)
    );
    controller.dispatch(split())?;
    assert_eq!(
        controller.focus_history().collect::<Vec<_>>(),
        vec![pane(1).id]
    );
    controller.dispatch(navigation(NavigationCommand::Focus(Direction::Left))?)?;
    assert_eq!(controller.layout().focused(), pane(1).id);
    assert_eq!(controller.undo_depth(), 1);
    assert_eq!(
        controller.focus_history().collect::<Vec<_>>(),
        vec![pane(2).id]
    );
    controller.dispatch(Command::FocusPrevious)?;
    assert_eq!(controller.layout().focused(), pane(2).id);
    controller.dispatch(Command::FocusPrevious)?;
    assert_eq!(controller.layout().focused(), pane(1).id);
    assert_eq!(controller.undo_depth(), 1);
    let history: Vec<_> = controller.focus_history().collect();
    assert!(
        controller
            .dispatch(navigation(NavigationCommand::Focus(Direction::Left))?)
            .is_err()
    );
    assert_eq!(controller.focus_history().collect::<Vec<_>>(), history);
    controller.dispatch(Command::Undo)?;
    assert_eq!(controller.layout().panes(), vec![pane(1)]);
    assert_eq!(controller.focus_history().count(), 0);
    assert_eq!(
        controller.dispatch(Command::FocusPrevious),
        Err(CommandError::NoPreviousFocus)
    );
    Ok(())
}

#[test]
fn closing_active_pane_prefers_recent_focus_and_undo_restores_exact_layout()
-> Result<(), CommandError> {
    let mut controller = LayoutController::new(Layout::single(pane(1)));
    controller.dispatch(split())?;
    controller.dispatch(Command::Layout(LayoutCommand::Split {
        target: pane(2).id,
        pane: pane(3),
        axis: Axis::Horizontal,
        ratio: SplitRatio::HALF,
    }))?;
    controller.dispatch(Command::Layout(LayoutCommand::Focus(pane(2).id)))?;
    controller.dispatch(Command::Layout(LayoutCommand::Focus(pane(3).id)))?;
    let before = controller.layout().clone();
    controller.dispatch(Command::Layout(LayoutCommand::Close(pane(3).id)))?;
    assert_eq!(controller.layout().focused(), pane(2).id);
    assert!(!controller.focus_history().any(|id| id == pane(3).id));
    controller.dispatch(Command::Undo)?;
    assert_eq!(controller.layout(), &before);
    assert_eq!(controller.focus_history().next_back(), Some(pane(2).id));
    Ok(())
}

#[test]
fn structural_directional_actions_are_undoable_but_clamped_noops_are_not()
-> Result<(), CommandError> {
    let mut controller = LayoutController::new(Layout::single(pane(1)));
    controller.dispatch(split())?;
    let before = controller.layout().clone();
    for action in [
        NavigationCommand::Move(Direction::Left),
        NavigationCommand::Swap(Direction::Left),
        NavigationCommand::Resize {
            direction: Direction::Left,
            step_bps: 500,
        },
    ] {
        controller.dispatch(navigation(action)?)?;
        assert_eq!(controller.undo_depth(), 2);
        controller.dispatch(Command::Undo)?;
        assert_eq!(controller.layout(), &before);
    }
    let resize = navigation(NavigationCommand::Resize {
        direction: Direction::Left,
        step_bps: 8000,
    })?;
    controller.dispatch(resize)?;
    let depth = controller.undo_depth();
    controller.dispatch(resize)?;
    assert_eq!(controller.undo_depth(), depth);
    Ok(())
}

#[test]
fn focus_history_is_bounded_unique_and_prunes_closed_panes() -> Result<(), CommandError> {
    let mut layout = Layout::single(pane(1));
    for id in 2..=40 {
        layout = layout.apply(LayoutCommand::Split {
            target: pane(id / 2).id,
            pane: pane(id),
            axis: Axis::Vertical,
            ratio: SplitRatio::HALF,
        })?;
    }
    let mut controller = LayoutController::new(layout);
    for id in 1..=40 {
        controller.dispatch(Command::Layout(LayoutCommand::Focus(pane(id).id)))?;
    }
    assert_eq!(controller.focus_history().count(), FOCUS_HISTORY_LIMIT);
    assert!(
        !controller
            .focus_history()
            .any(|id| id == controller.layout().focused())
    );
    let mut ids: Vec<_> = controller.focus_history().collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), FOCUS_HISTORY_LIMIT);
    controller.dispatch(Command::Layout(LayoutCommand::Close(pane(39).id)))?;
    assert!(!controller.focus_history().any(|id| id == pane(39).id));
    assert_eq!(controller.layout().focused(), pane(40).id);
    Ok(())
}

proptest! {
    #[test]
    fn mixed_layout_focus_close_and_undo_keep_histories_reachable(
        actions in prop::collection::vec((0_u8..9, any::<usize>(), 0_u8..4), 0..150)
    ) {
        let mut controller = LayoutController::new(Layout::single(pane(1)));
        let mut serial = 2;
        let mut snapshots: Vec<Layout> = Vec::new();
        for (operation, index, direction) in actions {
            let direction = match direction { 0 => Direction::Up, 1 => Direction::Down, 2 => Direction::Left, _ => Direction::Right };
            let panes = controller.layout().panes();
            let selected = panes[index % panes.len()].id;
            let command = match operation {
                0 => { let fresh = pane(serial); serial += 1;
                    Command::Layout(LayoutCommand::Split { target: selected, pane: fresh,
                        axis: if index % 2 == 0 { Axis::Vertical } else { Axis::Horizontal }, ratio: SplitRatio::HALF }) }
                1 => Command::Layout(LayoutCommand::Close(selected)),
                2 => Command::Layout(LayoutCommand::Focus(selected)),
                3 => Command::FocusPrevious,
                4 => Command::Undo,
                _ => {
                    let action = match operation {
                        5 => NavigationCommand::Focus(direction),
                        6 => NavigationCommand::Swap(direction),
                        7 => NavigationCommand::Move(direction),
                        _ => NavigationCommand::Resize { direction, step_bps: 1000 },
                    };
                    navigation(action).map_err(|e| TestCaseError::fail(e.to_string()))?
                }
            };
            let before = controller.layout().clone();
            let focus_before: Vec<_> = controller.focus_history().collect();
            let depth_before = controller.undo_depth();
            match controller.dispatch(command) {
                Ok(()) => {
                    if operation == 4 {
                        let expected = snapshots.pop();
                        prop_assert_eq!(expected.as_ref(), Some(controller.layout()));
                    } else if matches!(operation, 0 | 1 | 6 | 7 | 8) && controller.layout() != &before {
                        snapshots.push(before);
                        if snapshots.len() > UNDO_LIMIT { snapshots.remove(0); }
                    }
                }
                Err(_) => {
                    prop_assert_eq!(controller.layout(), &before);
                    prop_assert_eq!(controller.focus_history().collect::<Vec<_>>(), focus_before);
                    prop_assert_eq!(controller.undo_depth(), depth_before);
                }
            }
            prop_assert_eq!(controller.undo_depth(), snapshots.len());
            prop_assert!(controller.layout().validate().is_ok());
            let panes = controller.layout().panes();
            let history: Vec<_> = controller.focus_history().collect();
            prop_assert!(history.len() <= FOCUS_HISTORY_LIMIT);
            prop_assert!(history.iter().all(|id| *id != controller.layout().focused() && panes.iter().any(|p| p.id == *id)));
            let mut unique = history.clone(); unique.sort(); unique.dedup();
            prop_assert_eq!(unique.len(), history.len());
        }
    }

    #[test]
    fn arbitrary_swaps_and_undo_match_snapshot_model(actions in prop::collection::vec(any::<bool>(), 0..200)) {
        let initial = Layout::single(pane(1));
        let mut controller = LayoutController::new(initial);
        prop_assert!(controller.dispatch(split()).is_ok());
        let mut model = controller.layout().clone();
        let mut history = vec![];
        for undo in actions {
            if undo && !history.is_empty() {
                prop_assert!(controller.dispatch(Command::Undo).is_ok());
                if let Some(previous) = history.pop() { model = previous; }
            } else {
                let swap = LayoutCommand::Swap(pane(1).id, pane(2).id);
                let next = model.apply(swap);
                prop_assert!(next.is_ok());
                history.push(model.clone());
                if history.len() > UNDO_LIMIT { history.remove(0); }
                if let Ok(next) = next { model = next; }
                prop_assert!(controller.dispatch(Command::Layout(swap)).is_ok());
            }
            prop_assert_eq!(controller.layout(), &model);
            prop_assert!(controller.undo_depth() <= UNDO_LIMIT);
        }
    }
}
