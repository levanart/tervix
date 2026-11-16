//! Per-view SSH drafts; typing never starts a process or performs discovery.
use crate::i18n::{Key as K, Notice};
use crate::workspace::{Message, PaneView, Workspace};
use iced::{
    Element,
    widget::{button, column, text, text_input},
};
use tervix_application::{
    ViewId,
    ssh::Connection,
    terminal::{Lifecycle, Profile},
};

#[derive(Default)]
pub(super) struct Form {
    host: String,
    username: String,
    port: String,
    error: Option<Notice>,
}

#[derive(Debug, Clone)]
pub(super) enum Event {
    Host(String),
    Username(String),
    Port(String),
    Connect,
}

pub(super) fn host_input(id: ViewId) -> text_input::Id {
    text_input::Id::new(format!("ssh-host-{id:?}"))
}

pub(super) fn title_summary(state: &Workspace, id: ViewId) -> String {
    if active(state, id) {
        return format!(" | {}", state.language.text(K::SshSession));
    }
    state
        .ssh_forms
        .get(&id)
        .map(|f| {
            format!(
                " | {}",
                state
                    .language
                    .format(K::SshDraft, &[&f.host, &f.username, &f.port])
            )
        })
        .unwrap_or_else(|| format!(" | {}", state.language.text(K::SshNotConnected)))
}

pub(super) fn active(state: &Workspace, id: ViewId) -> bool {
    state.terminals.get(id).is_some_and(|entry| {
        matches!(
            entry.snapshot.lifecycle,
            Lifecycle::Starting | Lifecycle::Running
        )
    })
}

pub(super) fn update(state: &mut Workspace, message: &Message) -> bool {
    let Message::Ssh(id, event) = message else {
        return false;
    };
    if state.pending_close.is_some()
        || state.pending_paste.is_some()
        || state.chrome.pending_exit
        || state.chrome.shutting_down
        || state.views.get(id) != Some(&PaneView::Ssh)
        || !state
            .controller
            .layout()
            .panes()
            .iter()
            .any(|p| p.view == *id)
    {
        return true;
    }
    let running = active(state, *id);
    let form = state.ssh_forms.entry(*id).or_default();
    form.error = None;
    match event {
        Event::Host(value) => form.host = value.clone(),
        Event::Username(value) => form.username = value.clone(),
        Event::Port(value) => form.port = value.clone(),
        Event::Connect if !running => match Connection::new(&form.host, &form.username, &form.port)
        {
            Ok(connection) => {
                let profile = Profile::Ssh(connection);
                state.profiles.insert(*id, profile.clone());
                state.terminals.restart(*id, profile);
                state.terminal_caches.entry(*id).or_default().clear();
                state.status = K::SshConnecting.into();
                crate::terminal::reconcile(state);
            }
            Err(error) => {
                form.error = Some(
                    match error {
                        tervix_application::ssh::ConnectionError::Host => K::SshHostError,
                        tervix_application::ssh::ConnectionError::Username => K::SshUserError,
                        tervix_application::ssh::ConnectionError::Port => K::SshPortError,
                    }
                    .into(),
                )
            }
        },
        Event::Connect => form.error = Some(K::SshActive.into()),
    }
    true
}

