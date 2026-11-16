#![allow(clippy::unwrap_used)] // Fixtures/assertions fail immediately on broken test invariants.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Ready<T: Send>(Option<T>);
impl<T: Send> Reply<T> for Ready<T> {
    fn poll(&mut self) -> Option<Result<T, DockerError>> {
        self.0.take().map(Ok)
    }
}
fn reply<T: Send + 'static>(value: T) -> Box<dyn Reply<T>> {
    Box::new(Ready(Some(value)))
}
struct Reader {
    drops: Arc<AtomicUsize>,
    source: Arc<Mutex<Arc<LogSnapshot>>>,
    frozen: Option<Arc<LogSnapshot>>,
}
impl LogReader for Reader {
    fn latest(&mut self) -> Arc<LogSnapshot> {
        self.frozen
            .clone()
            .unwrap_or_else(|| self.source.lock().unwrap().clone())
    }
    fn set_paused(&mut self, paused: bool) {
        self.frozen = paused.then(|| self.latest());
    }
}
impl Drop for Reader {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
struct Fake {
    frame: Mutex<Arc<Snapshot>>,
    calls: AtomicUsize,
    log_calls: AtomicUsize,
    polls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
    actions: Mutex<Vec<(ContainerId, Action)>>,
    source: Arc<Mutex<Arc<LogSnapshot>>>,
}
impl DockerService for Fake {
    fn exec_generation(&self) -> u64 {
        self.frame.lock().unwrap().incarnation
    }
    fn contexts(&self) -> Result<Box<dyn Reply<Arc<[Context]>>>, DockerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(reply(Arc::from([])))
    }
    fn connect(&self, _: Endpoint) -> Result<(), DockerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn disconnect(&self) -> Result<(), DockerError> {
        Ok(())
    }
    fn set_visible(&self, _: bool) {}
    fn snapshot(&self) -> Arc<Snapshot> {
        self.frame.lock().unwrap().clone()
    }
    fn inspect(&self, _: u64, _: ContainerId) -> Result<Box<dyn Reply<Details>>, DockerError> {
        Err(DockerError::Unavailable)
    }
    fn stats(&self, _: u64, _: ContainerId) -> Result<Box<dyn Reply<Stats>>, DockerError> {
        Err(DockerError::Unavailable)
    }
    fn act(
        &self,
        incarnation: u64,
        id: ContainerId,
        action: Action,
    ) -> Result<Box<dyn Reply<()>>, DockerError> {
        assert_eq!(incarnation, self.exec_generation());
        self.actions.lock().unwrap().push((id, action));
        struct Once(Arc<AtomicUsize>);
        impl Reply<()> for Once {
            fn poll(&mut self) -> Option<Result<(), DockerError>> {
                assert_eq!(self.0.fetch_add(1, Ordering::SeqCst), 0);
                Some(Ok(()))
            }
        }
        Ok(Box::new(Once(self.polls.clone())))
    }
    fn logs(
        &self,
        _: u64,
        _: ContainerId,
    ) -> Result<Box<dyn Reply<Box<dyn LogReader>>>, DockerError> {
        self.log_calls.fetch_add(1, Ordering::SeqCst);
        Ok(reply(Box::new(Reader {
            drops: self.drops.clone(),
            source: self.source.clone(),
            frozen: None,
        }) as Box<dyn LogReader>))
    }
}
fn container(byte: &str, name: &str, state: &str) -> Container {
    Container {
        id: ContainerId::new(byte.repeat(64)).unwrap(),
        names: Arc::from([name.into()]),
        image: "image:latest".into(),
        state: state.into(),
        status: state.into(),
        created: None,
        ports: Arc::from([]),
    }
}
fn setup() -> (Workspace, Arc<Fake>, ViewId) {
    let frame = Arc::new(Snapshot {
        incarnation: 0,
        revision: 1,
        endpoint: Some(Endpoint::default()),
        state: ConnectionState::Connected,
        capabilities: Capabilities::default(),
        observed_at: None,
        containers: Arc::from([
            container("a", "zulu", "running"),
            container("b", "alpha", "exited"),
        ]),
    });
    let fake = Arc::new(Fake {
        frame: Mutex::new(frame),
        calls: AtomicUsize::new(0),
        log_calls: AtomicUsize::new(0),
        polls: Arc::new(AtomicUsize::new(0)),
        drops: Arc::new(AtomicUsize::new(0)),
        actions: Mutex::new(Vec::new()),
        source: Arc::new(Mutex::new(Arc::new(LogSnapshot {
            revision: 1,
            state: LogState::Following,
            entries: Arc::from([]),
            dropped_entries: 23,
            truncated_chunks: 2,
        }))),
    });
    let (mut state, _) = crate::workspace::initialize();
    let pane = state.controller.layout().panes()[0];
    state.views.insert(pane.view, PaneView::DockerContainers);
    let mut docker = Docker::new(fake.clone());
    docker.desired_endpoint = Some(Endpoint::default());
    state.docker = Some(docker);
    reconcile(&mut state);
    (state, fake, pane.view)
}
fn event(state: &mut Workspace, id: ViewId, event: Event) {
    let _ = crate::workspace::update(state, Message::Docker(id, event));
}
fn tick(state: &mut Workspace) {
    let _ = crate::workspace::update(state, Message::DockerTick);
}

