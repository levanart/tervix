use iced::keyboard::{
    self, Key, Modifiers,
    key::{Code, Named, Physical},
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::i18n::{Key as K, Language, Notice, Noun};
use crate::motion::{Motion, Snapshot};
use iced::widget::scrollable;
use iced::{Element, Subscription, Task};
use tervix_application::{
    Axis, Command, Direction, Layout, LayoutCommand, LayoutController, LayoutGeometry,
    NavigationCommand, Pane, PaneId, Size, SplitBoundary, SplitRatio, ViewId,
};
use uuid::Uuid;

const MINIMUM: (u32, u32) = (300, 220);
const SCROLLER: &str = "workspace-canvas";
const TRANSITION: Duration = Duration::from_millis(160);

pub(crate) struct Workspace {
    pub(super) docking: Option<crate::docking::Drag>,
    pub(super) dock_point: Option<iced::Point>,
    pub(super) launcher: Option<crate::launcher::Launcher>,
    pub(super) language: Language,
    pub(super) docker: Option<crate::docker::Docker>,
    pub(super) chrome: crate::chrome::Chrome,
    pub(super) monitor: Option<crate::monitoring::Monitor>,
    pub(super) process_views: HashMap<ViewId, crate::monitoring::ProcessView>,
    pub(super) history_seconds: HashMap<ViewId, u64>,
    pub(super) profiles: HashMap<ViewId, tervix_application::terminal::Profile>,
    pub(super) ssh_forms: HashMap<ViewId, crate::ssh::Form>,
    pub(super) distributions: Option<tervix_application::wsl::DistributionCatalog>,
    pub(super) discovery_error: Option<String>,
    pub(super) terminals: tervix_application::terminal_registry::TerminalRegistry,
    pub(super) terminal_sizes: HashMap<ViewId, (tervix_application::terminal::GridSize, Instant)>,
    pub(super) terminal_caches: HashMap<ViewId, iced::widget::canvas::Cache>,
    pub(super) pending_paste: Option<(crate::terminal::PasteTarget, String)>,
    pub(super) pending_close: Option<PaneId>,
    pub(super) wsl_drafts: HashMap<ViewId, crate::wsl::Choice>,
    pub(super) pending_wsl: Option<(ViewId, tervix_application::terminal::Profile)>,
    pub(super) controller: LayoutController,
    pub(super) window_size: iced::Size,
    pub(super) status: Notice,
    pub(super) configuring: bool,
    pub(super) reduced_motion: bool,
    pub(super) views: HashMap<ViewId, PaneView>,
    pub(super) zoomed: Option<PaneId>,
    cursor: Option<iced::Point>,
    drag: Option<ResizeDrag>,
    transition: Option<Instant>,
    pub(super) accent_progress: f32,
    pub(super) previous_focus: Option<PaneId>,
    pub(super) geometry_motion: Option<Motion>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum PaneView {
    #[default]
    Terminal,
    PowerShell,
    Cmd,
    Wsl,
    Ssh,
    DockerExec,
    Monitor,
    Processes,
    DockerContainers,
    DockerLogs,
    DockerInspect,
    Help,
    Settings,
}

impl PaneView {
    pub(super) fn label(self, language: Language) -> &'static str {
        language.text(match self {
            Self::Terminal => K::Terminal,
            Self::PowerShell => K::PowerShell,
            Self::Cmd => K::Cmd,
            Self::Wsl => K::Wsl,
            Self::Ssh => K::Ssh,
            Self::DockerExec => K::DockerExec,
            Self::Monitor => K::Monitor,
            Self::Processes => K::Processes,
            Self::DockerContainers => K::DockerContainers,
            Self::DockerLogs => K::DockerLogs,
            Self::DockerInspect => K::DockerInspect,
            Self::Help => K::Help,
            Self::Settings => K::Settings,
        })
    }
    pub(super) const ALL: [Self; 13] = [
        Self::Terminal,
        Self::PowerShell,
        Self::Cmd,
        Self::Wsl,
        Self::Ssh,
        Self::DockerExec,
        Self::Monitor,
        Self::Processes,
        Self::DockerContainers,
        Self::DockerLogs,
        Self::DockerInspect,
        Self::Help,
        Self::Settings,
    ];
}

#[derive(Debug, Clone, Copy)]
struct ResizeDrag {
    boundary: SplitBoundary,
    origin: iced::Point,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Action {
    NewTerminal,
    Split(Axis),
    Focus(PaneId),
    Navigate(NavigationCommand),
    Previous,
    Close,
    Undo,
    Configure,
    Zoom,
    CycleView,
    Motion,
    Distribution,
    RefreshDistributions,
    SortProcesses,
    ReverseProcesses,
    HistoryRange,
    SearchProcesses,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    DockStart(PaneId),
    DockPointer(iced::Point),
    DockRelease,
    Launcher(crate::launcher::Event),
    Language(Language),
    Docker(ViewId, crate::docker::Event),
    DockerTick,
    Chrome(crate::chrome::Event),
    Action(Action),
    PanePressed(PaneId),
    Resized(iced::Size),
    Begin(SplitBoundary),
    Cursor(iced::Point),
    Release,
    Cancel,
    ChangeView(PaneId, PaneView),
    PaneAction(PaneId, Action),
    ReducedMotion(bool),
    Frame(Instant),
    TerminalTick(Instant),
    TerminalMeasured(ViewId, tervix_application::terminal::GridSize),
    TerminalInput(Vec<u8>),
    Paste,
    Pasted(crate::terminal::PasteTarget, Option<String>),
    ConfirmPaste(bool),
    ConfirmClose(bool),
    Restart(ViewId),
    ServicesTick(Instant),
    ProcessQuery(ViewId, String),
    ProcessSort(ViewId, crate::monitoring::Sort),
    ProcessSelect(
        ViewId,
        Option<tervix_application::monitoring::ProcessIdentity>,
    ),
    History(ViewId, u64),
    RefreshWsl,
    SelectDistribution(ViewId, crate::wsl::Choice),
    StartWsl(ViewId),
    ConfirmWsl(bool),
    Ssh(ViewId, crate::ssh::Event),
    FocusField(bool),
}

pub(super) fn fresh_pane() -> Pane {
    Pane {
        id: PaneId::from_uuid(Uuid::new_v4()),
        view: ViewId::from_uuid(Uuid::new_v4()),
    }
}

pub(crate) fn initialize() -> (Workspace, Task<Message>) {
    (
        Workspace {
            docking: None,
            dock_point: None,
            launcher: None,
            language: Language::default(),
            docker: None,
            chrome: Default::default(),
            monitor: None,
            process_views: HashMap::new(),
            history_seconds: HashMap::new(),
            profiles: HashMap::new(),
            ssh_forms: HashMap::new(),
            distributions: None,
            discovery_error: None,
            terminals: Default::default(),
            terminal_sizes: HashMap::new(),
            terminal_caches: HashMap::new(),
            pending_paste: None,
            pending_close: None,
            wsl_drafts: HashMap::new(),
            pending_wsl: None,
            controller: LayoutController::new(Layout::single(fresh_pane())),
            window_size: iced::Size::new(1200.0, 820.0),
            status: K::Ready.into(),
            configuring: false,
            reduced_motion: false,
            views: HashMap::new(),
            zoomed: None,
            cursor: None,
            drag: None,
            transition: None,
            accent_progress: 1.0,
            previous_focus: None,
            geometry_motion: None,
        },
        iced::window::get_oldest().map(|id| Message::Chrome(crate::chrome::Event::Ready(id))),
    )
}

pub(crate) fn title(state: &Workspace) -> String {
    format!(
        "Tervix | {}{}{}",
        state
            .language
            .count(state.controller.layout().panes().len(), Noun::Pane),
        crate::monitoring::title_summary(state),
        crate::docker::title_summary(state)
    )
}

impl Workspace {
    pub(super) fn displayed_geometry(&self) -> Snapshot {
        self.geometry_motion
            .as_ref()
            .map(Motion::sample)
            .unwrap_or_else(|| {
                self.geometry()
                    .map(|g| Snapshot::measured(&g, self.zoomed))
                    .unwrap_or_default()
            })
    }

    pub(super) fn retarget_geometry(&mut self, from: Snapshot) {
        self.geometry_motion = if self.reduced_motion || self.drag.is_some() {
            None
        } else {
            self.geometry().ok().and_then(|g| {
                Motion::new(from, Snapshot::measured(&g, self.zoomed), Instant::now())
            })
        };
    }

    pub(super) fn viewport(&self) -> Result<Size, tervix_application::LayoutError> {
        Size::new(
            (self.window_size.width - 32.0).max(1.0) as u32,
            (self.window_size.height - 144.0).max(1.0) as u32,
        )
    }

    pub(super) fn geometry(&self) -> Result<LayoutGeometry, tervix_application::LayoutError> {
        self.controller
            .layout()
            .measure(self.viewport()?, Size::new(MINIMUM.0, MINIMUM.1)?)
    }

    fn command(&self, action: Action) -> Result<Command, tervix_application::LayoutError> {
        let focused = self.controller.layout().focused();
        Ok(match action {
            Action::Split(axis) => Command::Layout(LayoutCommand::Split {
                target: focused,
                pane: fresh_pane(),
                axis,
                ratio: SplitRatio::HALF,
            }),
            Action::Focus(id) => Command::Layout(LayoutCommand::Focus(id)),
            Action::Navigate(action) => Command::Layout(LayoutCommand::Navigate {
                action,
                viewport: self.viewport()?,
                minimum: Size::new(MINIMUM.0, MINIMUM.1)?,
            }),
            Action::Previous => Command::FocusPrevious,
            Action::Close => Command::Layout(LayoutCommand::Close(focused)),
            Action::Undo => Command::Undo,
            Action::NewTerminal
            | Action::Configure
            | Action::Zoom
            | Action::CycleView
            | Action::Motion
            | Action::Distribution
            | Action::RefreshDistributions
            | Action::SortProcesses
            | Action::ReverseProcesses
            | Action::HistoryRange
            | Action::SearchProcesses => {
                return Err(tervix_application::LayoutError::NoSplit);
            }
        })
    }

    pub(super) fn reveal_focus(&self) -> Task<Message> {
        if self.zoomed.is_some() {
            return Task::none();
        }
        let Ok(geometry) = self.geometry() else {
            return Task::none();
        };
        let Ok(viewport) = self.viewport() else {
            return Task::none();
        };
        let Some(tile) = geometry
            .panes()
            .iter()
            .find(|tile| tile.pane.id == self.controller.layout().focused())
        else {
            return Task::none();
        };
        scrollable::scroll_to(
            scrollable::Id::new(SCROLLER),
            scrollable::AbsoluteOffset {
                x: (tile.rect.x + tile.rect.width).saturating_sub(viewport.width()) as f32,
                y: (tile.rect.y + tile.rect.height).saturating_sub(viewport.height()) as f32,
            },
        )
    }

    fn animate(&mut self) {
        self.transition = (!self.reduced_motion).then(Instant::now);
        self.accent_progress = if self.reduced_motion { 1.0 } else { 0.0 };
    }

    pub(super) fn cancel(&mut self) {
        self.docking = None;
        self.dock_point = None;
        if self.controller.resize_active() {
            match self.controller.cancel_resize() {
                Ok(()) => self.status = K::ResizeCancelled.into(),
                Err(error) => self.status = error.to_string().into(),
            }
        }
        self.drag = None;
    }

    fn apply_action(&mut self, action: Action) {
        if !self.configuring && requires_edit(action) {
            self.status = K::EnableEdit.into();
            return;
        }
        self.cancel();
        match action {
            Action::SearchProcesses => {}
            Action::SortProcesses | Action::ReverseProcesses | Action::HistoryRange => {
                crate::monitoring::keyboard(self, action)
            }
            Action::Distribution => crate::wsl::cycle(self),
            Action::RefreshDistributions => {
                crate::wsl::update(self, &Message::RefreshWsl);
            }
            Action::Configure => {
                self.configuring = !self.configuring;
                if !self.configuring {
                    self.zoomed = None;
                }
                self.status = if self.configuring {
                    K::EditStatus
                } else {
                    K::NormalStatus
                }
                .into();
            }
            Action::Zoom => {
                self.zoomed = if self.zoomed.is_some() {
                    None
                } else {
                    Some(self.controller.layout().focused())
                };
                self.status = K::StretchStatus.into();
            }
            Action::CycleView => {
                if self.configuring {
                    let focused = self.controller.layout().focused();
                    if let Some(pane) = self
                        .controller
                        .layout()
                        .panes()
                        .iter()
                        .find(|p| p.id == focused)
                    {
                        let view = self.views.entry(pane.view).or_default();
                        *view = match view {
                            PaneView::Terminal => PaneView::PowerShell,
                            PaneView::PowerShell => PaneView::Cmd,
                            PaneView::Cmd => PaneView::Wsl,
                            PaneView::Wsl => PaneView::Ssh,
                            PaneView::Ssh => PaneView::Monitor,
                            PaneView::DockerExec => PaneView::Monitor,
                            PaneView::Monitor => PaneView::Processes,
                            PaneView::Processes => PaneView::DockerContainers,
                            PaneView::DockerContainers => PaneView::DockerLogs,
                            PaneView::DockerLogs => PaneView::DockerInspect,
                            PaneView::DockerInspect => PaneView::Help,
                            PaneView::Help => PaneView::Settings,
                            PaneView::Settings => PaneView::Terminal,
                        };
                        self.status = K::ViewChanged.into();
                    }
                } else {
                    self.status = K::EnableViewEdit.into();
                }
            }
            Action::Motion => {
                self.reduced_motion = !self.reduced_motion;
                self.status = if self.reduced_motion {
                    K::MotionOn
                } else {
                    K::MotionOff
                }
                .into();
            }
            _ => {
                let old_focus = self.controller.layout().focused();
                let result = self
                    .command(action)
                    .map_err(tervix_application::CommandError::from)
                    .and_then(|command| self.controller.dispatch(command));
                self.status = match result {
                    Ok(()) => K::LayoutUpdated.into(),
                    Err(error) => K::LayoutUnchanged.with([error.to_string()]),
                };
                self.previous_focus = Some(old_focus);
                if self.zoomed.is_some() {
                    self.zoomed = Some(self.controller.layout().focused());
                }
            }
        }
        self.animate();
    }

    fn preview_drag(&mut self, point: iced::Point) {
        let Some(drag) = self.drag else { return };
        let boundary = drag.boundary;
        let (delta, start, extent, first_minimum, second_minimum) = match boundary.axis {
            Axis::Vertical => (
                point.x - drag.origin.x,
                boundary.rect.x,
                boundary.rect.width,
                boundary.first_minimum.width(),
                boundary.second_minimum.width(),
            ),
            Axis::Horizontal => (
                point.y - drag.origin.y,
                boundary.rect.y,
                boundary.rect.height,
                boundary.first_minimum.height(),
                boundary.second_minimum.height(),
            ),
        };
        // Start from the actual, minimum-clamped divider, not the stored ratio.
        // Both points are window coordinates, so scrolling never offsets the delta.
        let share = (boundary.position - start) as f32 + delta;
        let visible_share = share.clamp(first_minimum as f32, (extent - second_minimum) as f32);
        if (visible_share - (boundary.position - start) as f32).abs() < 0.5 {
            // Restore the exact stored ratio when the divider cannot visibly move,
            // including fully minimum-constrained splits and return-to-origin.
            // A visually unchanged gesture must not consume an undo entry.
            self.cancel();
            if self.controller.begin_resize().is_ok() {
                self.drag = Some(drag);
                self.status = K::Resizing.into();
            }
            return;
        }
        let bps = (share / extent as f32 * 10_000.0)
            .round()
            .clamp(1000.0, 9000.0) as u16;
        let result = (|| {
            Ok::<_, tervix_application::CommandError>(LayoutCommand::ResizeSplit {
                anchor: boundary.anchor,
                ancestor_depth: boundary.ancestor_depth,
                ratio: SplitRatio::new(bps)?,
                viewport: self.viewport()?,
                minimum: Size::new(MINIMUM.0, MINIMUM.1)?,
            })
        })()
        .and_then(|command| self.controller.preview_resize(command));
        if let Err(error) = result {
            self.status = K::ResizeUnchanged.with([error.to_string()]);
        }
    }

    pub(super) fn dragging_boundary(&self, boundary: SplitBoundary) -> bool {
        self.drag.is_some_and(|drag| {
            drag.boundary.anchor == boundary.anchor
                && drag.boundary.ancestor_depth == boundary.ancestor_depth
        })
    }
}

pub(crate) fn update(state: &mut Workspace, message: Message) -> Task<Message> {
    // Pure presentation setting, including during confirmation dialogs. No reconcile,
    // focus operation, lifecycle transition or modal target mutation is permitted.
    if let Message::Language(language) = message {
        state.docking = None;
        state.dock_point = None;
        state.language = language;
        return Task::none();
    }
    let modal = state.pending_wsl.is_some()
        || state.pending_close.is_some()
        || state.pending_paste.is_some()
        || state.chrome.pending_exit
        || state
            .docker
            .as_ref()
            .is_some_and(|d| d.confirmation.is_some());
    if modal {
        if let Message::Chrome(event) = &message
            && !matches!(
                event,
                crate::chrome::Event::Ready(_)
                    | crate::chrome::Event::Moved
                    | crate::chrome::Event::Maximized(_)
                    | crate::chrome::Event::ResizeResult(_)
            )
            && !(state.chrome.pending_exit && matches!(event, crate::chrome::Event::ConfirmExit(_)))
        {
            return Task::none();
        }
        if state.pending_wsl.is_some() {
            let accept = match &message {
                Message::ConfirmWsl(value) => Some(*value),
                Message::Cancel => Some(false),
                Message::TerminalInput(bytes) if bytes == b"\r" => Some(true),
                Message::TerminalInput(bytes) if bytes == b"\x1b" => Some(false),
                _ => None,
            };
            if let Some(accept) = accept {
                crate::wsl::confirm(state, accept);
                return Task::none();
            }
        }
        if !matches!(
            message,
            Message::ServicesTick(_)
                | Message::TerminalTick(_)
                | Message::TerminalMeasured(_, _)
                | Message::DockerTick
                | Message::Frame(_)
                | Message::Resized(_)
                | Message::ConfirmClose(_)
                | Message::ConfirmPaste(_)
                | Message::Docker(_, crate::docker::Event::Confirm(_))
                | Message::TerminalInput(_)
                | Message::Cancel
                | Message::Chrome(_)
        ) {
            return Task::none();
        }
        if state.pending_wsl.is_some() && matches!(message, Message::TerminalInput(_)) {
            return Task::none();
        }
    }
    if state.docking.is_some()
        && matches!(
            message,
            Message::Docker(_, _)
                | Message::Chrome(_)
                | Message::Paste
                | Message::Pasted(_, _)
                | Message::PaneAction(_, _)
                | Message::ChangeView(_, _)
                | Message::Action(_)
                | Message::Ssh(_, _)
        )
    {
        state.cancel();
    }
    if let Some(task) = crate::launcher::update(state, &message) {
        return task;
    }
    if matches!(message, Message::Action(action) | Message::PaneAction(_, action)
        if !state.configuring && requires_edit(action))
    {
        state.status = K::EnableEdit.into();
        return Task::none();
    }
    // Widget messages can already be queued when a terminal confirmation opens.
    // Guard before feature reducers (not just terminal input) so hidden controls
    // cannot move focus, begin gestures, restart sessions or edit another pane.
    if (state.pending_close.is_some() || state.pending_paste.is_some())
        && matches!(
            message,
            Message::PanePressed(_)
                | Message::Begin(_)
                | Message::Cursor(_)
                | Message::Release
                | Message::DockStart(_)
                | Message::DockPointer(_)
                | Message::DockRelease
                | Message::Restart(_)
                | Message::ProcessQuery(_, _)
                | Message::ProcessSort(_, _)
                | Message::History(_, _)
                | Message::FocusField(_)
                | Message::ReducedMotion(_)
        )
    {
        return Task::none();
    }
    if let Some(task) = crate::docker::update(state, &message) {
        return task;
    }
    if let Some(task) = crate::chrome::update(state, &message) {
        if state.chrome.pending_exit || state.chrome.shutting_down {
            state.launcher = None;
        }
        crate::docker::reconcile(state);
        return task;
    }
    if let Message::FocusField(backwards) = message {
        return if backwards {
            iced::widget::focus_previous()
        } else {
            iced::widget::focus_next()
        };
    }
    if let Message::TerminalInput(bytes) = &message
        && (bytes == b"\t" || bytes == b"\x1b[Z")
        && state
            .controller
            .layout()
            .panes()
            .iter()
            .find(|p| p.id == state.controller.layout().focused())
            .is_some_and(
                |p| match state.views.get(&p.view).copied().unwrap_or_default() {
                    PaneView::Ssh => !crate::ssh::active(state, p.view),
                    PaneView::Settings
                    | PaneView::Help
                    | PaneView::Monitor
                    | PaneView::Processes
                    | PaneView::DockerContainers
                    | PaneView::DockerLogs
                    | PaneView::DockerInspect => true,
                    PaneView::Terminal
                    | PaneView::PowerShell
                    | PaneView::Cmd
                    | PaneView::Wsl
                    | PaneView::DockerExec => false,
                },
            )
    {
        return if bytes == b"\t" {
            iced::widget::focus_next()
        } else {
            iced::widget::focus_previous()
        };
    }
    if crate::ssh::update(state, &message) {
        return Task::none();
    }
    let window_state = if matches!(message, Message::Resized(_)) {
        state.chrome.id.map(|id| {
            iced::window::get_maximized(id)
                .map(|value| Message::Chrome(crate::chrome::Event::Maximized(value)))
        })
    } else {
        None
    };
    if crate::monitoring::update(state, &message) || crate::wsl::update(state, &message) {
        return Task::none();
    }
    if matches!(message, Message::ServicesTick(_)) {
        return Task::none();
    }
    if let Some(task) = crate::terminal::update(state, &message) {
        if matches!(message, Message::ConfirmClose(_)) {
            crate::monitoring::reconcile(state);
            crate::docker::reconcile(state);
        }
        return task;
    }
    let task = layout_update(state, message);
    crate::terminal::reconcile(state);
    crate::monitoring::reconcile(state);
    crate::docker::reconcile(state);
    Task::batch([task, window_state.unwrap_or_else(Task::none)])
}

fn layout_update(state: &mut Workspace, message: Message) -> Task<Message> {
    let focused_before = state.controller.layout().focused();
    let pointer_focus = matches!(message, Message::PanePressed(_));
    if matches!(message, Message::Action(Action::SearchProcesses)) {
        if let Some(pane) = state
            .controller
            .layout()
            .panes()
            .into_iter()
            .find(|p| p.id == state.controller.layout().focused())
            && state.views.get(&pane.view) == Some(&PaneView::Ssh)
        {
            return iced::widget::text_input::focus(crate::ssh::host_input(pane.view));
        }
        if let Some(pane) = state
            .controller
            .layout()
            .panes()
            .into_iter()
            .find(|p| p.id == state.controller.layout().focused())
            && state.views.get(&pane.view) == Some(&PaneView::Processes)
        {
            return iced::widget::text_input::focus(iced::widget::text_input::Id::new(format!(
                "search-{:?}",
                pane.view
            )));
        }
        return Task::none();
    }
    if matches!(message, Message::Action(action) | Message::PaneAction(_, action) if !state.configuring && requires_edit(action))
    {
        state.status = K::EnableEdit.into();
        return Task::none();
    }
    if matches!(message, Message::Cancel) && state.drag.is_none() && state.docking.is_none() {
        return Task::none();
    }
    // Frames, pointer movement, release and settings do not retarget geometry.
    // In particular, never measure/allocate a snapshot merely to advance time.
    let before = matches!(
        message,
        Message::Resized(_)
            | Message::Action(_)
            | Message::PanePressed(_)
            | Message::PaneAction(_, _)
            | Message::Cancel
    )
    .then(|| state.displayed_geometry());
    match message {
        Message::DockStart(id) => {
            state.start_dock(id);
            return Task::none();
        }
        Message::DockPointer(point) => {
            state.dock_pointer(point);
            return Task::none();
        }
        Message::DockRelease => {
            state.release_dock();
            return Task::none();
        }
        Message::Launcher(_) => return Task::none(),
        Message::Language(_) => return Task::none(),
        Message::Chrome(_) => return Task::none(),
        Message::Docker(_, _) | Message::DockerTick => return Task::none(),
        Message::Ssh(_, _) | Message::FocusField(_) => return Task::none(),
        Message::ServicesTick(_)
        | Message::ProcessQuery(_, _)
        | Message::ProcessSort(_, _)
        | Message::ProcessSelect(_, _)
        | Message::History(_, _)
        | Message::RefreshWsl
        | Message::SelectDistribution(_, _)
        | Message::StartWsl(_)
        | Message::ConfirmWsl(_) => return Task::none(),
        Message::TerminalTick(_)
        | Message::TerminalMeasured(_, _)
        | Message::TerminalInput(_)
        | Message::Paste
        | Message::Pasted(_, _)
        | Message::ConfirmPaste(_)
        | Message::ConfirmClose(_)
        | Message::Restart(_) => return Task::none(),
        Message::Resized(size) => {
            state.cancel();
            state.window_size = size;
        }
        Message::PanePressed(id) => {
            if id == state.controller.layout().focused() {
                return Task::none();
            }
            state.apply_action(Action::Focus(id));
        }
        Message::Action(action) => {
            if matches!(action, Action::Focus(id) if id == state.controller.layout().focused()) {
                // Clicking the active preview should not restart its transition or
                // snap a manually scrolled overflow canvas back to the focused tile.
                return Task::none();
            }
            state.apply_action(action);
        }
        Message::PaneAction(id, action) => {
            if !state.configuring && requires_edit(action) {
                state.status = K::EnableEdit.into();
                return Task::none();
            }
            state.cancel();
            let old_focus = state.controller.layout().focused();
            if state
                .controller
                .dispatch(Command::Layout(LayoutCommand::Focus(id)))
                .is_ok()
            {
                state.apply_action(action);
                state.previous_focus = Some(old_focus);
            }
        }
        Message::Begin(boundary) => {
            // Handles are interactive only at their authoritative measured position.
            // A queued press from a previous frame cannot start a displaced gesture.
            if !state.configuring || state.geometry_motion.is_some() {
                return Task::none();
            }
            state.cancel();
            let valid = state
                .geometry()
                .is_ok_and(|g| g.boundaries().contains(&boundary));
            if state.zoomed.is_none()
                && valid
                && let Some(origin) = state.cursor
            {
                match state.controller.begin_resize() {
                    Ok(()) => {
                        state.drag = Some(ResizeDrag { boundary, origin });
                        state.status = K::Resizing.into();
                    }
                    Err(error) => state.status = error.to_string().into(),
                }
            }
            return Task::none();
        }
        Message::Cursor(point) => {
            state.cursor = Some(point);
            state.preview_drag(point);
            return Task::none();
        }
        Message::Release => {
            if state.controller.resize_active() {
                state.status = match state.controller.commit_resize() {
                    Ok(()) => K::ResizeApplied.into(),
                    Err(error) => error.to_string().into(),
                };
            }
            state.drag = None;
            return Task::none();
        }
        Message::Cancel => {
            if state.drag.is_none() && state.docking.is_none() {
                return Task::none();
            }
            state.cancel();
            if let Some(before) = before {
                state.retarget_geometry(before);
            }
            return Task::none();
        }
        Message::ChangeView(id, view) => {
            if state.configuring {
                state.cancel();
                if let Some(pane) = state
                    .controller
                    .layout()
                    .panes()
                    .iter()
                    .find(|p| p.id == id)
                    .copied()
                {
                    state.apply_action(Action::Focus(id));
                    state.views.insert(pane.view, view);
                    state.status = K::ViewChanged.into();
                }
            }
            return Task::none();
        }
        Message::ReducedMotion(value) => {
            state.reduced_motion = value;
            if value {
                state.transition = None;
                state.accent_progress = 1.0;
                state.geometry_motion = None;
            }
            return Task::none();
        }
        Message::Frame(now) => {
            if let Some(motion) = &mut state.geometry_motion {
                motion.now = now;
                if motion.finished() {
                    state.geometry_motion = None;
                }
            }
            if let Some(start) = state.transition {
                let progress = (now.saturating_duration_since(start).as_secs_f32()
                    / TRANSITION.as_secs_f32())
                .min(1.0);
                state.accent_progress = progress * progress * (3.0 - 2.0 * progress);
                if progress >= 1.0 {
                    state.transition = None;
                }
            }
            return Task::none();
        }
    }
    if state.reduced_motion {
        state.transition = None;
        state.accent_progress = 1.0;
        state.geometry_motion = None;
    } else if let Some(before) = before {
        state.retarget_geometry(before);
    }
    let reveal = state.reveal_focus();
    if !pointer_focus && focused_before != state.controller.layout().focused() {
        // A text field in a sibling pane must not retain keyboard capture after
        // domain navigation moves focus back into a terminal. A nonexistent ID
        // explicitly unfocuses all text inputs via Iced's focus operation.
        Task::batch([
            reveal,
            iced::widget::text_input::focus(iced::widget::text_input::Id::new(
                "workspace-keyboard-router",
            )),
        ])
    } else {
        reveal
    }
}

pub(super) fn confirmed_close(state: &mut Workspace, id: PaneId) -> Task<Message> {
    // The modal has already captured and authorized this close. Completing it
    // must not depend on the presentation mode in which it is acknowledged.
    let editing = state.configuring;
    state.configuring = true;
    let task = layout_update(state, Message::PaneAction(id, Action::Close));
    state.configuring = editing;
    task
}

fn requires_edit(action: Action) -> bool {
    matches!(
        action,
        Action::Zoom
            | Action::Split(_)
            | Action::Close
            | Action::Undo
            | Action::CycleView
            | Action::Navigate(
                NavigationCommand::Resize { .. }
                    | NavigationCommand::Move(_)
                    | NavigationCommand::Swap(_)
            )
    )
}

pub(crate) fn subscription(state: &Workspace) -> Subscription<Message> {
    if state.chrome.shutting_down {
        return iced::time::every(Duration::from_millis(40)).map(Message::TerminalTick);
    }
    Subscription::batch([
        if state.docker.as_ref().is_some_and(|d| d.active()) {
            iced::time::every(Duration::from_millis(100)).map(|_| Message::DockerTick)
        } else {
            Subscription::none()
        },
        crate::terminal::subscription(state),
        if state.monitor.is_some()
            || state.distributions.as_ref().is_some_and(|c| {
                matches!(c.state(), tervix_application::wsl::DiscoveryState::Loading)
            })
        {
            let loading = state.distributions.as_ref().is_some_and(|c| {
                matches!(c.state(), tervix_application::wsl::DiscoveryState::Loading)
            });
            iced::time::every(Duration::from_millis(if loading {
                250
            } else if crate::monitoring::visible(state) {
                500
            } else {
                5000
            }))
            .map(Message::ServicesTick)
        } else {
            Subscription::none()
        },
        iced::window::close_requests().map(|_| Message::Chrome(crate::chrome::Event::Close)),
        iced::event::listen_with(|event, _, _| match event {
            iced::Event::Window(iced::window::Event::Moved(_)) => {
                Some(Message::Chrome(crate::chrome::Event::Moved))
            }
            _ => None,
        }),
        iced::event::listen_with(|event, _, _| match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(key),
                modifiers,
                ..
            }) if modifiers.alt() && modifiers.control() => match key {
                Named::F9 => Some(Message::Chrome(crate::chrome::Event::Minimize)),
                Named::F10 => Some(Message::Chrome(crate::chrome::Event::Maximize)),
                Named::Space => Some(Message::Chrome(crate::chrome::Event::SystemMenu)),
                _ => None,
            },
            _ => None,
        }),
        if state.pending_close.is_some()
            || state.pending_wsl.is_some()
            || state.pending_paste.is_some()
            || state.chrome.pending_exit
            || state
                .docker
                .as_ref()
                .is_some_and(|d| d.confirmation.is_some())
        {
            iced::event::listen_with(|event, status, _| routed_event(event, status, false, true))
        } else if state.launcher.is_some() {
            iced::event::listen_with(|event, _, _| crate::launcher::key(event))
        } else if state.configuring {
            iced::event::listen_with(|event, status, _| routed_event(event, status, true, false))
        } else {
            // Normal-mode hover is handled by widgets. Publishing every global
            // mouse movement would rebuild the complete workspace for no change.
            iced::event::listen_with(|event, status, _| routed_event(event, status, false, false))
        },
        iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
        if (state.transition.is_some() || state.geometry_motion.is_some()) && !state.reduced_motion
        {
            iced::window::frames().map(Message::Frame)
        } else {
            Subscription::none()
        },
    ])
}

