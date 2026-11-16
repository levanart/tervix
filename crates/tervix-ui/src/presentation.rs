//! Pure workspace presentation; pane and divider coordinates share one scroll canvas.

use iced::widget::{
    self, Stack, button, checkbox, column, container, mouse_area, pick_list, row, scrollable, text,
};
use iced::{Alignment, Border, Color, Element, Length, Padding, Theme};
use tervix_application::{Axis, Pane, PaneId, SplitBoundary};

use crate::i18n::{Choice, Key as K, Language, Noun};
use crate::workspace::{Action, Message, PaneView, Workspace};
use crate::{chrome, design};

const MUTED: Color = design::MUTED;
const BLUE: Color = design::BLUE;
const EDIT: Color = Color::from_rgb(100.0 / 255.0, 224.0 / 255.0, 192.0 / 255.0);

fn control(label: &'static str, message: Message) -> widget::Button<'static, Message> {
    let icon = match &message {
        Message::Action(Action::Configure) => Some(crate::icons::Icon::Move),
        Message::PaneAction(_, Action::Split(Axis::Vertical)) => {
            Some(crate::icons::Icon::SplitVertical)
        }
        Message::PaneAction(_, Action::Split(Axis::Horizontal)) => {
            Some(crate::icons::Icon::SplitHorizontal)
        }
        Message::PaneAction(_, Action::Close) => Some(crate::icons::Icon::Close),
        _ => None,
    };
    let mut content = row![].spacing(5).align_y(Alignment::Center);
    if let Some(icon) = icon {
        content = content.push(crate::icons::view(icon, 14., design::TEXT));
    }
    button(
        content.push(
            text(label)
                .size(12)
                .shaping(iced::widget::text::Shaping::Advanced),
        ),
    )
    .padding([6, 10])
    .style(design::control)
    .on_press(message)
}

pub(super) fn view(state: &Workspace) -> Element<'_, Message> {
    let l = state.language;
    let compact = state.window_size.width < 1100.0;
    let accent = if state.configuring { EDIT } else { BLUE };
    let mode = control(
        if compact && state.configuring {
            l.text(K::ConfigureCompactOn)
        } else if compact {
            l.text(K::ConfigureCompact)
        } else if state.configuring {
            l.text(K::ConfigureOn)
        } else {
            l.text(K::Configure)
        },
        Message::Action(Action::Configure),
    )
    .style(move |theme, status| {
        let mut style = design::selected(theme, status);
        style.border = Border {
            color: accent,
            width: 1.0,
            radius: 7.0.into(),
        };
        style
    });
    let header = chrome::titlebar(state);
    let mut undo = control(l.text(K::Undo), Message::Action(Action::Undo));
    if state.controller.undo_depth() == 0 {
        undo = undo.on_press_maybe(None);
    }
    let mode_hint = if state.configuring {
        l.text(K::EditHint)
    } else {
        l.text(K::WorkspaceHint)
    };
    let mut tools = row![].align_y(Alignment::Center).spacing(8);
    // Reserve natural widths for actions; only the explanatory hint may wrap.
    if compact {
        tools = tools.push(widget::horizontal_space());
    } else {
        tools = tools
            .push(
                container(text("01").size(11).color(design::TEAL))
                    .padding([5, 8])
                    .style(design::surface),
            )
            .push(text(mode_hint).size(12).color(accent).width(Length::Fill));
    }
    tools = tools
        .push(control(
            l.text(K::Settings),
            Message::Launcher(crate::launcher::Event::Settings),
        ))
        .push(language_picker(l))
        .push(mode);
    if state.configuring && state.zoomed.is_some() {
        tools = tools.push(control(
            l.text(if compact {
                K::RestoreTilesCompact
            } else {
                K::RestoreTiles
            }),
            Message::Action(Action::Zoom),
        ));
    }
    if state.configuring {
        tools = tools.push(undo);
    }
    let tools = tools.width(Length::Fill).height(28);
    let tools = row![crate::launcher::toolbar_button(l), tools]
        .spacing(8)
        .height(28);
    let canvas = canvas(state);
    let footer = row![
        text(state.status.render(l)).size(11).color(MUTED),
        widget::horizontal_space(),
        text(l.count(state.controller.layout().panes().len(), Noun::Pane))
            .size(11)
            .color(MUTED),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .height(14);
    // 32 padding + 52 header + 28 toolbar + 14 footer + 3*6 gaps = 144.
    let solid = state.chrome.solid;
    let maximized = state.chrome.maximized;
    let workspace = container(column![header, tools, canvas, footer].spacing(6))
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(
                Color {
                    a: if solid { 1.0 } else { 0.94 },
                    ..design::CANVAS
                }
                .into(),
            ),
            border: Border {
                color: design::BORDER,
                width: 1.0,
                radius: if maximized { 0.0 } else { 16.0 }.into(),
            },
            ..Default::default()
        });
    let workspace = chrome::frame(state, workspace.into());
    if state.pending_close.is_some() || state.pending_paste.is_some() || state.chrome.pending_exit {
        let exiting = state.chrome.pending_exit;
        let closing = state.pending_close.is_some() || exiting;
        let dialog = container(
            column![
                text(if exiting {
                    l.text(K::CloseWorkspaceQuestion)
                } else if closing {
                    l.text(K::CloseTerminalQuestion)
                } else {
                    l.text(K::PasteQuestion)
                })
                .size(22),
                text(if exiting {
                    l.text(K::CloseWorkspaceDetail)
                } else if closing {
                    l.text(K::CloseTerminalDetail)
                } else {
                    l.text(K::PasteDetail)
                })
                .size(14),
                language_picker(l),
                text(l.text(K::ConfirmHint)).size(12).color(MUTED),
                row![
                    control(
                        l.text(K::Cancel),
                        if exiting {
                            Message::Chrome(chrome::Event::ConfirmExit(false))
                        } else if closing {
                            Message::ConfirmClose(false)
                        } else {
                            Message::ConfirmPaste(false)
                        }
                    )
                    .style(design::control),
                    control(
                        if exiting {
                            l.text(K::CloseWorkspace)
                        } else if closing {
                            l.text(K::TerminateShell)
                        } else {
                            l.text(K::Paste)
                        },
                        if exiting {
                            Message::Chrome(chrome::Event::ConfirmExit(true))
                        } else if closing {
                            Message::ConfirmClose(true)
                        } else {
                            Message::ConfirmPaste(true)
                        }
                    )
                    .style(design::danger)
                ]
                .spacing(10),
            ]
            .spacing(16),
        )
        .padding(24)
        .width(440)
        .style(|theme| {
            let mut style = design::surface(theme);
            style.shadow = design::shadow();
            style
        });
        Stack::new()
            .push(workspace)
            .push(
                mouse_area(
                    container(dialog)
                        .center(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(design::SCRIM.into()),
                            ..Default::default()
                        }),
                )
                .on_press(Message::TerminalInput(Vec::new())),
            )
            .into()
    } else if state.pending_wsl.is_some() {
        let dialog = container(
            column![
                text(l.text(K::ConfirmWsl)).size(16),
                row![
                    button(l.text(K::Cancel)).on_press(Message::ConfirmWsl(false)),
                    button(l.text(K::Confirm)).on_press(Message::ConfirmWsl(true))
                ]
                .spacing(8)
            ]
            .spacing(12),
        )
        .padding(24)
        .width((state.window_size.width - 64.0).min(540.0))
        .style(design::surface);
        Stack::new()
            .push(workspace)
            .push(
                mouse_area(
                    container(dialog)
                        .center(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(design::SCRIM.into()),
                            ..Default::default()
                        }),
                )
                .on_press(Message::TerminalInput(Vec::new())),
            )
            .into()
    } else if let Some(dialog) = crate::docker_view::confirmation(state) {
        Stack::new()
            .push(workspace)
            .push(
                mouse_area(
                    container(dialog)
                        .center(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(design::SCRIM.into()),
                            ..Default::default()
                        }),
                )
                .on_press(Message::TerminalInput(Vec::new())),
            )
            .into()
    } else {
        crate::launcher::overlay(state, workspace)
    }
}

