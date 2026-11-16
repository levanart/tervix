use bollard::{Docker, models::ContainerStatsResponse, query_parameters as query};
use futures_util::{FutureExt, StreamExt, future::BoxFuture, stream::BoxStream};
use std::sync::Arc;
use tervix_application::docker::*;

pub(crate) struct Chunk {
    pub channel: LogChannel,
    pub bytes: Vec<u8>,
}

// Actual I/O boundary, also used by deterministic failure/cancellation tests.
pub(crate) trait Engine: Send + Sync {
    fn exec_client(&self) -> Option<Docker> {
        None
    }
    fn list(&self) -> BoxFuture<'_, Result<Arc<[Container]>, DockerError>>;
    fn inspect(&self, id: ContainerId) -> BoxFuture<'_, Result<Details, DockerError>>;
    fn stats(&self, id: ContainerId) -> BoxFuture<'_, Result<Stats, DockerError>>;
    fn act(&self, id: ContainerId, action: Action) -> BoxFuture<'_, Result<(), DockerError>>;
    fn logs<'a>(&'a self, id: &'a ContainerId) -> BoxStream<'a, Result<Chunk, DockerError>>;
}

pub(crate) trait Connector: Send + Sync {
    fn connect(&self, endpoint: Endpoint) -> BoxFuture<'_, Result<Arc<dyn Engine>, DockerError>>;
}

pub(crate) struct NativeConnector;

impl Connector for NativeConnector {
    fn connect(&self, endpoint: Endpoint) -> BoxFuture<'_, Result<Arc<dyn Engine>, DockerError>> {
        async move {
            let client = client(&endpoint)?;
            let negotiated = client.negotiate_version().await.map_err(map_error)?;
            Ok(Arc::new(NativeEngine(negotiated)) as Arc<dyn Engine>)
        }
        .boxed()
    }
}

pub(crate) fn validate(endpoint: &Endpoint) -> Result<(), DockerError> {
    match endpoint {
        Endpoint::NamedPipe(path) => {
            // Never accept UNC remote machines or arbitrary device namespaces.
            let name = path
                .strip_prefix("//./pipe/")
                .ok_or(DockerError::InvalidInput)?;
            if name.is_empty()
                || name.len() > 128
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(DockerError::InvalidInput);
            }
            if cfg!(windows) {
                Ok(())
            } else {
                Err(DockerError::Unsupported)
            }
        }
        Endpoint::UnixSocket(path) => {
            if !path.starts_with('/')
                || path.starts_with("//")
                || path.len() > 256
                || path.chars().any(char::is_control)
            {
                return Err(DockerError::InvalidInput);
            }
            if cfg!(unix) {
                Ok(())
            } else {
                Err(DockerError::Unsupported)
            }
        }
    }
}

pub(crate) fn client(endpoint: &Endpoint) -> Result<Docker, DockerError> {
    validate(endpoint)?;
    match endpoint {
        #[cfg(windows)]
        Endpoint::NamedPipe(path) => {
            Docker::connect_with_named_pipe(path, 5, bollard::API_DEFAULT_VERSION)
                .map_err(map_error)
        }
        #[cfg(unix)]
        Endpoint::UnixSocket(path) => {
            Docker::connect_with_unix(path, 5, bollard::API_DEFAULT_VERSION).map_err(map_error)
        }
        _ => Err(DockerError::Unsupported),
    }
}

pub(crate) fn map_error(error: bollard::errors::Error) -> DockerError {
    use bollard::errors::Error;
    match error {
        Error::DockerResponseServerError { status_code, .. } => match status_code {
            401 | 403 => DockerError::AccessDenied,
            404 => DockerError::NotFound,
            409 => DockerError::Conflict,
            other => DockerError::Engine(other),
        },
        Error::RequestTimeoutError => DockerError::Timeout,
        Error::IOError { err } if err.kind() == std::io::ErrorKind::PermissionDenied => {
            DockerError::AccessDenied
        }
        Error::JsonDataError { .. }
        | Error::JsonSerdeError { .. }
        | Error::APIVersionParseError { .. } => DockerError::Protocol,
        other => {
            let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&other);
            for _ in 0..16 {
                let Some(error) = source else {
                    break;
                };
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
                {
                    return DockerError::AccessDenied;
                }
                source = error.source();
            }
            DockerError::Unavailable
        }
    }
}

