use super::*;
use crate::workspace::{self, Action, Message, PaneView};
use tervix_application::Axis;

fn workspace() -> Workspace {
    let (mut state, _) = workspace::initialize();
    state.reduced_motion = true;
    state.configuring = true;
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let _ = workspace::update(&mut state, Message::Action(Action::Split(axis)));
    }
    state.configuring = true;
    state
}
fn begin(state: &mut Workspace) {
    state.dock_pointer(Point::new(20., 20.));
    state.start_dock(state.controller.layout().panes()[0].id);
}
#[allow(clippy::unwrap_used)] // Valid measured fixture.
fn target(state: &Workspace, x: f32, y: f32) -> Point {
    let rect = state.geometry().unwrap().panes()[1].rect;
    Point::new(
        rect.x as f32 + rect.width as f32 * x,
        rect.y as f32 + rect.height as f32 * y,
    )
}
#[test]
#[allow(clippy::unwrap_used)] // Valid measured fixture and proposal.
fn every_tool_can_swap_or_dock_without_preview_mutation_or_focus_change() {
    for view in PaneView::ALL {
        for (x, y, side) in [
            (0.5, 0.5, None),
            (0.01, 0.5, Some(Direction::Left)),
            (0.99, 0.5, Some(Direction::Right)),
            (0.5, 0.01, Some(Direction::Up)),
            (0.5, 0.99, Some(Direction::Down)),
        ] {
            let mut state = workspace();
            let source = state.controller.layout().panes()[0];
            state.views.insert(source.view, view);
            let before = state.controller.layout().clone();
            let undo = state.controller.undo_depth();
            let history: Vec<_> = state.controller.focus_history().collect();
            begin(&mut state);
            state.dock_pointer(target(&state, x, y));
            assert_eq!(state.controller.layout(), &before);
            assert_eq!(state.controller.undo_depth(), undo);
            assert_eq!(
                state.controller.focus_history().collect::<Vec<_>>(),
                history
            );
            let preview = state.docking.as_ref().unwrap().preview.as_ref().unwrap();
            assert_eq!(
                matches!(preview.command, LayoutCommand::MovePane { .. }),
                side.is_some()
            );
            let expected = before.apply(preview.command).unwrap();
            state.release_dock();
            assert_eq!(state.controller.layout(), &expected);
            assert_eq!(state.controller.undo_depth(), undo + 1);
            assert_eq!(state.controller.layout().focused(), before.focused());
            assert_eq!(state.views.get(&source.view), Some(&view));
            state.controller.dispatch(Command::Undo).unwrap();
            assert_eq!(state.controller.layout(), &before);
        }
    }
}
#[test]
#[allow(clippy::unwrap_used)] // Existing distinct fixture panes.
fn cancellation_threshold_single_pane_and_stale_topology_are_side_effect_free() {
    for cancellation in [
        Message::Cancel,
        Message::Language(crate::i18n::Language::En),
        Message::Launcher(crate::launcher::Event::Open),
        Message::Resized(iced::Size::new(900., 600.)),
        Message::Action(Action::Configure),
    ] {
        let mut state = workspace();
        let before = state.controller.layout().clone();
        let undo = state.controller.undo_depth();
        let history: Vec<_> = state.controller.focus_history().collect();
        begin(&mut state);
        state.dock_pointer(target(&state, 0.5, 0.5));
        let _ = workspace::update(&mut state, cancellation);
        assert!(state.docking.is_none());
        let _ = workspace::update(&mut state, Message::DockRelease);
        assert_eq!(state.controller.layout(), &before);
        assert_eq!(state.controller.undo_depth(), undo);
        assert_eq!(
            state.controller.focus_history().collect::<Vec<_>>(),
            history
        );
    }
    let mut state = workspace();
    let before = state.controller.layout().clone();
    begin(&mut state);
    state.dock_pointer(Point::new(22., 22.));
    state.release_dock();
    assert_eq!(state.controller.layout(), &before);
    begin(&mut state);
    state.dock_pointer(target(&state, 0.5, 0.5));
    state.pending_close = Some(before.focused());
    state.release_dock();
    assert_eq!(state.controller.layout(), &before);
    state.pending_close = None;
    begin(&mut state);
    state.dock_pointer(target(&state, 0.5, 0.5));
    state
        .controller
        .dispatch(Command::Layout(LayoutCommand::Swap(
            before.panes()[0].id,
            before.panes()[1].id,
        )))
        .unwrap();
    let changed = state.controller.layout().clone();
    state.release_dock();
    assert_eq!(state.controller.layout(), &changed);
    let (mut single, _) = workspace::initialize();
    single.configuring = true;
    let before = single.controller.layout().clone();
    begin(&mut single);
    single.dock_pointer(Point::new(500., 200.));
    single.release_dock();
    assert_eq!(single.controller.layout(), &before);
    assert_eq!(single.controller.undo_depth(), 0);
}