fn canvas(state: &Workspace) -> Element<'_, Message> {
    let Ok(geometry) = state.geometry() else {
        return text(state.language.text(K::LayoutMeasureError)).into();
    };
    let mut width = if state.zoomed.is_some() {
        geometry.viewport().width()
    } else {
        geometry.canvas().width()
    } as f32;
    let mut height = if state.zoomed.is_some() {
        geometry.viewport().height()
    } else {
        geometry.canvas().height()
    } as f32;
    let mut layers = Stack::new().push(widget::Space::new(1, 1));
    let mut keys = vec![None];
    // Stable domain IDs determine geometry; the active tile draws last during
    // topology/stretch transitions, where valid-size allocations can overlap.
    let focused = state.controller.layout().focused();
    let panes = state.controller.layout().panes();
    for pane in panes
        .iter()
        .filter(|p| p.id != focused)
        .chain(panes.iter().filter(|p| p.id == focused))
    {
        if state.zoomed.is_some_and(|id| id != pane.id) {
            continue;
        }
        let rect = state
            .geometry_motion
            .as_ref()
            .and_then(|motion| motion.rectangle(pane.id))
            .or_else(|| {
                geometry
                    .panes()
                    .iter()
                    .find(|tile| tile.pane.id == pane.id)
                    .map(|tile| {
                        if state.zoomed == Some(pane.id) {
                            iced::Rectangle {
                                x: 0.0,
                                y: 0.0,
                                width: geometry.viewport().width().max(300) as f32,
                                height: geometry.viewport().height().max(220) as f32,
                            }
                        } else {
                            iced::Rectangle {
                                x: tile.rect.x as f32,
                                y: tile.rect.y as f32,
                                width: tile.rect.width as f32,
                                height: tile.rect.height as f32,
                            }
                        }
                    })
            });
        if let Some(rect) = rect {
            width = width.max(rect.x + rect.width);
            height = height.max(rect.y + rect.height);
            layers = layers.push(
                container(render_pane(state, *pane, rect.width, rect.height)).padding(Padding {
                    top: rect.y,
                    left: rect.x,
                    right: 0.0,
                    bottom: 0.0,
                }),
            );
            keys.push(Some(pane.id));
        }
    }
    // A moving divider must never advertise a hit target at its final position.
    // Pointer tracking itself is immediate; handles return after settling.
    if state.configuring && state.zoomed.is_none() && state.geometry_motion.is_none() {
        for boundary in geometry.boundaries() {
            layers = layers.push(divider(state, *boundary));
            keys.push(None);
        }
    }
    if let Some(preview) = state.docking.as_ref().and_then(|d| d.preview.as_ref()) {
        let r = preview.rect;
        layers = layers.push(
            container(
                container(text(state.language.text(preview.label)).size(16))
                    .center_x(r.width)
                    .center_y(r.height)
                    .style(|_| container::Style {
                        background: Some(Color::from_rgba8(43, 157, 142, 0.30).into()),
                        border: Border {
                            color: EDIT,
                            width: 3.0,
                            radius: 10.0.into(),
                        },
                        ..Default::default()
                    }),
            )
            .padding(Padding {
                top: r.y,
                left: r.x,
                right: 0.,
                bottom: 0.,
            }),
        );
        keys.push(None);
    }
    if state.docking.as_ref().is_some_and(|d| d.preview.is_none())
        && let Some(point) = state.dock_point
    {
        layers = layers.push(
            container(
                container(text(state.language.text(K::DockInvalid)).size(12))
                    .padding(8)
                    .width(260)
                    .style(design::surface),
            )
            .padding(Padding {
                top: (point.y + 20.).min((height - 64.).max(0.)),
                left: point.x.min((width - 260.).max(0.)),
                right: 0.,
                bottom: 0.,
            }),
        );
        keys.push(None);
    }
    let mut pane_stack = crate::pane_stack::PaneStack::new(
        layers.width(width).height(height).into(),
        keys,
        panes.iter().map(|pane| pane.id).collect(),
    );
    pane_stack.docking_enabled = state.configuring;
    pane_stack.docking_active = state.docking.is_some();
    pane_stack.docking_valid = state.docking.as_ref().is_some_and(|d| d.preview.is_some());
    let canvas = Element::new(pane_stack);
    scrollable(canvas)
        .id(scrollable::Id::new("workspace-canvas"))
        .on_scroll(|_| Message::Cancel)
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::new(),
            horizontal: scrollable::Scrollbar::new(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn divider(state: &Workspace, boundary: SplitBoundary) -> Element<'_, Message> {
    let (x, y, width, height, cursor) = match boundary.axis {
        Axis::Vertical => (
            boundary.position as f32 - 4.0,
            boundary.rect.y as f32,
            8.0,
            boundary.rect.height as f32,
            iced::mouse::Interaction::ResizingHorizontally,
        ),
        Axis::Horizontal => (
            boundary.rect.x as f32,
            boundary.position as f32 - 4.0,
            boundary.rect.width as f32,
            8.0,
            iced::mouse::Interaction::ResizingVertically,
        ),
    };
    let dragging = state.dragging_boundary(boundary);
    let tint = if dragging {
        BLUE
    } else if state.configuring {
        EDIT
    } else {
        Color::from_rgb8(69, 87, 119)
    };
    let thickness = if dragging { 4 } else { 2 };
    let line = match boundary.axis {
        Axis::Vertical => container(widget::Space::new(thickness, (height - 16.0).max(1.0)))
            .style(move |_| container::Style {
                background: Some(tint.into()),
                ..Default::default()
            })
            .center(Length::Fill),
        Axis::Horizontal => container(widget::Space::new((width - 16.0).max(1.0), thickness))
            .style(move |_| container::Style {
                background: Some(tint.into()),
                ..Default::default()
            })
            .center(Length::Fill),
    };
    // Padding belongs OUTSIDE the mouse area: only the 8px divider captures input.
    // The entire stack is inside the scroller, including every nested boundary.
    container(
        mouse_area(line.width(width).height(height))
            .on_press(Message::Begin(boundary))
            .interaction(cursor),
    )
    .padding(Padding {
        top: y,
        left: x,
        right: 0.0,
        bottom: 0.0,
    })
    .into()
}

