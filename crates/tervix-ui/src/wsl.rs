//! Selector state is presentation-only; discovery and launch run behind application ports.
use crate::i18n::{Choice as Item, Key as K};
use crate::workspace::{Message, PaneView, Workspace};
use iced::{
    Element,
    widget::{button, column, pick_list, row, text},
};
use tervix_application::{
    ViewId,
    terminal::Profile,
    wsl::{DiscoveryState, DistributionName},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Choice {
    Default,
    Distribution(DistributionName),
}

pub(super) fn cycle(state: &mut Workspace) {
    let Some(pane) = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.id == state.controller.layout().focused())
    else {
        return;
    };
    if state.views.get(&pane.view) != Some(&PaneView::Wsl) {
        state.status = K::WslChoose.into();
        return;
    }
    let names = match state.distributions.as_ref().map(|c| c.state()) {
        Some(DiscoveryState::Ready(names)) => names,
        _ => {
            state.status = K::WslNotReady.into();
            return;
        }
    };
    let draft = state.wsl_drafts.get(&pane.view).map(|choice| match choice {
        Choice::Default => Profile::Wsl(None),
        Choice::Distribution(name) => name.profile(),
    });
    let current = draft.as_ref().or_else(|| state.profiles.get(&pane.view));
    let index = names
        .iter()
        .position(|name| current == Some(&name.profile()));
    let choice = match index {
        Some(i) => names
            .get(i + 1)
            .cloned()
            .map(Choice::Distribution)
            .unwrap_or(Choice::Default),
        None => names
            .first()
            .cloned()
            .map(Choice::Distribution)
            .unwrap_or(Choice::Default),
    };
    update(state, &Message::SelectDistribution(pane.view, choice));
}

pub(super) fn update(state: &mut Workspace, message: &Message) -> bool {
    match message {
        Message::ServicesTick(_) => {
            if let Some(catalog) = &mut state.distributions
                && matches!(catalog.state(), DiscoveryState::Loading)
            {
                catalog.poll();
            }
            false // monitoring also consumes this shared tick
        }
        Message::RefreshWsl => {
            if state.pending_close.is_none()
                && state.pending_paste.is_none()
                && let Some(catalog) = &mut state.distributions
            {
                catalog.refresh();
            }
            true
        }
        Message::SelectDistribution(id, choice) => {
            if state.pending_close.is_some() || state.pending_paste.is_some() {
                return true;
            }
            let live = state
                .controller
                .layout()
                .panes()
                .iter()
                .any(|p| p.view == *id)
                && state.views.get(id) == Some(&PaneView::Wsl);
            if live {
                state.wsl_drafts.insert(*id, choice.clone());
            }
            true
        }
        Message::StartWsl(id) => {
            if state.views.get(id) == Some(&PaneView::Wsl)
                && state
                    .controller
                    .layout()
                    .panes()
                    .iter()
                    .any(|p| p.view == *id)
            {
                let profile = match state.wsl_drafts.get(id) {
                    Some(Choice::Distribution(name)) => name.profile(),
                    Some(Choice::Default) => Profile::Wsl(None),
                    None => state
                        .profiles
                        .get(id)
                        .cloned()
                        .unwrap_or(Profile::Wsl(None)),
                };
                state.pending_wsl = Some((*id, profile));
                let active = state.terminals.get(*id).is_some_and(|e| {
                    matches!(
                        e.snapshot.lifecycle,
                        tervix_application::terminal::Lifecycle::Starting
                            | tervix_application::terminal::Lifecycle::Running
                    )
                });
                if !active {
                    confirm(state, true);
                }
            }
            true
        }
        Message::ConfirmWsl(accept) => {
            confirm(state, *accept);
            true
        }
        _ => false,
    }
}

pub(super) fn confirm(state: &mut Workspace, accept: bool) {
    if let Some((id, profile)) = state.pending_wsl.take()
        && accept
        && state.views.get(&id) == Some(&PaneView::Wsl)
        && state
            .controller
            .layout()
            .panes()
            .iter()
            .any(|p| p.view == id)
    {
        state.profiles.insert(id, profile.clone());
        state.terminals.restart(id, profile);
        state.status = K::WslReplaced.into();
        crate::terminal::reconcile(state);
    }
}

