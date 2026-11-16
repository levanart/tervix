use super::*;
use crate::workspace::{self, initialize, update as dispatch};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tervix_application::{
    SessionId,
    terminal::{GridSize, Snapshot, TerminalError, TerminalProvider, TerminalSession},
    wsl::{Discovery, DistributionCatalog},
};

struct Provider(Arc<AtomicUsize>);
struct Session(SessionId);
impl TerminalProvider for Provider {
    fn start(&self, _: Profile, _: GridSize) -> Result<Box<dyn TerminalSession>, TerminalError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(Session(
            SessionId::from_uuid(uuid::Uuid::new_v4()),
        )))
    }
}
impl TerminalSession for Session {
    fn id(&self) -> SessionId {
        self.0
    }
    fn wakeup(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::default()
    }
    fn poll(&mut self) -> Option<Snapshot> {
        None
    }
    fn input(&self, _: Vec<u8>) -> Result<(), TerminalError> {
        Ok(())
    }
    fn resize(&self, _: GridSize) -> Result<(), TerminalError> {
        Ok(())
    }
    fn close(&self) {}
}
fn fixture() -> (Workspace, Arc<AtomicUsize>) {
    let (mut state, _) = initialize();
    let starts = Arc::new(AtomicUsize::new(0));
    state
        .terminals
        .set_provider(Arc::new(Provider(starts.clone())));
    crate::terminal::reconcile(&mut state);
    (state, starts)
}

#[test]
fn settings_access_is_global_and_independent_of_layout_editing()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, starts) = fixture();
    let original = state.controller.layout().panes()[0];
    let session = state
        .terminals
        .get(original.view)
        .ok_or("session")?
        .session_id;
    event(&mut state, Event::Settings);
    assert!(!state.configuring);
    let settings = state.controller.layout().focused();
    assert_eq!(state.controller.layout().panes().len(), 2);
    let _ = dispatch(&mut state, Message::ReducedMotion(true));
    let _ = dispatch(&mut state, Message::Language(Language::En));
    assert!(state.reduced_motion);
    assert_eq!(state.language, Language::En);
    event(&mut state, Event::Settings);
    assert_eq!(state.controller.layout().focused(), settings);
    assert_eq!(state.controller.layout().panes().len(), 2);
    assert_eq!(
        state
            .terminals
            .get(original.view)
            .ok_or("preserved")?
            .session_id,
        session
    );
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn wsl_draft_requires_explicit_replacement_and_modal_blocks_queued_actions()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, starts) = fixture();
    event(&mut state, Event::Open);
    event(&mut state, Event::Choose(Choice::Wsl(None)));
    let pane = *state.controller.layout().panes().last().ok_or("pane")?;
    let session = state.terminals.get(pane.view).ok_or("session")?.session_id;
    let choice = crate::wsl::Choice::Distribution(DistributionName::new("Ubuntu")?);
    let _ = dispatch(
        &mut state,
        Message::SelectDistribution(pane.view, choice.clone()),
    );
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(
        state
            .terminals
            .get(pane.view)
            .ok_or("unchanged")?
            .session_id,
        session
    );
    let _ = dispatch(&mut state, Message::StartWsl(pane.view));
    assert!(state.pending_wsl.is_some());
    let layout = state.controller.layout().clone();
    let _ = dispatch(
        &mut state,
        Message::SelectDistribution(pane.view, crate::wsl::Choice::Default),
    );
    event(&mut state, Event::Settings);
    let _ = dispatch(&mut state, Message::Action(Action::Configure));
    let _ = dispatch(
        &mut state,
        Message::ProcessQuery(pane.view, "hidden".into()),
    );
    assert_eq!(state.controller.layout(), &layout);
    assert!(!state.configuring);
    assert!(!state.process_views.contains_key(&pane.view));
    assert_eq!(state.wsl_drafts.get(&pane.view), Some(&choice));
    let _ = dispatch(&mut state, Message::ConfirmWsl(false));
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    let _ = dispatch(&mut state, Message::StartWsl(pane.view));
    let _ = dispatch(&mut state, Message::TerminalInput(b"\r".to_vec()));
    assert!(state.pending_wsl.is_none());
    assert_eq!(starts.load(Ordering::Relaxed), 3);
    assert_ne!(
        state
            .terminals
            .get(pane.view)
            .ok_or("replacement")?
            .session_id,
        session
    );
    assert_eq!(
        state.profiles.get(&pane.view),
        Some(&Profile::Wsl(Some("Ubuntu".into())))
    );
    Ok(())
}
fn event(state: &mut Workspace, e: Event) {
    let _ = dispatch(state, Message::Launcher(e));
}
struct Distros;
impl Discovery for Distros {
    fn refresh(&mut self) -> bool {
        true
    }
    fn poll(&mut self) -> Option<DiscoveryState> {
        Some(DiscoveryState::Ready(vec![
            DistributionName::new("Ubuntu Dev; $(literal)").ok()?,
        ]))
    }
}

