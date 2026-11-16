//! Workspace ownership, selection and identity-preserving page transfers.

use std::collections::BTreeSet;

use proptest::prelude::*;
use tervix_core::{
    Axis, Layout, LayoutCommand, LayoutError, Pane, PaneId, SplitRatio, Tab, TabId, ViewId, Window,
    WindowId, Workspace, WorkspaceCommand as Command, WorkspaceError as Error, WorkspaceId,
};
use uuid::Uuid;

fn window_id(n: u128) -> WindowId {
    WindowId::from_uuid(Uuid::from_u128(n))
}

fn tab_id(n: u128) -> TabId {
    TabId::from_uuid(Uuid::from_u128(n))
}

fn pane(n: u128) -> Pane {
    Pane {
        id: PaneId::from_uuid(Uuid::from_u128(n)),
        view: ViewId::from_uuid(Uuid::from_u128(n)),
    }
}

fn tab(n: u128) -> Tab {
    Tab::new(tab_id(n), Layout::single(pane(n)))
}

fn initial() -> Result<Workspace, Error> {
    Workspace::new(
        WorkspaceId::from_uuid(Uuid::from_u128(1)),
        "Development".into(),
        Window::new(window_id(1), tab(1)),
    )
}

fn pages() -> Result<Workspace, Error> {
    initial()?
        .apply(Command::AddTab {
            window: window_id(1),
            tab: tab(2),
        })?
        .apply(Command::AddTab {
            window: window_id(1),
            tab: tab(3),
        })?
        .apply(Command::AddWindow(Window::new(window_id(2), tab(4))))
}

#[test]
fn switching_windows_and_tabs_retains_each_layout_focus() -> Result<(), Error> {
    let layout = Layout::single(pane(1)).apply(LayoutCommand::Split {
        target: pane(1).id,
        pane: pane(10),
        axis: Axis::Vertical,
        ratio: SplitRatio::HALF,
    })?;
    let workspace = Workspace::new(
        WorkspaceId::from_uuid(Uuid::from_u128(1)),
        "Development".into(),
        Window::new(window_id(1), Tab::new(tab_id(1), layout.clone())),
    )?
    .apply(Command::AddTab {
        window: window_id(1),
        tab: tab(2),
    })?
    .apply(Command::AddWindow(Window::new(window_id(2), tab(3))))?
    .apply(Command::SelectTab(tab_id(1)))?;
    assert_eq!(workspace.active(), window_id(2));
    let workspace = workspace.apply(Command::Activate(window_id(1)))?;
    assert_eq!(workspace.windows()[0].selected(), tab_id(1));
    assert_eq!(workspace.windows()[0].tabs()[0].layout(), &layout);
    assert_eq!(workspace.windows()[1].selected(), tab_id(3));
    Ok(())
}

#[test]
fn duplicate_identities_are_rejected_across_windows() -> Result<(), Error> {
    let workspace = initial()?;
    for (window, error) in [
        (Window::new(window_id(1), tab(2)), Error::DuplicateWindow),
        (Window::new(window_id(2), tab(1)), Error::DuplicateTab),
        (
            Window::new(
                window_id(2),
                Tab::new(
                    tab_id(2),
                    Layout::single(Pane {
                        id: pane(1).id,
                        view: pane(2).view,
                    }),
                ),
            ),
            Error::Layout(LayoutError::DuplicatePane),
        ),
        (
            Window::new(
                window_id(2),
                Tab::new(
                    tab_id(2),
                    Layout::single(Pane {
                        id: pane(2).id,
                        view: pane(1).view,
                    }),
                ),
            ),
            Error::Layout(LayoutError::DuplicateView),
        ),
    ] {
        let before = workspace.clone();
        assert_eq!(workspace.apply(Command::AddWindow(window)), Err(error));
        assert_eq!(workspace, before);
    }
    Ok(())
}