#[test]
fn queued_resubscribe_keeps_live_reader_but_ended_stream_can_retry() {
    let (mut state, fake, id) = setup();
    event(
        &mut state,
        id,
        Event::Select(container("a", "zulu", "running").id),
    );
    event(&mut state, id, Event::Open(PaneView::DockerLogs));
    let logs = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| state.views.get(&p.view) == Some(&PaneView::DockerLogs))
        .unwrap()
        .view;
    event(&mut state, logs, Event::Subscribe); // pending ticket
    assert_eq!(fake.log_calls.load(Ordering::SeqCst), 1);
    tick(&mut state);
    for paused in [false, true, false] {
        if state.docker.as_ref().unwrap().panes[&logs].paused != paused {
            event(&mut state, logs, Event::Pause);
        }
        event(&mut state, logs, Event::Subscribe); // already following
        tick(&mut state);
        assert_eq!(fake.log_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fake.drops.load(Ordering::SeqCst), 0);
    }
    let mut ended = (**fake.source.lock().unwrap()).clone();
    ended.revision += 1;
    ended.state = LogState::Ended;
    *fake.source.lock().unwrap() = Arc::new(ended);
    tick(&mut state);
    event(&mut state, logs, Event::Subscribe);
    assert_eq!(fake.log_calls.load(Ordering::SeqCst), 2);
    assert_eq!(fake.drops.load(Ordering::SeqCst), 1);
    state.configuring = true;
    let pane = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.view == logs)
        .unwrap();
    let _ = crate::workspace::update(
        &mut state,
        Message::ChangeView(pane.id, PaneView::DockerContainers),
    );
    event(&mut state, logs, Event::Subscribe); // queued from the previous module
    assert_eq!(fake.log_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn child_views_split_the_origin_even_when_another_pane_is_focused() {
    for kind in [
        PaneView::DockerLogs,
        PaneView::DockerInspect,
        PaneView::DockerExec,
    ] {
        let (mut state, _, id) = setup();
        state.window_size = iced::Size::new(1600.0, 1000.0);
        let source = state.controller.layout().panes()[0];
        let other = crate::workspace::fresh_pane();
        state
            .controller
            .dispatch(Command::Layout(LayoutCommand::Split {
                target: source.id,
                pane: other,
                axis: Axis::Horizontal,
                ratio: SplitRatio::HALF,
            }))
            .unwrap();
        let before = state
            .geometry()
            .unwrap()
            .panes()
            .iter()
            .find(|p| p.pane == other)
            .unwrap()
            .rect;
        assert_eq!(state.controller.layout().focused(), other.id);
        event(
            &mut state,
            id,
            Event::Select(ContainerId::new("a".repeat(64)).unwrap()),
        );
        event(
            &mut state,
            id,
            if kind == PaneView::DockerExec {
                Event::Exec(false)
            } else {
                Event::Open(kind)
            },
        );
        assert_eq!(state.controller.layout().panes().len(), 3);
        let after = state
            .geometry()
            .unwrap()
            .panes()
            .iter()
            .find(|p| p.pane == other)
            .unwrap()
            .rect;
        assert_eq!(before, after, "unrelated focused pane must not be split");
        assert!(
            state
                .controller
                .layout()
                .panes()
                .iter()
                .any(|p| state.views.get(&p.view) == Some(&kind))
        );
    }
}

#[test]
fn confirmation_keyboard_uses_source_identity_instead_of_unrelated_focus() {
    let (mut state, fake, id) = setup();
    let source = state.controller.layout().panes()[0];
    let other = crate::workspace::fresh_pane();
    state
        .controller
        .dispatch(Command::Layout(LayoutCommand::Split {
            target: source.id,
            pane: other,
            axis: Axis::Vertical,
            ratio: SplitRatio::HALF,
        }))
        .unwrap();
    event(
        &mut state,
        id,
        Event::Select(ContainerId::new("a".repeat(64)).unwrap()),
    );
    event(&mut state, id, Event::Act(Action::Stop));
    assert_eq!(
        state
            .docker
            .as_ref()
            .unwrap()
            .confirmation
            .as_ref()
            .unwrap()
            .source,
        id
    );
    let _ = crate::workspace::update(&mut state, Message::TerminalInput(b"\r".to_vec()));
    assert_eq!(fake.actions.lock().unwrap().len(), 1);
    assert_eq!(state.controller.layout().focused(), other.id);
}

#[test]
fn narrow_container_details_have_a_real_back_button_and_preserve_selection() {
    use iced::advanced::{Layout, Shell, layout, widget::Tree};
    let (mut state, fake, id) = setup();
    let target = ContainerId::new("a".repeat(64)).unwrap();
    event(&mut state, id, Event::Select(target.clone()));
    let renderer = crate::test_renderer::renderer(iced::Font::default());
    for language in [crate::i18n::Language::Ru, crate::i18n::Language::En] {
        state.language = language;
        let mut found = false;
        for y in (25..120).step_by(3) {
            let size = iced::Size::new(262.0, 150.0); // content inside a 300 x 220 tile
            let mut element =
                crate::docker_view::view(&state, id, PaneView::DockerContainers, 300.0);
            let mut tree = Tree::new(&element);
            let node = element.as_widget().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
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
                    iced::mouse::Cursor::Available(iced::Point::new(15.0, y as f32)),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &iced::Rectangle::with_size(size),
                );
            }
            if messages
                .iter()
                .any(|m| matches!(m, Message::Docker(source, Event::Back) if *source == id))
            {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "minimum-size detail navigation must be reachable in both languages"
        );
    }
    event(&mut state, id, Event::Back);
    let p = &state.docker.as_ref().unwrap().panes[&id];
    assert!(!p.show_details);
    assert_eq!(p.selected, Some(target));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn log_content_has_one_flexible_scroller_instead_of_nested_fixed_height_boxes() {
    use iced::advanced::{
        Layout, layout,
        widget::{Id, Operation, Tree, operation::Scrollable},
    };
    struct Probe(Vec<f32>);
    impl Operation for Probe {
        fn container(
            &mut self,
            _: Option<&Id>,
            _: iced::Rectangle,
            children: &mut dyn FnMut(&mut dyn Operation),
        ) {
            children(self);
        }
        fn scrollable(
            &mut self,
            _: &mut dyn Scrollable,
            _: Option<&Id>,
            bounds: iced::Rectangle,
            _: iced::Rectangle,
            _: iced::Vector,
        ) {
            self.0.push(bounds.height);
        }
    }
    let (mut state, _, id) = setup();
    event(
        &mut state,
        id,
        Event::Select(ContainerId::new("a".repeat(64)).unwrap()),
    );
    event(&mut state, id, Event::Open(PaneView::DockerLogs));
    let logs = state.controller.layout().panes().last().unwrap().view;
    tick(&mut state);
    tick(&mut state);
    let renderer = crate::test_renderer::renderer(iced::Font::default());
    for language in [crate::i18n::Language::Ru, crate::i18n::Language::En] {
        state.language = language;
        let mut heights = Vec::new();
        for height in [150.0, 500.0] {
            let size = iced::Size::new(262.0, height);
            let element = crate::docker_view::view(&state, logs, PaneView::DockerLogs, 300.0);
            let mut tree = Tree::new(&element);
            let node = element.as_widget().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, size),
            );
            let mut probe = Probe(Vec::new());
            element
                .as_widget()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut probe);
            assert_eq!(
                probe.0.len(),
                1,
                "one content scroller; controls/footer stay outside"
            );
            assert!(probe.0[0] > 0.0, "minimum pane must retain a log viewport");
            heights.push(probe.0[0]);
        }
        assert!(
            heights[1] > heights[0] + 300.0,
            "log content consumes additional available height"
        );
    }
}

