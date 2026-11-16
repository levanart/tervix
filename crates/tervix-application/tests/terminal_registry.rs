//! Resource reconciliation regression tests with a counting I/O boundary.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tervix_application::{ViewId, terminal::*, terminal_registry::TerminalRegistry};
use tervix_core::PaneId;

struct Provider(Arc<AtomicUsize>, Arc<AtomicUsize>);
struct Session(Arc<AtomicUsize>, tervix_application::SessionId);

#[test]
fn actual_docking_and_single_step_undo_keep_fake_sessions_and_focus_history()
-> Result<(), Box<dyn std::error::Error>> {
    use tervix_application::{
        Axis, Command, Direction, Layout, LayoutCommand, LayoutController, Pane, Size, SplitRatio,
    };
    let starts = Arc::new(AtomicUsize::new(0));
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = TerminalRegistry::default();
    registry.set_provider(Arc::new(Provider(starts.clone(), closes.clone())));
    let pane = |n| Pane {
        id: PaneId::from_uuid(uuid::Uuid::from_u128(n)),
        view: ViewId::from_uuid(uuid::Uuid::from_u128(n + 100)),
    };
    let mut controller =
        LayoutController::new(Layout::single(pane(1)).apply(LayoutCommand::Split {
            target: pane(1).id,
            pane: pane(2),
            axis: Axis::Vertical,
            ratio: SplitRatio::HALF,
        })?);
    let reconcile = |registry: &mut TerminalRegistry, controller: &LayoutController| {
        registry.reconcile(
            controller
                .layout()
                .panes()
                .into_iter()
                .map(|p| (p.view, Profile::Cmd)),
        );
    };
    reconcile(&mut registry, &controller);
    let session = registry.get(pane(1).view).and_then(|e| e.session_id);
    let original = controller.layout().clone();
    for command in [
        LayoutCommand::MovePane {
            source: pane(1).id,
            target: pane(2).id,
            side: Direction::Down,
            viewport: Size::new(1200, 800)?,
            minimum: Size::new(300, 220)?,
        },
        LayoutCommand::Swap(pane(1).id, pane(2).id),
    ] {
        controller.dispatch(Command::Layout(command))?;
        assert_eq!(controller.undo_depth(), 1);
        reconcile(&mut registry, &controller);
        assert_eq!(
            registry.get(pane(1).view).and_then(|e| e.session_id),
            session
        );
        assert_eq!(controller.focus_history().count(), 0);
        controller.dispatch(Command::Undo)?;
        reconcile(&mut registry, &controller);
        assert_eq!(controller.layout(), &original);
        assert_eq!(controller.undo_depth(), 0);
        assert_eq!(starts.load(Ordering::Relaxed), 2);
        assert_eq!(closes.load(Ordering::Relaxed), 0);
    }
    Ok(())
}

#[test]
fn docker_exec_identity_survives_moves_and_undo_cannot_recreate_remote_process()
-> Result<(), Box<dyn std::error::Error>> {
    use tervix_application::docker::{ContainerId, Endpoint, ExecOptions};
    let starts = Arc::new(AtomicUsize::new(0));
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = TerminalRegistry::default();
    registry.set_provider(Arc::new(Provider(starts.clone(), closes.clone())));
    let id = ViewId::from_uuid(uuid::Uuid::from_u128(301));
    let other = ViewId::from_uuid(uuid::Uuid::from_u128(302));
    let profile = Profile::DockerExec(ExecOptions {
        endpoint: Endpoint::default(),
        generation: 9,
        container: ContainerId::new("a".repeat(64))?,
        program: "/bin/sh".into(),
        args: vec![],
    });
    registry.reconcile([(id, profile.clone()), (other, Profile::Cmd)]);
    let incarnation = registry.get(id).and_then(|e| e.session_id);
    registry.reconcile([(other, Profile::Cmd), (id, profile.clone())]);
    registry.resize(id, GridSize::new(120, 40))?;
    registry.input(id, b"hello".to_vec())?;
    assert_eq!(registry.get(id).and_then(|e| e.session_id), incarnation);
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(closes.load(Ordering::Relaxed), 0);
    registry.reconcile([(other, Profile::Cmd)]);
    registry.reconcile([(other, Profile::Cmd), (id, profile.clone())]);
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(closes.load(Ordering::Relaxed), 1);
    assert_eq!(
        registry.get(id).map(|e| &e.snapshot.lifecycle),
        Some(&Lifecycle::Closed)
    );
    registry.restart(id, profile);
    assert_ne!(registry.get(id).and_then(|e| e.session_id), incarnation);
    assert_eq!(starts.load(Ordering::Relaxed), 3);
    Ok(())
}