fn pane_control(
    label: &'static str,
    id: PaneId,
    action: Action,
) -> widget::Button<'static, Message> {
    control(label, Message::PaneAction(id, action))
}

fn render_pane(state: &Workspace, pane: Pane, width: f32, height: f32) -> Element<'_, Message> {
    let l = state.language;
    let active = state.controller.layout().focused() == pane.id;
    let view = state.views.get(&pane.view).copied().unwrap_or_default();
    let tool_color = match view {
        PaneView::Monitor | PaneView::Processes => design::TEAL,
        PaneView::Wsl => design::VIOLET,
        PaneView::Settings | PaneView::Help => Color::from_rgb8(239, 187, 125),
        _ => BLUE,
    };
    let target = if state.configuring { EDIT } else { tool_color };
    let neutral = design::BORDER;
    let strength = if active {
        0.4 + 0.6 * state.accent_progress
    } else if state.previous_focus == Some(pane.id) {
        0.6 * (1.0 - state.accent_progress)
    } else {
        0.0
    };
    let accent = Color {
        r: neutral.r + (target.r - neutral.r) * strength,
        g: neutral.g + (target.g - neutral.g) * strength,
        b: neutral.b + (target.b - neutral.b) * strength,
        a: 1.0,
    };
    let label = if view == PaneView::Wsl {
        match state.profiles.get(&pane.view) {
            Some(tervix_application::terminal::Profile::Wsl(Some(name))) => format!("WSL · {name}"),
            Some(tervix_application::terminal::Profile::WslLaunch(launch)) => format!(
                "WSL · {}",
                launch
                    .distribution
                    .as_ref()
                    .map_or(l.text(K::SystemDefault), |name| name.as_str())
            ),
            _ => format!("WSL · {}", l.text(K::SystemDefault)),
        }
    } else {
        view.label(l).to_owned()
    };
    let docking = state.configuring && state.zoomed.is_none() && state.geometry_motion.is_none();
    let handle: Element<'_, Message> = if docking {
        widget::tooltip(
            container(crate::icons::view(crate::icons::Icon::Move, 18., EDIT)).center(28),
            text(l.text(K::DockHandle)),
            widget::tooltip::Position::Bottom,
        )
        .into()
    } else {
        container(crate::icons::view(view.into(), 18., tool_color))
            .padding([4, 7])
            .style(design::surface)
            .into()
    };
    let heading = row![
        handle,
        text(label).size(14).width(Length::Fill),
        text(l.text(if active { K::Focused } else { K::Pane }))
            .size(10)
            .color(if active { target } else { MUTED })
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let mut body = column![].spacing(10);
    if state.configuring {
        let selector = pick_list(
            PaneView::ALL.map(|v| Choice::new(v, v.label(l))),
            Some(Choice::new(view, view.label(l))),
            move |view| Message::ChangeView(pane.id, view.value),
        )
        .text_size(12)
        .style(design::picker)
        .padding(6);
        let mut close =
            pane_control(l.text(K::Close), pane.id, Action::Close).style(design::danger);
        if state.controller.layout().panes().len() == 1 {
            close = close.on_press_maybe(None);
        }
        let mut controls = column![
            row![text(l.text(K::View)).size(10).color(MUTED), selector]
                .spacing(8)
                .align_y(Alignment::Center),
            row![
                pane_control(l.text(K::Add), pane.id, Action::Split(Axis::Vertical)),
                pane_control(l.text(K::SplitV), pane.id, Action::Split(Axis::Vertical)),
                pane_control(l.text(K::SplitH), pane.id, Action::Split(Axis::Horizontal))
            ]
            .spacing(4),
            row![
                pane_control(
                    if state.zoomed.is_some() {
                        l.text(K::Restore)
                    } else {
                        l.text(K::Stretch)
                    },
                    pane.id,
                    Action::Zoom
                ),
                close
            ]
            .spacing(4),
        ]
        .spacing(6);
        if view == PaneView::Wsl {
            controls = controls.push(crate::wsl::selector(state, pane.view));
        }
        body = body.push(
            scrollable(controls)
                .id(scrollable::Id::new(format!("controls-{:?}", pane.view)))
                .height((height * 0.55).min(if view == PaneView::Ssh {
                    350.0
                } else if view == PaneView::Wsl {
                    190.0
                } else {
                    118.0
                })),
        );
    }
    if view == PaneView::Wsl && !state.configuring {
        body = body.push(
            scrollable(crate::wsl::selector(state, pane.view)).height((height * 0.45).min(110.0)),
        );
    }
    body = body.push(
        if matches!(
            view,
            PaneView::Terminal
                | PaneView::PowerShell
                | PaneView::Cmd
                | PaneView::Wsl
                | PaneView::Ssh
                | PaneView::DockerExec
        ) {
            if view == PaneView::DockerExec
                && !state.terminals.get(pane.view).is_some_and(|entry| {
                    matches!(
                        entry.profile,
                        tervix_application::terminal::Profile::DockerExec(_)
                    )
                })
            {
                text(l.text(K::ExecDisconnected)).into()
            } else if view == PaneView::Ssh && !crate::ssh::active(state, pane.view) {
                scrollable(crate::ssh::form(state, pane.view))
                    .height(Length::Fill)
                    .into()
            } else {
                crate::terminal::view(state, pane.view)
            }
        } else if matches!(view, PaneView::Monitor | PaneView::Processes) {
            crate::monitoring::view(state, pane.view, view == PaneView::Processes, width)
        } else if crate::docker::is_docker(view) {
            crate::docker_view::view(state, pane.view, view, width)
        } else {
            pane_content(state, view)
        },
    );
    let body: Element<'_, Message> = if matches!(
        view,
        PaneView::Terminal
            | PaneView::PowerShell
            | PaneView::Cmd
            | PaneView::Wsl
            | PaneView::Ssh
            | PaneView::DockerExec
            | PaneView::Monitor
            | PaneView::Processes
            | PaneView::DockerContainers
            | PaneView::DockerLogs
            | PaneView::DockerInspect
    ) {
        body.height(Length::Fill).into()
    } else {
        scrollable(body).height(Length::Fill).into()
    };
    // The drag handle stays at its measured header position even when Help or
    // Settings content scrolls; no body content can become an accidental handle.
    let body = container(column![heading, body].spacing(10).height(Length::Fill))
        .padding(14)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(design::PANE.into()),
            border: Border {
                color: accent,
                width: 1.0,
                radius: 14.0.into(),
            },
            ..Default::default()
        });
    container(Element::new(crate::pane_focus::PaneFocus {
        docking,
        content: body.into(),
        id: pane.id,
    }))
    .padding(5)
    .width(width)
    .height(height)
    .into()
}

