//! Pointer gestures are atomic with respect to undo and other commands.

use tervix_application::{
    Axis, Command, CommandError, Layout, LayoutCommand, LayoutController, LayoutError, Pane,
    PaneId, Size, SplitRatio, UNDO_LIMIT, ViewId,
};
use uuid::Uuid;

fn pane(id: u128) -> Pane {
    Pane {
        id: PaneId::from_uuid(Uuid::from_u128(id)),
        view: ViewId::from_uuid(Uuid::from_u128(id + 100)),
    }
}

fn controller() -> Result<LayoutController, CommandError> {
    let layout = Layout::single(pane(1)).apply(LayoutCommand::Split {
        target: pane(1).id,
        pane: pane(2),
        axis: Axis::Vertical,
        ratio: SplitRatio::HALF,
    })?;
    Ok(LayoutController::new(layout))
}

fn resize(share: u16) -> Result<LayoutCommand, CommandError> {
    Ok(LayoutCommand::Resize {
        pane: pane(1).id,
        ratio: SplitRatio::new(share)?,
    })
}

#[test]
fn many_previews_commit_as_one_exact_undo() -> Result<(), CommandError> {
    let mut controller = controller()?;
    let original = controller.layout().clone();
    let focus_history: Vec<_> = controller.focus_history().collect();
    controller.begin_resize()?;
    assert!(controller.resize_active());
    for share in [5500, 6000, 7000] {
        let command = resize(share)?;
        controller.preview_resize(command)?;
        assert_eq!(controller.layout(), &original.apply(command)?);
        assert_eq!(controller.undo_depth(), 0);
        assert_eq!(
            controller.focus_history().collect::<Vec<_>>(),
            focus_history
        );
    }
    let preview = controller.layout().clone();
    controller.commit_resize()?;
    assert!(!controller.resize_active());
    assert_eq!(controller.layout(), &preview);
    assert_eq!(controller.undo_depth(), 1);
    controller.dispatch(Command::Undo)?;
    assert_eq!(controller.layout(), &original);
    Ok(())
}

#[test]
fn cancel_preserves_existing_history_and_focus_history() -> Result<(), CommandError> {
    let mut controller = controller()?;
    controller.dispatch(Command::Layout(LayoutCommand::Focus(pane(1).id)))?;
    let before_resize = controller.layout().clone();
    controller.dispatch(Command::Layout(resize(6000)?))?;
    let origin = controller.layout().clone();
    let focus_history: Vec<_> = controller.focus_history().collect();
    controller.begin_resize()?;
    controller.preview_resize(resize(8000)?)?;
    controller.cancel_resize()?;
    assert!(!controller.resize_active());
    assert_eq!(controller.layout(), &origin);
    assert_eq!(controller.undo_depth(), 1);
    assert_eq!(
        controller.focus_history().collect::<Vec<_>>(),
        focus_history
    );
    controller.dispatch(Command::Undo)?;
    assert_eq!(controller.layout(), &before_resize);
    Ok(())
}

#[test]
fn empty_and_returned_to_origin_gestures_do_not_record_undo() -> Result<(), CommandError> {
    let mut controller = controller()?;
    let origin = controller.layout().clone();
    controller.begin_resize()?;
    controller.commit_resize()?;
    controller.begin_resize()?;
    controller.preview_resize(resize(7500)?)?;
    controller.preview_resize(resize(5000)?)?;
    controller.commit_resize()?;
    assert_eq!(controller.layout(), &origin);
    assert_eq!(controller.undo_depth(), 0);
    Ok(())
}

#[test]
fn invalid_previews_and_interleaved_actions_preserve_the_gesture() -> Result<(), CommandError> {
    let mut controller = controller()?;
    controller.begin_resize()?;
    controller.preview_resize(resize(6000)?)?;
    let preview = controller.layout().clone();
    let focus_history: Vec<_> = controller.focus_history().collect();
    assert_eq!(
        controller.begin_resize(),
        Err(CommandError::ResizeInProgress)
    );
    for command in [
        Command::Undo,
        Command::FocusPrevious,
        Command::Layout(LayoutCommand::Focus(pane(1).id)),
        Command::Layout(LayoutCommand::Close(pane(2).id)),
        Command::Layout(resize(7000)?),
    ] {
        assert_eq!(
            controller.dispatch(command),
            Err(CommandError::ResizeInProgress)
        );
    }
    assert_eq!(
        controller.preview_resize(LayoutCommand::Focus(pane(1).id)),
        Err(CommandError::NotResizeCommand)
    );
    assert_eq!(
        controller.preview_resize(LayoutCommand::Resize {
            pane: pane(99).id,
            ratio: SplitRatio::HALF,
        }),
        Err(CommandError::Layout(LayoutError::PaneNotFound))
    );
    assert_eq!(controller.layout(), &preview);
    assert_eq!(controller.undo_depth(), 0);
    assert_eq!(
        controller.focus_history().collect::<Vec<_>>(),
        focus_history
    );
    assert!(controller.resize_active());
    controller.preview_resize(resize(7000)?)?;
    controller.commit_resize()?;
    assert_eq!(controller.undo_depth(), 1);
    Ok(())
}