#[test]
fn manual_log_scroll_disables_follow_without_changing_filter_or_pause() {
    let (mut state, _, id) = setup();
    state.views.insert(id, PaneView::DockerLogs);
    event(&mut state, id, Event::Query("error".into()));
    event(&mut state, id, Event::Channel(Channel::Stderr));
    event(&mut state, id, Event::Pause);
    event(&mut state, id, Event::Scrolled(false));
    let p = &state.docker.as_ref().unwrap().panes[&id];
    assert!(!p.follow && p.paused);
    assert_eq!(p.query, "error");
    assert_eq!(p.channel, Channel::Stderr);
    let entries = [
        (LogChannel::Stdout, "error ignored"),
        (LogChannel::Stderr, "error included"),
        (LogChannel::Stderr, "healthy"),
    ]
    .map(|(channel, text)| LogEntry {
        sequence: 1,
        channel,
        timestamp: None,
        text: text.into(),
        truncated: false,
    });
    let filter = p.log_filter();
    assert_eq!(
        entries
            .iter()
            .filter(|e| filter.matches(e))
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>(),
        vec!["error included"]
    );
    event(&mut state, id, Event::Scrolled(true));
    assert!(!state.docker.as_ref().unwrap().panes[&id].follow);
    event(&mut state, id, Event::Follow(true));
    assert!(state.docker.as_ref().unwrap().panes[&id].follow);
}