fn pane_content(state: &Workspace, view: PaneView) -> Element<'_, Message> {
    let l = state.language;
    let content = match view {
        PaneView::Monitor
        | PaneView::Processes
        | PaneView::DockerContainers
        | PaneView::DockerLogs
        | PaneView::DockerInspect => column![],
        PaneView::Terminal
        | PaneView::PowerShell
        | PaneView::Cmd
        | PaneView::Wsl
        | PaneView::Ssh
        | PaneView::DockerExec => column![text(l.text(K::Terminal))],
        PaneView::Help => column![
            text(l.text(K::HelpTitle)).size(21),
            text(l.text(K::HelpIntro)).size(13).color(MUTED),
            text(l.text(K::HelpKeys)).size(13),
            text(l.text(K::PointerResize)).size(15),
            text(l.text(K::HelpTools)).size(13).color(MUTED),
            text(l.text(K::HelpDrag)).size(13).color(MUTED),
            text(l.text(K::HelpOverflow)).size(13).color(MUTED),
            text(l.text(K::HelpTerminal)).size(12).color(MUTED),
        ],
        PaneView::Settings => column![
            text(l.text(K::Preferences)).size(21),
            text(l.text(K::SessionHelp)).size(12).color(MUTED),
            text(l.text(K::General)).size(15),
            text(l.text(K::LanguageLabel)).size(14),
            language_picker(l),
            text(l.text(K::Appearance)).size(10).color(EDIT),
            checkbox(l.text(K::ReducedMotion), state.reduced_motion)
                .on_toggle(Message::ReducedMotion)
                .text_size(14),
            checkbox(l.text(K::Solid), state.chrome.solid)
                .on_toggle(|value| Message::Chrome(chrome::Event::Solid(value)))
                .text_size(14),
            text(l.text(K::TransparencyHelp)).size(13).color(MUTED),
            text(l.text(K::MotionHelp)).size(13).color(MUTED),
            text(l.text(K::SessionOnly)).size(10).color(EDIT),
            text(l.text(K::SessionHelp)).size(13).color(MUTED),
        ],
    };
    content.spacing(14).padding([8, 0]).into()
}