#[test]
fn idle_gesture_actions_fail_without_mutation() -> Result<(), CommandError> {
    let mut controller = controller()?;
    let origin = controller.layout().clone();
    assert_eq!(
        controller.preview_resize(resize(6000)?),
        Err(CommandError::NoResizeInProgress)
    );
    assert_eq!(
        controller.commit_resize(),
        Err(CommandError::NoResizeInProgress)
    );
    assert_eq!(
        controller.cancel_resize(),
        Err(CommandError::NoResizeInProgress)
    );
    assert_eq!(controller.layout(), &origin);
    assert_eq!(controller.undo_depth(), 0);
    Ok(())
}

#[test]
fn gesture_commit_respects_the_shared_undo_limit() -> Result<(), CommandError> {
    let mut controller = controller()?;
    for index in 0..UNDO_LIMIT + 5 {
        controller.begin_resize()?;
        controller.preview_resize(resize(if index % 2 == 0 { 6000 } else { 7000 })?)?;
        controller.commit_resize()?;
    }
    assert_eq!(controller.undo_depth(), UNDO_LIMIT);
    controller.begin_resize()?;
    controller.preview_resize(resize(8000)?)?;
    controller.cancel_resize()?;
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

#[test]
fn recursive_boundary_previews_commit_cancel_and_dispatch_share_validation()
-> Result<(), CommandError> {
    let mut controller = controller()?;
    for (target, id) in [(1, 3), (2, 4)] {
        controller.dispatch(Command::Layout(LayoutCommand::Split {
            target: pane(target).id,
            pane: pane(id),
            axis: Axis::Horizontal,
            ratio: SplitRatio::HALF,
        }))?;
    }
    let origin = controller.layout().clone();
    let history = controller.undo_depth();
    let focus: Vec<_> = controller.focus_history().collect();
    let viewport = Size::new(1000, 800)?;
    let minimum = Size::new(80, 40)?;
    let root = controller.layout().measure(viewport, minimum)?.boundaries()[0];
    assert_eq!(root.ancestor_depth, 1);
    let command = |share| -> Result<LayoutCommand, LayoutError> {
        Ok(LayoutCommand::ResizeSplit {
            anchor: root.anchor,
            ancestor_depth: root.ancestor_depth,
            ratio: SplitRatio::new(share)?,
            viewport,
            minimum,
        })
    };
    controller.begin_resize()?;
    for share in [6000, 7000, 8000] {
        controller.preview_resize(command(share)?)?;
    }
    let preview = controller.layout().clone();
    assert_eq!(controller.undo_depth(), history);
    assert_eq!(
        controller.preview_resize(LayoutCommand::ResizeSplit {
            anchor: root.anchor,
            ancestor_depth: u8::MAX,
            ratio: SplitRatio::HALF,
            viewport,
            minimum,
        }),
        Err(CommandError::Layout(LayoutError::NoSplit))
    );
    assert_eq!(controller.layout(), &preview);
    controller.commit_resize()?;
    assert_eq!(controller.undo_depth(), history + 1);
    controller.dispatch(Command::Undo)?;
    assert_eq!(controller.layout(), &origin);
    controller.begin_resize()?;
    controller.preview_resize(command(6000)?)?;
    controller.cancel_resize()?;
    assert_eq!(controller.layout(), &origin);
    assert_eq!(controller.focus_history().collect::<Vec<_>>(), focus);
    controller.dispatch(Command::Layout(command(7000)?))?;
    assert_eq!(controller.undo_depth(), history + 1);
    controller.dispatch(Command::Undo)?;
    assert_eq!(controller.layout(), &origin);
    Ok(())
}