fn routed_event(
    event: iced::Event,
    status: iced::event::Status,
    configuring: bool,
    modal: bool,
) -> Option<Message> {
    if modal {
        modal_event_message(event)
    } else if configuring {
        event_message(event, status)
    } else {
        normal_event_message(event, status)
    }
}

fn modal_event_message(event: iced::Event) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => match key {
            Key::Named(Named::Enter) => Some(Message::TerminalInput(vec![b'\r'])),
            Key::Named(Named::Escape) => Some(Message::TerminalInput(vec![27])),
            _ => None,
        },
        _ => None,
    }
}

fn normal_event_message(event: iced::Event, status: iced::event::Status) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            text,
            ..
        }) => physical_shortcut(key.clone(), physical_key, modifiers)
            .map(Message::Action)
            .or_else(|| {
                (status == iced::event::Status::Ignored)
                    .then(|| crate::terminal::key(key, modifiers, text.as_deref()))
                    .flatten()
            }),
        _ => None,
    }
}

fn event_message(event: iced::Event, status: iced::event::Status) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Tab),
            modifiers,
            ..
        }) if status == iced::event::Status::Ignored
            && !modifiers.control()
            && !modifiers.alt() =>
        {
            Some(Message::FocusField(modifiers.shift()))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
            Some(Message::Cursor(position))
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
            Some(Message::Release)
        }
        iced::Event::Window(iced::window::Event::Unfocused)
        | iced::Event::Mouse(iced::mouse::Event::CursorLeft) => Some(Message::Cancel),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        }) => Some(Message::Cancel),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            physical_key,
            modifiers,
            ..
        }) => physical_shortcut(key, physical_key, modifiers).map(Message::Action),
        _ => None,
    }
}