pub(super) fn language_picker(language: Language) -> Element<'static, Message> {
    pick_list(
        [Language::Ru, Language::En],
        Some(language),
        Message::Language,
    )
    .text_size(12)
    .padding(5)
    .style(design::picker)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{layout, widget::Tree};

    #[test]
    fn minimum_wsl_pane_can_scroll_to_start_in_normal_and_edit_modes() {
        use iced::advanced::{Layout, Shell};
        let (mut state, _) = crate::workspace::initialize();
        let pane = state.controller.layout().panes()[0];
        state.views.insert(pane.view, PaneView::Wsl);
        let renderer = crate::test_renderer::renderer(iced::Font::with_name("Segoe UI"));
        let size = iced::Size::new(300.0, 220.0);
        let viewport = iced::Rectangle::with_size(size);
        for language in [Language::Ru, Language::En] {
            state.language = language;
            for editing in [false, true] {
                state.configuring = editing;
                let mut found = false;
                for y in (60..205).step_by(3) {
                    let mut element = render_pane(&state, pane, size.width, size.height);
                    let mut tree = Tree::new(&element);
                    let node = element.as_widget().layout(
                        &mut tree,
                        &renderer,
                        &layout::Limits::new(iced::Size::ZERO, size),
                    );
                    let mut messages = Vec::new();
                    for (event, point) in [
                        (
                            iced::mouse::Event::WheelScrolled {
                                delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -100.0 },
                            },
                            iced::Point::new(276.0, 100.0),
                        ),
                        (
                            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
                            iced::Point::new(100.0, y as f32),
                        ),
                        (
                            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
                            iced::Point::new(100.0, y as f32),
                        ),
                    ] {
                        let _ = element.as_widget_mut().on_event(
                            &mut tree,
                            iced::Event::Mouse(event),
                            Layout::new(&node),
                            iced::mouse::Cursor::Available(point),
                            &renderer,
                            &mut iced::advanced::clipboard::Null,
                            &mut Shell::new(&mut messages),
                            &viewport,
                        );
                    }
                    found |= messages.iter().any(
                        |message| matches!(message, Message::StartWsl(id) if *id == pane.view),
                    );
                }
                assert!(
                    found,
                    "WSL Start must be reachable: {language:?}, edit={editing}"
                );
            }
        }
    }

    #[test]
    fn every_pane_builds_in_both_languages_and_reuses_its_widget_tree_at_minimum_size() {
        let (mut state, _) = crate::workspace::initialize();
        state.window_size = iced::Size::new(680.0, 480.0);
        state.configuring = true;
        let pane = state.controller.layout().panes()[0];
        let renderer = crate::test_renderer::renderer(iced::Font::with_name("Segoe UI"));
        for kind in PaneView::ALL {
            state.views.insert(pane.view, kind);
            let mut tree = Tree::empty();
            for language in [Language::Ru, Language::En, Language::Ru] {
                let _ = crate::workspace::update(&mut state, Message::Language(language));
                let element = view(&state);
                tree.diff(&element);
                let node = element.as_widget().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, state.window_size),
                );
                assert!(node.bounds().width.is_finite() && node.bounds().height.is_finite());
                assert_eq!(state.controller.layout().panes(), vec![pane]);
                assert_eq!(state.views[&pane.view], kind);
                assert!(!kind.label(language).is_empty());
            }
        }
    }
}
