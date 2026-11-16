use serde::Deserialize;
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
use tervix_application::docker::*;

const FILE_LIMIT: u64 = 64 * 1024;

#[derive(Deserialize, Default)]
struct Config {
    #[serde(rename = "currentContext", default)]
    current: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Metadata {
    name: String,
    endpoints: HashMap<String, Transport>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Transport {
    host: String,
    #[serde(default)]
    skip_tls_verify: bool,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, DockerError> {
    // No follow of context metadata symlinks. Credentials and TLS directories are never opened.
    let info = std::fs::symlink_metadata(path).map_err(|_| DockerError::Unavailable)?;
    if !info.is_file() || info.len() > FILE_LIMIT {
        return Err(DockerError::InvalidInput);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| DockerError::AccessDenied)?
        .take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DockerError::Unavailable)?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err(DockerError::InvalidInput);
    }
    serde_json::from_slice(&bytes).map_err(|_| DockerError::Protocol)
}

pub(crate) fn discover() -> Result<Arc<[Context]>, DockerError> {
    let root = std::env::var_os("DOCKER_CONFIG")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".docker"))
        })
        .ok_or(DockerError::Unavailable)?;
    discover_at(&root)
}

fn discover_at(root: &Path) -> Result<Arc<[Context]>, DockerError> {
    let config_path = root.join("config.json");
    let config: Config = if config_path.exists() {
        read(&config_path)?
    } else {
        Config::default()
    };
    let mut result = vec![Context {
        name: "default".into(),
        selected: config.current.is_empty() || config.current == "default",
        endpoint: Some(Endpoint::default()),
        error: None,
    }];
    let directory = root.join("contexts").join("meta");
    if !directory.exists() {
        return Ok(result.into());
    }
    let directories = std::fs::read_dir(directory).map_err(|_| DockerError::AccessDenied)?;
    for entry in directories.take(128) {
        let entry = entry.map_err(|_| DockerError::Unavailable)?;
        if !entry
            .file_type()
            .map_err(|_| DockerError::Unavailable)?
            .is_dir()
        {
            continue;
        }
        let meta: Metadata = match read(&entry.path().join("meta.json")) {
            Ok(meta) => meta,
            Err(_) => {
                continue;
            } // One malformed context cannot hide other local contexts.
        };
        let endpoint = meta
            .endpoints
            .get("docker")
            .ok_or(DockerError::Unsupported)
            .and_then(|transport| parse_transport(&transport.host, transport.skip_tls_verify));
        let (endpoint, error) = match endpoint {
            Ok(endpoint) => (Some(endpoint), None),
            Err(error) => (None, Some(error)),
        };
        result.push(Context {
            selected: meta.name == config.current,
            name: crate::engine::text(&meta.name, 128),
            endpoint,
            error,
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result.into())
}

fn parse_transport(host: &str, skip_verify: bool) -> Result<Endpoint, DockerError> {
    if skip_verify {
        return Err(DockerError::Unsupported);
    }
    let endpoint = if let Some(name) = host.strip_prefix("npipe:////./pipe/") {
        Endpoint::NamedPipe(format!("//./pipe/{name}"))
    } else if let Some(path) = host.strip_prefix("unix://") {
        Endpoint::UnixSocket(path.into())
    } else {
        return Err(DockerError::Unsupported);
    };
    // Validates local namespace and platform without making a connection.
    crate::engine::validate(&endpoint)?;
    Ok(endpoint)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed JSON fixtures must deserialize or fail the test.
mod tests {
    use super::*;

    #[test]
    fn refuses_insecure_remote_and_unverified_transports() {
        for host in [
            "tcp://localhost:2375",
            "ssh://user@host",
            "https://host:2376",
            "npipe:////remote/pipe/docker_engine",
            "unix://relative",
        ] {
            assert!(parse_transport(host, false).is_err());
        }
        assert!(parse_transport("npipe:////./pipe/docker_engine", true).is_err());
        #[cfg(windows)]
        assert_eq!(
            parse_transport("npipe:////./pipe/dockerDesktopLinuxEngine", false),
            Ok(Endpoint::NamedPipe(
                "//./pipe/dockerDesktopLinuxEngine".into()
            ))
        );
    }

    #[test]
    fn ignores_auth_and_tls_material_in_json() {
        let config: Config = serde_json::from_str(
            r#"{"currentContext":"desktop-linux","auths":{"private":{"auth":"secret"}}}"#,
        )
        .unwrap();
        assert_eq!(config.current, "desktop-linux");
        let meta: Metadata = serde_json::from_str(r#"{"Name":"desktop-linux","Endpoints":{"docker":{"Host":"npipe:////./pipe/docker_engine","SkipTLSVerify":false}},"TLSMaterial":{"docker":["secret"]}}"#).unwrap();
        assert_eq!(meta.name, "desktop-linux");
        assert_eq!(meta.endpoints.len(), 1);
    }
}
