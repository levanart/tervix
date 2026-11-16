use super::*;
use crate::engine::Chunk;
use futures_util::{
    FutureExt, StreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use std::sync::atomic::AtomicUsize;

fn id() -> ContainerId {
    ContainerId::new("a".repeat(64)).unwrap()
}

#[derive(Default)]
struct Fake {
    fail_list: AtomicBool,
    lists: AtomicUsize,
    streams: AtomicUsize,
    actions: std::sync::Mutex<Vec<Action>>,
    log_error: bool,
}

impl Engine for Fake {
    fn list(&self) -> BoxFuture<'_, Result<Arc<[Container]>, DockerError>> {
        self.lists.fetch_add(1, Ordering::Relaxed);
        if self.fail_list.swap(false, Ordering::AcqRel) {
            return async { Err(DockerError::Unavailable) }.boxed();
        }
        async { Ok(Arc::from([])) }.boxed()
    }
    fn inspect(&self, _: ContainerId) -> BoxFuture<'_, Result<Details, DockerError>> {
        async { Err(DockerError::NotFound) }.boxed()
    }
    fn stats(&self, _: ContainerId) -> BoxFuture<'_, Result<Stats, DockerError>> {
        async {
            Ok(Stats {
                observed_at: SystemTime::now(),
                cpu_percent: None,
                memory_bytes: None,
                memory_limit: None,
            })
        }
        .boxed()
    }
    fn act(&self, _: ContainerId, action: Action) -> BoxFuture<'_, Result<(), DockerError>> {
        self.actions.lock().unwrap().push(action);
        async { Ok(()) }.boxed()
    }
    fn logs<'a>(&'a self, _: &'a ContainerId) -> BoxStream<'a, Result<Chunk, DockerError>> {
        self.streams.fetch_add(1, Ordering::Relaxed);
        if self.log_error {
            return stream::iter([Err(DockerError::AccessDenied)]).boxed();
        }
        stream::iter([Ok(Chunk {
            channel: LogChannel::Stdout,
            bytes: b"hello\n".to_vec(),
        })])
        .chain(stream::pending())
        .boxed()
    }
}

struct FakeConnector {
    engine: Arc<Fake>,
    attempts: AtomicUsize,
    fail_first: bool,
}

impl Connector for FakeConnector {
    fn connect(&self, _: Endpoint) -> BoxFuture<'_, Result<Arc<dyn Engine>, DockerError>> {
        let attempt = self.attempts.fetch_add(1, Ordering::Relaxed);
        async move {
            if self.fail_first && attempt == 0 {
                return Err(DockerError::Unavailable);
            }
            Ok(self.engine.clone() as Arc<dyn Engine>)
        }
        .boxed()
    }
}

fn actor(engine: Arc<Fake>, fail_first: bool) -> Actor {
    let (_, incoming) = mpsc::channel(QUEUE);
    Actor {
        exec_tasks: Vec::new(),
        exec_generation: Arc::new(AtomicU64::new(0)),
        incoming,
        latest: Arc::new(ArcSwap::from_pointee(Snapshot {
            incarnation: 0,
            revision: 0,
            endpoint: None,
            state: ConnectionState::Disconnected,
            capabilities: Capabilities::default(),
            observed_at: None,
            containers: Arc::from([]),
        })),
        visible: Arc::new(AtomicBool::new(true)),
        epoch: Arc::new(AtomicU64::new(0)),
        changed: Arc::new(Notify::new()),
        generation: 0,
        endpoint: None,
        engine: None,
        sources: HashMap::new(),
        stats_cache: HashMap::new(),
        context_job: None,
        source_cancel: CancellationToken::new(),
        failures: 0,
        due: tokio::time::Instant::now(),
        connector: Arc::new(FakeConnector {
            engine,
            attempts: AtomicUsize::new(0),
            fail_first,
        }),
    }
}

async fn connect(actor: &mut Actor) {
    actor
        .handle(Envelope {
            epoch: 0,
            incarnation: actor.exec_generation.load(Ordering::Acquire),
            request: Request::Connect(Endpoint::default()),
        })
        .await;
}

#[tokio::test]
async fn disconnected_requests_and_read_only_retry_recovery() {
    let fake = Arc::new(Fake::default());
    let mut actor = actor(fake.clone(), true);
    let (tx, rx) = oneshot::channel();
    actor
        .handle(Envelope {
            epoch: 0,
            incarnation: actor.exec_generation.load(Ordering::Acquire),
            request: Request::Stats(id(), tx),
        })
        .await;
    assert_eq!(rx.await.unwrap().unwrap_err(), DockerError::Disconnected);
    assert_eq!(fake.lists.load(Ordering::Relaxed), 0);
    connect(&mut actor).await;
    assert_eq!(
        actor.latest.load().state,
        ConnectionState::Retrying(DockerError::Unavailable)
    );
    assert!(actor.due > tokio::time::Instant::now());
    actor.refresh().await;
    assert_eq!(actor.latest.load().state, ConnectionState::Connected);
    assert!(actor.latest.load().observed_at.is_some());
    assert_eq!(fake.lists.load(Ordering::Relaxed), 1);
    assert!(fake.actions.lock().unwrap().is_empty());
    for failure in 1..100 {
        assert!(backoff(failure) <= Duration::from_millis(30_250));
    }
}