#[test]
fn launcher_cannot_bypass_identity_bound_docker_confirmation() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Act(Action::Stop));
    let target = state
        .docker
        .as_ref()
        .unwrap()
        .confirmation
        .as_ref()
        .unwrap()
        .target
        .clone();
    let before = state.controller.layout().clone();
    for e in [
        crate::launcher::Event::Open,
        crate::launcher::Event::Choose(crate::launcher::Choice::Cmd),
    ] {
        let _ = crate::workspace::update(&mut state, Message::Launcher(e));
    }
    assert!(state.launcher.is_none());
    assert_eq!(*state.controller.layout(), before);
    let confirmation = state
        .docker
        .as_ref()
        .unwrap()
        .confirmation
        .as_ref()
        .unwrap();
    assert_eq!(confirmation.target.id, target.id);
    assert_eq!(confirmation.target.incarnation, target.incarnation);
    assert!(fake.actions.lock().unwrap().is_empty());
}

#[test]
fn language_toggle_preserves_docker_confirmation_and_does_not_poll_or_dispatch() {
    use crate::i18n::Language;
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Act(Action::Stop));
    let d = state.docker.as_ref().unwrap();
    let confirmation = d.confirmation.as_ref().unwrap();
    let target = confirmation.target.clone();
    let action = confirmation.action.clone();
    let snapshot = d.snapshot.clone();
    let notice_en = d.notice.render(Language::En);
    let notice_ru = d.notice.render(Language::Ru);
    assert_ne!(notice_en, notice_ru);
    let calls = fake.calls.load(Ordering::SeqCst);
    for language in [Language::En, Language::Ru] {
        let _ = crate::workspace::update(&mut state, Message::Language(language));
        let d = state.docker.as_ref().unwrap();
        let c = d.confirmation.as_ref().unwrap();
        assert_eq!(c.target.id, target.id);
        assert_eq!(c.target.generation, target.generation);
        assert_eq!(c.target.incarnation, target.incarnation);
        assert_eq!(c.action, action);
        assert!(Arc::ptr_eq(&d.snapshot, &snapshot));
        assert_eq!(fake.calls.load(Ordering::SeqCst), calls);
        assert_eq!(fake.polls.load(Ordering::SeqCst), 0);
        assert!(fake.actions.lock().unwrap().is_empty());
        assert!(crate::docker_view::confirmation(&state).is_some());
        for kind in [
            PaneView::DockerContainers,
            PaneView::DockerLogs,
            PaneView::DockerInspect,
        ] {
            let _ = crate::docker_view::view(&state, id, kind, 300.0);
        }
    }
    event(&mut state, id, Event::Confirm(true));
    let actions = fake.actions.lock().unwrap();
    assert_eq!(actions.as_slice(), &[(target.id, action)]);
}