#[test]
fn layout_edits_cannot_reuse_ids_from_inactive_tabs() -> Result<(), Error> {
    let workspace = pages()?;
    let before = workspace.clone();
    let result = workspace.apply(Command::Layout {
        tab: tab_id(1),
        command: LayoutCommand::Split {
            target: pane(1).id,
            pane: pane(4),
            axis: Axis::Horizontal,
            ratio: SplitRatio::HALF,
        },
    });
    assert_eq!(result, Err(Error::Layout(LayoutError::DuplicatePane)));
    assert_eq!(workspace, before);
    let result = workspace.apply(Command::Layout {
        tab: tab_id(1),
        command: LayoutCommand::Split {
            target: pane(1).id,
            pane: Pane {
                id: pane(10).id,
                view: pane(4).view,
            },
            axis: Axis::Vertical,
            ratio: SplitRatio::HALF,
        },
    });
    assert_eq!(result, Err(Error::Layout(LayoutError::DuplicateView)));
    assert_eq!(workspace, before);
    Ok(())
}

#[test]
fn transfer_preserves_full_layout_and_selects_destination() -> Result<(), Error> {
    let workspace = pages()?.apply(Command::Layout {
        tab: tab_id(3),
        command: LayoutCommand::Split {
            target: pane(3).id,
            pane: pane(10),
            axis: Axis::Horizontal,
            ratio: SplitRatio::new(3_000)?,
        },
    })?;
    let moved = workspace.windows()[0].tabs()[2].clone();
    let next = workspace.apply(Command::MoveTab {
        tab: tab_id(3),
        window: window_id(2),
        index: 0,
    })?;
    assert_eq!(next.windows()[1].tabs()[0], moved);
    assert_eq!(next.windows()[1].selected(), moved.id());
    assert_eq!(next.windows()[0].selected(), tab_id(2));
    assert_eq!(next.active(), workspace.active());
    assert_eq!(next.windows()[0].tabs().len(), 2);
    Ok(())
}

#[test]
fn reorder_uses_post_removal_index_and_preserves_selection() -> Result<(), Error> {
    let workspace = pages()?;
    let next = workspace.apply(Command::MoveTab {
        tab: tab_id(1),
        window: window_id(1),
        index: 2,
    })?;
    assert_eq!(
        next.windows()[0]
            .tabs()
            .iter()
            .map(Tab::id)
            .collect::<Vec<_>>(),
        vec![tab_id(2), tab_id(3), tab_id(1)]
    );
    assert_eq!(next.windows()[0].selected(), tab_id(3));
    let unchanged = next.apply(Command::MoveTab {
        tab: tab_id(3),
        window: window_id(1),
        index: 1,
    })?;
    assert_eq!(unchanged, next);
    let single = initial()?;
    assert_eq!(
        single.apply(Command::MoveTab {
            tab: tab_id(1),
            window: window_id(1),
            index: 0,
        })?,
        single
    );
    Ok(())
}

#[test]
fn close_uses_adjacent_fallback_and_retains_nonselected_focus() -> Result<(), Error> {
    let workspace = pages()?.apply(Command::SelectTab(tab_id(2)))?;
    let closed = workspace.apply(Command::CloseTab(tab_id(2)))?;
    assert_eq!(closed.windows()[0].selected(), tab_id(3));
    let closed = closed.apply(Command::CloseTab(tab_id(1)))?;
    assert_eq!(closed.windows()[0].selected(), tab_id(3));
    let closed = closed.apply(Command::CloseWindow(window_id(2)))?;
    assert_eq!(closed.active(), window_id(1));
    assert_eq!(closed.windows()[0].selected(), tab_id(3));
    let left = pages()?.apply(Command::CloseTab(tab_id(3)))?;
    assert_eq!(left.windows()[0].selected(), tab_id(2));
    Ok(())
}