#[test]
fn ssh_profiles_keep_identity_through_topology_and_require_explicit_restart()
-> Result<(), Box<dyn std::error::Error>> {
    use tervix_application::ssh::Connection;
    let starts = Arc::new(AtomicUsize::new(0));
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = TerminalRegistry::default();
    registry.set_provider(Arc::new(Provider(starts.clone(), closes.clone())));
    let id = ViewId::from_uuid(uuid::Uuid::from_u128(101));
    let other = ViewId::from_uuid(uuid::Uuid::from_u128(102));
    let first = Profile::Ssh(Connection::new("dev-alias", "", "")?);
    let second = Profile::Ssh(Connection::new("other-alias", "developer", "2222")?);
    registry.reconcile([(id, first.clone()), (other, second.clone())]);
    let incarnation = registry.get(id).and_then(|e| e.session_id);
    registry.reconcile([(other, second.clone()), (id, first.clone())]);
    assert_eq!(registry.get(id).and_then(|e| e.session_id), incarnation);
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    registry.reconcile([(other, second.clone())]);
    assert_eq!(closes.load(Ordering::Relaxed), 1);
    registry.reconcile([(id, first.clone()), (other, second)]);
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(
        registry.get(id).map(|e| &e.snapshot.lifecycle),
        Some(&Lifecycle::Closed)
    );
    registry.restart(id, first);
    assert_ne!(registry.get(id).and_then(|e| e.session_id), incarnation);
    assert_eq!(starts.load(Ordering::Relaxed), 3);
    Ok(())
}