#[test]
fn delayed_log_and_inspect_buttons_cannot_retarget_selection_or_connection() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    let target = state.docker.as_ref().unwrap().target(id).unwrap();
    event(&mut state, id, Event::Step(1));
    for kind in [PaneView::DockerLogs, PaneView::DockerInspect] {
        event(
            &mut state,
            id,
            Event::OpenAt(
                target.generation,
                target.incarnation,
                target.id.clone(),
                kind,
            ),
        );
        assert_eq!(state.controller.layout().panes().len(), 1);
    }
    let mut next = (**fake.frame.lock().unwrap()).clone();
    next.incarnation += 1;
    *fake.frame.lock().unwrap() = Arc::new(next);
    tick(&mut state);
    event(&mut state, id, Event::Select(target.id.clone()));
    for kind in [PaneView::DockerLogs, PaneView::DockerInspect] {
        event(
            &mut state,
            id,
            Event::OpenAt(
                target.generation,
                target.incarnation,
                target.id.clone(),
                kind,
            ),
        );
        assert_eq!(state.controller.layout().panes().len(), 1);
    }
    let current = state.docker.as_ref().unwrap().target(id).unwrap();
    event(
        &mut state,
        id,
        Event::OpenAt(
            current.generation,
            current.incarnation,
            current.id,
            PaneView::DockerLogs,
        ),
    );
    assert_eq!(state.controller.layout().panes().len(), 2);
}

#[test]
fn queued_docker_actions_cannot_execute_after_pane_changes_module() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    let target = state.docker.as_ref().unwrap().target(id).unwrap();
    let pane = state.controller.layout().panes()[0];
    state.configuring = true;
    let _ = crate::workspace::update(&mut state, Message::ChangeView(pane.id, PaneView::Settings));
    event(
        &mut state,
        id,
        Event::ExecAt(
            target.generation,
            target.incarnation,
            target.id.clone(),
            false,
        ),
    );
    event(
        &mut state,
        id,
        Event::ActAt(
            target.generation,
            target.incarnation,
            target.id,
            Action::Stop,
        ),
    );
    event(&mut state, id, Event::Connect);
    event(&mut state, id, Event::Confirm(true));
    assert_eq!(state.controller.layout().panes().len(), 1);
    assert!(state.docker.as_ref().unwrap().confirmation.is_none());
    assert!(fake.actions.lock().unwrap().is_empty());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn connection_failure_drops_ready_log_inspect_and_stats_before_polling() {
    struct MustNotPoll<T: Send>(std::marker::PhantomData<T>);
    impl<T: Send> Reply<T> for MustNotPoll<T> {
        fn poll(&mut self) -> Option<Result<T, DockerError>> {
            unreachable!("old-incarnation ticket must be dropped before polling")
        }
    }
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Open(PaneView::DockerLogs));
    let logs = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.view != id)
        .unwrap()
        .view;
    let p = state.docker.as_mut().unwrap().panes.get_mut(&logs).unwrap();
    p.inspect = Some(Box::new(MustNotPoll(std::marker::PhantomData)));
    p.stats_ticket = Some(Box::new(MustNotPoll(std::marker::PhantomData)));
    let mut next = (**fake.frame.lock().unwrap()).clone();
    next.incarnation += 1;
    next.state = ConnectionState::Retrying(DockerError::Unavailable);
    *fake.frame.lock().unwrap() = Arc::new(next);
    tick(&mut state);
    let p = &state.docker.as_ref().unwrap().panes[&logs];
    assert!(!p.loading());
    assert!(p.reader.is_none() && p.frame.is_none() && p.details.is_none() && p.stats.is_none());
    assert_eq!(fake.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn reconnect_invalidates_confirmation_logs_and_queued_buttons_even_before_ui_tick() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    let target = state.docker.as_ref().unwrap().target(id).unwrap();
    event(&mut state, id, Event::Open(PaneView::DockerLogs));
    tick(&mut state);
    let logs = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.view != id)
        .unwrap()
        .view;
    assert!(state.docker.as_ref().unwrap().panes[&logs].reader.is_some());
    event(&mut state, id, Event::Act(Action::Stop));
    let mut next = (**fake.frame.lock().unwrap()).clone();
    next.incarnation += 1;
    next.revision += 1;
    *fake.frame.lock().unwrap() = Arc::new(next);
    // A delayed confirmation must fail without waiting for the next 100ms tick.
    event(&mut state, id, Event::Confirm(true));
    assert!(fake.actions.lock().unwrap().is_empty());
    tick(&mut state);
    assert_eq!(fake.drops.load(Ordering::SeqCst), 1);
    assert!(state.docker.as_ref().unwrap().panes[&logs].frame.is_none());
    event(&mut state, logs, Event::Subscribe);
    assert!(state.docker.as_ref().unwrap().panes[&logs].logs.is_none());
    event(&mut state, id, Event::Select(target.id.clone()));
    event(
        &mut state,
        id,
        Event::ActAt(
            target.generation,
            target.incarnation,
            target.id,
            Action::Stop,
        ),
    );
    assert!(state.docker.as_ref().unwrap().confirmation.is_none());
    event(&mut state, id, Event::Act(Action::Start));
    event(&mut state, id, Event::Confirm(true));
    assert_eq!(fake.actions.lock().unwrap().len(), 1);
}

