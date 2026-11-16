//! Terminal input/presentation adapter. All session operations are nonblocking application calls.
use crate::workspace::{Action, Message, PaneView, Workspace};
use iced::{
    Color, Element, Point, Rectangle, Renderer, Size, Theme,
    keyboard::{Key, Modifiers, key::Named},
    widget::{button, canvas, column, text},
};
use std::time::Duration;
use tervix_application::{
    ViewId,
    terminal::{Lifecycle, Profile, Snapshot},
};

pub(super) const CELL_WIDTH: f32 = 8.4;
pub(super) const CELL_HEIGHT: f32 = 18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PasteTarget {
    view: ViewId,
    session: tervix_application::SessionId,
}

impl PasteTarget {
    fn current(state: &Workspace) -> Option<Self> {
        let view = focused(state)?;
        Some(Self {
            view,
            session: state.terminals.get(view)?.session_id?,
        })
    }

    fn valid(self, state: &Workspace) -> bool {
        state
            .controller
            .layout()
            .panes()
            .iter()
            .any(|p| p.view == self.view)
            && state.terminals.get(self.view).is_some_and(|e| {
                e.session_id == Some(self.session)
                    && matches!(
                        e.snapshot.lifecycle,
                        Lifecycle::Starting | Lifecycle::Running
                    )
            })
    }
}

pub(super) fn subscription(state: &Workspace) -> iced::Subscription<Message> {
    use iced::futures::SinkExt;
    use std::sync::{Arc, atomic::Ordering};
    let mut signals = state.terminals.wakeups();
    signals.sort_by_key(|s| Arc::as_ptr(s) as usize);
    let ids: Vec<_> = signals.iter().map(|s| Arc::as_ptr(s) as usize).collect();
    let output = if signals.is_empty() {
        iced::Subscription::none()
    } else {
        iced::Subscription::run_with_id(
            ("terminal-output", ids),
            iced::stream::channel(1, move |mut output| async move {
                // This worker only observes coalesced flags. Idle sessions produce no GUI
                // updates; an output flood can queue at most one notification.
                let _ = std::thread::Builder::new()
                    .name("terminal-notifications".into())
                    .spawn(move || {
                        let mut first = true;
                        while !output.is_closed() {
                            let changed = signals.iter().fold(first, |changed, s| {
                                s.swap(false, Ordering::AcqRel) || changed
                            });
                            first = false;
                            if changed
                                && iced::futures::executor::block_on(
                                    output.send(Message::TerminalTick(std::time::Instant::now())),
                                )
                                .is_err()
                            {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(16));
                        }
                    });
                std::future::pending::<()>().await;
            }),
        )
    };
    let resizing = state.terminal_sizes.iter().any(|(id, (size, _))| {
        state.terminals.get(*id).is_some_and(|e| {
            matches!(
                e.snapshot.lifecycle,
                Lifecycle::Running | Lifecycle::Starting
            ) && e.snapshot.size != *size
        })
    });
    iced::Subscription::batch([
        output,
        if resizing {
            iced::time::every(Duration::from_millis(40)).map(Message::TerminalTick)
        } else {
            iced::Subscription::none()
        },
    ])
}

fn profile(view: PaneView) -> Option<Profile> {
    match view {
        PaneView::Terminal => Some(Profile::Default),
        PaneView::PowerShell => Some(Profile::PowerShell),
        PaneView::Cmd => Some(Profile::Cmd),
        PaneView::Wsl => Some(Profile::Wsl(None)),
        _ => None,
    }
}