pub(super) fn form(state: &Workspace, id: ViewId) -> Element<'_, Message> {
    let l = state.language;
    let draft = state.ssh_forms.get(&id);
    let input = move |value: Event| Message::Ssh(id, value);
    let submit = Message::Ssh(id, Event::Connect);
    let mut body = column![
        text(l.text(K::SshHost)).size(12),
        text_input(
            l.text(K::SshHostPlaceholder),
            draft.map_or("", |f| f.host.as_str())
        )
        .id(host_input(id))
        .on_input(move |s| input(Event::Host(s)))
        .on_submit(submit.clone())
        .size(12),
        text(l.text(K::SshUser)).size(12),
        text_input(
            l.text(K::SshConfigDefault),
            draft.map_or("", |f| f.username.as_str())
        )
        .on_input(move |s| input(Event::Username(s)))
        .on_submit(submit.clone())
        .size(12),
        text(l.text(K::SshPort)).size(12),
        text_input(
            l.text(K::SshConfigDefault),
            draft.map_or("", |f| f.port.as_str())
        )
        .on_input(move |s| input(Event::Port(s)))
        .on_submit(submit.clone())
        .size(12),
        button(l.text(K::ConnectSsh)).on_press_maybe((!active(state, id)).then_some(submit)),
        text(l.text(K::SshHelp)).size(11),
    ]
    .spacing(5);
    if let Some(error) = draft.and_then(|f| f.error.as_ref()) {
        body = body.push(text(error.render(l)).size(12));
    }
    if let Some(entry) = state.terminals.get(id) {
        match &entry.snapshot.lifecycle {
            Lifecycle::Failed(error) => {
                body = body.push(text(l.format(K::Error, &[error])).size(12))
            }
            Lifecycle::Exited(code) => {
                body = body.push(text(l.format(K::SshExited, &[&code.to_string()])).size(12))
            }
            Lifecycle::Starting | Lifecycle::Running | Lifecycle::Closing | Lifecycle::Closed => {}
        }
    }
    body.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Action, initialize, update as dispatch};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tervix_application::{
        Axis,
        terminal::{GridSize, TerminalError, TerminalProvider, TerminalSession},
    };

    struct Provider(Arc<AtomicUsize>);
    impl TerminalProvider for Provider {
        fn start(
            &self,
            _: Profile,
            _: GridSize,
        ) -> Result<Box<dyn TerminalSession>, TerminalError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Err(TerminalError::Startup("test unavailable".into()))
        }
    }

    #[test]
    fn language_switch_retranslates_validation_without_changing_draft_or_connecting() {
        use crate::i18n::Language;
        let (mut state, _) = initialize();
        let starts = Arc::new(AtomicUsize::new(0));
        state
            .terminals
            .set_provider(Arc::new(Provider(starts.clone())));
        state.configuring = true;
        let pane = state.controller.layout().panes()[0];
        let _ = dispatch(&mut state, Message::ChangeView(pane.id, PaneView::Ssh));
        let _ = dispatch(
            &mut state,
            Message::Ssh(pane.view, Event::Host("prod-alias".into())),
        );
        let _ = dispatch(&mut state, Message::Ssh(pane.view, Event::Port("0".into())));
        let _ = dispatch(&mut state, Message::Ssh(pane.view, Event::Connect));
        for language in [Language::En, Language::Ru] {
            let _ = dispatch(&mut state, Message::Language(language));
            let draft = &state.ssh_forms[&pane.view];
            assert_eq!(draft.host, "prod-alias");
            assert_eq!(draft.port, "0");
            assert_eq!(
                draft.error.as_ref().map(|n| n.render(language)),
                Some(language.text(K::SshPortError).to_owned())
            );
            assert_eq!(starts.load(Ordering::Relaxed), 0);
            assert!(state.profiles.is_empty());
            let _ = form(&state, pane.view);
        }
    }

    #[test]
    fn drafts_are_explicit_per_view_and_undo_does_not_reconnect()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut state, _) = initialize();
        let starts = Arc::new(AtomicUsize::new(0));
        state
            .terminals
            .set_provider(Arc::new(Provider(starts.clone())));
        state.configuring = true;
        let first = state.controller.layout().panes()[0];
        let _ = dispatch(&mut state, Message::ChangeView(first.id, PaneView::Ssh));
        assert_eq!(starts.load(Ordering::Relaxed), 0);
        let _ = dispatch(
            &mut state,
            Message::Ssh(first.view, Event::Host("dev-alias".into())),
        );
        let _ = dispatch(
            &mut state,
            Message::Ssh(first.view, Event::Port("0".into())),
        );
        let _ = dispatch(&mut state, Message::Ssh(first.view, Event::Connect));
        assert_eq!(starts.load(Ordering::Relaxed), 0);
        assert!(state.ssh_forms[&first.view].error.is_some());
        let _ = dispatch(
            &mut state,
            Message::Ssh(first.view, Event::Port("22".into())),
        );
        state.pending_close = Some(first.id);
        let _ = dispatch(&mut state, Message::Ssh(first.view, Event::Connect));
        assert_eq!(starts.load(Ordering::Relaxed), 0);
        state.pending_close = None;
        let _ = dispatch(&mut state, Message::Ssh(first.view, Event::Connect));
        assert_eq!(starts.load(Ordering::Relaxed), 1);
        let selected = Profile::Ssh(Connection::new("dev-alias", "", "22")?);
        assert_eq!(state.profiles.get(&first.view), Some(&selected));
        let _ = dispatch(&mut state, Message::Action(Action::Split(Axis::Vertical)));
        let second = state
            .controller
            .layout()
            .panes()
            .into_iter()
            .find(|p| p.id != first.id)
            .ok_or("second pane")?;
        let _ = dispatch(&mut state, Message::ChangeView(second.id, PaneView::Ssh));
        assert!(!state.profiles.contains_key(&second.view));
        let before = starts.load(Ordering::Relaxed);
        let _ = dispatch(&mut state, Message::PaneAction(first.id, Action::Close));
        let _ = dispatch(&mut state, Message::Ssh(first.view, Event::Connect)); // stale queued action
        assert_eq!(starts.load(Ordering::Relaxed), before);
        let _ = dispatch(&mut state, Message::Action(Action::Undo));
        assert_eq!(starts.load(Ordering::Relaxed), before);
        assert_eq!(state.profiles.get(&first.view), Some(&selected));
        assert_eq!(state.ssh_forms[&first.view].host, "dev-alias");
        assert_eq!(
            state
                .terminals
                .get(first.view)
                .map(|e| &e.snapshot.lifecycle),
            Some(&Lifecycle::Closed)
        );
        let _ = dispatch(&mut state, Message::Ssh(first.view, Event::Connect));
        assert_eq!(starts.load(Ordering::Relaxed), before + 1);
        // Re-selecting SSH after a local shell must not use the saved remote
        // profile to start a connection. WSL must not inherit that profile either.
        let _ = dispatch(&mut state, Message::ChangeView(first.id, PaneView::Wsl));
        assert_eq!(
            state.terminals.get(first.view).map(|e| &e.profile),
            Some(&Profile::Wsl(None))
        );
        let after_local = starts.load(Ordering::Relaxed);
        let _ = dispatch(&mut state, Message::ChangeView(first.id, PaneView::Ssh));
        assert_eq!(starts.load(Ordering::Relaxed), after_local);
        assert_eq!(state.profiles.get(&first.view), Some(&selected));
        Ok(())
    }
}