#[test]
fn queued_same_endpoint_connect_cannot_reuse_previous_connected_publication() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Connect);
    tick(&mut state);
    let docker = state.docker.as_ref().unwrap();
    assert_eq!(docker.snapshot.state, ConnectionState::Connecting);
    assert!(docker.snapshot.containers.is_empty());
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Act(Action::Stop));
    assert!(state.docker.as_ref().unwrap().confirmation.is_none());
    let mut next = (**fake.frame.lock().unwrap()).clone();
    next.incarnation += 1;
    next.revision += 1;
    *fake.frame.lock().unwrap() = Arc::new(next);
    tick(&mut state);
    assert!(state.docker.as_ref().unwrap().connected());
}

#[test]
fn exec_is_explicit_retained_on_move_and_tombstoned_on_close_undo() {
    use tervix_application::terminal::{
        GridSize, Profile, TerminalError, TerminalProvider, TerminalSession,
    };
    struct Provider(Arc<AtomicUsize>);
    impl TerminalProvider for Provider {
        fn start(
            &self,
            profile: Profile,
            _: GridSize,
        ) -> Result<Box<dyn TerminalSession>, TerminalError> {
            assert!(matches!(profile, Profile::DockerExec(_)));
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(TerminalError::Startup("fixture".into()))
        }
    }
    let (mut state, _, id) = setup();
    let starts = Arc::new(AtomicUsize::new(0));
    state
        .terminals
        .set_provider(Arc::new(Provider(starts.clone())));
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Exec(true));
    let pane = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.view != id)
        .unwrap();
    assert_eq!(state.views[&pane.view], PaneView::DockerExec);
    let Profile::DockerExec(options) = &state.profiles[&pane.view] else {
        unreachable!()
    };
    assert_eq!(options.program, "/bin/bash");
    assert_eq!(options.container.as_str().len(), 64);
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    state.configuring = true;
    let _ = crate::workspace::update(
        &mut state,
        Message::Action(WorkspaceAction::Navigate(
            tervix_application::NavigationCommand::Swap(tervix_application::Direction::Left),
        )),
    );
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let _ = crate::workspace::update(&mut state, Message::Action(WorkspaceAction::Close));
    let _ = crate::workspace::update(&mut state, Message::Action(WorkspaceAction::Undo));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(
        state.terminals.get(pane.view).unwrap().snapshot.lifecycle,
        tervix_application::terminal::Lifecycle::Closed
    );
    let _ = crate::workspace::update(&mut state, Message::Restart(pane.view));
    assert_eq!(starts.load(Ordering::SeqCst), 2);
}

#[test]
fn stale_exec_buttons_cannot_open_against_a_new_target_or_endpoint() {
    let (mut state, _, id) = setup();
    event(&mut state, id, Event::Step(1));
    let target = state.docker.as_ref().unwrap().target(id).unwrap();
    event(
        &mut state,
        id,
        Event::ExecAt(target.generation + 1, 0, target.id.clone(), false),
    );
    assert_eq!(state.controller.layout().panes().len(), 1);
    event(
        &mut state,
        id,
        Event::ExecAt(target.generation, 1, target.id.clone(), false),
    );
    assert_eq!(state.controller.layout().panes().len(), 1);
    event(&mut state, id, Event::Step(1));
    event(
        &mut state,
        id,
        Event::ExecAt(target.generation, 0, target.id, false),
    );
    assert_eq!(state.controller.layout().panes().len(), 1);
}