pub(super) fn reconcile(state: &mut Workspace) {
    let requested: Vec<_> = state
        .controller
        .layout()
        .panes()
        .iter()
        .filter_map(|p| {
            let kind = state.views.get(&p.view).copied().unwrap_or_default();
            if matches!(kind, PaneView::Ssh | PaneView::DockerExec) {
                return state
                    .profiles
                    .get(&p.view)
                    .filter(|p| {
                        matches!(
                            (kind, p),
                            (PaneView::Ssh, Profile::Ssh(_))
                                | (PaneView::DockerExec, Profile::DockerExec(_))
                        )
                    })
                    // Connect/Restart alone create SSH sessions. Switching back
                    // from another shell must not silently reconnect remotely.
                    .filter(|profile| {
                        state
                            .terminals
                            .get(p.view)
                            .is_some_and(|entry| &entry.profile == *profile)
                    })
                    .cloned()
                    .map(|profile| (p.view, profile));
            }
            profile(kind).map(|default| {
                (
                    p.view,
                    if kind == PaneView::Wsl {
                        state
                            .profiles
                            .get(&p.view)
                            .filter(|p| matches!(p, Profile::Wsl(_) | Profile::WslLaunch(_)))
                            .cloned()
                            .unwrap_or(default)
                    } else {
                        default
                    },
                )
            })
        })
        .collect();
    state.terminals.reconcile(requested);
    let reachable = state.controller.reachable_views();
    state.terminals.retain_reachable(&reachable);
    state.views.retain(|id, _| reachable.contains(id));
    state.profiles.retain(|id, _| reachable.contains(id));
    state.ssh_forms.retain(|id, _| reachable.contains(id));
    state.wsl_drafts.retain(|id, _| reachable.contains(id));
    let live: std::collections::HashSet<_> = state
        .controller
        .layout()
        .panes()
        .iter()
        .map(|p| p.view)
        .collect();
    state.terminal_caches.retain(|id, _| live.contains(id));
    state.terminal_sizes.retain(|id, _| live.contains(id));
}

fn focused(state: &Workspace) -> Option<ViewId> {
    let id = state.controller.layout().focused();
    state
        .controller
        .layout()
        .panes()
        .iter()
        .find(|p| p.id == id)
        .filter(|p| {
            let kind = state.views.get(&p.view).copied().unwrap_or_default();
            matches!(kind, PaneView::Ssh | PaneView::DockerExec) || profile(kind).is_some()
        })
        .map(|p| p.view)
}

fn send(state: &mut Workspace, mut bytes: Vec<u8>) {
    if state.configuring || state.pending_close.is_some() || state.pending_paste.is_some() {
        return;
    }
    if let Some(id) = focused(state) {
        if state
            .terminals
            .get(id)
            .is_some_and(|e| e.snapshot.application_cursor)
            && bytes.starts_with(b"\x1b[")
            && bytes.len() == 3
            && b"ABCDHF".contains(&bytes[2])
        {
            bytes[1] = b'O';
        }
        if let Err(error) = state.terminals.input(id, bytes) {
            state.status = error.to_string().into();
        }
    }
}

