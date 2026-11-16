//! Explicit new-view creation through the existing transactional layout controller.
use crate::{
    design,
    i18n::{Key as K, Language},
    icons::{self, Icon},
    workspace::{Action, Message, PaneView, Workspace},
};
use iced::{
    Alignment, Element, Length, Task,
    keyboard::{self, Key, key::Named},
    widget::{button, column, container, mouse_area, row, scrollable, text},
};
use tervix_application::{
    Axis, Command, LayoutCommand, PaneId, SplitRatio,
    terminal::Profile,
    wsl::{DiscoveryState, DistributionName},
};

#[cfg(test)]
#[path = "launcher_tests.rs"]
mod tests;

pub(super) struct Launcher {
    target: PaneId,
    axis: Axis,
    selected: usize,
    choices: Vec<Choice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Choice {
    Default,
    PowerShell,
    Cmd,
    Wsl(Option<DistributionName>),
    Ssh,
    Tool(PaneView),
}

impl Choice {
    fn kind(&self) -> PaneView {
        match self {
            Self::Default => PaneView::Terminal,
            Self::PowerShell => PaneView::PowerShell,
            Self::Cmd => PaneView::Cmd,
            Self::Wsl(_) => PaneView::Wsl,
            Self::Ssh => PaneView::Ssh,
            Self::Tool(kind) => *kind,
        }
    }
    fn label(&self, l: Language) -> String {
        match self {
            Self::Wsl(Some(name)) => format!("WSL · {}", name.as_str()),
            Self::Wsl(None) => l.text(K::WslDefault).into(),
            _ => self.kind().label(l).into(),
        }
    }
    fn profile(&self) -> Option<Profile> {
        match self {
            Self::Default => Some(Profile::Default),
            Self::PowerShell => Some(Profile::PowerShell),
            Self::Cmd => Some(Profile::Cmd),
            Self::Wsl(Some(name)) => Some(name.profile()),
            Self::Wsl(None) => Some(Profile::Wsl(None)),
            Self::Ssh | Self::Tool(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Event {
    Open,
    Settings,
    Cancel,
    Step(bool),
    Axis(Axis),
    Choose(Choice),
    Accept,
    Refresh,
}

fn choices(state: &Workspace) -> Vec<Choice> {
    let mut choices = vec![
        Choice::Default,
        Choice::PowerShell,
        Choice::Cmd,
        Choice::Wsl(None),
    ];
    if let Some(DiscoveryState::Ready(names)) = state.distributions.as_ref().map(|c| c.state()) {
        choices.extend(names.iter().cloned().map(|name| Choice::Wsl(Some(name))));
    }
    choices.push(Choice::Ssh);
    choices.extend(
        [
            PaneView::Monitor,
            PaneView::Processes,
            PaneView::DockerContainers,
            PaneView::Settings,
            PaneView::Help,
        ]
        .map(Choice::Tool),
    );
    choices
}

fn blocked(state: &Workspace) -> bool {
    state.pending_close.is_some()
        || state.pending_paste.is_some()
        || state.chrome.pending_exit
        || state.chrome.shutting_down
        || state
            .docker
            .as_ref()
            .is_some_and(|d| d.confirmation.is_some())
}

pub(super) fn update(state: &mut Workspace, message: &Message) -> Option<Task<Message>> {
    let event = match message {
        Message::Launcher(event) => Some(event.clone()),
        Message::Action(Action::NewTerminal) => Some(Event::Open),
        _ => None,
    };
    if event.is_some() && blocked(state) {
        state.status = K::LauncherPending.into();
        return Some(Task::none());
    }
    if let Some(event) = event {
        match event {
            Event::Settings => {
                state.cancel();
                if let Some(pane) = state
                    .controller
                    .layout()
                    .panes()
                    .into_iter()
                    .find(|p| state.views.get(&p.view) == Some(&PaneView::Settings))
                {
                    let before = state.displayed_geometry();
                    state.zoomed = None;
                    let _ = state
                        .controller
                        .dispatch(Command::Layout(LayoutCommand::Focus(pane.id)));
                    state.retarget_geometry(before);
                    return Some(state.reveal_focus());
                }
                let _ = update(state, &Message::Launcher(Event::Open));
                return Some(create(state, Choice::Tool(PaneView::Settings)));
            }
            Event::Open => {
                state.cancel();
                if let Some(catalog) = &mut state.distributions
                    && matches!(catalog.state(), DiscoveryState::Idle)
                {
                    catalog.refresh();
                }
                state.launcher = Some(Launcher {
                    target: state.controller.layout().focused(),
                    axis: Axis::Vertical,
                    selected: 0,
                    choices: choices(state),
                });
            }
            Event::Refresh => {
                if state.launcher.is_some()
                    && let Some(catalog) = &mut state.distributions
                {
                    catalog.refresh();
                }
            }
            Event::Cancel => state.launcher = None,
            Event::Step(backwards) => {
                if let Some(menu) = &mut state.launcher {
                    menu.selected = if backwards {
                        (menu.selected + menu.choices.len() - 1) % menu.choices.len()
                    } else {
                        (menu.selected + 1) % menu.choices.len()
                    };
                    return Some(iced::widget::scrollable::snap_to(
                        iced::widget::scrollable::Id::new("new-terminal-choices"),
                        iced::widget::scrollable::RelativeOffset {
                            x: 0.,
                            y: menu.selected as f32 / (menu.choices.len() - 1).max(1) as f32,
                        },
                    ));
                }
            }
            Event::Axis(axis) => {
                if let Some(menu) = &mut state.launcher {
                    menu.axis = axis;
                }
            }
            Event::Choose(choice) => return Some(create(state, choice)),
            Event::Accept => {
                if let Some(choice) = state
                    .launcher
                    .as_ref()
                    .and_then(|m| m.choices.get(m.selected))
                    .cloned()
                {
                    return Some(create(state, choice));
                }
            }
        }
        return Some(Task::none());
    }
    if state.launcher.is_some()
        && matches!(message, Message::ServicesTick(_))
        && let Some(catalog) = &mut state.distributions
        && matches!(catalog.state(), DiscoveryState::Loading)
        && catalog.poll()
    {
        let choices = choices(state);
        if let Some(menu) = &mut state.launcher {
            let selected = &menu.choices[menu.selected];
            menu.selected = choices.iter().position(|c| c == selected).unwrap_or(0);
            menu.choices = choices;
        }
    }
    if state.launcher.is_some() {
        // Background snapshots continue; queued UI actions cannot reach panes behind the menu.
        return match message {
            Message::Chrome(_)
            | Message::TerminalTick(_)
            | Message::TerminalMeasured(_, _)
            | Message::ServicesTick(_)
            | Message::DockerTick
            | Message::Resized(_)
            | Message::Frame(_) => None,
            Message::Cancel => {
                state.launcher = None;
                Some(Task::none())
            }
            _ => Some(Task::none()),
        };
    }
    None
}

fn create(state: &mut Workspace, choice: Choice) -> Task<Message> {
    let Some(menu) = &state.launcher else {
        return Task::none();
    };
    if !menu.choices.contains(&choice) {
        return Task::none();
    }
    let target = menu.target;
    let axis = menu.axis;
    let before = state.displayed_geometry();
    let pane = super::workspace::fresh_pane();
    if let Err(error) = state
        .controller
        .dispatch(Command::Layout(LayoutCommand::Split {
            target,
            pane,
            axis,
            ratio: SplitRatio::HALF,
        }))
    {
        state.status = K::LauncherFailed.with([error.to_string()]);
        return Task::none();
    }
    state.views.insert(pane.view, choice.kind());
    if let Some(profile) = choice.profile() {
        state.profiles.insert(pane.view, profile);
    }
    let ssh = choice == Choice::Ssh;
    if ssh {
        state.ssh_forms.entry(pane.view).or_default();
    }
    state.launcher = None;
    state.zoomed = None;
    state.previous_focus = Some(target);
    state.status = K::LauncherCreated.into();
    state.retarget_geometry(before);
    crate::terminal::reconcile(state);
    if ssh {
        Task::batch([
            state.reveal_focus(),
            iced::widget::text_input::focus(crate::ssh::host_input(pane.view)),
        ])
    } else {
        Task::batch([
            state.reveal_focus(),
            iced::widget::text_input::focus(iced::widget::text_input::Id::new(
                "workspace-keyboard-router",
            )),
        ])
    }
}

pub(super) fn key(event: iced::Event) -> Option<Message> {
    if matches!(event, iced::Event::Window(iced::window::Event::Unfocused)) {
        return Some(Message::Launcher(Event::Cancel));
    }
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event else {
        return None;
    };
    let event = match key {
        Key::Named(Named::Escape) => Event::Cancel,
        Key::Named(Named::ArrowUp) => Event::Step(true),
        Key::Named(Named::ArrowDown) => Event::Step(false),
        Key::Named(Named::Tab) => Event::Step(modifiers.shift()),
        Key::Named(Named::ArrowLeft) => Event::Axis(Axis::Vertical),
        Key::Named(Named::ArrowRight) => Event::Axis(Axis::Horizontal),
        Key::Named(Named::Enter | Named::Space) => Event::Accept,
        Key::Named(Named::F5) => Event::Refresh,
        _ => return None,
    };
    Some(Message::Launcher(event))
}

pub(super) fn toolbar_button(l: Language) -> Element<'static, Message> {
    iced::widget::tooltip(
        button(
            row![
                icons::view(Icon::Add, 16., design::TEXT),
                text(l.text(K::NewTerminal)).size(13)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([5, 10])
        .on_press(Message::Launcher(Event::Open)),
        container(text(l.text(K::NewTerminalHint)).size(12))
            .padding(8)
            .style(design::surface),
        iced::widget::tooltip::Position::Bottom,
    )
    .into()
}

pub(super) fn overlay<'a>(
    state: &'a Workspace,
    workspace: Element<'a, Message>,
) -> Element<'a, Message> {
    let Some(menu) = &state.launcher else {
        return workspace;
    };
    let l = state.language;
    let mut choices = column![].spacing(5);
    for (i, choice) in menu.choices.iter().enumerate() {
        if i == 0 {
            choices = choices.push(
                text(l.text(K::TerminalCategory))
                    .size(12)
                    .color(design::MUTED),
            );
        }
        if matches!(choice, Choice::Tool(_))
            && (i == 0 || !matches!(menu.choices[i - 1], Choice::Tool(_)))
        {
            choices = choices.push(text(l.text(K::ToolCategory)).size(12).color(design::MUTED));
        }
        let selected = menu.selected == i;
        choices = choices.push(
            button(
                row![
                    icons::view(choice.kind().into(), 20., design::TEAL),
                    text(choice.label(l)).size(15)
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .width(Length::Fill)
            .padding(9)
            .style(move |theme, status| {
                if selected {
                    design::selected(theme, status)
                } else {
                    design::control(theme, status)
                }
            })
            .on_press(Message::Launcher(Event::Choose(choice.clone()))),
        );
    }
    let axis_button = |axis, icon, label| {
        button(row![icons::view(icon, 18., design::TEXT), text(label).size(13)].spacing(6))
            .padding(8)
            .style(move |theme, status| {
                if menu.axis == axis {
                    design::selected(theme, status)
                } else {
                    design::control(theme, status)
                }
            })
            .on_press(Message::Launcher(Event::Axis(axis)))
    };
    let discovery = match state.distributions.as_ref().map(|c| c.state()) {
        Some(DiscoveryState::Loading) => l.text(K::WslLoading).to_owned(),
        Some(DiscoveryState::Ready(names)) if names.is_empty() => l.text(K::WslEmpty).to_owned(),
        Some(DiscoveryState::Ready(names)) => {
            l.format(K::WslInstalled, &[&names.len().to_string()])
        }
        Some(DiscoveryState::Error(e)) => l.format(K::DiscoveryError, &[&e.to_string()]),
        Some(DiscoveryState::Unavailable) => l.text(K::WslUnavailable).to_owned(),
        Some(DiscoveryState::Idle) => l.text(K::WslDiscover).to_owned(),
        None => state
            .discovery_error
            .as_ref()
            .map(|e| l.format(K::DiscoveryError, &[e]))
            .unwrap_or_else(|| l.text(K::DiscoveryUnavailable).to_owned()),
    };
    let dialog = container(
        column![
            text(l.text(K::NewTerminal)).size(23),
            text(l.text(K::LauncherPlacement)).size(13),
            row![
                axis_button(
                    Axis::Vertical,
                    Icon::SplitVertical,
                    l.text(K::LauncherRight)
                ),
                axis_button(
                    Axis::Horizontal,
                    Icon::SplitHorizontal,
                    l.text(K::LauncherBelow)
                )
            ]
            .spacing(8),
            scrollable(choices)
                .id(iced::widget::scrollable::Id::new("new-terminal-choices"))
                .height(Length::Fill),
            text(discovery).size(11).color(design::MUTED),
            button(l.text(K::LauncherRefreshWsl)).on_press(Message::Launcher(Event::Refresh)),
            text(l.text(K::LauncherKeys)).size(12),
            text(state.status.render(l)).size(11).color(design::MUTED),
            button(l.text(K::Cancel)).on_press(Message::Launcher(Event::Cancel)),
        ]
        .spacing(10),
    )
    .padding(20)
    .width(460)
    .height((state.window_size.height - 48.).clamp(300., 650.))
    .style(design::surface);
    iced::widget::Stack::new()
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
            .on_press(Message::Launcher(Event::Cancel)),
        )
        .into()
}