#[test]
fn each_profile_creates_fresh_view_and_session_without_replacing_existing_shells()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, starts) = fixture();
    let original = state.controller.layout().panes()[0];
    let original_session = state
        .terminals
        .get(original.view)
        .ok_or("original")?
        .session_id;
    let mut sessions = std::collections::HashSet::from([original_session]);
    for (i, choice) in [
        Choice::Default,
        Choice::Default,
        Choice::PowerShell,
        Choice::Cmd,
        Choice::Wsl(None),
    ]
    .into_iter()
    .enumerate()
    {
        event(&mut state, Event::Open);
        event(&mut state, Event::Axis(Axis::Horizontal));
        event(&mut state, Event::Choose(choice.clone()));
        assert!(state.launcher.is_none());
        assert_eq!(state.controller.layout().panes().len(), i + 2);
        let pane = state
            .controller
            .layout()
            .panes()
            .into_iter()
            .find(|p| p.id == state.controller.layout().focused())
            .ok_or("new pane")?;
        assert_ne!(pane.view, original.view);
        let entry = state.terminals.get(pane.view).ok_or("new session")?;
        assert_eq!(Some(entry.profile.clone()), choice.profile());
        assert!(sessions.insert(entry.session_id));
        assert_eq!(
            state
                .terminals
                .get(original.view)
                .ok_or("preserved")?
                .session_id,
            original_session
        );
        assert_eq!(starts.load(Ordering::Relaxed), i + 2);
    }
    Ok(())
}

#[test]
fn async_distribution_updates_preserve_selection_and_launch_exact_per_view_profile()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, _) = fixture();
    state.distributions = Some(DistributionCatalog::new(Box::new(Distros)));
    event(&mut state, Event::Open);
    event(&mut state, Event::Step(true)); // Help remains selected when distros arrive.
    let _ = dispatch(&mut state, Message::ServicesTick(std::time::Instant::now()));
    let menu = state.launcher.as_ref().ok_or("launcher")?;
    assert_eq!(menu.choices[menu.selected], Choice::Tool(PaneView::Help));
    let name = DistributionName::new("Ubuntu Dev; $(literal)")?;
    assert!(menu.choices.contains(&Choice::Wsl(Some(name.clone()))));
    event(&mut state, Event::Choose(Choice::Wsl(Some(name.clone()))));
    let pane = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.id == state.controller.layout().focused())
        .ok_or("pane")?;
    assert_eq!(state.profiles.get(&pane.view), Some(&name.profile()));
    assert_eq!(
        state.terminals.get(pane.view).ok_or("entry")?.profile,
        name.profile()
    );
    Ok(())
}

#[test]
fn ssh_launcher_only_opens_form_and_connect_is_explicit() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut state, starts) = fixture();
    event(&mut state, Event::Open);
    event(&mut state, Event::Choose(Choice::Ssh));
    let pane = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.id == state.controller.layout().focused())
        .ok_or("ssh")?;
    assert!(!state.configuring && state.ssh_forms.contains_key(&pane.view));
    assert!(state.terminals.get(pane.view).is_none());
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    let _ = dispatch(
        &mut state,
        Message::Ssh(pane.view, crate::ssh::Event::Host("dev-alias".into())),
    );
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    let _ = dispatch(
        &mut state,
        Message::Ssh(pane.view, crate::ssh::Event::Connect),
    );
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert!(matches!(
        state.profiles.get(&pane.view),
        Some(Profile::Ssh(_))
    ));
    Ok(())
}

#[test]
fn escape_pending_confirmations_and_queued_actions_cannot_mutate_layout()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut state, starts) = fixture();
    let before = state.controller.layout().clone();
    for exiting in [false, true] {
        state.chrome.pending_exit = exiting;
        state.pending_close = (!exiting).then_some(before.focused());
        event(&mut state, Event::Open);
        event(&mut state, Event::Choose(Choice::Cmd));
        assert!(state.launcher.is_none());
        assert_eq!(*state.controller.layout(), before);
    }
    state.chrome.pending_exit = false;
    state.pending_close = None;
    state.controller.begin_resize()?;
    event(&mut state, Event::Open);
    assert!(!state.controller.resize_active());
    let _ = dispatch(&mut state, Message::Action(Action::Split(Axis::Vertical)));
    let _ = dispatch(
        &mut state,
        Message::TerminalInput(b"echo should not reach shell".to_vec()),
    );
    assert_eq!(*state.controller.layout(), before);
    event(&mut state, Event::Cancel);
    event(&mut state, Event::Choose(Choice::Cmd)); // stale widget message
    assert_eq!(*state.controller.layout(), before);
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    assert_eq!(state.controller.undo_depth(), 0);
    Ok(())
}