pub(super) fn update(state: &mut Workspace, message: &Message) -> Option<iced::Task<Message>> {
    if (state.pending_close.is_some() || state.pending_paste.is_some())
        && matches!(
            message,
            Message::Action(_)
                | Message::PaneAction(_, _)
                | Message::ChangeView(_, _)
                | Message::SelectDistribution(_, _)
                | Message::RefreshWsl
        )
    {
        state.status = crate::i18n::Key::PendingAction.into();
        return Some(iced::Task::none());
    }
    match message {
        Message::TerminalTick(now) => {
            for id in state.terminals.poll() {
                state.terminal_caches.entry(id).or_default().clear();
            }
            for (id, (size, since)) in &mut state.terminal_sizes {
                if now.saturating_duration_since(*since) >= Duration::from_millis(120)
                    && state
                        .terminals
                        .get(*id)
                        .is_some_and(|e| e.snapshot.size != *size)
                {
                    let _ = state.terminals.resize(*id, *size);
                    *since = *now;
                }
            }
        }
        Message::TerminalMeasured(id, size) => {
            // Measured after actual widget allocation, disabled through interpolation
            // and pointer gestures. Resize only after 120 ms without a grid change.
            state
                .terminal_sizes
                .insert(*id, (*size, std::time::Instant::now()));
        }
        Message::TerminalInput(bytes) => {
            if state.pending_close.is_some() && (bytes == b"\r" || bytes == b"\x1b") {
                return update(state, &Message::ConfirmClose(bytes == b"\r"));
            }
            if state.pending_paste.is_some() && (bytes == b"\r" || bytes == b"\x1b") {
                return update(state, &Message::ConfirmPaste(bytes == b"\r"));
            }
            send(state, bytes.clone());
        }
        Message::Paste => {
            return Some(
                if state.configuring
                    || state.pending_close.is_some()
                    || state.pending_paste.is_some()
                {
                    iced::Task::none()
                } else if let Some(target) = PasteTarget::current(state) {
                    iced::clipboard::read().map(move |value| Message::Pasted(target, value))
                } else {
                    iced::Task::none()
                },
            );
        }
        Message::Pasted(target, Some(value)) => {
            if !target.valid(state)
                || state.pending_close.is_some()
                || state.pending_paste.is_some()
                || state.configuring
            {
                state.status = crate::i18n::Key::PasteTargetChanged.into();
            } else if value.len() > 65500 {
                state.status = crate::i18n::Key::PasteTooLarge.into();
            } else if value.chars().any(|c| c.is_control() && c != '\t') {
                state.pending_paste = Some((*target, value.clone()));
            } else {
                paste(state, *target, value);
            }
        }
        Message::Pasted(_, None) => {}
        Message::ConfirmPaste(accept) => {
            if let Some((target, value)) = state.pending_paste.take()
                && *accept
            {
                paste(state, target, &value);
            }
        }
        Message::Action(Action::Close) | Message::PaneAction(_, Action::Close) => {
            let id = match message {
                Message::PaneAction(id, _) => *id,
                _ => state.controller.layout().focused(),
            };
            if state.controller.layout().panes().len() > 1
                && state
                    .controller
                    .layout()
                    .panes()
                    .iter()
                    .find(|p| p.id == id)
                    .and_then(|p| state.terminals.get(p.view))
                    .is_some_and(|e| {
                        matches!(
                            e.snapshot.lifecycle,
                            Lifecycle::Running | Lifecycle::Starting
                        )
                    })
            {
                state.pending_close = Some(id);
            } else {
                return None;
            }
        }
        Message::ConfirmClose(accept) => {
            if let Some(id) = state.pending_close.take()
                && *accept
            {
                let task = super::workspace::confirmed_close(state, id);
                reconcile(state);
                return Some(task);
            }
        }
        Message::Restart(id) => {
            let live_terminal = state.controller.layout().panes().iter().any(|p| {
                p.view == *id
                    && state.views.get(id).is_none_or(|kind| {
                        profile(*kind).is_some()
                            || matches!(kind, PaneView::Ssh | PaneView::DockerExec)
                    })
            });
            if live_terminal
                && let Some(entry) = state.terminals.get(*id)
                && !matches!(
                    entry.snapshot.lifecycle,
                    Lifecycle::Running | Lifecycle::Starting
                )
            {
                let profile = entry.profile.clone();
                state.terminals.restart(*id, profile);
            }
        }
        _ => return None,
    }
    Some(iced::Task::none())
}

fn paste(state: &mut Workspace, target: PasteTarget, value: &str) {
    if !target.valid(state) {
        state.status = crate::i18n::Key::PasteEnded.into();
        return;
    }
    let bracketed = state
        .terminals
        .get(target.view)
        .is_some_and(|e| e.snapshot.bracketed_paste);
    let value: String = value
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let bytes = if bracketed {
        format!("\x1b[200~{value}\x1b[201~").into_bytes()
    } else {
        value.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    };
    if let Err(error) = state.terminals.input(target.view, bytes) {
        state.status = error.to_string().into();
    }
}