#[test]
fn pane_creation_and_discovery_never_connect() {
    let (mut state, fake, id) = setup();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    event(&mut state, id, Event::Discover);
    tick(&mut state);
    tick(&mut state);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(state.docker.as_ref().unwrap().contexts.is_none());
    event(&mut state, id, Event::Endpoint(2));
    event(&mut state, id, Event::Connect);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(
        state
            .docker
            .as_ref()
            .unwrap()
            .notice
            .render(crate::i18n::Language::En)
            .contains("Unsupported")
    );
}
#[test]
fn selection_uses_filtered_sorted_identity_not_refresh_index() {
    let (mut state, _, id) = setup();
    event(&mut state, id, Event::Step(1));
    assert_eq!(
        state.docker.as_ref().unwrap().panes[&id]
            .selected
            .as_ref()
            .unwrap()
            .as_str(),
        "b".repeat(64)
    );
    event(&mut state, id, Event::Query("running".into()));
    event(&mut state, id, Event::Step(1));
    assert_eq!(
        state.docker.as_ref().unwrap().panes[&id]
            .selected
            .as_ref()
            .unwrap()
            .as_str(),
        "a".repeat(64)
    );
    event(&mut state, id, Event::Query("no matches".into()));
    event(&mut state, id, Event::Step(1));
    assert!(state.docker.as_ref().unwrap().target(id).is_some());
}
#[test]
fn confirmation_captures_full_target_and_ticket_is_polled_once() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Act(Action::Stop));
    assert!(fake.actions.lock().unwrap().is_empty());
    let c = state
        .docker
        .as_ref()
        .unwrap()
        .confirmation
        .as_ref()
        .unwrap();
    assert_eq!(c.target.name, "alpha");
    assert_eq!(c.target.id.as_str(), "b".repeat(64));
    event(
        &mut state,
        id,
        Event::Select(ContainerId::new("a".repeat(64)).unwrap()),
    );
    event(&mut state, id, Event::Confirm(true));
    tick(&mut state);
    tick(&mut state);
    assert_eq!(fake.actions.lock().unwrap()[0].0.as_str(), "b".repeat(64));
    assert_eq!(fake.polls.load(Ordering::SeqCst), 1);
}
#[test]
fn cancel_and_old_generation_cannot_dispatch() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Act(Action::Restart));
    event(&mut state, id, Event::Confirm(false));
    assert!(fake.actions.lock().unwrap().is_empty());
    let d = state.docker.as_mut().unwrap();
    let old = d.target(id).unwrap();
    d.invalidate();
    d.submit(old, Action::Stop);
    assert!(fake.actions.lock().unwrap().is_empty());
    assert!(d.notice.render(crate::i18n::Language::En).contains("Stale"));
}
#[test]
fn queued_connect_never_adopts_previous_engine_rows() {
    let (mut state, _, id) = setup();
    event(&mut state, id, Event::Endpoint(1));
    event(&mut state, id, Event::Connect);
    tick(&mut state);
    let d = state.docker.as_ref().unwrap();
    assert_eq!(d.snapshot.state, ConnectionState::Connecting);
    assert!(d.snapshot.containers.is_empty());
    assert!(d.target(id).is_none());
}
#[test]
fn logs_close_undo_and_endpoint_switch_release_reader_without_reopening() {
    let (mut state, fake, id) = setup();
    state.configuring = true;
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Open(PaneView::DockerLogs));
    tick(&mut state);
    let log = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.view != id)
        .unwrap();
    assert!(
        state.docker.as_ref().unwrap().panes[&log.view]
            .reader
            .is_some()
    );
    let _ = crate::workspace::update(&mut state, Message::Action(WorkspaceAction::Close));
    assert_eq!(fake.drops.load(Ordering::SeqCst), 1);
    let _ = crate::workspace::update(&mut state, Message::Action(WorkspaceAction::Undo));
    tick(&mut state);
    assert!(
        state.docker.as_ref().unwrap().panes[&log.view]
            .reader
            .is_none()
    );
    event(&mut state, log.view, Event::Subscribe);
    tick(&mut state);
    event(&mut state, log.view, Event::Disconnect);
    assert_eq!(fake.drops.load(Ordering::SeqCst), 2);
}
#[test]
fn pause_preserves_frame_and_resume_exposes_new_overflow_counters() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Open(PaneView::DockerLogs));
    tick(&mut state);
    let log = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.view != id)
        .unwrap()
        .view;
    event(&mut state, log, Event::Pause);
    *fake.source.lock().unwrap() = Arc::new(LogSnapshot {
        revision: 2,
        state: LogState::Ended,
        entries: Arc::from([]),
        dropped_entries: 100,
        truncated_chunks: 9,
    });
    tick(&mut state);
    assert_eq!(
        state.docker.as_ref().unwrap().panes[&log]
            .frame
            .as_ref()
            .unwrap()
            .revision,
        1
    );
    event(&mut state, log, Event::Pause);
    tick(&mut state);
    let frame = state.docker.as_ref().unwrap().panes[&log]
        .frame
        .as_ref()
        .unwrap();
    assert_eq!(frame.dropped_entries, 100);
    assert_eq!(frame.state, LogState::Ended);
}
#[test]
fn duplicate_log_panes_have_independent_readers_and_close_does_not_mutate() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    for _ in 0..2 {
        event(&mut state, id, Event::Open(PaneView::DockerLogs));
        tick(&mut state);
    }
    let d = state.docker.as_ref().unwrap();
    assert_eq!(d.panes.values().filter(|p| p.reader.is_some()).count(), 2);
    assert!(fake.actions.lock().unwrap().is_empty());
    assert_eq!(
        d.panes
            .values()
            .filter_map(|p| p.target.as_ref())
            .map(|t| t.id.clone())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        1
    );
}

