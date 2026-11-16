//! Toolkit window commands and injected native sizing. No Win32 types or calls in UI.
use crate::i18n::Key as K;
use crate::{
    design,
    workspace::{Message, Workspace},
};
use iced::{
    Alignment, Element, Length, Padding, Rectangle, Size, Task,
    widget::{self, button, column, container, mouse_area, row, text, tooltip},
};
use std::time::{Duration, Instant};

type ResizeWindow = for<'a> fn(iced::window::raw_window_handle::WindowHandle<'a>, u8) -> bool;

#[derive(Default)]
pub(super) struct Chrome {
    pub(super) icon: Option<widget::image::Handle>,
    pub(super) id: Option<iced::window::Id>,
    pub(super) maximized: bool,
    pub(super) pending_exit: bool,
    pub(super) shutting_down: bool,
    pub(super) solid: bool,
    pub(super) resize_window: Option<ResizeWindow>,
    last_press: Option<Instant>,
}

#[derive(Debug, Clone)]
pub(super) enum Event {
    Ready(Option<iced::window::Id>),
    PressTitle,
    Minimize,
    Maximize,
    Maximized(bool),
    Close,
    ConfirmExit(bool),
    Resize(u8),
    ResizeResult(bool),
    Solid(bool),
    SystemMenu,
    Moved,
}

pub(super) fn update(state: &mut Workspace, message: &Message) -> Option<Task<Message>> {
    if state.chrome.shutting_down {
        return Some(
            if matches!(message, Message::TerminalTick(_)) && state.terminals.shutdown() {
                iced::exit()
            } else {
                Task::none()
            },
        );
    }
    if let Message::TerminalInput(bytes) = message
        && state.chrome.pending_exit
    {
        return Some(match bytes.as_slice() {
            b"\r" => confirm_exit(state, true),
            b"\x1b" => confirm_exit(state, false),
            _ => Task::none(),
        });
    }
    let Message::Chrome(event) = message else {
        if state.chrome.pending_exit
            && matches!(
                message,
                Message::Action(_)
                    | Message::PaneAction(_, _)
                    | Message::ChangeView(_, _)
                    | Message::Paste
                    | Message::Pasted(_, _)
                    | Message::SelectDistribution(_, _)
                    | Message::PanePressed(_)
                    | Message::Begin(_)
                    | Message::Restart(_)
                    | Message::ConfirmClose(_)
                    | Message::ConfirmPaste(_)
                    | Message::ProcessQuery(_, _)
                    | Message::ProcessSort(_, _)
                    | Message::History(_, _)
                    | Message::RefreshWsl
                    | Message::ReducedMotion(_)
            )
        {
            return Some(Task::none());
        }
        return None;
    };
    let id = state.chrome.id;
    Some(match event {
        Event::Moved => {
            state.chrome.last_press = None;
            Task::none()
        }
        Event::Ready(id) => {
            state.chrome.id = *id;
            Task::none()
        }
        Event::Solid(solid) => {
            state.chrome.solid = *solid;
            Task::none()
        }
        Event::Maximized(value) => {
            state.chrome.maximized = *value;
            Task::none()
        }
        Event::ConfirmExit(accept) => confirm_exit(state, *accept),
        Event::Close => {
            let active = state.controller.layout().panes().iter().any(|p| {
                state.terminals.get(p.view).is_some_and(|e| {
                    matches!(
                        e.snapshot.lifecycle,
                        tervix_application::terminal::Lifecycle::Starting
                            | tervix_application::terminal::Lifecycle::Running
                    )
                })
            });
            if active {
                // Modal routing consumes the release/Escape that would normally
                // finish an edit gesture. Restore it before installing the modal.
                state.cancel();
                state.chrome.pending_exit = true;
                state.pending_close = None;
                state.pending_paste = None;
                state.chrome.last_press = None;
                Task::none()
            } else {
                confirm_exit(state, true)
            }
        }
        Event::ResizeResult(ok) => {
            if !ok {
                state.status = K::NativeResizeUnavailable.into();
            }
            Task::none()
        }
        _ => {
            let Some(id) = id else {
                return Some(Task::none());
            };
            match event {
                Event::PressTitle => {
                    let now = Instant::now();
                    let double = state.chrome.last_press.take().is_some_and(|last| {
                        now.saturating_duration_since(last) <= Duration::from_millis(450)
                    });
                    if double {
                        iced::window::toggle_maximize(id)
                    } else {
                        state.chrome.last_press = Some(now);
                        iced::window::drag(id)
                    }
                }
                Event::Minimize => {
                    state.chrome.last_press = None;
                    iced::window::minimize(id, true)
                }
                Event::Maximize => {
                    state.chrome.last_press = None;
                    iced::window::toggle_maximize(id)
                }
                Event::SystemMenu => iced::window::show_system_menu(id),
                Event::Resize(edge) if !state.chrome.maximized => {
                    if let Some(resize) = state.chrome.resize_window {
                        let edge = *edge;
                        iced::window::run_with_handle(id, move |handle| resize(handle, edge))
                            .map(|ok| Message::Chrome(Event::ResizeResult(ok)))
                    } else {
                        Task::none()
                    }
                }
                _ => Task::none(),
            }
        }
    })
}