pub(super) fn selector(state: &Workspace, id: ViewId) -> Element<'_, Message> {
    let l = state.language;
    let item = |choice: Choice| {
        let label = match &choice {
            Choice::Default => l.text(K::WslDefault).to_owned(),
            Choice::Distribution(name) => name.as_str().to_owned(),
        };
        Item::new(choice, label)
    };
    let selected = match state.profiles.get(&id) {
        Some(Profile::Wsl(Some(name))) => DistributionName::new(name.clone())
            .ok()
            .map(Choice::Distribution)
            .unwrap_or(Choice::Default),
        Some(Profile::WslLaunch(launch)) => launch
            .distribution
            .clone()
            .map(Choice::Distribution)
            .unwrap_or(Choice::Default),
        _ => Choice::Default,
    };
    let selected = state.wsl_drafts.get(&id).cloned().unwrap_or(selected);
    let mut choices = vec![Choice::Default];
    let label = match state.distributions.as_ref().map(|c| c.state()) {
        Some(DiscoveryState::Ready(names)) => {
            choices.extend(names.iter().cloned().map(Choice::Distribution));
            if names.is_empty() {
                l.text(K::WslEmpty).into()
            } else {
                l.format(K::WslInstalled, &[&names.len().to_string()])
            }
        }
        Some(DiscoveryState::Idle) => l.text(K::WslDiscover).into(),
        Some(DiscoveryState::Loading) => l.text(K::WslLoading).into(),
        Some(DiscoveryState::Unavailable) => l.text(K::WslUnavailable).into(),
        Some(DiscoveryState::Error(error)) => l.format(K::DiscoveryError, &[&error.to_string()]),
        None => state
            .discovery_error
            .clone()
            .map(|e| l.format(K::DiscoveryError, &[&e]))
            .unwrap_or_else(|| l.text(K::DiscoveryUnavailable).into()),
    };
    // Keep a previously selected name visible even if refresh no longer returns it.
    if !choices.contains(&selected) {
        choices.push(selected.clone());
    }
    column![
        row![
            pick_list(
                choices.into_iter().map(item).collect::<Vec<_>>(),
                Some(item(selected)),
                move |choice| { Message::SelectDistribution(id, choice.value) }
            )
            .text_size(12)
            .width(iced::Length::Fill)
            .padding(5),
            button(text(l.text(K::Refresh)).size(12)).on_press(Message::RefreshWsl)
        ]
        .spacing(6),
        text(label).size(11).color(crate::design::MUTED),
        button(text(l.text(K::ReplaceWsl)).size(12)).on_press(Message::StartWsl(id)),
    ]
    .spacing(4)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Action, initialize, update as dispatch};
    use tervix_application::{
        Axis,
        wsl::{Discovery, DistributionCatalog},
    };
    struct DiscoveryStub;
    struct TerminalStub;
    impl tervix_application::terminal::TerminalProvider for TerminalStub {
        fn start(
            &self,
            _: Profile,
            _: tervix_application::terminal::GridSize,
        ) -> Result<
            Box<dyn tervix_application::terminal::TerminalSession>,
            tervix_application::terminal::TerminalError,
        > {
            Err(tervix_application::terminal::TerminalError::Ended)
        }
    }
    impl Discovery for DiscoveryStub {
        fn refresh(&mut self) -> bool {
            true
        }
        fn poll(&mut self) -> Option<DiscoveryState> {
            Some(DiscoveryState::Ready(vec![
                DistributionName::new("Ubuntu").ok()?,
            ]))
        }
    }
    #[test]
    fn selections_are_independent_survive_reconcile_refresh_and_undo()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut state, _) = initialize();
        state
            .terminals
            .set_provider(std::sync::Arc::new(TerminalStub));
        state.configuring = true;
        state.distributions = Some(DistributionCatalog::new(Box::new(DiscoveryStub)));
        let first = state.controller.layout().panes()[0];
        let _ = dispatch(&mut state, Message::ChangeView(first.id, PaneView::Wsl));
        let _ = dispatch(&mut state, Message::RefreshWsl);
        let _ = dispatch(&mut state, Message::ServicesTick(std::time::Instant::now()));
        cycle(&mut state);
        assert!(matches!(
            state.wsl_drafts.get(&first.view),
            Some(Choice::Distribution(_))
        ));
        cycle(&mut state);
        assert_eq!(state.wsl_drafts.get(&first.view), Some(&Choice::Default));
        cycle(&mut state);
        assert_ne!(
            state.profiles.get(&first.view),
            Some(&DistributionName::new("Ubuntu")?.profile())
        );
        let _ = dispatch(&mut state, Message::StartWsl(first.view));
        let selected = DistributionName::new("Ubuntu")?.profile();
        assert_eq!(state.profiles.get(&first.view), Some(&selected));
        let _ = dispatch(&mut state, Message::Action(Action::Split(Axis::Vertical)));
        let second = state
            .controller
            .layout()
            .panes()
            .into_iter()
            .find(|p| p.id != first.id)
            .ok_or("second pane")?;
        let _ = dispatch(&mut state, Message::ChangeView(second.id, PaneView::Wsl));
        crate::terminal::reconcile(&mut state);
        assert_eq!(
            state.terminals.get(first.view).map(|e| &e.profile),
            Some(&selected)
        );
        assert_eq!(
            state.terminals.get(second.view).map(|e| &e.profile),
            Some(&Profile::Wsl(None))
        );
        let _ = dispatch(&mut state, Message::RefreshWsl);
        let _ = dispatch(&mut state, Message::ServicesTick(std::time::Instant::now()));
        let _ = dispatch(&mut state, Message::PaneAction(first.id, Action::Close));
        let _ = dispatch(&mut state, Message::Action(Action::Undo));
        assert_eq!(state.profiles.get(&first.view), Some(&selected));
        assert_eq!(
            state.terminals.get(first.view).map(|e| &e.profile),
            Some(&selected)
        );
        state.pending_close = Some(first.id);
        update(
            &mut state,
            &Message::SelectDistribution(first.view, Choice::Default),
        );
        assert_eq!(state.profiles.get(&first.view), Some(&selected));
        Ok(())
    }
}