#[test]
fn domain_depth_limit_reports_error_without_orphan_profiles_or_session_starts() {
    let (mut state, starts) = fixture();
    for _ in 0..32 {
        event(&mut state, Event::Open);
        event(&mut state, Event::Choose(Choice::Default));
    }
    let before = state.controller.layout().clone();
    let count = starts.load(Ordering::Relaxed);
    let profiles = state.profiles.clone();
    event(&mut state, Event::Open);
    event(&mut state, Event::Choose(Choice::Cmd));
    assert_eq!(*state.controller.layout(), before);
    assert_eq!(state.profiles, profiles);
    assert_eq!(starts.load(Ordering::Relaxed), count);
    assert!(state.launcher.is_some());
    for l in [Language::Ru, Language::En] {
        assert!(
            state
                .status
                .render(l)
                .starts_with(l.text(K::LauncherFailed).split("{0}").next().unwrap_or(""))
        );
    }
}

#[test]
fn toolbar_widget_emits_open_and_menu_keyboard_navigation_is_headless()
-> Result<(), Box<dyn std::error::Error>> {
    use iced::advanced::{Layout, Shell, layout, widget::Tree};
    let (mut state, _) = fixture();
    let renderer = crate::test_renderer::renderer(iced::Font::default());
    let mut element = workspace::view(&state);
    let mut tree = Tree::new(&element);
    let node = element.as_widget().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(iced::Size::ZERO, state.window_size),
    );
    let mut messages = Vec::new();
    for e in [
        iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
        iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
    ] {
        let _ = element.as_widget_mut().on_event(
            &mut tree,
            iced::Event::Mouse(e),
            Layout::new(&node),
            iced::mouse::Cursor::Available(iced::Point::new(80., 88.)),
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &iced::Rectangle::with_size(state.window_size),
        );
    }
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::Launcher(Event::Open)))
    );
    drop(element);
    event(&mut state, Event::Open);
    for (named, expected) in [
        (Named::ArrowDown, "step"),
        (Named::ArrowRight, "axis"),
        (Named::Escape, "cancel"),
    ] {
        let msg = key(iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(named),
            modified_key: Key::Named(named),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::empty(),
            text: None,
        }))
        .ok_or("key event")?;
        let _ = dispatch(&mut state, msg);
        match expected {
            "step" => assert_eq!(state.launcher.as_ref().ok_or("menu")?.selected, 1),
            "axis" => assert_eq!(
                state.launcher.as_ref().ok_or("menu")?.axis,
                Axis::Horizontal
            ),
            _ => assert!(state.launcher.is_none()),
        }
    }
    Ok(())
}

#[test]
fn menu_choice_widget_emits_launch_without_backdrop_cancel_in_both_languages() {
    use iced::advanced::{Layout, Shell, layout, widget::Tree};
    for language in [Language::Ru, Language::En] {
        let (mut state, _) = fixture();
        state.language = language;
        event(&mut state, Event::Open);
        let renderer = crate::test_renderer::renderer(iced::Font::default());
        let mut element = workspace::view(&state);
        let mut tree = Tree::new(&element);
        let node = element.as_widget().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, state.window_size),
        );
        let mut messages = Vec::new();
        for e in [
            iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left),
            iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left),
        ] {
            let _ = element.as_widget_mut().on_event(
                &mut tree,
                iced::Event::Mouse(e),
                Layout::new(&node),
                iced::mouse::Cursor::Available(iced::Point::new(440., 258.)),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &iced::Rectangle::with_size(state.window_size),
            );
        }
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::Launcher(Event::Choose(Choice::Default))))
        );
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, Message::Launcher(Event::Cancel)))
        );
        drop(element);
        for message in messages {
            let _ = dispatch(&mut state, message);
        }
        assert_eq!(state.controller.layout().panes().len(), 2);
        // Rebuild the overlay at the minimum supported size without creating a native window.
        state.window_size = iced::Size::new(680., 480.);
        event(&mut state, Event::Open);
        let element = workspace::view(&state);
        let mut tree = Tree::new(&element);
        let node = element.as_widget().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, state.window_size),
        );
        assert_eq!(node.size(), state.window_size);
    }
}
