//! Domain invariants and model-based randomized command sequences.

use proptest::prelude::*;
use tervix_core::{
    Axis, Layout, LayoutCommand, LayoutError, LayoutNode, Pane, PaneId, SplitRatio, ViewId,
};
use uuid::Uuid;

fn pane(value: u128) -> Pane {
    Pane {
        id: PaneId::from_uuid(Uuid::from_u128(value)),
        view: ViewId::from_uuid(Uuid::from_u128(value + 10_000)),
    }
}

fn split(target: PaneId, new: Pane) -> LayoutCommand {
    LayoutCommand::Split {
        target,
        pane: new,
        axis: Axis::Vertical,
        ratio: SplitRatio::HALF,
    }
}

#[test]
fn recursive_split_close_and_focus() -> Result<(), LayoutError> {
    let original = Layout::single(pane(1));
    let nested = original
        .apply(split(pane(1).id, pane(2)))?
        .apply(split(pane(2).id, pane(3)))?;
    assert_eq!(nested.panes(), vec![pane(1), pane(2), pane(3)]);
    assert_eq!(nested.focused(), pane(3).id);
    let closed = nested.apply(LayoutCommand::Close(pane(3).id))?;
    assert_eq!(closed.panes(), vec![pane(1), pane(2)]);
    assert_eq!(closed.focused(), pane(1).id);
    assert_eq!(original.panes(), vec![pane(1)]);
    Ok(())
}

#[test]
fn failed_commands_are_atomic() {
    let layout = Layout::single(pane(1));
    for (command, error) in [
        (LayoutCommand::Close(pane(1).id), LayoutError::LastPane),
        (LayoutCommand::Focus(pane(9).id), LayoutError::PaneNotFound),
        (LayoutCommand::Close(pane(9).id), LayoutError::PaneNotFound),
        (
            LayoutCommand::Swap(pane(1).id, pane(9).id),
            LayoutError::PaneNotFound,
        ),
        (split(pane(1).id, pane(1)), LayoutError::DuplicatePane),
        (
            split(
                pane(1).id,
                Pane {
                    id: pane(2).id,
                    view: pane(1).view,
                },
            ),
            LayoutError::DuplicateView,
        ),
        (
            LayoutCommand::Resize {
                pane: pane(1).id,
                ratio: SplitRatio::HALF,
            },
            LayoutError::NoSplit,
        ),
    ] {
        assert_eq!(layout.apply(command), Err(error));
        assert_eq!(layout, Layout::single(pane(1)));
    }
}

#[test]
fn swap_preserves_view_identity_and_focus() -> Result<(), LayoutError> {
    let layout = Layout::single(pane(1))
        .apply(split(pane(1).id, pane(2)))?
        .apply(split(pane(2).id, pane(3)))?;
    let swapped = layout.apply(LayoutCommand::Swap(pane(1).id, pane(3).id))?;
    assert_eq!(swapped.panes(), vec![pane(3), pane(2), pane(1)]);
    assert_eq!(swapped.focused(), pane(3).id);
    assert_eq!(
        swapped.apply(LayoutCommand::Swap(pane(1).id, pane(3).id))?,
        layout
    );
    assert_eq!(
        layout.apply(LayoutCommand::Swap(pane(1).id, pane(1).id))?,
        layout
    );
    Ok(())
}

#[test]
fn resize_changes_only_immediate_parent() -> Result<(), LayoutError> {
    let layout = Layout::single(pane(1))
        .apply(split(pane(1).id, pane(2)))?
        .apply(split(pane(2).id, pane(3)))?;
    let ratio = SplitRatio::new(7_000)?;
    let resized = layout.apply(LayoutCommand::Resize {
        pane: pane(3).id,
        ratio,
    })?;
    let expected = LayoutNode::Split {
        axis: Axis::Vertical,
        ratio: SplitRatio::HALF,
        first: Box::new(LayoutNode::Pane(pane(1))),
        second: Box::new(LayoutNode::Split {
            axis: Axis::Vertical,
            ratio,
            first: Box::new(LayoutNode::Pane(pane(2))),
            second: Box::new(LayoutNode::Pane(pane(3))),
        }),
    };
    assert_eq!(resized.root(), &expected);
    assert_eq!(resized.panes(), layout.panes());
    Ok(())
}

#[test]
fn raw_tree_validation_rejects_duplicates_and_unreachable_focus() {
    let duplicate = LayoutNode::Split {
        axis: Axis::Horizontal,
        ratio: SplitRatio::HALF,
        first: Box::new(LayoutNode::Pane(pane(1))),
        second: Box::new(LayoutNode::Pane(pane(1))),
    };
    assert_eq!(
        Layout::new(duplicate, pane(1).id),
        Err(LayoutError::DuplicatePane)
    );
    assert_eq!(
        Layout::new(LayoutNode::Pane(pane(1)), pane(2).id),
        Err(LayoutError::PaneNotFound)
    );
    for value in [0, 999, 9001, u16::MAX] {
        assert_eq!(SplitRatio::new(value), Err(LayoutError::InvalidRatio));
    }
    assert!(SplitRatio::new(1000).is_ok());
    assert!(SplitRatio::new(9000).is_ok());
}

#[test]
fn excessive_depth_is_rejected_without_committing() -> Result<(), LayoutError> {
    let mut layout = Layout::single(pane(1));
    for value in 2..=33 {
        layout = layout.apply(split(pane(value - 1).id, pane(value)))?;
    }
    assert_eq!(
        layout.apply(split(pane(33).id, pane(34))),
        Err(LayoutError::ResourceLimit)
    );
    assert_eq!(layout.panes().len(), 33);
    Ok(())
}

proptest! {
    #[test]
    fn random_commands_match_tile_order_model(operations in prop::collection::vec((0_u8..5, any::<usize>()), 0..150)) {
        let mut layout = Layout::single(pane(1));
        let mut expected = vec![pane(1)];
        let mut serial = 2;
        for (operation, index) in operations {
            let position = index % expected.len();
            let target = expected[position].id;
            let command = match operation {
                0 => split(target, pane(serial)),
                1 => LayoutCommand::Close(target),
                2 => LayoutCommand::Swap(target, expected[(position + 1) % expected.len()].id),
                3 => LayoutCommand::Focus(target),
                _ => LayoutCommand::Resize { pane: target, ratio: SplitRatio::HALF },
            };
            let before = layout.clone();
            match layout.apply(command) {
                Ok(next) => {
                    match operation {
                        0 => { expected.insert(position + 1, pane(serial)); serial += 1; }
                        1 => { expected.remove(position); }
                        2 => { let other = (position + 1) % expected.len(); expected.swap(position, other); }
                        _ => {}
                    }
                    layout = next;
                }
                Err(error) => {
                    prop_assert!(matches!(error, LayoutError::LastPane | LayoutError::NoSplit | LayoutError::ResourceLimit));
                    prop_assert_eq!(&layout, &before);
                }
            }
            prop_assert_eq!(layout.panes(), expected.clone());
            prop_assert!(layout.validate().is_ok());
            prop_assert!(expected.iter().any(|pane| pane.id == layout.focused()));
        }
    }
}