#[test]
fn invalid_commands_preserve_the_entire_composition() -> Result<(), Error> {
    let workspace = initial()?;
    for (command, error) in [
        (Command::CloseWindow(window_id(1)), Error::LastWindow),
        (Command::CloseTab(tab_id(1)), Error::LastTab),
        (Command::CloseTab(tab_id(9)), Error::TabNotFound),
        (Command::Activate(window_id(9)), Error::WindowNotFound),
        (Command::SelectTab(tab_id(9)), Error::TabNotFound),
        (
            Command::MoveTab {
                tab: tab_id(1),
                window: window_id(1),
                index: 1,
            },
            Error::InvalidIndex,
        ),
        (
            Command::AddTab {
                window: window_id(9),
                tab: tab(2),
            },
            Error::WindowNotFound,
        ),
    ] {
        let before = workspace.clone();
        assert_eq!(workspace.apply(command), Err(error));
        assert_eq!(workspace, before);
    }
    let workspace = pages()?;
    assert_eq!(
        workspace.apply(Command::MoveTab {
            tab: tab_id(4),
            window: window_id(1),
            index: 0,
        }),
        Err(Error::LastTab)
    );
    assert_eq!(
        workspace.apply(Command::MoveTab {
            tab: tab_id(3),
            window: window_id(2),
            index: 2,
        }),
        Err(Error::InvalidIndex)
    );
    Ok(())
}

#[test]
fn names_and_resource_limits_are_validated() -> Result<(), Error> {
    assert_eq!(
        Workspace::new(
            WorkspaceId::from_uuid(Uuid::from_u128(1)),
            " \t\n".into(),
            Window::new(window_id(1), tab(1))
        ),
        Err(Error::InvalidName)
    );
    let mut workspace = initial()?;
    for n in 2..=32 {
        workspace = workspace.apply(Command::AddWindow(Window::new(window_id(n), tab(n))))?;
    }
    assert_eq!(
        workspace.apply(Command::AddWindow(Window::new(window_id(33), tab(33)))),
        Err(Error::ResourceLimit)
    );
    let mut workspace = initial()?;
    for n in 2..=256 {
        workspace = workspace.apply(Command::AddTab {
            window: window_id(1),
            tab: tab(n),
        })?;
    }
    assert_eq!(
        workspace.apply(Command::AddTab {
            window: window_id(1),
            tab: tab(257)
        }),
        Err(Error::ResourceLimit)
    );
    Ok(())
}

proptest! {
    #[test]
    fn random_reorders_transfers_and_selections_preserve_pages(
        actions in prop::collection::vec((0u8..5, 1u128..5, 1u128..3, 0usize..6), 0..100)
    ) {
        let mut workspace = pages()?;
        let originals = workspace.windows().iter().flat_map(|w| w.tabs().iter().cloned())
            .map(|t| (t.id(), t)).collect::<std::collections::BTreeMap<_, _>>();
        for (action, id, window, index) in actions {
            let before = workspace.clone();
            let command = match action {
                0 => Command::MoveTab { tab: tab_id(id), window: window_id(window), index },
                1 => Command::SelectTab(tab_id(id)),
                2 => Command::Activate(window_id(window)),
                3 => Command::AddTab { window: window_id(window), tab: tab(id) },
                _ => Command::Layout { tab: tab_id(id), command: LayoutCommand::Focus(pane(99).id) },
            };
            match workspace.apply(command) {
                Ok(next) => workspace = next,
                Err(_) => prop_assert_eq!(&workspace, &before),
            }
            workspace.validate()?;
            let mut ids = BTreeSet::new();
            for window in workspace.windows() {
                prop_assert!(window.tabs().iter().any(|t| t.id() == window.selected()));
                for tab in window.tabs() {
                    prop_assert!(ids.insert(tab.id()));
                    prop_assert_eq!(Some(tab), originals.get(&tab.id()));
                }
            }
            prop_assert_eq!(ids.len(), originals.len());
        }
    }
}
