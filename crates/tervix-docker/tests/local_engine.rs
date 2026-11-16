//! Explicit opt-in read-only Engine smoke. Never creates or changes a container.
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tervix_application::docker::{ConnectionState, DockerService, Endpoint};
use tervix_docker::NativeDocker;

#[test]
#[allow(clippy::unwrap_used, clippy::panic)] // Opt-in acceptance reports real Engine failures.
#[ignore = "requires explicit local Docker Engine authorization and installation"]
fn local_engine_read_only() {
    let service: Arc<dyn DockerService> = Arc::new(NativeDocker::new().unwrap());
    assert_eq!(service.snapshot().state, ConnectionState::Disconnected);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(service.snapshot().revision, 0);
    let endpoint = match std::env::var("TERVIX_TEST_DOCKER_PIPE") {
        Ok(pipe) => Endpoint::NamedPipe(pipe),
        Err(_) => Endpoint::default(),
    };
    service.connect(endpoint).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let frame = service.snapshot();
        match &frame.state {
            ConnectionState::Connected => {
                println!(
                    "Engine connected: {} containers (read-only)",
                    frame.containers.len()
                );
                if let Some(container) = frame.containers.first() {
                    let mut inspect = service
                        .inspect(frame.incarnation, container.id.clone())
                        .unwrap();
                    loop {
                        if let Some(result) = inspect.poll() {
                            assert_eq!(result.unwrap().id, container.id);
                            break;
                        }
                        assert!(Instant::now() < deadline, "inspect deadline exceeded");
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                break;
            }
            ConnectionState::Retrying(error) => panic!("local Engine unavailable: {error}"),
            _ => {}
        }
        assert!(Instant::now() < deadline, "connection deadline exceeded");
        std::thread::sleep(Duration::from_millis(20));
    }
    service.disconnect().unwrap();
}

#[test]
#[allow(clippy::unwrap_used)] // Opt-in acceptance of installed read-only metadata discovery.
#[ignore = "explicitly reads the user's local Docker context metadata"]
fn local_context_discovery_read_only() {
    let service = NativeDocker::new().unwrap();
    let mut contexts = service.contexts().unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if let Some(result) = contexts.poll() {
            let contexts = result.unwrap();
            assert!(contexts.iter().any(|context| context.name == "default"));
            for context in contexts.iter() {
                println!(
                    "context={} selected={} supported={}",
                    context.name,
                    context.selected,
                    context.endpoint.is_some()
                );
            }
            assert_eq!(service.snapshot().state, ConnectionState::Disconnected);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "context discovery deadline exceeded"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