#[test]
fn shutdown_keeps_ports_until_workers_acknowledge_teardown() {
    struct DelayedProvider(Arc<std::sync::atomic::AtomicBool>);
    struct DelayedSession(Arc<std::sync::atomic::AtomicBool>);
    impl TerminalProvider for DelayedProvider {
        fn start(
            &self,
            _: Profile,
            _: GridSize,
        ) -> Result<Box<dyn TerminalSession>, TerminalError> {
            Ok(Box::new(DelayedSession(self.0.clone())))
        }
    }
    impl TerminalSession for DelayedSession {
        fn id(&self) -> tervix_application::SessionId {
            tervix_application::SessionId::from_uuid(uuid::Uuid::from_u128(99))
        }
        fn wakeup(&self) -> Arc<std::sync::atomic::AtomicBool> {
            Arc::default()
        }
        fn poll(&mut self) -> Option<Snapshot> {
            let mut frame = Snapshot::starting(GridSize::new(80, 24));
            frame.lifecycle = Lifecycle::Closing;
            if self.0.load(Ordering::Acquire) {
                frame.lifecycle = Lifecycle::Closed;
            }
            Some(frame)
        }
        fn input(&self, _: Vec<u8>) -> Result<(), TerminalError> {
            Ok(())
        }
        fn resize(&self, _: GridSize) -> Result<(), TerminalError> {
            Ok(())
        }
        fn close(&self) {}
    }
    let acknowledged = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut registry = TerminalRegistry::default();
    registry.set_provider(Arc::new(DelayedProvider(acknowledged.clone())));
    let id = ViewId::from_uuid(uuid::Uuid::from_u128(99));
    registry.reconcile([(id, Profile::Cmd)]);
    assert!(!registry.shutdown());
    assert!(registry.input(id, Vec::new()).is_ok());
    acknowledged.store(true, Ordering::Release);
    assert!(registry.shutdown());
    assert!(matches!(
        registry.input(id, Vec::new()),
        Err(TerminalError::Ended)
    ));
    assert!(registry.shutdown());
}
impl TerminalSession for Session {
    fn id(&self) -> tervix_application::SessionId {
        self.1
    }
    fn wakeup(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::default()
    }
    fn poll(&mut self) -> Option<Snapshot> {
        let mut frame = Snapshot::starting(GridSize::new(80, 24));
        frame.lifecycle = Lifecycle::Running;
        frame.cells = vec![Cell {
            column: 0,
            row: 0,
            text: "x".repeat(8192),
            foreground: [255; 3],
            background: [0; 3],
        }]
        .into();
        Some(frame)
    }
    fn input(&self, _: Vec<u8>) -> Result<(), TerminalError> {
        Ok(())
    }
    fn resize(&self, _: GridSize) -> Result<(), TerminalError> {
        Ok(())
    }
    fn close(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
impl TerminalProvider for Provider {
    fn start(&self, _: Profile, _: GridSize) -> Result<Box<dyn TerminalSession>, TerminalError> {
        let incarnation = self.0.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(Box::new(Session(
            self.1.clone(),
            tervix_application::SessionId::from_uuid(uuid::Uuid::from_u128(incarnation as u128)),
        )))
    }
}

#[test]
fn layout_focus_resize_move_and_undo_never_resurrect_closed_sessions() {
    let starts = Arc::new(AtomicUsize::new(0));
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = TerminalRegistry::default();
    registry.set_provider(Arc::new(Provider(starts.clone(), closes.clone())));
    let id = ViewId::from_uuid(uuid::Uuid::from_u128(1));
    let other = ViewId::from_uuid(uuid::Uuid::from_u128(2));
    let _pane_identity_is_separate = PaneId::from_uuid(uuid::Uuid::from_u128(1));
    for _ in 0..10 {
        registry.reconcile([(id, Profile::Default)]);
    }
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    registry.reconcile([(other, Profile::Cmd), (id, Profile::Default)]);
    registry.reconcile([(other, Profile::Cmd)]);
    assert_eq!(closes.load(Ordering::Relaxed), 1);
    registry.reconcile([(id, Profile::Default), (other, Profile::Cmd)]);
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(
        registry.get(id).map(|e| &e.snapshot.lifecycle),
        Some(&Lifecycle::Closed)
    );
    registry.restart(id, Profile::Default);
    assert_eq!(starts.load(Ordering::Relaxed), 3);
}

#[test]
fn selected_distribution_survives_reordering_and_replacement_closes_old_port() {
    let starts = Arc::new(AtomicUsize::new(0));
    let closes = Arc::new(AtomicUsize::new(0));
    let mut registry = TerminalRegistry::default();
    registry.set_provider(Arc::new(Provider(starts.clone(), closes.clone())));
    let id = ViewId::from_uuid(uuid::Uuid::from_u128(1));
    let other = ViewId::from_uuid(uuid::Uuid::from_u128(2));
    let selected = Profile::Wsl(Some("Ubuntu".into()));
    registry.reconcile([(id, selected.clone()), (other, Profile::Cmd)]);
    let incarnation = registry.get(id).and_then(|entry| entry.session_id);
    registry.reconcile([(other, Profile::Cmd), (id, selected.clone())]);
    assert_eq!(
        registry.get(id).and_then(|entry| entry.session_id),
        incarnation
    );
    assert_eq!(starts.load(Ordering::Relaxed), 2);
    assert_eq!(
        registry.get(id).map(|entry| &entry.profile),
        Some(&selected)
    );
    registry.restart(id, Profile::Wsl(Some("Debian".into())));
    assert_ne!(
        registry.get(id).and_then(|entry| entry.session_id),
        incarnation
    );
    assert_eq!(closes.load(Ordering::Relaxed), 1);
    assert_eq!(starts.load(Ordering::Relaxed), 3);
    registry.reconcile([(id, Profile::PowerShell), (other, Profile::Cmd)]);
    assert_eq!(closes.load(Ordering::Relaxed), 2);
}

#[test]
#[allow(clippy::unwrap_used)] // Fixture invariants; failures should fail this test.
fn churn_prunes_tombstones_beyond_undo_reachability() {
    use tervix_application::{
        Axis, Command, Layout, LayoutCommand, LayoutController, Pane, SplitRatio, UNDO_LIMIT,
    };
    let pane = |n| Pane {
        id: PaneId::from_uuid(uuid::Uuid::from_u128(n)),
        view: ViewId::from_uuid(uuid::Uuid::from_u128(n)),
    };
    let starts = Arc::new(AtomicUsize::new(0));
    let mut registry = TerminalRegistry::default();
    registry.set_provider(Arc::new(Provider(starts.clone(), Arc::default())));
    let first = pane(1);
    let mut controller = LayoutController::new(Layout::single(first));
    for n in 2..102 {
        let next = pane(n);
        controller
            .dispatch(Command::Layout(LayoutCommand::Split {
                target: first.id,
                pane: next,
                axis: Axis::Vertical,
                ratio: SplitRatio::HALF,
            }))
            .unwrap();
        registry.reconcile(
            controller
                .layout()
                .panes()
                .iter()
                .map(|p| (p.view, Profile::Default)),
        );
        registry.poll();
        assert!(!registry.get(next.view).unwrap().snapshot.cells.is_empty());
        controller
            .dispatch(Command::Layout(LayoutCommand::Close(next.id)))
            .unwrap();
        registry.reconcile([(first.view, Profile::Default)]);
        let reachable = controller.reachable_views();
        registry.retain_reachable(&reachable);
        assert!(reachable.len() <= UNDO_LIMIT + 1);
        assert!(registry.get(next.view).unwrap().snapshot.cells.is_empty());
    }
    assert!(registry.get(pane(2).view).is_none());
    let before = starts.load(Ordering::Relaxed);
    controller.dispatch(Command::Undo).unwrap();
    registry.reconcile(
        controller
            .layout()
            .panes()
            .iter()
            .map(|p| (p.view, Profile::Default)),
    );
    assert_eq!(starts.load(Ordering::Relaxed), before);
    assert_eq!(
        registry.get(pane(101).view).unwrap().snapshot.lifecycle,
        Lifecycle::Closed
    );
}