#[test]
fn queued_old_button_is_rejected_after_endpoint_generation_change() {
    let (mut state, fake, id) = setup();
    event(&mut state, id, Event::Step(1));
    let target = state.docker.as_ref().unwrap().target(id).unwrap();
    state.docker.as_mut().unwrap().invalidate();
    event(&mut state, id, Event::Step(1));
    event(
        &mut state,
        id,
        Event::ActAt(
            target.generation,
            target.incarnation,
            target.id,
            Action::Stop,
        ),
    );
    assert!(state.docker.as_ref().unwrap().confirmation.is_none());
    assert!(fake.actions.lock().unwrap().is_empty());
}

#[test]
fn pending_log_ticket_is_released_on_close_and_exit_modal_blocks_docker_keys() {
    let (mut state, fake, id) = setup();
    state.configuring = true;
    event(&mut state, id, Event::Step(1));
    event(&mut state, id, Event::Open(PaneView::DockerLogs));
    let _ = crate::workspace::update(&mut state, Message::Action(WorkspaceAction::Close));
    assert_eq!(fake.drops.load(Ordering::SeqCst), 1);
    state.chrome.pending_exit = true;
    event(&mut state, id, Event::Connect);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn native_keyboard_actions_share_confirmation_and_all_panes_render_at_minimum_size() {
    let (mut state, fake, id) = setup();
    state.window_size = iced::Size::new(680.0, 480.0);
    for bytes in [b"j".to_vec(), b"t".to_vec()] {
        let _ = crate::workspace::update(&mut state, Message::TerminalInput(bytes));
    }
    assert!(state.docker.as_ref().unwrap().confirmation.is_some());
    assert!(crate::docker_view::confirmation(&state).is_some());
    let _ = crate::workspace::view(&state);
    let _ = crate::workspace::update(&mut state, Message::TerminalInput(b"n".to_vec()));
    assert!(state.docker.as_ref().unwrap().confirmation.is_some());
    let _ = crate::workspace::update(&mut state, Message::TerminalInput(b"\x1b".to_vec()));
    assert!(state.docker.as_ref().unwrap().confirmation.is_none());
    for kind in [PaneView::DockerLogs, PaneView::DockerInspect] {
        event(&mut state, id, Event::Open(kind));
        tick(&mut state);
        let _ = crate::workspace::view(&state);
    }
    assert!(fake.actions.lock().unwrap().is_empty());
}