pub(super) fn key(key: Key, modifiers: Modifiers, text: Option<&str>) -> Option<Message> {
    if modifiers.logo() || (modifiers.control() && modifiers.alt()) {
        return None;
    }
    if modifiers.control()
        && modifiers.shift()
        && matches!(key.as_ref(),Key::Character(c) if c.eq_ignore_ascii_case("v"))
    {
        return Some(Message::Paste);
    }
    let bytes = match key.as_ref() {
        Key::Named(Named::Space) => vec![if modifiers.control() { 0 } else { b' ' }],
        Key::Named(Named::Enter) => vec![b'\r'],
        Key::Named(Named::Backspace) => vec![127],
        Key::Named(Named::Tab) => {
            if modifiers.shift() {
                b"\x1b[Z".to_vec()
            } else {
                vec![b'\t']
            }
        }
        Key::Named(Named::Escape) => vec![27],
        Key::Named(named) => match named {
            Named::ArrowUp => b"\x1b[A".to_vec(),
            Named::ArrowDown => b"\x1b[B".to_vec(),
            Named::ArrowRight => b"\x1b[C".to_vec(),
            Named::ArrowLeft => b"\x1b[D".to_vec(),
            Named::Home => b"\x1b[H".to_vec(),
            Named::End => b"\x1b[F".to_vec(),
            Named::Delete => b"\x1b[3~".to_vec(),
            Named::PageUp => b"\x1b[5~".to_vec(),
            Named::PageDown => b"\x1b[6~".to_vec(),
            _ => return None,
        },
        Key::Character(value) if modifiers.control() => {
            let c = value.chars().next()?.to_ascii_uppercase();
            if ('@'..='_').contains(&c) {
                vec![c as u8 & 0x1f]
            } else {
                return None;
            }
        }
        Key::Character(value) => text.unwrap_or(value).as_bytes().to_vec(),
        _ => text.filter(|s| !s.is_empty())?.as_bytes().to_vec(),
    };
    let bytes = if modifiers.alt() {
        [vec![27], bytes].concat()
    } else {
        bytes
    };
    Some(Message::TerminalInput(bytes))
}

pub(super) fn view(state: &Workspace, id: ViewId) -> Element<'_, Message> {
    let l = state.language;
    use crate::i18n::Key as K;
    let Some(entry) = state.terminals.get(id) else {
        return text(l.text(K::NoTerminal)).into();
    };
    match &entry.snapshot.lifecycle {
        Lifecycle::Starting => text(l.text(K::Starting)).into(),
        Lifecycle::Closing => text(l.text(K::Closing)).into(),
        Lifecycle::Failed(error) => column![
            text(l.format(K::Failed, &[error])),
            button(l.text(K::RestartTerminal)).on_press(Message::Restart(id))
        ]
        .spacing(8)
        .into(),
        lifecycle => {
            let cache = state.terminal_caches.get(&id);
            let screen = canvas::Canvas::new(Screen {
                snapshot: &entry.snapshot,
                cache,
            })
            .width(iced::Length::Fill)
            .height(iced::Length::Fill);
            let screen = Element::new(crate::terminal_measure::Measured {
                content: screen.into(),
                id,
                enabled: state.geometry_motion.is_none() && !state.controller.resize_active(),
            });
            let mut body = column![screen].height(iced::Length::Fill);
            if matches!(entry.profile, Profile::DockerExec(_)) {
                body = body.push(text(l.text(K::ExecDetach)).size(11));
            }
            if !matches!(lifecycle, Lifecycle::Running) {
                body = body
                    .push(text(match lifecycle {
                        Lifecycle::Exited(code) => l.format(K::Exited, &[&code.to_string()]),
                        Lifecycle::Closed => l.text(K::Closed).into(),
                        Lifecycle::Running => l.text(K::Running).into(),
                        Lifecycle::Starting => l.text(K::Starting).into(),
                        Lifecycle::Closing => l.text(K::Closing).into(),
                        Lifecycle::Failed(error) => l.format(K::Failed, &[error]),
                    }))
                    .push(button(l.text(K::RestartTerminal)).on_press(Message::Restart(id)));
            }
            body.into()
        }
    }
}