fn confirm_exit(state: &mut Workspace, accept: bool) -> Task<Message> {
    let pending = std::mem::take(&mut state.chrome.pending_exit);
    if accept
        && (pending
            || !state.controller.layout().panes().iter().any(|p| {
                state.terminals.get(p.view).is_some_and(|e| {
                    matches!(
                        e.snapshot.lifecycle,
                        tervix_application::terminal::Lifecycle::Starting
                            | tervix_application::terminal::Lifecycle::Running
                    )
                })
            }))
    {
        state.cancel();
        state.chrome.shutting_down = true;
        state.status = K::ClosingShells.into();
        if state.terminals.shutdown() {
            iced::exit()
        } else {
            Task::none()
        }
    } else {
        Task::none()
    }
}

fn window_button(
    icon: crate::icons::Icon,
    help: &'static str,
    event: Event,
    danger: bool,
) -> Element<'static, Message> {
    tooltip(
        button(container(crate::icons::view(icon, 18., design::TEXT)).center(Length::Fill))
            .width(42)
            .height(32)
            .padding(0)
            .style(move |theme, status| {
                if danger && matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    design::danger(theme, status)
                } else {
                    design::control(theme, status)
                }
            })
            .on_press(Message::Chrome(event)),
        container(text(help).size(12))
            .padding(8)
            .style(design::surface),
        tooltip::Position::Bottom,
    )
    .into()
}

pub(super) fn titlebar(state: &Workspace) -> Element<'_, Message> {
    let l = state.language;
    // Only this area drags: tools and system buttons are siblings, never children.
    let brand = row![
        widget::image(
            state
                .chrome
                .icon
                .clone()
                .unwrap_or_else(|| widget::image::Handle::from_rgba(1, 1, vec![0, 0, 0, 0]))
        )
        .width(32)
        .height(32),
        column![
            text("tervix").size(20),
            text(l.text(K::DeveloperWorkspace))
                .size(9)
                .color(design::MUTED)
        ]
        .spacing(1),
        container(text(l.text(K::LocalSession)).size(10).color(design::TEAL))
            .padding([5, 12])
            .style(design::surface),
        widget::horizontal_space(),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .height(44);
    let drag = mouse_area(brand)
        .on_press(Message::Chrome(Event::PressTitle))
        .on_right_press(Message::Chrome(Event::SystemMenu));
    row![
        drag,
        window_button(
            crate::icons::Icon::Minimize,
            l.text(K::Minimize),
            Event::Minimize,
            false
        ),
        window_button(
            if state.chrome.maximized {
                crate::icons::Icon::Restore
            } else {
                crate::icons::Icon::Maximize
            },
            l.text(K::Maximize),
            Event::Maximize,
            false
        ),
        window_button(
            crate::icons::Icon::Close,
            l.text(K::CloseWindow),
            Event::Close,
            true
        ),
    ]
    .spacing(5)
    .align_y(Alignment::Center)
    .height(52)
    .into()
}