#[tokio::test]
async fn duplicate_readers_share_stream_and_drop_cancels_demand() {
    let fake = Arc::new(Fake::default());
    let mut actor = actor(fake.clone(), false);
    connect(&mut actor).await;
    let mut a = actor.subscribe(id()).unwrap();
    let mut b = actor.subscribe(id()).unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(fake.streams.load(Ordering::Relaxed), 1);
    assert_eq!(a.latest().entries.len(), 1);
    assert!(Arc::ptr_eq(&a.latest(), &b.latest()));
    drop(a);
    assert!(!actor.sources[&id()].task.is_finished());
    drop(b);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(actor.sources[&id()].task.is_finished());
}

#[tokio::test]
async fn stream_failure_and_disconnect_are_explicit() {
    let mut actor = actor(
        Arc::new(Fake {
            log_error: true,
            ..Default::default()
        }),
        false,
    );
    connect(&mut actor).await;
    let mut reader = actor.subscribe(id()).unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while reader.latest().state == LogState::Following {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        reader.latest().state,
        LogState::Failed(DockerError::AccessDenied)
    );
    // A live stream observes disconnect as cancellation, rather than fictional continuity.
    actor.engine = Some(Arc::new(Fake::default()));
    let mut reader = actor.subscribe(id()).unwrap();
    tokio::task::yield_now().await;
    actor.clear();
    tokio::time::timeout(Duration::from_secs(1), async {
        while reader.latest().state == LogState::Following {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(reader.latest().state, LogState::Cancelled);
    assert!(actor.latest.load().observed_at.is_none());
}

#[tokio::test]
async fn cancelled_or_old_endpoint_actions_do_not_dispatch() {
    let fake = Arc::new(Fake::default());
    let mut actor = actor(fake.clone(), false);
    connect(&mut actor).await;
    let (tx, rx) = oneshot::channel();
    drop(rx);
    actor
        .handle(Envelope {
            epoch: 0,
            incarnation: actor.exec_generation.load(Ordering::Acquire),
            request: Request::Act(id(), Action::Start, tx),
        })
        .await;
    actor.epoch.store(1, Ordering::Release);
    let (tx, rx) = oneshot::channel();
    actor
        .handle(Envelope {
            epoch: 0,
            incarnation: actor.exec_generation.load(Ordering::Acquire),
            request: Request::Act(id(), Action::Stop, tx),
        })
        .await;
    assert!(rx.await.is_err());
    // A new-generation request arriving before its Connect must not use the old engine.
    let (tx, rx) = oneshot::channel();
    actor
        .handle(Envelope {
            epoch: 1,
            incarnation: actor.exec_generation.load(Ordering::Acquire),
            request: Request::Act(id(), Action::Restart, tx),
        })
        .await;
    assert!(rx.await.is_err());
    assert!(fake.actions.lock().unwrap().is_empty());
}

#[tokio::test]
async fn actions_are_one_shot_and_reply_mapping_is_preserved() {
    let fake = Arc::new(Fake::default());
    let mut actor = actor(fake.clone(), false);
    connect(&mut actor).await;
    let actions = [
        Action::Start,
        Action::Stop,
        Action::Restart,
        Action::Pause,
        Action::Unpause,
        Action::Remove { confirmed: id() },
    ];
    for action in actions.clone() {
        let (tx, rx) = oneshot::channel();
        actor
            .handle(Envelope {
                epoch: 0,
                incarnation: actor.exec_generation.load(Ordering::Acquire),
                request: Request::Act(id(), action, tx),
            })
            .await;
        assert_eq!(rx.await.unwrap(), Ok(()));
    }
    actor.refresh().await;
    assert_eq!(*fake.actions.lock().unwrap(), actions);
    let (tx, rx) = oneshot::channel();
    actor
        .handle(Envelope {
            epoch: 0,
            incarnation: actor.exec_generation.load(Ordering::Acquire),
            request: Request::Inspect(id(), tx),
        })
        .await;
    assert_eq!(rx.await.unwrap().unwrap_err(), DockerError::NotFound);
}

#[tokio::test]
async fn deadlines_and_generation_cancellation_interrupt_pending_io() {
    let epoch = Arc::new(AtomicU64::new(0));
    let changed = Arc::new(Notify::new());
    let result: Result<(), _> = bounded(
        std::future::pending(),
        &epoch,
        0,
        &changed,
        Duration::from_millis(5),
    )
    .await;
    assert_eq!(result, Err(DockerError::Timeout));
    let e = epoch.clone();
    let n = changed.clone();
    let trigger = tokio::spawn(async move {
        tokio::task::yield_now().await;
        e.store(1, Ordering::Release);
        n.notify_waiters();
    });
    let result: Result<(), _> =
        bounded(std::future::pending(), &epoch, 0, &changed, DEADLINE).await;
    trigger.await.unwrap();
    assert_eq!(result, Err(DockerError::Cancelled));
}

#[tokio::test]
async fn same_generation_wakeup_cannot_cancel_its_own_new_connection() {
    let epoch = AtomicU64::new(2);
    let changed = Arc::new(Notify::new());
    let (tx, rx) = oneshot::channel();
    let n = changed.clone();
    let notifier = tokio::spawn(async move {
        tokio::task::yield_now().await;
        n.notify_waiters();
        tokio::task::yield_now().await;
        let _ = tx.send(());
    });
    let result = bounded(
        async { rx.await.map_err(|_| DockerError::Unavailable) },
        &epoch,
        2,
        &changed,
        DEADLINE,
    )
    .await;
    notifier.await.unwrap();
    assert_eq!(result, Ok(()));
}

#[test]
fn bounded_queue_overflow_is_explicit_and_remove_requires_matching_id() {
    let (commands, _rx) = mpsc::channel(1);
    let actor = actor(Arc::new(Fake::default()), false);
    let service = NativeDocker {
        exec_generation: Arc::new(AtomicU64::new(0)),
        commands,
        latest: actor.latest.clone(),
        visible: actor.visible.clone(),
        epoch: actor.epoch.clone(),
        changed: actor.changed.clone(),
        stop: CancellationToken::new(),
    };
    service.disconnect().unwrap();
    assert_eq!(service.connect(Endpoint::default()), Err(DockerError::Busy));
    assert!(matches!(
        service.act(
            0,
            id(),
            Action::Remove {
                confirmed: ContainerId::new("b".repeat(64)).unwrap()
            }
        ),
        Err(DockerError::InvalidInput)
    ));
    assert!(ContainerId::new("../bad".into()).is_err());
}

#[tokio::test]
async fn queued_work_cannot_cross_same_endpoint_connection_failure() {
    let fake = Arc::new(Fake::default());
    let mut actor = actor(fake.clone(), false);
    connect(&mut actor).await;
    let old = actor.latest.load().incarnation;
    let (commands, incoming) = mpsc::channel(QUEUE);
    actor.incoming = incoming;
    let service = NativeDocker {
        exec_generation: actor.exec_generation.clone(),
        commands,
        latest: actor.latest.clone(),
        visible: actor.visible.clone(),
        epoch: actor.epoch.clone(),
        changed: actor.changed.clone(),
        stop: CancellationToken::new(),
    };
    let mut action = service.act(old, id(), Action::Stop).unwrap();
    let mut logs = service.logs(old, id()).unwrap();
    let mut inspect = service.inspect(old, id()).unwrap();
    let mut stats = service.stats(old, id()).unwrap();
    fake.fail_list.store(true, Ordering::Release);
    actor.refresh().await;
    assert_ne!(service.snapshot().incarnation, old);
    actor.refresh().await;
    assert_eq!(service.snapshot().state, ConnectionState::Connected);
    for _ in 0..4 {
        let command = actor.incoming.recv().await.unwrap();
        actor.handle(command).await;
    }
    assert_eq!(action.poll(), Some(Err(DockerError::Cancelled)));
    assert!(matches!(logs.poll(), Some(Err(DockerError::Cancelled))));
    assert!(matches!(inspect.poll(), Some(Err(DockerError::Cancelled))));
    assert!(matches!(stats.poll(), Some(Err(DockerError::Cancelled))));
    assert!(fake.actions.lock().unwrap().is_empty());
    assert!(actor.sources.is_empty());
    // Even submission after recovery must retain the caller's old target identity.
    let mut stale = service.act(old, id(), Action::Start).unwrap();
    let command = actor.incoming.recv().await.unwrap();
    actor.handle(command).await;
    assert_eq!(stale.poll(), Some(Err(DockerError::Cancelled)));
    let mut fresh = service
        .act(service.snapshot().incarnation, id(), Action::Start)
        .unwrap();
    let command = actor.incoming.recv().await.unwrap();
    actor.handle(command).await;
    assert_eq!(fresh.poll(), Some(Ok(())));
    assert_eq!(*fake.actions.lock().unwrap(), vec![Action::Start]);
}