fn physical_shortcut(key: Key, physical: Physical, modifiers: Modifiers) -> Option<Action> {
    // Layout commands use physical letter positions so a non-Latin OS keymap
    // does not make the documented workspace shortcuts inaccessible.
    let letter = match physical {
        Physical::Code(Code::KeyV) => Some("v"),
        Physical::Code(Code::KeyN) => Some("n"),
        Physical::Code(Code::KeyH) => Some("h"),
        Physical::Code(Code::KeyW) => Some("w"),
        Physical::Code(Code::KeyZ) => Some("z"),
        Physical::Code(Code::KeyP) => Some("p"),
        Physical::Code(Code::KeyE) => Some("e"),
        Physical::Code(Code::KeyF) => Some("f"),
        Physical::Code(Code::KeyT) => Some("t"),
        Physical::Code(Code::KeyR) => Some("r"),
        Physical::Code(Code::KeyD) => Some("d"),
        Physical::Code(Code::KeyL) => Some("l"),
        Physical::Code(Code::KeyS) => Some("s"),
        Physical::Code(Code::KeyO) => Some("o"),
        Physical::Code(Code::KeyB) => Some("b"),
        Physical::Code(Code::KeyQ) => Some("q"),
        _ => None,
    };
    shortcut(
        letter.map_or(key, |value| Key::Character(value.into())),
        modifiers,
    )
}

