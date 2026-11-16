//! Pane-owned presentation over the single injected, nonblocking Engine service.
use std::{collections::HashMap, sync::Arc};

use crate::i18n::{Key as K, Notice};
use crate::workspace::{Action as WorkspaceAction, Message, PaneView, Workspace};
use iced::{
    Task,
    widget::{scrollable, text_input},
};
use tervix_application::{
    Axis, Command, LayoutCommand, Pane, PaneId, SplitRatio, ViewId, docker::*,
};

#[derive(Debug, Clone)]
pub(super) enum Event {
    Setup,
    Back,
    Actions,
    Scrolled(bool),
    Exec(bool),
    ExecAt(u64, u64, ContainerId, bool),
    Endpoint(usize),
    Discover,
    Connect,
    Disconnect,
    Query(String),
    Sort(Sort),
    Select(ContainerId),
    Step(i32),
    Open(PaneView),
    OpenAt(u64, u64, ContainerId, PaneView),
    Subscribe,
    Pause,
    Follow(bool),
    Channel(Channel),
    Copy,
    Inspect,
    Act(Action),
    ActAt(u64, u64, ContainerId, Action),
    Confirm(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Sort {
    #[default]
    Name,
    Status,
    Image,
    Ports,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Channel {
    #[default]
    All,
    Stdout,
    Stderr,
    Combined,
}
impl Channel {
    pub(super) fn value(self) -> Option<LogChannel> {
        match self {
            Self::All => None,
            Self::Stdout => Some(LogChannel::Stdout),
            Self::Stderr => Some(LogChannel::Stderr),
            Self::Combined => Some(LogChannel::Combined),
        }
    }
}

#[derive(Clone)]
pub(super) struct Target {
    pub incarnation: u64,
    pub id: ContainerId,
    pub name: String,
    pub generation: u64,
}
pub(super) struct Confirmation {
    pub source: ViewId,
    pub target: Target,
    pub action: Action,
}
pub(super) struct PaneState {
    pub setup: bool,
    pub actions: bool,
    pub show_details: bool,
    pub query: String,
    pub sort: Sort,
    pub selected: Option<ContainerId>,
    pub target: Option<Target>,
    pub channel: Channel,
    pub paused: bool,
    pub follow: bool,
    pub frame: Option<Arc<LogSnapshot>>,
    pub details: Option<Details>,
    pub stats: Option<Stats>,
    pub error: Option<Notice>,
    reader: Option<Box<dyn LogReader>>,
    logs: Option<Box<dyn Reply<Box<dyn LogReader>>>>,
    inspect: Option<Box<dyn Reply<Details>>>,
    stats_ticket: Option<Box<dyn Reply<Stats>>>,
}
impl Default for PaneState {
    fn default() -> Self {
        Self {
            setup: false,
            actions: false,
            show_details: false,
            query: String::new(),
            sort: Sort::Name,
            selected: None,
            target: None,
            channel: Channel::All,
            paused: false,
            follow: true,
            frame: None,
            details: None,
            stats: None,
            error: None,
            reader: None,
            logs: None,
            inspect: None,
            stats_ticket: None,
        }
    }
}

impl PaneState {
    pub(super) fn log_filter(&self) -> LogFilter {
        LogFilter {
            channel: self.channel.value(),
            contains: self.query.clone(),
        }
    }
    pub(super) fn loading(&self) -> bool {
        self.logs.is_some() || self.inspect.is_some() || self.stats_ticket.is_some()
    }
}
pub(super) struct Docker {
    pub service: Arc<dyn DockerService>,
    pub snapshot: Arc<Snapshot>,
    pub endpoints: Vec<(Notice, Option<Endpoint>)>,
    pub endpoint: usize,
    pub panes: HashMap<ViewId, PaneState>,
    pub confirmation: Option<Confirmation>,
    pub notice: Notice,
    pub generation: u64,
    contexts: Option<Box<dyn Reply<Arc<[Context]>>>>,
    action: Option<Box<dyn Reply<()>>>,
    visible: bool,
    desired_endpoint: Option<Endpoint>,
    pending_connection: Option<u64>,
}
impl Docker {
    pub fn new(service: Arc<dyn DockerService>) -> Self {
        let snapshot = service.snapshot();
        Self {
            service,
            snapshot,
            endpoints: vec![
                (K::LocalEngine.into(), Some(Endpoint::default())),
                (
                    K::DesktopEngine.into(),
                    Some(Endpoint::NamedPipe(
                        "//./pipe/dockerDesktopLinuxEngine".into(),
                    )),
                ),
                (K::IndependentWsl.into(), None),
            ],
            endpoint: 0,
            panes: HashMap::new(),
            confirmation: None,
            notice: K::DockerInitial.into(),
            generation: 0,
            contexts: None,
            action: None,
            visible: false,
            desired_endpoint: None,
            pending_connection: None,
        }
    }
    pub fn active(&self) -> bool {
        self.visible || self.contexts.is_some() || self.action.is_some()
    }
    fn connected(&self) -> bool {
        self.snapshot.state == ConnectionState::Connected
            && self.snapshot.incarnation == self.service.exec_generation()
    }
    fn valid(&self, target: &Target) -> bool {
        target.generation == self.generation
            && target.incarnation == self.snapshot.incarnation
            && self.connected()
    }
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.confirmation = None;
        self.action = None;
        for pane in self.panes.values_mut() {
            pane.reader = None;
            pane.logs = None;
            pane.inspect = None;
            pane.stats_ticket = None;
            pane.frame = None;
            pane.details = None;
            pane.stats = None;
            pane.selected = None;
            if pane.target.is_some() {
                pane.error = Some(K::EndpointChanged.into());
            }
        }
    }
    fn tick(&mut self) -> Vec<ViewId> {
        let snapshot = self.service.snapshot();
        // Connect is queued: publication may still belong to the previous daemon.
        if snapshot.endpoint == self.desired_endpoint
            && self.pending_connection != Some(snapshot.incarnation)
        {
            if snapshot.incarnation != self.snapshot.incarnation {
                self.invalidate();
            }
            self.snapshot = snapshot;
            self.pending_connection = None;
        }
        if let Some(result) = self.contexts.as_mut().and_then(|t| t.poll()) {
            self.contexts = None;
            match result {
                Ok(contexts) => {
                    self.endpoints.truncate(3);
                    self.endpoints.extend(contexts.iter().map(|c| {
                        (
                            K::Context.with([
                                c.name.clone(),
                                // Star marks the configured context without altering its name.
                                if c.selected {
                                    "*".into()
                                } else {
                                    String::new()
                                },
                                c.error.map(|e| format!(" · {e}")).unwrap_or_default(),
                            ]),
                            c.endpoint.clone(),
                        )
                    }));
                    self.endpoint = self.endpoint.min(self.endpoints.len().saturating_sub(1));
                    self.notice = K::ContextsDiscovered.into();
                }
                Err(e) => self.notice = e.to_string().into(),
            }
        }
        if let Some(result) = self.action.as_mut().and_then(|t| t.poll()) {
            self.action = None;
            self.notice = match result {
                Ok(()) => K::ActionCompleted.into(),
                Err(e) => K::ActionError.with([e.to_string()]),
            };
        }
        let mut scroll = Vec::new();
        for (id, p) in &mut self.panes {
            if let Some(result) = p.logs.as_mut().and_then(|t| t.poll()) {
                p.logs = None;
                match result {
                    Ok(mut reader) => {
                        reader.set_paused(p.paused);
                        p.reader = Some(reader);
                        p.error = None;
                    }
                    Err(e) => p.error = Some(e.to_string().into()),
                }
            }
            if let Some(reader) = &mut p.reader {
                let frame = reader.latest();
                if p.follow
                    && !p.paused
                    && p.frame
                        .as_ref()
                        .is_none_or(|f| f.revision != frame.revision)
                {
                    scroll.push(*id);
                }
                p.frame = Some(frame);
            }
            if let Some(result) = p.inspect.as_mut().and_then(|t| t.poll()) {
                p.inspect = None;
                match result {
                    Ok(d) => p.details = Some(d),
                    Err(e) => p.error = Some(e.to_string().into()),
                }
            }
            if let Some(result) = p.stats_ticket.as_mut().and_then(|t| t.poll()) {
                p.stats_ticket = None;
                match result {
                    Ok(s) => p.stats = Some(s),
                    Err(e) => {
                        p.stats = None;
                        p.error = Some(e.to_string().into());
                    }
                }
            }
        }
        scroll
    }
    fn target(&self, view: ViewId) -> Option<Target> {
        let p = self.panes.get(&view)?;
        if let Some(target) = &p.target {
            return self.valid(target).then(|| target.clone());
        }
        let row = self
            .snapshot
            .containers
            .iter()
            .find(|r| Some(&r.id) == p.selected.as_ref())?;
        self.connected().then(|| Target {
            incarnation: self.snapshot.incarnation,
            id: row.id.clone(),
            name: name(row),
            generation: self.generation,
        })
    }
    fn submit(&mut self, target: Target, action: Action) {
        if !self.valid(&target) {
            self.notice = K::StaleTarget.into();
            return;
        }
        if self.action.is_some() {
            self.notice = K::ActionWait.into();
            return;
        }
        match self.service.act(target.incarnation, target.id, action) {
            Ok(t) => {
                self.action = Some(t);
                self.notice = K::ActionPending.into();
            }
            Err(e) => self.notice = e.to_string().into(),
        }
    }
}
pub(super) fn name(row: &Container) -> String {
    row.names
        .first()
        .cloned()
        .unwrap_or_else(|| row.id.as_str()[..12].into())
}
pub(super) fn ports(row: &Container) -> String {
    row.ports
        .iter()
        .map(|p| {
            format!(
                "{}{}:{}/{}",
                p.host_ip.as_deref().unwrap_or(""),
                p.public.map(|v| format!(":{v} → ")).unwrap_or_default(),
                p.private,
                p.protocol
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}
pub(super) fn rows<'a>(frame: &'a Snapshot, pane: &PaneState) -> Vec<&'a Container> {
    let query = pane.query.to_lowercase();
    let mut rows: Vec<_> = frame
        .containers
        .iter()
        .filter(|r| {
            format!(
                "{} {} {} {} {}",
                name(r),
                r.image,
                r.state,
                r.status,
                ports(r)
            )
            .to_lowercase()
            .contains(&query)
        })
        .collect();
    rows.sort_by_cached_key(|r| {
        (
            match pane.sort {
                Sort::Name => name(r),
                Sort::Status => r.state.clone(),
                Sort::Image => r.image.clone(),
                Sort::Ports => ports(r),
            },
            r.id.clone(),
        )
    });
    rows
}
pub(super) fn is_docker(kind: PaneView) -> bool {
    matches!(
        kind,
        PaneView::DockerContainers | PaneView::DockerLogs | PaneView::DockerInspect
    )
}
pub(super) fn search_id(id: ViewId) -> text_input::Id {
    text_input::Id::new(format!("docker-search-{id:?}"))
}
pub(super) fn scroll_id(id: ViewId) -> scrollable::Id {
    scrollable::Id::new(format!("docker-logs-{id:?}"))
}

pub(super) fn title_summary(state: &Workspace) -> String {
    let Some(pane) = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.id == state.controller.layout().focused())
    else {
        return String::new();
    };
    if state.views.get(&pane.view) == Some(&PaneView::Ssh) {
        return crate::ssh::title_summary(state, pane.view);
    }
    if !is_docker(state.views.get(&pane.view).copied().unwrap_or_default()) {
        return String::new();
    }
    state
        .docker
        .as_ref()
        .map(|d| {
            format!(
                " | Docker {} | {}: {}",
                state.language.text(match d.snapshot.state {
                    ConnectionState::Disconnected => K::DockerDisconnected,
                    ConnectionState::Connecting => K::DockerConnecting,
                    ConnectionState::Connected => K::Running,
                    ConnectionState::Retrying(_) => K::Stale,
                }),
                state.language.text(K::Filter),
                d.panes.get(&pane.view).map_or("", |p| p.query.as_str())
            )
        })
        .unwrap_or_default()
}

pub(super) fn reconcile(state: &mut Workspace) {
    let Some(d) = &mut state.docker else { return };
    let live: Vec<_> = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .filter(|p| is_docker(state.views.get(&p.view).copied().unwrap_or_default()))
        .map(|p| p.view)
        .collect();
    d.visible = !live.is_empty() && !state.chrome.shutting_down;
    d.service.set_visible(d.visible);
    let reachable = state.controller.reachable_views();
    d.panes.retain(|id, _| reachable.contains(id));
    for (id, p) in &mut d.panes {
        if !live.contains(id) || state.chrome.shutting_down {
            p.reader = None;
            p.logs = None;
            p.inspect = None;
            p.stats_ticket = None;
        }
        if state.views.get(id) != Some(&PaneView::DockerLogs) {
            p.reader = None;
            p.logs = None;
        }
    }
    for id in live {
        d.panes.entry(id).or_default();
    }
}

pub(super) fn update(state: &mut Workspace, message: &Message) -> Option<Task<Message>> {
    if state.pending_close.is_some()
        || state.pending_paste.is_some()
        || state.chrome.pending_exit
        || state.chrome.shutting_down
    {
        return matches!(message, Message::Docker(_, _) | Message::DockerTick).then(Task::none);
    }
    let focused = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.id == state.controller.layout().focused());
    let docker_focus =
        focused.filter(|p| is_docker(state.views.get(&p.view).copied().unwrap_or_default()));
    let pending = state.docker.as_ref()?.confirmation.is_some();
    if pending
        && !matches!(
            message,
            Message::Docker(_, Event::Confirm(_))
                | Message::TerminalInput(_)
                | Message::DockerTick
                | Message::TerminalTick(_)
                | Message::ServicesTick(_)
                | Message::Frame(_)
                | Message::Chrome(_)
        )
    {
        return Some(Task::none());
    }
    let event = match message {
        Message::Docker(id, event) => Some((*id, event.clone())),
        Message::TerminalInput(bytes) if pending => {
            if bytes != b"\r" && bytes != b"\x1b" {
                return Some(Task::none());
            }
            state
                .docker
                .as_ref()?
                .confirmation
                .as_ref()
                .map(|c| (c.source, Event::Confirm(bytes == b"\r")))
        }
        Message::Action(_) | Message::PaneAction(_, _) | Message::ChangeView(_, _) if pending => {
            return Some(Task::none());
        }
        Message::Action(WorkspaceAction::SearchProcesses) if docker_focus.is_some() => {
            return docker_focus.map(|p| text_input::focus(search_id(p.view)));
        }
        Message::Action(WorkspaceAction::SortProcesses) if docker_focus.is_some() => docker_focus
            .map(|p| {
                let sort = state
                    .docker
                    .as_ref()
                    .and_then(|d| d.panes.get(&p.view))
                    .map_or(Sort::Name, |p| p.sort);
                (
                    p.view,
                    Event::Sort(match sort {
                        Sort::Name => Sort::Status,
                        Sort::Status => Sort::Image,
                        Sort::Image => Sort::Ports,
                        Sort::Ports => Sort::Name,
                    }),
                )
            }),
        Message::TerminalInput(bytes) if docker_focus.is_some() && !state.configuring => {
            if bytes == b"\t" {
                return Some(iced::widget::focus_next());
            }
            if bytes == b"\x1b[Z" {
                return Some(iced::widget::focus_previous());
            }
            let event = match bytes.as_slice() {
                b"\x1b[A" | b"k" => Some(Event::Step(-1)),
                b"\x1b[B" | b"j" => Some(Event::Step(1)),
                b"l" | b"\r" => Some(Event::Open(PaneView::DockerLogs)),
                b"i" => Some(Event::Open(PaneView::DockerInspect)),
                b"e" => Some(Event::Exec(false)),
                b" " => Some(Event::Pause),
                b"r" => Some(Event::Subscribe),
                b"c" => Some(Event::Connect),
                b"x" => Some(Event::Disconnect),
                b"s" => Some(Event::Act(Action::Start)),
                b"t" => Some(Event::Act(Action::Stop)),
                b"R" => Some(Event::Act(Action::Restart)),
                b"p" => Some(Event::Act(Action::Pause)),
                b"u" => Some(Event::Act(Action::Unpause)),
                b"d" => docker_focus
                    .and_then(|p| state.docker.as_ref()?.target(p.view))
                    .map(|t| Event::Act(Action::Remove { confirmed: t.id })),
                _ => None,
            };
            event.and_then(|e| docker_focus.map(|p| (p.view, e)))
        }
        _ => None,
    };
    if !matches!(message, Message::DockerTick) && event.is_none() {
        return None;
    }
    let mut d = state.docker.take()?;
    let mut task = Task::none();
    if matches!(message, Message::DockerTick) {
        task = Task::batch(
            d.tick()
                .into_iter()
                .map(|id| scrollable::snap_to(scroll_id(id), scrollable::RelativeOffset::END)),
        );
    }
    if let Some((id, event)) = event {
        if pending && !matches!(event, Event::Confirm(_)) {
            state.docker = Some(d);
            return Some(Task::none());
        }
        let live = state
            .controller
            .layout()
            .panes()
            .iter()
            .any(|p| p.view == id);
        if live && is_docker(state.views.get(&id).copied().unwrap_or_default()) {
            task = handle(state, &mut d, id, event);
        }
    }
    state.docker = Some(d);
    reconcile(state);
    Some(task)
}

fn handle(state: &mut Workspace, d: &mut Docker, id: ViewId, event: Event) -> Task<Message> {
    let kind = state.views.get(&id);
    if (matches!(
        event,
        Event::Subscribe
            | Event::Pause
            | Event::Follow(_)
            | Event::Scrolled(_)
            | Event::Channel(_)
            | Event::Copy
    ) && kind != Some(&PaneView::DockerLogs))
        || (matches!(event, Event::Inspect) && kind != Some(&PaneView::DockerInspect))
    {
        return Task::none();
    }
    match event {
        Event::Setup => {
            let p = d.panes.entry(id).or_default();
            p.setup = !p.setup;
        }
        Event::Back => d.panes.entry(id).or_default().show_details = false,
        Event::Actions => {
            let p = d.panes.entry(id).or_default();
            p.actions = !p.actions;
        }
        Event::Scrolled(at_end) => {
            if !at_end {
                d.panes.entry(id).or_default().follow = false;
            }
        }
        Event::Endpoint(index) => {
            if index < d.endpoints.len() {
                d.endpoint = index;
            }
        }
        Event::Discover => match d.service.contexts() {
            Ok(t) => {
                d.contexts = Some(t);
                d.notice = K::ContextsLoading.into();
            }
            Err(e) => d.notice = e.to_string().into(),
        },
        Event::Connect => {
            if let Some(endpoint) = d.endpoints.get(d.endpoint).and_then(|e| e.1.clone()) {
                let previous = d.service.exec_generation();
                match d.service.connect(endpoint.clone()) {
                    Ok(()) => {
                        d.invalidate();
                        d.pending_connection = Some(previous);
                        d.desired_endpoint = Some(endpoint.clone());
                        let mut snapshot = (*d.snapshot).clone();
                        snapshot.endpoint = Some(endpoint);
                        snapshot.state = ConnectionState::Connecting;
                        snapshot.containers = Arc::from([]);
                        snapshot.observed_at = None;
                        d.snapshot = Arc::new(snapshot);
                        d.notice = K::ConnectingHost.into();
                    }
                    Err(e) => d.notice = e.to_string().into(),
                }
            } else {
                d.notice = K::UnsupportedTransport.into();
            }
        }
        Event::Disconnect => match d.service.disconnect() {
            Ok(()) => {
                d.invalidate();
                d.pending_connection = None;
                d.desired_endpoint = None;
                let mut snapshot = (*d.snapshot).clone();
                snapshot.endpoint = None;
                snapshot.state = ConnectionState::Disconnected;
                snapshot.containers = Arc::from([]);
                snapshot.observed_at = None;
                d.snapshot = Arc::new(snapshot);
                d.notice = K::DisconnectedReaders.into();
            }
            Err(e) => d.notice = e.to_string().into(),
        },
        Event::Query(q) => d.panes.entry(id).or_default().query = q,
        Event::Sort(sort) => d.panes.entry(id).or_default().sort = sort,
        Event::Select(selected) => {
            let p = d.panes.entry(id).or_default();
            p.selected = Some(selected);
            p.show_details = true;
            p.actions = false;
        }
        Event::Step(delta) => {
            let p = d.panes.entry(id).or_default();
            let rows = rows(&d.snapshot, p);
            if !rows.is_empty() {
                let index = rows.iter().position(|r| Some(&r.id) == p.selected.as_ref());
                let next = index.map_or(0, |i| {
                    (i as i32 + delta).clamp(0, rows.len() as i32 - 1) as usize
                });
                p.selected = Some(rows[next].id.clone());
            }
        }
        Event::Exec(_) | Event::Open(_) => {
            let kind = if let Event::Open(kind) = event {
                kind
            } else {
                PaneView::DockerExec
            };
            if let Some(target) = d.target(id) {
                let Some(source) = state
                    .controller
                    .layout()
                    .panes()
                    .into_iter()
                    .find(|p| p.view == id)
                else {
                    return Task::none();
                };
                let pane = Pane {
                    id: PaneId::from_uuid(uuid::Uuid::new_v4()),
                    view: ViewId::from_uuid(uuid::Uuid::new_v4()),
                };
                let result = state
                    .controller
                    .dispatch(Command::Layout(LayoutCommand::Split {
                        target: source.id,
                        pane,
                        axis: Axis::Vertical,
                        ratio: SplitRatio::HALF,
                    }));
                match result {
                    Ok(()) => {
                        state.views.insert(pane.view, kind);
                        if let Event::Exec(bash) = event {
                            if let Some(endpoint) = d.snapshot.endpoint.clone() {
                                let profile = tervix_application::terminal::Profile::DockerExec(
                                    ExecOptions {
                                        endpoint,
                                        generation: target.incarnation,
                                        container: target.id,
                                        program: if bash { "/bin/bash" } else { "/bin/sh" }.into(),
                                        args: vec![],
                                    },
                                );
                                state.profiles.insert(pane.view, profile.clone());
                                state.terminals.restart(pane.view, profile);
                                d.notice = K::ExecStarted.into();
                            }
                            return Task::none();
                        }
                        d.panes.insert(
                            pane.view,
                            PaneState {
                                target: Some(target),
                                ..Default::default()
                            },
                        );
                        return handle(
                            state,
                            d,
                            pane.view,
                            if kind == PaneView::DockerLogs {
                                Event::Subscribe
                            } else {
                                Event::Inspect
                            },
                        );
                    }
                    Err(e) => d.notice = e.to_string().into(),
                }
            } else {
                d.notice = K::SelectContainer.into();
            }
        }
        Event::OpenAt(generation, incarnation, container, kind) => {
            if let Some(target) = d.target(id)
                && target.generation == generation
                && target.incarnation == incarnation
                && target.id == container
                && matches!(kind, PaneView::DockerLogs | PaneView::DockerInspect)
            {
                return handle(state, d, id, Event::Open(kind));
            }
            d.notice = K::StaleTarget.into();
        }
        Event::ExecAt(generation, incarnation, container, bash) => {
            if generation != d.generation
                || incarnation != d.service.exec_generation()
                || d.target(id).is_none_or(|target| target.id != container)
            {
                d.notice = K::StaleTarget.into();
            } else {
                return handle(state, d, id, Event::Exec(bash));
            }
        }
        Event::Subscribe | Event::Inspect => {
            if let Some(target) = d.target(id) {
                let p = d.panes.entry(id).or_default();
                p.error = None;
                if matches!(event, Event::Subscribe) {
                    // Queued clicks/key repeats must not replace a live reader.
                    // Ended streams still require an explicit resubscribe.
                    let following = p
                        .reader
                        .as_mut()
                        .is_some_and(|reader| matches!(reader.latest().state, LogState::Following));
                    if p.logs.is_none() && !following {
                        p.reader = None;
                        match d.service.logs(target.incarnation, target.id) {
                            Ok(t) => p.logs = Some(t),
                            Err(e) => p.error = Some(e.to_string().into()),
                        }
                    }
                } else {
                    if p.inspect.is_none() {
                        match d.service.inspect(target.incarnation, target.id.clone()) {
                            Ok(t) => p.inspect = Some(t),
                            Err(e) => p.error = Some(e.to_string().into()),
                        }
                    }
                    if p.stats_ticket.is_none() {
                        match d.service.stats(target.incarnation, target.id) {
                            Ok(t) => p.stats_ticket = Some(t),
                            Err(e) => p.error = Some(e.to_string().into()),
                        }
                    }
                }
            } else {
                d.notice = K::ContainerAbsent.into();
            }
        }
        Event::Pause => {
            let p = d.panes.entry(id).or_default();
            p.paused = !p.paused;
            if let Some(r) = &mut p.reader {
                r.set_paused(p.paused);
            }
        }
        Event::Follow(value) => {
            d.panes.entry(id).or_default().follow = value;
            if value {
                // Follow is navigation, independent of stream pause or revision.
                return scrollable::snap_to(scroll_id(id), scrollable::RelativeOffset::END);
            }
        }
        Event::Channel(channel) => d.panes.entry(id).or_default().channel = channel,
        Event::Copy => {
            if let Some(p) = d.panes.get(&id)
                && let Some(frame) = &p.frame
            {
                let filter = p.log_filter();
                let text = frame
                    .entries
                    .iter()
                    .filter(|e| filter.matches(e))
                    .map(|e| {
                        format!(
                            "{} {:?} {}",
                            e.timestamp.as_deref().unwrap_or(""),
                            e.channel,
                            e.text
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                return iced::clipboard::write(text);
            }
        }
        Event::Act(action) => {
            if let Some(target) = d.target(id) {
                // Every lifecycle transition receives the same identity-bound confirmation.
                d.confirmation = Some(Confirmation {
                    source: id,
                    target,
                    action,
                });
            }
        }
        Event::ActAt(generation, incarnation, container, action) => {
            if let Some(target) = d.target(id)
                && target.generation == generation
                && target.incarnation == incarnation
                && target.id == container
            {
                d.confirmation = Some(Confirmation {
                    source: id,
                    target,
                    action,
                });
            } else {
                d.notice = K::StaleTarget.into();
            }
        }
        Event::Confirm(confirmed) => {
            if let Some(c) = d.confirmation.take() {
                if confirmed {
                    d.submit(c.target, c.action);
                } else {
                    d.notice = K::ActionCancelled.into();
                }
            }
        }
    }
    Task::none()
}

#[cfg(test)]
#[path = "docker_tests.rs"]
mod tests;