#[test]
fn moving_geometry_and_stretch_disable_start_and_commit_animates_only_after_release() {
    let mut state = workspace();
    // A queued press from a header now displaced elsewhere is not a valid start.
    state.dock_pointer(Point::new(200.0, 200.0));
    state.start_dock(state.controller.layout().panes()[0].id);
    assert!(state.docking.is_none());
    state.reduced_motion = false;
    let _ = workspace::update(&mut state, Message::Action(Action::Split(Axis::Horizontal)));
    begin(&mut state);
    assert!(state.docking.is_none());
    let _ = workspace::update(
        &mut state,
        Message::Frame(std::time::Instant::now() + std::time::Duration::from_secs(1)),
    );
    state.zoomed = Some(state.controller.layout().focused());
    begin(&mut state);
    assert!(state.docking.is_none());
    state.zoomed = None;
    begin(&mut state);
    state.dock_pointer(target(&state, 0.5, 0.5));
    assert!(state.geometry_motion.is_none());
    state.release_dock();
    assert!(state.geometry_motion.is_some());
}

#[test]
fn actual_presentation_header_events_commit_one_drag_for_every_tool()
-> Result<(), Box<dyn std::error::Error>> {
    use iced::advanced::{Layout, Shell, layout, mouse, widget::Tree};
    let renderer = crate::test_renderer::renderer(iced::Font::default());
    for view in PaneView::ALL {
        let mut state = workspace();
        let source = state.controller.layout().panes()[0];
        state.views.insert(source.view, view);
        let before = state.controller.layout().clone();
        let undo = state.controller.undo_depth();
        let mut tree = Tree::new(workspace::view(&state));
        let target = target(&state, 0.5, 0.5);
        for (event, point) in [
            (
                mouse::Event::ButtonPressed(mouse::Button::Left),
                Point::new(45., 137.),
            ),
            (
                mouse::Event::CursorMoved {
                    position: Point::new(target.x + 16., target.y + 108.),
                },
                Point::new(target.x + 16., target.y + 108.),
            ),
            (
                mouse::Event::ButtonReleased(mouse::Button::Left),
                Point::new(target.x + 16., target.y + 108.),
            ),
        ] {
            let messages = {
                let mut element = workspace::view(&state);
                tree.diff(&element);
                let node = element.as_widget().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, state.window_size),
                );
                let mut messages = Vec::new();
                element.as_widget_mut().on_event(
                    &mut tree,
                    iced::Event::Mouse(event),
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &Rectangle::with_size(state.window_size),
                );
                messages
            };
            assert!(
                !messages
                    .iter()
                    .any(|m| matches!(m, Message::PanePressed(_) | Message::TerminalInput(_))),
                "{view:?}: {messages:?}"
            );
            for message in messages {
                let _ = workspace::update(&mut state, message);
            }
        }
        assert_ne!(state.controller.layout(), &before, "{view:?}");
        assert_eq!(state.controller.undo_depth(), undo + 1);
        assert_eq!(state.controller.layout().focused(), before.focused());
        assert!(state.docking.is_none());
    }
    Ok(())
}