// Strip terminal controls/bidi controls; preserve line feeds/tabs only for logs.
// This is display sanitization, not credential detection: arbitrary logs may contain secrets.
pub(crate) fn text(value: &str, max_bytes: usize) -> String {
    let mut result = String::new();
    for c in value.chars() {
        if result.len() + c.len_utf8() > max_bytes {
            break;
        }
        if (!c.is_control() || c == '\n' || c == '\t')
            && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            result.push(c);
        }
    }
    result
}

pub(crate) struct NativeEngine(pub Docker);

impl Engine for NativeEngine {
    fn exec_client(&self) -> Option<Docker> {
        Some(self.0.clone())
    }
    fn list(&self) -> BoxFuture<'_, Result<Arc<[Container]>, DockerError>> {
        async {
            let rows = self
                .0
                .list_containers(Some(query::ListContainersOptions {
                    all: true,
                    ..Default::default()
                }))
                .await
                .map_err(map_error)?;
            let mut containers = Vec::with_capacity(rows.len());
            for row in rows {
                let id = ContainerId::new(row.id.ok_or(DockerError::Protocol)?)
                    .map_err(|_| DockerError::Protocol)?;
                containers.push(Container {
                    id,
                    names: row
                        .names
                        .unwrap_or_default()
                        .iter()
                        .take(32)
                        .map(|s| text(s, 256))
                        .collect(),
                    image: text(&row.image.unwrap_or_default(), 512),
                    state: row.state.map(|s| s.to_string()).unwrap_or_default(),
                    status: text(&row.status.unwrap_or_default(), 512),
                    created: row.created,
                    ports: row
                        .ports
                        .unwrap_or_default()
                        .into_iter()
                        .take(256)
                        .map(|p| Port {
                            private: p.private_port,
                            public: p.public_port,
                            protocol: p.typ.map(|p| p.to_string()).unwrap_or_default(),
                            host_ip: p.ip.map(|s| text(&s, 64)),
                        })
                        .collect(),
                });
            }
            containers.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(containers.into())
        }
        .boxed()
    }

    fn inspect(&self, id: ContainerId) -> BoxFuture<'_, Result<Details, DockerError>> {
        async move {
            let row = self
                .0
                .inspect_container(id.as_str(), None::<query::InspectContainerOptions>)
                .await
                .map_err(map_error)?;
            if row.id.as_deref() != Some(id.as_str()) {
                return Err(DockerError::Protocol);
            }
            let state = row.state.unwrap_or_default();
            Ok(Details {
                id,
                name: text(&row.name.unwrap_or_default(), 256),
                image: text(&row.image.unwrap_or_default(), 512),
                platform: text(&row.platform.unwrap_or_default(), 128),
                running: state.running,
                paused: state.paused,
                exit_code: state.exit_code,
                started_at: state.started_at.map(|s| text(&s, 64)),
                finished_at: state.finished_at.map(|s| text(&s, 64)),
            })
        }
        .boxed()
    }

    fn stats(&self, id: ContainerId) -> BoxFuture<'_, Result<Stats, DockerError>> {
        async move {
            // one_shot=false asks Engine to take its two samples for a real CPU delta.
            let mut stream = self.0.stats(
                id.as_str(),
                Some(query::StatsOptions {
                    stream: false,
                    one_shot: false,
                }),
            );
            let row = stream
                .next()
                .await
                .ok_or(DockerError::Protocol)?
                .map_err(map_error)?;
            Ok(stats(&row))
        }
        .boxed()
    }

    fn act(&self, id: ContainerId, action: Action) -> BoxFuture<'_, Result<(), DockerError>> {
        async move {
            let docker = &self.0;
            let name = id.as_str();
            match action {
                Action::Start => {
                    docker
                        .start_container(name, None::<query::StartContainerOptions>)
                        .await
                }
                Action::Stop => {
                    docker
                        .stop_container(
                            name,
                            Some(query::StopContainerOptions {
                                t: Some(3),
                                ..Default::default()
                            }),
                        )
                        .await
                }
                Action::Restart => {
                    docker
                        .restart_container(
                            name,
                            Some(query::RestartContainerOptions {
                                t: Some(3),
                                ..Default::default()
                            }),
                        )
                        .await
                }
                Action::Pause => docker.pause_container(name).await,
                Action::Unpause => docker.unpause_container(name).await,
                Action::Remove { confirmed } => {
                    if confirmed != id {
                        return Err(DockerError::InvalidInput);
                    }
                    docker
                        .remove_container(
                            name,
                            Some(query::RemoveContainerOptions {
                                force: false,
                                v: false,
                                link: false,
                            }),
                        )
                        .await
                }
            }
            .map_err(map_error)
        }
        .boxed()
    }

    fn logs<'a>(&'a self, id: &'a ContainerId) -> BoxStream<'a, Result<Chunk, DockerError>> {
        self.0
            .logs(
                id.as_str(),
                Some(query::LogsOptions {
                    follow: true,
                    stdout: true,
                    stderr: true,
                    timestamps: true,
                    tail: "200".into(),
                    ..Default::default()
                }),
            )
            .map(|result| {
                result
                    .map(|output| {
                        use bollard::container::LogOutput;
                        let (channel, message) = match output {
                            LogOutput::StdOut { message } => (LogChannel::Stdout, message),
                            LogOutput::StdErr { message } => (LogChannel::Stderr, message),
                            LogOutput::Console { message } | LogOutput::StdIn { message } => {
                                (LogChannel::Combined, message)
                            }
                        };
                        Chunk {
                            channel,
                            bytes: message[..message.len().min(64 * 1024 + 1)].to_vec(),
                        }
                    })
                    .map_err(map_error)
            })
            .boxed()
    }
}