fn shortcut(key: Key, modifiers: Modifiers) -> Option<Action> {
    if !modifiers.control() || !modifiers.alt() || modifiers.logo() {
        return None;
    }
    let direction = match key.as_ref() {
        Key::Named(Named::ArrowLeft) => Some(Direction::Left),
        Key::Named(Named::ArrowRight) => Some(Direction::Right),
        Key::Named(Named::ArrowUp) => Some(Direction::Up),
        Key::Named(Named::ArrowDown) => Some(Direction::Down),
        _ => None,
    };
    if let Some(direction) = direction {
        return Some(Action::Navigate(if modifiers.shift() {
            NavigationCommand::Resize {
                direction,
                step_bps: 500,
            }
        } else {
            NavigationCommand::Focus(direction)
        }));
    }
    if modifiers.shift() {
        return None;
    }
    match key.as_ref() {
        Key::Character(value) if value.eq_ignore_ascii_case("n") => Some(Action::NewTerminal),
        Key::Character(value) if value.eq_ignore_ascii_case("v") => {
            Some(Action::Split(Axis::Vertical))
        }
        Key::Character(value) if value.eq_ignore_ascii_case("h") => {
            Some(Action::Split(Axis::Horizontal))
        }
        Key::Character(value) if value.eq_ignore_ascii_case("w") => Some(Action::Close),
        Key::Character(value) if value.eq_ignore_ascii_case("z") => Some(Action::Undo),
        Key::Character(value) if value.eq_ignore_ascii_case("p") => Some(Action::Previous),
        Key::Character(value) if value.eq_ignore_ascii_case("e") => Some(Action::Configure),
        Key::Character(value) if value.eq_ignore_ascii_case("f") => Some(Action::Zoom),
        Key::Character(value) if value.eq_ignore_ascii_case("t") => Some(Action::CycleView),
        Key::Character(value) if value.eq_ignore_ascii_case("r") => Some(Action::Motion),
        Key::Character(value) if value.eq_ignore_ascii_case("d") => Some(Action::Distribution),
        Key::Character(value) if value.eq_ignore_ascii_case("l") => {
            Some(Action::RefreshDistributions)
        }
        Key::Character(value) if value.eq_ignore_ascii_case("q") => Some(Action::SearchProcesses),
        Key::Character(value) if value.eq_ignore_ascii_case("s") => Some(Action::SortProcesses),
        Key::Character(value) if value.eq_ignore_ascii_case("o") => Some(Action::ReverseProcesses),
        Key::Character(value) if value.eq_ignore_ascii_case("b") => Some(Action::HistoryRange),
        _ => None,
    }
}

