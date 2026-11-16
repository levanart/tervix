//! Actual Bollard named-pipe upgrade/duplex fixtures. No real daemon is contacted.
#![allow(clippy::unwrap_used)]
#![allow(clippy::panic)] // Unexpected fixture lifecycle must fail the test.
use super::exec::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tervix_application::{docker::*, terminal::*};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::ServerOptions,
};
use tokio_util::sync::CancellationToken;

async fn fixture(mode: u8) {
    static SERIAL: AtomicUsize = AtomicUsize::new(0);
    let name = format!(
        "tervix-exec-fixture-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    );
    let path = format!(r"\\.\pipe\{name}");
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .in_buffer_size(1024)
        .out_buffer_size(1024)
        .create(&path)
        .unwrap();
    let (events, mut observed) = tokio::sync::mpsc::channel::<String>(16);
    let stop = CancellationToken::new();
    let server_stop = stop.clone();
    let listener = tokio::spawn(async move {
        let mut pipe = first;
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! { biased; () = server_stop.cancelled() => break, result = pipe.connect() => result.unwrap() }
            let mut connected = pipe;
            pipe = ServerOptions::new()
                .in_buffer_size(1024)
                .out_buffer_size(1024)
                .create(&path)
                .unwrap();
            let events = events.clone();
            jobs.spawn(async move {
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") { header.push(connected.read_u8().await.unwrap()); assert!(header.len() < 16384); }
                let header = String::from_utf8(header).unwrap();
                let length = header.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                let mut body = vec![0; length];
                connected.read_exact(&mut body).await.unwrap();
                let route = header.lines().next().unwrap().to_string();
                events.send(format!("{route}\n{}", String::from_utf8_lossy(&body))).await.unwrap();
                if route.contains("/start ") {
                    if mode == 3 {
                        let body = r#"{"message":"secret missing executable"}"#;
                        connected.write_all(format!("HTTP/1.1 404 Missing\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
                    } else {
                        connected.write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n\x1b[32mfixture\x1b[0m\r\n").await.unwrap();
                        let mut bytes = [0; 5];
                        if mode == 7 || mode == 8 {
                            // Fill both pipe directions. The client must drain output
                            // while its large stdin write is still backpressured.
                            tokio::time::sleep(Duration::from_millis(150)).await;
                            connected.write_all(&b"flood\r\n".repeat(32768)).await.unwrap();
                            if mode == 8 {
                                tokio::time::sleep(Duration::from_millis(200)).await;
                                let mut discarded = Vec::new();
                                connected.read_to_end(&mut discarded).await.unwrap();
                                events.send("detached".into()).await.unwrap();
                                return;
                            }
                            let mut paste = vec![0; 65536];
                            connected.read_exact(&mut paste).await.unwrap();
                            assert!(paste.iter().all(|b| *b == b'y'));
                            events.send("stdin".into()).await.unwrap();
                            connected.write_all(b"duplex-complete\r\n").await.unwrap();
                        } else if mode == 0 || mode == 4 {
                            connected.read_exact(&mut bytes).await.unwrap();
                            assert_eq!(&bytes, b"hello");
                            events.send("stdin".into()).await.unwrap();
                            connected.write_all(b"reply\r\n").await.unwrap();
                            // Keep upgrade open until test has observed debounced resize.
                            tokio::time::sleep(Duration::from_millis(250)).await;
                        } else {
                            let count = connected.read(&mut bytes).await.unwrap_or(0);
                            assert_eq!(count, 0);
                            events.send("detached".into()).await.unwrap();
                        }
                    }
                } else if mode == 6 && route.contains("/containers/") && route.contains("/exec ") {
                    // Unresponsive create: client deadline must close the request.
                    let mut byte = [0;1];
                    let _ = connected.read(&mut byte).await;
                } else {
                    let response = if route.contains("/version ") { r#"{"ApiVersion":"1.49"}"#.to_string() }
                        else if route.contains("/containers/json") { "[]".to_string() }
                        else if route.contains("/containers/") { format!(r#"{{"Id":"{}"}}"#, "b".repeat(64)) }
                        else if route.contains("/json ") { format!(r#"{{"ID":"{}","Running":{},"ExitCode":7}}"#, "b".repeat(64), mode == 4) }
                        else { String::new() };
                    let status = if mode == 5 && route.contains("/resize?") { "409 Conflict" } else { "200 OK" };
                    let response = if status.starts_with("409") { r#"{"message":"secret resize failure"}"#.into() } else { response };
                    connected.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
                }
                connected.flush().await.unwrap();
                tokio::time::sleep(Duration::from_millis(30)).await;
            });
        }
        jobs.abort_all();
    });
    let endpoint = Endpoint::NamedPipe(format!("//./pipe/{name}"));
    let service = crate::NativeDocker::new().unwrap();
    service.connect(endpoint.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while service.snapshot().state != ConnectionState::Connected {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let options = ExecOptions {
        endpoint,
        generation: service.exec_generation(),
        container: ContainerId::new("a".repeat(64)).unwrap(),
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
    };
    let mut session = service
        .exec(options.clone(), GridSize::new(80, 24))
        .unwrap();
    let mut wire = Vec::new();
    let mut sent = false;
    let mut ended = None;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            while let Ok(event) = observed.try_recv() {
                wire.push(event);
            }
            if let Some(frame) = session.poll() {
                match frame.lifecycle {
                    Lifecycle::Running if !sent => {
                        sent = true;
                        match mode {
                            0 | 4 => {
                                session.input(b"hello".to_vec()).unwrap();
                                session.resize(GridSize::new(120, 40)).unwrap();
                            }
                            1 => session.close(),
                            2 => service.disconnect().unwrap(),
                            7 | 8 => session.input(vec![b'y'; 65536]).unwrap(),
                            _ => {}
                        }
                    }
                    Lifecycle::Running
                        if mode == 8 && frame.cells.iter().any(|c| c.text == "l") =>
                    {
                        session.close();
                    }
                    Lifecycle::Exited(_) | Lifecycle::Failed(_) | Lifecycle::Closed => {
                        ended = Some(frame);
                        break;
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    if mode == 8 {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !wire.iter().any(|e| e == "detached") {
                wire.push(observed.recv().await.unwrap());
            }
        })
        .await
        .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(60)).await;
    while let Ok(event) = observed.try_recv() {
        wire.push(event);
    }
    let frame = ended.unwrap();
    match mode {
        0 => {
            assert_eq!(frame.lifecycle, Lifecycle::Exited(7));
            assert_eq!(frame.size, GridSize::new(120, 40));
            assert!(frame.cells.iter().any(|c| c.text == "r"));
            assert!(
                frame
                    .cells
                    .iter()
                    .any(|c| c.text == "f" && c.foreground == [152, 195, 121])
            );
            assert!(wire.iter().any(|e| e == "stdin"));
            assert!(
                wire.iter()
                    .any(|e| e.contains("h=40") && e.contains("w=120"))
            );
        }
        7 => {
            assert_eq!(frame.lifecycle, Lifecycle::Exited(7));
            assert!(wire.iter().any(|e| e == "stdin"));
            assert!(frame.cells.iter().any(|c| c.text == "d"));
        }
        1 | 8 => {
            assert_eq!(frame.lifecycle, Lifecycle::Closed);
            assert!(wire.iter().any(|e| e == "detached"));
        }
        2 => {
            assert!(matches!(frame.lifecycle, Lifecycle::Failed(_)));
            assert!(wire.iter().any(|e| e == "detached"));
            assert!(service.exec_generation() != options.generation);
            assert!(matches!(
                service.exec(options.clone(), GridSize::new(80, 24)),
                Err(DockerError::InvalidInput)
            ));
            service.connect(options.endpoint.clone()).unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                while service.snapshot().state != ConnectionState::Connected {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert!(matches!(
                service.exec(options.clone(), GridSize::new(80, 24)),
                Err(DockerError::InvalidInput)
            ));
        }
        3 => {
            let Lifecycle::Failed(error) = frame.lifecycle else {
                panic!("expected missing shell error")
            };
            assert!(error.contains("shell may be missing"));
            assert!(!error.contains("secret"));
        }
        4 => {
            let Lifecycle::Failed(error) = frame.lifecycle else {
                panic!("expected unknown EOF")
            };
            assert!(error.contains("EOF"));
        }
        5 | 6 => {
            let Lifecycle::Failed(error) = frame.lifecycle else {
                panic!("expected I/O failure")
            };
            assert!(error.contains(if mode == 5 { "conflicts" } else { "timed out" }));
            assert!(!error.contains("secret"));
        }
        _ => unreachable!(),
    }
    let create = wire
        .iter()
        .find(|e| e.contains("/containers/") && e.contains("/exec "))
        .unwrap();
    let config: serde_json::Value =
        serde_json::from_str(create.split_once('\n').unwrap().1).unwrap();
    assert_eq!(config["Cmd"], serde_json::json!(["/bin/sh", "-i"]));
    for flag in ["AttachStdin", "AttachStdout", "AttachStderr", "Tty"] {
        assert_eq!(config[flag], true);
    }
    assert_eq!(config["Privileged"], false);
    stop.cancel();
    listener.await.unwrap();
}

#[tokio::test]
async fn tty_duplex_resize_and_exit() {
    fixture(0).await;
}
#[tokio::test]
async fn backpressured_stdin_does_not_block_output_drain() {
    fixture(7).await;
}
#[tokio::test]
async fn close_cancels_backpressured_duplex_without_kill() {
    fixture(8).await;
}
#[tokio::test]
async fn close_detaches_without_kill() {
    fixture(1).await;
}
#[tokio::test]
async fn generation_cancellation_detaches() {
    fixture(2).await;
}
#[tokio::test]
async fn missing_shell_is_visible_and_sanitized() {
    fixture(3).await;
}
#[tokio::test]
async fn eof_is_not_fictional_process_exit() {
    fixture(4).await;
}
#[tokio::test]
async fn resize_failure_detaches_and_is_visible() {
    fixture(5).await;
}
#[tokio::test]
async fn startup_timeout_is_not_retried() {
    fixture(6).await;
}

#[test]
fn bounded_input_and_cancel_before_dispatch() {
    let (mut session, worker) = session(GridSize::new(80, 24));
    assert!(matches!(
        session.input(vec![0; 65537]),
        Err(TerminalError::Busy)
    ));
    for _ in 0..32 {
        session.input(vec![0; 65536]).unwrap();
    }
    assert!(matches!(session.input(vec![0]), Err(TerminalError::Busy)));
    drop(worker);
    assert!(matches!(
        session.poll().unwrap().lifecycle,
        Lifecycle::Failed(_)
    ));
    assert!(matches!(session.input(vec![0]), Err(TerminalError::Ended)));
}