fn stats(row: &ContainerStatsResponse) -> Stats {
    let cpu = (|| {
        let current = row.cpu_stats.as_ref()?;
        let previous = row.precpu_stats.as_ref()?;
        let cpu = current
            .cpu_usage
            .as_ref()?
            .total_usage?
            .checked_sub(previous.cpu_usage.as_ref()?.total_usage?)?;
        let system = current
            .system_cpu_usage?
            .checked_sub(previous.system_cpu_usage?)?;
        let cores = current
            .online_cpus
            .filter(|v| *v > 0)
            .map(u64::from)
            .or_else(|| {
                current
                    .cpu_usage
                    .as_ref()?
                    .percpu_usage
                    .as_ref()
                    .map(|v| v.len() as u64)
            })?;
        if system == 0 || cores == 0 {
            return None;
        }
        let percent = cpu as f64 / system as f64 * cores as f64 * 100.0;
        percent.is_finite().then_some(percent)
    })();
    Stats {
        observed_at: std::time::SystemTime::now(),
        cpu_percent: cpu,
        memory_bytes: row.memory_stats.as_ref().and_then(|m| m.usage),
        memory_limit: row.memory_stats.as_ref().and_then(|m| m.limit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{ContainerCpuStats, ContainerCpuUsage};

    #[test]
    fn real_cpu_delta_and_missing_fields() {
        assert!(
            stats(&ContainerStatsResponse::default())
                .cpu_percent
                .is_none()
        );
        let cpu = |total, system| ContainerCpuStats {
            cpu_usage: Some(ContainerCpuUsage {
                total_usage: Some(total),
                ..Default::default()
            }),
            system_cpu_usage: Some(system),
            online_cpus: Some(4),
            ..Default::default()
        };
        let mut row = ContainerStatsResponse {
            cpu_stats: Some(cpu(200, 1000)),
            precpu_stats: Some(cpu(100, 500)),
            ..Default::default()
        };
        assert_eq!(stats(&row).cpu_percent, Some(80.0));
        row.precpu_stats = Some(cpu(300, 500));
        assert!(stats(&row).cpu_percent.is_none());
        row.precpu_stats = Some(cpu(100, 1000));
        assert!(stats(&row).cpu_percent.is_none());
    }

    #[test]
    fn error_body_never_escapes_and_remote_pipe_rejected() {
        let error = bollard::errors::Error::DockerResponseServerError {
            status_code: 403,
            message: "secret_token".into(),
        };
        assert_eq!(map_error(error), DockerError::AccessDenied);
        assert!(matches!(
            client(&Endpoint::NamedPipe("//remote/pipe/docker_engine".into())),
            Err(DockerError::InvalidInput)
        ));
        assert_eq!(text("a\x1b\x07\u{202e}b", 20), "ab");
    }
}

#[cfg(all(test, windows))]
mod pipe_tests;