pub(crate) fn view(state: &Workspace) -> Element<'_, Message> {
    crate::presentation::view(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_search_keeps_text_but_cannot_trap_reserved_layout_shortcuts() {
        let event = |modifiers| {
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Character("e".into()),
                modified_key: Key::Character("e".into()),
                physical_key: Physical::Code(Code::KeyE),
                location: keyboard::Location::Standard,
                modifiers,
                text: Some("e".into()),
            })
        };
        assert!(
            normal_event_message(event(Modifiers::empty()), iced::event::Status::Captured)
                .is_none()
        );
        for configuring in [false, true] {
            assert!(matches!(
                routed_event(
                    event(Modifiers::CTRL | Modifiers::ALT),
                    iced::event::Status::Captured,
                    configuring,
                    false
                ),
                Some(Message::Action(Action::Configure))
            ));
        }
    }

    #[test]
    #[allow(clippy::unwrap_used)] // Every tested modal key must produce a message.
    fn modal_enter_escape_precede_both_keyboard_modes() {
        for configuring in [false, true] {
            for (key, expected) in [(Named::Enter, b'\r'), (Named::Escape, 27)] {
                let (mut state, _) = initialize();
                state.configuring = true;
                let _ = update(&mut state, Message::Action(Action::Split(Axis::Vertical)));
                state.configuring = configuring;
                let closing = state.controller.layout().focused();
                state.pending_close = Some(closing);
                let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key: Key::Named(key),
                    modified_key: Key::Named(key),
                    physical_key: Physical::Code(Code::Enter),
                    location: keyboard::Location::Standard,
                    modifiers: Modifiers::empty(),
                    text: None,
                });
                let message = routed_event(
                    event,
                    iced::event::Status::Captured,
                    configuring,
                    state.pending_close.is_some(),
                )
                .unwrap();
                assert!(matches!(&message, Message::TerminalInput(bytes) if bytes == &[expected]));
                let _ = update(&mut state, message);
                assert!(state.pending_close.is_none());
                assert_eq!(
                    state.controller.layout().panes().len(),
                    if expected == b'\r' { 1 } else { 2 }
                );
            }
        }
    }

    fn settle(state: &mut Workspace) {
        let _ = update(state, Message::Frame(Instant::now() + TRANSITION));
        assert!(state.geometry_motion.is_none());
    }

    #[test]
    fn geometry_actions_animate_real_allocations_and_retarget_without_jumps()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut state, _) = initialize();
        state.configuring = true;
        let first = state.controller.layout().focused();
        let original = state.displayed_geometry().panes[&first];
        let _ = update(&mut state, Message::Action(Action::Split(Axis::Vertical)));
        assert_eq!(state.displayed_geometry().panes[&first], original);
        let started = state
            .geometry_motion
            .as_ref()
            .ok_or("split geometry motion")?
            .now;
        let _ = update(&mut state, Message::Frame(started + TRANSITION / 2));
        let midway = state.displayed_geometry();
        let destination = Snapshot::measured(&state.geometry()?, None);
        assert!(midway.panes[&first].width < original.width);
        assert!(midway.panes[&first].width > destination.panes[&first].width);
        // Rapid commands start from the currently painted allocation, not from
        // the preceding domain destination. Undo must also animate restoration.
        let _ = update(&mut state, Message::Action(Action::Undo));
        assert_eq!(
            state.displayed_geometry().panes[&first],
            midway.panes[&first]
        );
        settle(&mut state);
        assert_eq!(state.displayed_geometry().panes[&first], original);
        let _ = update(&mut state, Message::Action(Action::Split(Axis::Vertical)));
        settle(&mut state);
        for action in [
            Action::Zoom,
            Action::Zoom,
            Action::Navigate(NavigationCommand::Resize {
                direction: Direction::Left,
                step_bps: 500,
            }),
            Action::Close,
            Action::Undo,
        ] {
            let before = state.displayed_geometry();
            let _ = update(&mut state, Message::Action(action));
            assert!(state.geometry_motion.is_some(), "{action:?}");
            for (id, rect) in &state.displayed_geometry().panes {
                if let Some(previous) = before.panes.get(id) {
                    assert_eq!(rect, previous, "{action:?}");
                }
                assert!(rect.width >= 300.0 && rect.height >= 220.0);
            }
            settle(&mut state);
            assert_eq!(
                state.displayed_geometry().panes,
                Snapshot::measured(&state.geometry()?, state.zoomed).panes
            );
        }
        Ok(())
    }

    #[test]
    fn pointer_tracks_directly_cancel_animates_and_reduced_motion_settles()
    -> Result<(), tervix_application::LayoutError> {
        let mut state = nested_workspace();
        settle(&mut state);
        let boundary = state.geometry()?.boundaries()[0];
        start_drag(&mut state, boundary);
        let _ = update(&mut state, Message::Cursor(iced::Point::new(351.0, 183.0)));
        assert!(state.geometry_motion.is_none());
        assert_eq!(
            state.displayed_geometry().panes,
            Snapshot::measured(&state.geometry()?, None).panes
        );
        let preview = state.displayed_geometry();
        let _ = update(&mut state, Message::Cancel);
        assert!(state.geometry_motion.is_some());
        assert_eq!(state.displayed_geometry().panes, preview.panes);
        let boundary = state.geometry()?.boundaries()[0];
        let _ = update(&mut state, Message::Begin(boundary));
        assert!(
            !state.controller.resize_active(),
            "moving handles are not hit targets"
        );
        let _ = update(&mut state, Message::ReducedMotion(true));
        assert!(state.geometry_motion.is_none());
        assert!(state.transition.is_none());
        let _ = update(&mut state, Message::Action(Action::Zoom));
        assert!(state.geometry_motion.is_none());
        assert_eq!(
            state.displayed_geometry().panes,
            Snapshot::measured(&state.geometry()?, state.zoomed).panes
        );
        Ok(())
    }

    #[test]
    fn idle_has_no_animation_subscription_and_normal_hover_publishes_no_update() {
        use iced::advanced::subscription::into_recipes;
        let (mut state, _) = initialize();
        let idle = into_recipes(subscription(&state)).len();
        assert!(
            normal_event_message(
                iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                    position: iced::Point::new(20.0, 20.0)
                }),
                iced::event::Status::Ignored,
            )
            .is_none()
        );
        state.configuring = true;
        let _ = update(&mut state, Message::Action(Action::Split(Axis::Vertical)));
        assert_eq!(into_recipes(subscription(&state)).len(), idle + 1);
        settle(&mut state);
        assert_eq!(into_recipes(subscription(&state)).len(), idle);
        let _ = update(&mut state, Message::Action(Action::Configure));
        let _ = update(&mut state, Message::Action(Action::Motion));
        assert!(state.reduced_motion);
        assert_eq!(into_recipes(subscription(&state)).len(), idle);
    }

    #[test]
    fn normal_mode_rejects_keyboard_and_queued_pane_stretch_without_side_effects() {
        let mut state = nested_workspace();
        settle(&mut state);
        let original = state.controller.layout().clone();
        let depth = state.controller.undo_depth();
        for message in [
            Message::Action(Action::Split(Axis::Vertical)),
            Message::Action(Action::Close),
            Message::Action(Action::Undo),
            Message::Action(Action::Navigate(NavigationCommand::Move(Direction::Left))),
            Message::Action(Action::Navigate(NavigationCommand::Swap(Direction::Left))),
            Message::Action(Action::Navigate(NavigationCommand::Resize {
                direction: Direction::Left,
                step_bps: 500,
            })),
            Message::Action(Action::Zoom),
            Message::PaneAction(original.panes()[0].id, Action::Zoom),
        ] {
            let _ = update(&mut state, message);
            assert_eq!(*state.controller.layout(), original);
            assert_eq!(state.controller.undo_depth(), depth);
            assert!(state.zoomed.is_none());
            assert!(state.geometry_motion.is_none());
            assert!(state.transition.is_none());
        }
    }

    #[test]
    fn leaving_edit_mode_cancels_active_preview_without_undo_and_release_is_harmless()
    -> Result<(), tervix_application::LayoutError> {
        let mut state = nested_workspace();
        let original = state.controller.layout().clone();
        let depth = state.controller.undo_depth();
        let boundary = state.geometry()?.boundaries()[0];
        start_drag(&mut state, boundary);
        let _ = update(&mut state, Message::Cursor(iced::Point::new(351.0, 183.0)));
        assert_ne!(*state.controller.layout(), original);
        let preview = state.displayed_geometry();
        let _ = update(&mut state, Message::Action(Action::Configure));
        assert!(!state.configuring);
        assert!(state.drag.is_none());
        assert!(!state.controller.resize_active());
        assert_eq!(*state.controller.layout(), original);
        assert_eq!(state.controller.undo_depth(), depth);
        assert_eq!(state.displayed_geometry().panes, preview.panes);
        let _ = update(&mut state, Message::Release);
        assert_eq!(state.controller.undo_depth(), depth);
        settle(&mut state);
        assert!(state.transition.is_none());
        Ok(())
    }

    #[test]
    fn shortcuts_require_layout_chord_and_preserve_terminal_keys() {
        assert!(matches!(
            physical_shortcut(
                Key::Character("т".into()),
                Physical::Code(Code::KeyN),
                Modifiers::CTRL | Modifiers::ALT
            ),
            Some(Action::NewTerminal)
        ));
        assert!(shortcut(Key::Named(Named::Tab), Modifiers::CTRL | Modifiers::ALT).is_none());
        assert!(matches!(
            shortcut(Key::Character("p".into()), Modifiers::CTRL | Modifiers::ALT),
            Some(Action::Previous)
        ));
        assert!(matches!(
            physical_shortcut(
                Key::Character("м".into()),
                Physical::Code(Code::KeyV),
                Modifiers::CTRL | Modifiers::ALT
            ),
            Some(Action::Split(Axis::Vertical))
        ));
        assert!(shortcut(Key::Character("v".into()), Modifiers::CTRL).is_none());
        assert!(shortcut(Key::Character("c".into()), Modifiers::CTRL | Modifiers::ALT).is_none());
        assert!(matches!(
            shortcut(Key::Character("v".into()), Modifiers::CTRL | Modifiers::ALT),
            Some(Action::Split(Axis::Vertical))
        ));
        assert!(matches!(
            shortcut(
                Key::Named(Named::ArrowRight),
                Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT
            ),
            Some(Action::Navigate(NavigationCommand::Resize {
                direction: Direction::Right,
                step_bps: 500
            }))
        ));
    }

    #[test]
    fn ui_actions_reuse_controller_and_preserve_state_on_rejected_close()
    -> Result<(), tervix_application::LayoutError> {
        let (mut state, _) = initialize();
        state.configuring = true;
        let original = state.controller.layout().clone();
        let _ = update(&mut state, Message::Action(Action::Close));
        assert_eq!(*state.controller.layout(), original);
        assert!(
            state
                .status
                .render(Language::En)
                .contains("cannot close the last pane")
        );
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let _ = update(&mut state, Message::Action(Action::Split(axis)));
        }
        assert_eq!(state.controller.layout().panes().len(), 3);
        let _ = update(&mut state, Message::Resized(iced::Size::new(680.0, 480.0)));
        let geometry = state.geometry()?;
        assert!(geometry.overflows_y());
        let _ = update(&mut state, Message::Action(Action::Undo));
        let _ = update(&mut state, Message::Action(Action::Undo));
        assert_eq!(*state.controller.layout(), original);
        Ok(())
    }

    fn nested_workspace() -> Workspace {
        let (mut state, _) = initialize();
        state.configuring = true;
        let first = state.controller.layout().focused();
        let _ = update(
            &mut state,
            Message::Resized(iced::Size::new(2400.0, 1700.0)),
        );
        for axis in [Axis::Vertical, Axis::Horizontal, Axis::Vertical] {
            let _ = update(&mut state, Message::Action(Action::Focus(first)));
            let _ = update(&mut state, Message::Action(Action::Split(axis)));
        }
        state.configuring = false;
        state
    }

    fn start_drag(state: &mut Workspace, boundary: SplitBoundary) {
        state.configuring = true;
        settle(state);
        // Window point deliberately unrelated to canvas coordinates (scrolled canvas).
        let _ = update(state, Message::Cursor(iced::Point::new(271.0, 183.0)));
        let _ = update(state, Message::Begin(boundary));
        assert!(state.controller.resize_active());
    }

    #[test]
    fn every_recursive_divider_resizes_only_in_edit_mode_with_one_undo()
    -> Result<(), tervix_application::LayoutError> {
        for configuring in [false, true] {
            let mut state = nested_workspace();
            state.configuring = configuring;
            let boundaries = state.geometry()?.boundaries().to_vec();
            assert!(boundaries.iter().any(|b| b.ancestor_depth > 0));
            for boundary in boundaries {
                let original = state.controller.layout().clone();
                let history = state.controller.undo_depth();
                if !configuring {
                    settle(&mut state);
                    let _ = update(&mut state, Message::Cursor(iced::Point::new(271.0, 183.0)));
                    let _ = update(&mut state, Message::Begin(boundary));
                    let _ = update(&mut state, Message::Cursor(iced::Point::new(351.0, 263.0)));
                    let _ = update(&mut state, Message::Release);
                    assert!(!state.controller.resize_active());
                    assert_eq!(*state.controller.layout(), original);
                    assert_eq!(state.controller.undo_depth(), history);
                    continue;
                }
                start_drag(&mut state, boundary);
                for step in [20.0, 40.0, 60.0] {
                    let point = match boundary.axis {
                        Axis::Vertical => iced::Point::new(271.0 + step, 183.0),
                        Axis::Horizontal => iced::Point::new(271.0, 183.0 + step),
                    };
                    let _ = update(&mut state, Message::Cursor(point));
                    assert_eq!(state.controller.undo_depth(), history);
                }
                assert_ne!(*state.controller.layout(), original);
                let _ = update(&mut state, Message::Release);
                assert!(!state.controller.resize_active());
                assert_eq!(state.controller.undo_depth(), history + 1);
                let _ = update(&mut state, Message::Action(Action::Undo));
                assert_eq!(*state.controller.layout(), original);
            }
        }
        Ok(())
    }

    #[test]
    fn cancelled_and_noop_drags_do_not_add_history() -> Result<(), tervix_application::LayoutError>
    {
        let mut state = nested_workspace();
        let original = state.controller.layout().clone();
        let depth = state.controller.undo_depth();
        let boundary = state.geometry()?.boundaries()[0];
        for cancellation in [
            Message::Cancel,
            Message::Resized(iced::Size::new(900.0, 600.0)),
        ] {
            start_drag(&mut state, boundary);
            let _ = update(&mut state, Message::Cursor(iced::Point::new(90.0, 90.0)));
            let _ = update(&mut state, cancellation);
            assert_eq!(*state.controller.layout(), original);
            assert_eq!(state.controller.undo_depth(), depth);
            assert!(state.drag.is_none());
            let _ = update(&mut state, Message::Release);
            let _ = update(
                &mut state,
                Message::Resized(iced::Size::new(2400.0, 1700.0)),
            );
        }
        start_drag(&mut state, boundary);
        let _ = update(&mut state, Message::Cursor(iced::Point::new(310.0, 183.0)));
        let _ = update(&mut state, Message::Cursor(iced::Point::new(271.0, 183.0)));
        let _ = update(&mut state, Message::Release);
        assert_eq!(*state.controller.layout(), original);
        assert_eq!(state.controller.undo_depth(), depth);
        Ok(())
    }

    #[test]
    fn minimum_constrained_drag_preserves_exact_ratio_without_phantom_undo()
    -> Result<(), tervix_application::CommandError> {
        for axis in [Axis::Vertical, Axis::Horizontal] {
            let (mut state, _) = initialize();
            let _ = update(&mut state, Message::Resized(iced::Size::new(680.0, 480.0)));
            state
                .controller
                .dispatch(Command::Layout(LayoutCommand::Split {
                    target: state.controller.layout().focused(),
                    pane: fresh_pane(),
                    axis,
                    ratio: SplitRatio::new(1000)?,
                }))?;
            let boundary = state.geometry()?.boundaries()[0];
            let original = state.controller.layout().clone();
            let depth = state.controller.undo_depth();
            start_drag(&mut state, boundary);
            assert!(state.dragging_boundary(boundary));
            let point = match axis {
                Axis::Vertical => iced::Point::new(-1000.0, 183.0),
                Axis::Horizontal => iced::Point::new(271.0, -1000.0),
            };
            let _ = update(&mut state, Message::Cursor(point));
            assert_eq!(*state.controller.layout(), original);
            assert!(state.controller.resize_active());
            let _ = update(&mut state, Message::Release);
            assert_eq!(state.controller.undo_depth(), depth);
            assert!(!state.dragging_boundary(boundary));
        }
        Ok(())
    }

    #[test]
    fn clicking_focused_pane_does_not_restart_animation_or_replace_status() {
        let (mut state, _) = initialize();
        let _ = update(&mut state, Message::Action(Action::Configure));
        let transition = state.transition;
        let status = state.status.clone();
        let focused = state.controller.layout().focused();
        let _ = update(&mut state, Message::Action(Action::Focus(focused)));
        assert_eq!(state.transition, transition);
        assert_eq!(state.status, status);
    }

    #[test]
    fn drag_clamps_minima_and_rejects_stale_handles() -> Result<(), tervix_application::LayoutError>
    {
        let mut state = nested_workspace();
        let boundary = state.geometry()?.boundaries()[0];
        start_drag(&mut state, boundary);
        for x in [-100_000.0, 100_000.0] {
            let _ = update(&mut state, Message::Cursor(iced::Point::new(x, 183.0)));
            for tile in state.geometry()?.panes() {
                assert!(tile.rect.width >= MINIMUM.0);
                assert!(tile.rect.height >= MINIMUM.1);
            }
        }
        let _ = update(&mut state, Message::Release);
        let _ = update(&mut state, Message::Begin(boundary));
        assert!(
            !state.controller.resize_active(),
            "old ratio is a stale handle"
        );
        Ok(())
    }

    #[test]
    fn configuration_views_zoom_and_settings_preserve_layout_and_identity() {
        let mut state = nested_workspace();
        let original = state.controller.layout().clone();
        let focused = original.focused();
        let view_id = original
            .panes()
            .iter()
            .find(|p| p.id == focused)
            .map(|p| p.view);
        let _ = update(&mut state, Message::ChangeView(focused, PaneView::Settings));
        assert!(
            state.views.is_empty(),
            "Layout changes require Edit layout mode"
        );
        let _ = update(&mut state, Message::Action(Action::Configure));
        assert!(state.configuring);
        let _ = update(&mut state, Message::ChangeView(focused, PaneView::Settings));
        assert_eq!(
            view_id.and_then(|id| state.views.get(&id)),
            Some(&PaneView::Settings)
        );
        let _ = update(&mut state, Message::ReducedMotion(true));
        assert!(state.reduced_motion);
        assert!(state.transition.is_none());
        let _ = update(&mut state, Message::PaneAction(focused, Action::Zoom));
        assert_eq!(state.zoomed, Some(focused));
        assert_eq!(*state.controller.layout(), original);
        let _ = update(&mut state, Message::Action(Action::Configure));
        assert!(!state.configuring);
        assert!(state.zoomed.is_none(), "leaving edit mode restores tiles");
        let _ = update(&mut state, Message::Action(Action::Zoom));
        assert!(state.zoomed.is_none());
        assert_eq!(*state.controller.layout(), original);
        assert!(state.transition.is_none());
        // Closed view metadata remains available for layout undo with the same ViewId.
        let _ = update(&mut state, Message::Action(Action::Close));
        let _ = update(&mut state, Message::Action(Action::Undo));
        assert_eq!(*state.controller.layout(), original);
        assert_eq!(
            view_id.and_then(|id| state.views.get(&id)),
            Some(&PaneView::Settings)
        );
    }

    #[test]
    fn accent_animation_is_bounded_and_configuration_chord_is_physical() {
        let (mut state, _) = initialize();
        assert!(state.transition.is_none());
        assert!(matches!(
            physical_shortcut(
                Key::Character("у".into()),
                Physical::Code(Code::KeyE),
                Modifiers::CTRL | Modifiers::ALT
            ),
            Some(Action::Configure)
        ));
        let _ = update(&mut state, Message::Action(Action::Configure));
        let start = state.transition;
        assert!(start.is_some());
        if let Some(start) = start {
            let _ = update(
                &mut state,
                Message::Frame(start + Duration::from_millis(80)),
            );
            assert!((state.accent_progress - 0.5).abs() < 0.01);
            let _ = update(&mut state, Message::Frame(start + TRANSITION));
            assert_eq!(state.accent_progress, 1.0);
            assert!(state.transition.is_none());
        }
    }

    #[test]
    fn captured_global_gesture_events_still_commit_and_cancel()
    -> Result<(), tervix_application::LayoutError> {
        let mut state = nested_workspace();
        let original = state.controller.layout().clone();
        let boundary = state.geometry()?.boundaries()[0];
        let escape = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            modified_key: Key::Named(Named::Escape),
            physical_key: Physical::Code(Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
        });
        for cancel in [escape, iced::Event::Window(iced::window::Event::Unfocused)] {
            start_drag(&mut state, boundary);
            let movement = event_message(
                iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                    position: iced::Point::new(351.0, 183.0),
                }),
                iced::event::Status::Captured,
            );
            assert!(movement.is_some());
            if let Some(message) = movement {
                let _ = update(&mut state, message);
            }
            assert_ne!(*state.controller.layout(), original);
            let message = event_message(cancel, iced::event::Status::Captured);
            assert!(matches!(message, Some(Message::Cancel)));
            if let Some(message) = message {
                let _ = update(&mut state, message);
            }
            assert_eq!(*state.controller.layout(), original);
            assert!(!state.controller.resize_active());
        }
        start_drag(&mut state, boundary);
        let message = event_message(
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            )),
            iced::event::Status::Captured,
        );
        assert!(matches!(message, Some(Message::Release)));
        if let Some(message) = message {
            let _ = update(&mut state, message);
        }
        assert!(!state.controller.resize_active());
        Ok(())
    }

    #[test]
    fn keyboard_configuration_can_reach_all_views_and_settings() {
        let (mut state, _) = initialize();
        let _ = update(&mut state, Message::Action(Action::CycleView));
        assert!(state.views.is_empty());
        let _ = update(&mut state, Message::Action(Action::Configure));
        for expected in [
            PaneView::PowerShell,
            PaneView::Cmd,
            PaneView::Wsl,
            PaneView::Ssh,
            PaneView::Monitor,
            PaneView::Processes,
            PaneView::DockerContainers,
            PaneView::DockerLogs,
            PaneView::DockerInspect,
            PaneView::Help,
            PaneView::Settings,
            PaneView::Terminal,
        ] {
            let _ = update(&mut state, Message::Action(Action::CycleView));
            assert!(state.views.values().any(|view| *view == expected));
            // Every view must also build in clean, editing and stretched layouts.
            let _ = view(&state);
            let _ = update(&mut state, Message::Action(Action::Zoom));
            let _ = view(&state);
            let _ = update(&mut state, Message::Action(Action::Configure));
            let _ = view(&state);
            let _ = update(&mut state, Message::Action(Action::Configure));
            let _ = update(&mut state, Message::Action(Action::Zoom));
        }
        let _ = update(&mut state, Message::Action(Action::Motion));
        assert!(state.reduced_motion);
        assert!(state.transition.is_none());
    }

    #[test]
    fn nested_canvas_builds_with_overflow_controls_and_zoom() {
        let mut state = nested_workspace();
        for size in [
            iced::Size::new(680.0, 480.0),
            iced::Size::new(1200.0, 820.0),
        ] {
            let _ = update(&mut state, Message::Resized(size));
            for configuring in [true, false] {
                state.configuring = configuring;
                let _ = view(&state);
                let _ = update(&mut state, Message::Action(Action::Zoom));
                let _ = view(&state);
                let _ = update(&mut state, Message::Action(Action::Zoom));
            }
        }
    }
}