/// Nonoverlapping logical hit regions; corners take precedence over edge strips.
fn edges(size: Size) -> [(u8, Rectangle); 8] {
    let (w, h, e, c) = (size.width, size.height, 6.0, 16.0);
    [
        (
            0,
            Rectangle {
                x: 0.0,
                y: c,
                width: e,
                height: (h - 2.0 * c).max(0.0),
            },
        ),
        (
            1,
            Rectangle {
                x: w - e,
                y: c,
                width: e,
                height: (h - 2.0 * c).max(0.0),
            },
        ),
        (
            2,
            Rectangle {
                x: c,
                y: 0.0,
                width: (w - 2.0 * c).max(0.0),
                height: e,
            },
        ),
        (
            3,
            Rectangle {
                x: c,
                y: h - e,
                width: (w - 2.0 * c).max(0.0),
                height: e,
            },
        ),
        (
            4,
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: c,
                height: c,
            },
        ),
        (
            5,
            Rectangle {
                x: w - c,
                y: 0.0,
                width: c,
                height: c,
            },
        ),
        (
            6,
            Rectangle {
                x: 0.0,
                y: h - c,
                width: c,
                height: c,
            },
        ),
        (
            7,
            Rectangle {
                x: w - c,
                y: h - c,
                width: c,
                height: c,
            },
        ),
    ]
}

pub(super) fn frame<'a>(state: &Workspace, content: Element<'a, Message>) -> Element<'a, Message> {
    let mut stack = widget::Stack::new().push(content);
    if cfg!(windows) && !state.chrome.maximized {
        for (edge, rect) in edges(state.window_size) {
            let cursor = match edge {
                0 | 1 => iced::mouse::Interaction::ResizingHorizontally,
                2 | 3 => iced::mouse::Interaction::ResizingVertically,
                4 | 7 => iced::mouse::Interaction::ResizingDiagonallyDown,
                _ => iced::mouse::Interaction::ResizingDiagonallyUp,
            };
            stack = stack.push(
                container(
                    mouse_area(widget::Space::new(rect.width, rect.height))
                        .on_press(Message::Chrome(Event::Resize(edge)))
                        .interaction(cursor),
                )
                .padding(Padding {
                    top: rect.y,
                    left: rect.x,
                    right: 0.0,
                    bottom: 0.0,
                }),
            );
        }
    }
    stack.width(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Point;
    #[test]
    fn clicking_window_control_never_emits_a_titlebar_drag() {
        use iced::advanced::{Layout, Shell, layout, widget::Tree};
        let (state, _) = crate::workspace::initialize();
        let renderer = crate::test_renderer::renderer(iced::Font::default());
        let mut element = crate::workspace::view(&state);
        let mut tree = Tree::new(&element);
        let bounds = Rectangle::with_size(state.window_size);
        let node = element.as_widget().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, state.window_size),
        );
        let mut messages = Vec::new();
        for event in [
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
        ] {
            let _ = element.as_widget_mut().on_event(
                &mut tree,
                iced::Event::Mouse(event),
                Layout::new(&node),
                iced::mouse::Cursor::Available(Point::new(state.window_size.width - 84.0, 42.0)),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &bounds,
            );
        }
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Chrome(Event::Maximize)))
        );
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, Message::Chrome(Event::PressTitle)))
        );
    }
    #[test]
    fn eight_edges_cover_corners_without_stealing_controls_or_interior() {
        let edges = edges(Size::new(1200.0, 800.0));
        for (point, expected) in [
            (Point::new(1.0, 1.0), 4),
            (Point::new(1199.0, 1.0), 5),
            (Point::new(1.0, 799.0), 6),
            (Point::new(1199.0, 799.0), 7),
            (Point::new(1.0, 400.0), 0),
            (Point::new(1199.0, 400.0), 1),
            (Point::new(600.0, 1.0), 2),
            (Point::new(600.0, 799.0), 3),
        ] {
            let hits: Vec<_> = edges
                .iter()
                .filter(|(_, rect)| rect.contains(point))
                .map(|(edge, _)| *edge)
                .collect();
            assert_eq!(hits, vec![expected]);
        }
        for point in [Point::new(1150.0, 30.0), Point::new(600.0, 400.0)] {
            assert!(!edges.iter().any(|(_, r)| r.contains(point)));
        }
    }
    #[test]
    fn exit_modal_blocks_layout_and_escape_cancels_without_closing_pane() {
        let (mut state, _) = crate::workspace::initialize();
        state.chrome.pending_exit = true;
        let before = state.controller.layout().panes();
        let _ = crate::workspace::update(
            &mut state,
            Message::Action(crate::workspace::Action::Split(
                tervix_application::Axis::Vertical,
            )),
        );
        assert_eq!(state.controller.layout().panes(), before);
        let _ = crate::workspace::update(&mut state, Message::TerminalInput(vec![27]));
        assert!(!state.chrome.pending_exit);
        assert_eq!(state.controller.layout().panes(), before);
    }
}
