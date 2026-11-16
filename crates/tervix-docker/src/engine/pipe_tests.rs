//! Real Bollard HTTP over a private fake pipe; never contacts a Docker daemon.
#![allow(clippy::unwrap_used)] // Fixture setup and wire assertions must fail loudly.

use super::*;
use std::{
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::ServerOptions,
};

async fn exchange<T>(
    status: u16,
    body: Vec<u8>,
    operation: impl FnOnce(NativeEngine) -> T,
) -> (T::Output, String)
where
    T: Future,
{
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let name = format!(
        "tervix-docker-fixture-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let mut pipe = ServerOptions::new()
        .first_pipe_instance(true)
        .create(format!(r"\\.\pipe\{name}"))
        .unwrap();
    let server = tokio::spawn(async move {
        pipe.connect().await.unwrap();
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            let byte = pipe.read_u8().await.unwrap();
            bytes.push(byte);
            assert!(bytes.len() < 16 * 1024);
        }
        let header = format!(
            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n",
            body.len()
        );
        pipe.write_all(header.as_bytes()).await.unwrap();
        pipe.write_all(&body).await.unwrap();
        pipe.flush().await.unwrap();
        // Hyper's pipe reader must receive the response before the server handle closes.
        let mut buffer = [0u8; 1];
        let _ = tokio::time::timeout(Duration::from_millis(100), pipe.read(&mut buffer)).await;
        String::from_utf8(bytes).unwrap()
    });
    let docker = client(&Endpoint::NamedPipe(format!("//./pipe/{name}"))).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), operation(NativeEngine(docker)))
        .await
        .unwrap();
    (result, server.await.unwrap())
}

fn id() -> ContainerId {
    ContainerId::new("a".repeat(64)).unwrap()
}

#[tokio::test]
async fn lifecycle_wire_methods_paths_timeouts_and_no_volume_deletion() {
    let actions = [
        (Action::Start, "POST", "/start"),
        (Action::Stop, "POST", "/stop?t=3"),
        (Action::Restart, "POST", "/restart?t=3"),
        (Action::Pause, "POST", "/pause"),
        (Action::Unpause, "POST", "/unpause"),
        (
            Action::Remove { confirmed: id() },
            "DELETE",
            "?v=false&force=false&link=false",
        ),
    ];
    for (action, method, suffix) in actions {
        let (result, wire) = exchange(204, vec![], |engine| async move {
            engine.act(id(), action).await
        })
        .await;
        assert_eq!(result, Ok(()));
        let line = wire.lines().next().unwrap();
        assert!(line.starts_with(method), "{line}");
        assert!(
            line.contains(&format!("/containers/{}", id().as_str())),
            "{line}"
        );
        assert!(line.contains(suffix), "{line}");
    }
}

#[tokio::test]
async fn all_container_list_and_inspect_are_safe_dtos_and_errors_are_sanitized() {
    let body = format!(
        r#"[{{"Id":"{}","Names":["/fixture"],"Image":"alpine","State":"exited","Status":"Exited (0)","Ports":[{{"PrivatePort":80,"PublicPort":8080,"Type":"tcp","IP":"127.0.0.1"}}],"Command":"secret-command","Labels":{{"token":"secret"}}}}]"#,
        id().as_str()
    );
    let (result, wire) = exchange(200, body.into_bytes(), |engine| async move {
        engine.list().await
    })
    .await;
    let rows = result.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ports[0].public, Some(8080));
    assert_eq!(rows[0].state, "exited");
    assert!(!format!("{rows:?}").contains("secret"));
    assert!(wire.lines().next().unwrap().contains("all=true"));
    let body = format!(
        r#"{{"Id":"{}","Name":"/fixture","Image":"sha256:image","Platform":"linux","State":{{"Running":false,"Paused":false,"ExitCode":0}},"Config":{{"Env":["PASSWORD=secret"],"Cmd":["secret-command"]}},"Mounts":[{{"Source":"secret-path"}}]}}"#,
        id().as_str()
    );
    let (result, _) = exchange(200, body.into_bytes(), |engine| async move {
        engine.inspect(id()).await
    })
    .await;
    let details = result.unwrap();
    assert_eq!(details.running, Some(false));
    assert!(!format!("{details:?}").contains("secret"));
    let (result, _) = exchange(
        403,
        br#"{"message":"secret-credential"}"#.to_vec(),
        |engine| async move { engine.inspect(id()).await },
    )
    .await;
    assert_eq!(result.unwrap_err(), DockerError::AccessDenied);
}

#[tokio::test]
async fn actual_multiplexed_logs_preserve_channels_and_reach_eof() {
    let mut body = Vec::new();
    for (channel, message) in [
        (1u8, "2026-11-09T00:00:00Z stdout\n"),
        (2u8, "2026-11-09T00:00:01Z stderr\n"),
    ] {
        body.extend_from_slice(&[channel, 0, 0, 0]);
        body.extend_from_slice(&(message.len() as u32).to_be_bytes());
        body.extend_from_slice(message.as_bytes());
    }
    let (result, wire) = exchange(200, body, |engine| async move {
        let id = id();
        let mut stream = engine.logs(&id);
        let a = stream.next().await.unwrap().unwrap();
        let b = stream.next().await.unwrap().unwrap();
        assert!(stream.next().await.is_none());
        (a, b)
    })
    .await;
    assert_eq!(result.0.channel, LogChannel::Stdout);
    assert_eq!(result.1.channel, LogChannel::Stderr);
    let line = wire.lines().next().unwrap();
    for option in [
        "follow=true",
        "stdout=true",
        "stderr=true",
        "timestamps=true",
        "tail=200",
    ] {
        assert!(line.contains(option), "{line}");
    }
}