struct Screen<'a> {
    snapshot: &'a Snapshot,
    cache: Option<&'a canvas::Cache>,
}
impl canvas::Program<Message> for Screen<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let draw = |frame: &mut canvas::Frame| {
            frame.fill_rectangle(
                Point::ORIGIN,
                frame.size(),
                rgb(crate::design::TERMINAL_BACKGROUND),
            );
            for cell in self.snapshot.cells.iter() {
                let position = Point::new(
                    f32::from(cell.column) * CELL_WIDTH,
                    f32::from(cell.row) * CELL_HEIGHT,
                );
                if cell.background != crate::design::TERMINAL_BACKGROUND {
                    frame.fill_rectangle(
                        position,
                        Size::new(CELL_WIDTH, CELL_HEIGHT),
                        rgb(cell.background),
                    );
                }
                if cell.text != " " {
                    frame.fill_text(canvas::Text {
                        content: cell.text.clone(),
                        position,
                        color: rgb(cell.foreground),
                        size: 14.0.into(),
                        font: iced::Font::with_name("Consolas"),
                        line_height: iced::widget::text::LineHeight::Absolute(CELL_HEIGHT.into()),
                        shaping: iced::widget::text::Shaping::Advanced,
                        ..Default::default()
                    });
                }
            }
            if let Some((x, y)) = self.snapshot.cursor {
                frame.fill_rectangle(
                    Point::new(f32::from(x) * CELL_WIDTH, f32::from(y) * CELL_HEIGHT + 16.0),
                    Size::new(CELL_WIDTH, 2.0),
                    Color::from_rgb8(160, 200, 255),
                );
            }
        };
        let geometry = if let Some(cache) = self.cache {
            cache.draw(renderer, bounds.size(), draw)
        } else {
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            draw(&mut frame);
            frame.into_geometry()
        };
        vec![geometry]
    }
}
fn rgb(c: [u8; 3]) -> Color {
    Color::from_rgb8(c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, atomic::AtomicUsize};
    use tervix_application::{SessionId, terminal::*};

    type Inputs = Arc<Mutex<Vec<(SessionId, Vec<u8>)>>>;
    struct Provider(Arc<AtomicUsize>, Inputs);
    struct Session(SessionId, Inputs);
    impl TerminalProvider for Provider {
        fn start(
            &self,
            _: Profile,
            _: GridSize,
        ) -> Result<Box<dyn TerminalSession>, TerminalError> {
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            Ok(Box::new(Session(
                SessionId::from_uuid(uuid::Uuid::from_u128(n as u128)),
                self.1.clone(),
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
        fn input(&self, bytes: Vec<u8>) -> Result<(), TerminalError> {
            self.1
                .lock()
                .map_err(|_| TerminalError::Ended)?
                .push((self.0, bytes));
            Ok(())
        }
        fn resize(&self, _: GridSize) -> Result<(), TerminalError> {
            Ok(())
        }
        fn close(&self) {}
    }

    #[test]
    fn queued_restart_cannot_revive_closed_or_replaced_view()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::workspace::{self, Action};
        for replace in [false, true] {
            let (mut state, _) = workspace::initialize();
            let starts = Arc::new(AtomicUsize::new(0));
            state
                .terminals
                .set_provider(Arc::new(Provider(starts.clone(), Arc::default())));
            reconcile(&mut state);
            state.configuring = true;
            let original = state.controller.layout().panes()[0];
            if replace {
                let _ = workspace::update(
                    &mut state,
                    Message::ChangeView(original.id, PaneView::Settings),
                );
            } else {
                let _ = workspace::update(
                    &mut state,
                    Message::Action(Action::Split(tervix_application::Axis::Vertical)),
                );
                let _ =
                    workspace::update(&mut state, Message::PaneAction(original.id, Action::Close));
                let _ = workspace::update(&mut state, Message::ConfirmClose(true));
            }
            let before = starts.load(std::sync::atomic::Ordering::Relaxed);
            let _ = workspace::update(&mut state, Message::Restart(original.view));
            assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), before);
            assert_eq!(
                state
                    .terminals
                    .get(original.view)
                    .ok_or("tombstone")?
                    .snapshot
                    .lifecycle,
                Lifecycle::Closed
            );
            // Restoring the terminal makes a subsequent explicit Restart valid.
            let _ = workspace::update(
                &mut state,
                if replace {
                    Message::ChangeView(original.id, PaneView::Terminal)
                } else {
                    Message::Action(Action::Undo)
                },
            );
            let _ = workspace::update(&mut state, Message::Restart(original.view));
            assert_eq!(
                starts.load(std::sync::atomic::Ordering::Relaxed),
                before + 1
            );
        }
        Ok(())
    }

    #[test]
    fn launcher_cannot_bypass_multiline_paste_confirmation()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut state, _) = crate::workspace::initialize();
        let starts = Arc::new(AtomicUsize::new(0));
        state
            .terminals
            .set_provider(Arc::new(Provider(starts.clone(), Arc::default())));
        reconcile(&mut state);
        let target = PasteTarget::current(&state).ok_or("paste target")?;
        state.pending_paste = Some((target, "one\ntwo".into()));
        let before = state.controller.layout().clone();
        for event in [
            crate::launcher::Event::Open,
            crate::launcher::Event::Choose(crate::launcher::Choice::Cmd),
        ] {
            let _ = crate::workspace::update(&mut state, Message::Launcher(event));
        }
        assert!(state.launcher.is_none());
        assert_eq!(state.pending_paste, Some((target, "one\ntwo".into())));
        assert_eq!(*state.controller.layout(), before);
        assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), 1);
        Ok(())
    }

    #[test]
    fn terminal_confirmations_block_stale_focus_and_resize_after_language_and_launcher_events()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::workspace::{self, Action};
        for paste in [false, true] {
            let (mut state, _) = workspace::initialize();
            state.reduced_motion = true;
            let starts = Arc::new(AtomicUsize::new(0));
            state
                .terminals
                .set_provider(Arc::new(Provider(starts.clone(), Arc::default())));
            reconcile(&mut state);
            state.configuring = true;
            let _ = workspace::update(
                &mut state,
                Message::Action(Action::Split(tervix_application::Axis::Vertical)),
            );
            state.configuring = true;
            let target = PasteTarget::current(&state).ok_or("paste target")?;
            let before = state.controller.layout().clone();
            let undo = state.controller.undo_depth();
            let history: Vec<_> = state.controller.focus_history().collect();
            let boundary = state.geometry()?.boundaries()[0];
            state.pending_close = (!paste).then_some(before.focused());
            state.pending_paste = paste.then_some((target, "one\ntwo".into()));
            for message in [
                Message::Language(crate::i18n::Language::En),
                Message::Launcher(crate::launcher::Event::Open),
                Message::Launcher(crate::launcher::Event::Choose(crate::launcher::Choice::Cmd)),
                Message::PanePressed(before.panes()[0].id),
                Message::Cursor(Point::new(500., 200.)),
                Message::Begin(boundary),
                Message::Cursor(Point::new(650., 200.)),
                Message::Release,
            ] {
                let _ = workspace::update(&mut state, message);
                assert_eq!(state.controller.layout(), &before);
                assert_eq!(state.controller.undo_depth(), undo);
                assert_eq!(
                    state.controller.focus_history().collect::<Vec<_>>(),
                    history
                );
                assert!(!state.controller.resize_active());
                assert_eq!(PasteTarget::current(&state), Some(target));
                assert!(state.launcher.is_none());
            }
            assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), 2);
            assert_eq!(state.pending_close, (!paste).then_some(before.focused()));
            assert_eq!(
                state.pending_paste,
                paste.then_some((target, "one\ntwo".into()))
            );
            let _ = workspace::update(&mut state, Message::TerminalInput(vec![27]));
            assert!(state.pending_close.is_none() && state.pending_paste.is_none());
        }
        Ok(())
    }

    #[test]
    fn language_toggle_preserves_live_session_layout_and_all_terminal_modal_targets()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::i18n::Language;
        use crate::workspace::{self, Action};
        let (mut state, _) = workspace::initialize();
        let starts = Arc::new(AtomicUsize::new(0));
        let inputs: Inputs = Arc::default();
        state
            .terminals
            .set_provider(Arc::new(Provider(starts.clone(), inputs.clone())));
        reconcile(&mut state);
        state.configuring = true;
        let _ = workspace::update(
            &mut state,
            Message::Action(Action::Split(tervix_application::Axis::Vertical)),
        );
        state.configuring = false;
        let target = PasteTarget::current(&state).ok_or("paste target")?;
        let before = state.controller.layout().clone();
        let undo = state.controller.undo_depth();
        let original_status = state.status.clone();
        for modal in 0..3 {
            state.pending_close = (modal == 0).then_some(state.controller.layout().focused());
            state.pending_paste = (modal == 1).then_some((target, "первая\nsecond {0}".into()));
            state.chrome.pending_exit = modal == 2;
            let pending_close = state.pending_close;
            let pending_paste = state.pending_paste.clone();
            for language in [Language::En, Language::Ru] {
                let _ = workspace::update(&mut state, Message::Language(language));
                assert_eq!(state.language, language);
                assert_eq!(state.controller.layout(), &before);
                assert_eq!(state.controller.undo_depth(), undo);
                assert_eq!(state.status, original_status);
                assert_eq!(state.pending_close, pending_close);
                assert_eq!(state.pending_paste, pending_paste);
                assert_eq!(state.chrome.pending_exit, modal == 2);
                assert_eq!(PasteTarget::current(&state), Some(target));
                assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), 2);
                assert!(inputs.lock().map_err(|_| "fixture lock")?.is_empty());
                let _ = workspace::view(&state);
            }
        }
        state.chrome.pending_exit = false;
        let _ = workspace::update(
            &mut state,
            Message::TerminalInput("echo Привет".as_bytes().to_vec()),
        );
        assert_eq!(
            inputs.lock().map_err(|_| "fixture lock")?[0].1,
            "echo Привет".as_bytes()
        );
        Ok(())
    }

    #[test]
    fn workspace_exit_cancels_resize_and_blocks_queued_restart()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::chrome::Event;
        let (mut state, _) = crate::workspace::initialize();
        let starts = Arc::new(AtomicUsize::new(0));
        state
            .terminals
            .set_provider(Arc::new(Provider(starts.clone(), Arc::default())));
        reconcile(&mut state);
        let view = state.controller.layout().panes()[0].view;
        state.controller.begin_resize()?;
        let _ = crate::workspace::update(&mut state, Message::Chrome(Event::Close));
        assert!(state.chrome.pending_exit);
        assert!(!state.controller.resize_active());
        let before = starts.load(std::sync::atomic::Ordering::Relaxed);
        let _ = crate::workspace::update(&mut state, Message::Restart(view));
        assert_eq!(starts.load(std::sync::atomic::Ordering::Relaxed), before);
        let _ = crate::workspace::update(&mut state, Message::TerminalInput(vec![27]));
        assert!(!state.chrome.pending_exit);
        assert!(!state.chrome.shutting_down);
        Ok(())
    }

    #[test]
    #[allow(clippy::unwrap_used)] // Deterministic mock identities and unpoisoned fixture locks.
    fn delayed_clipboard_and_confirmation_keep_original_session() {
        let (mut state, _) = crate::workspace::initialize();
        let inputs: Inputs = Arc::default();
        state
            .terminals
            .set_provider(Arc::new(Provider(Arc::default(), inputs.clone())));
        reconcile(&mut state);
        let original = PasteTarget::current(&state).unwrap();
        let _ = update(&mut state, &Message::Paste);
        state.configuring = true;
        let _ = crate::workspace::update(
            &mut state,
            Message::Action(Action::Split(tervix_application::Axis::Vertical)),
        );
        assert_ne!(focused(&state), Some(original.view));
        state.configuring = false;
        let _ = update(&mut state, &Message::Pasted(original, Some("hello".into())));
        assert_eq!(
            *inputs.lock().unwrap(),
            vec![(original.session, b"hello".to_vec())]
        );
        let _ = update(
            &mut state,
            &Message::Pasted(original, Some("one\ntwo".into())),
        );
        assert!(state.pending_paste.is_some());
        state.terminals.restart(original.view, Profile::Default);
        let _ = update(&mut state, &Message::ConfirmPaste(true));
        assert_eq!(inputs.lock().unwrap().len(), 1);
        let _ = update(&mut state, &Message::Pasted(original, Some("late".into())));
        assert_eq!(inputs.lock().unwrap().len(), 1);
        let target = PasteTarget {
            view: original.view,
            session: state
                .terminals
                .get(original.view)
                .unwrap()
                .session_id
                .unwrap(),
        };
        let _ = update(
            &mut state,
            &Message::Pasted(target, Some("confirm\nme".into())),
        );
        let _ = update(&mut state, &Message::ConfirmPaste(true));
        assert_eq!(
            inputs.lock().unwrap()[1],
            (target.session, b"confirm\rme".to_vec())
        );
    }

    #[test]
    #[allow(clippy::unwrap_used)] // Deterministic mock identities and unpoisoned fixture locks.
    fn closed_pane_releases_ui_allocations_and_delayed_paste() {
        let (mut state, _) = crate::workspace::initialize();
        state.configuring = true;
        let inputs: Inputs = Arc::default();
        state
            .terminals
            .set_provider(Arc::new(Provider(Arc::default(), inputs.clone())));
        reconcile(&mut state);
        for _ in 0..70 {
            let _ = crate::workspace::update(
                &mut state,
                Message::Action(Action::Split(tervix_application::Axis::Vertical)),
            );
            let target = PasteTarget::current(&state).unwrap();
            state.terminal_caches.entry(target.view).or_default();
            state.terminal_sizes.insert(
                target.view,
                (GridSize::new(80, 24), std::time::Instant::now()),
            );
            let _ = crate::workspace::update(&mut state, Message::Action(Action::Close));
            let _ = crate::workspace::update(&mut state, Message::ConfirmClose(true));
            let _ = update(&mut state, &Message::Pasted(target, Some("late".into())));
            assert!(state.terminal_caches.is_empty());
            assert!(state.terminal_sizes.is_empty());
            assert!(state.views.len() <= tervix_application::UNDO_LIMIT + 1);
        }
        assert!(inputs.lock().unwrap().is_empty());
    }
    fn bytes(key_value: Key, text: Option<&str>) -> Vec<u8> {
        match key(key_value, Modifiers::empty(), text) {
            Some(Message::TerminalInput(bytes)) => bytes,
            _ => Vec::new(),
        }
    }
    #[test]
    fn named_space_and_unidentified_unicode_text_reach_the_shell() {
        assert_eq!(bytes(Key::Named(Named::Space), Some(" ")), b" ");
        assert_eq!(bytes(Key::Unidentified, Some("開発")), "開発".as_bytes());
        assert_eq!(bytes(Key::Character("a".into()), Some("A")), b"A");
        assert!(
            matches!(key(Key::Character("c".into()),Modifiers::CTRL,None),Some(Message::TerminalInput(bytes)) if bytes == [3])
        );
    }
}
