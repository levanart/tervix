//! Interactive native OpenSSH profiles. Authentication and host policy belong to OpenSSH.

/// Validated destination and optional overrides, never credentials or remote commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    host: String,
    username: Option<String>,
    port: Option<u16>,
}

/// Invalid connection form input. Messages deliberately do not echo input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionError {
    /// Destination is empty, option-like, or contains unsupported characters.
    #[error(
        "Host must be a literal SSH alias, DNS name, IPv4 or IPv6 address (maximum 253 characters)"
    )]
    Host,
    /// Username is not a simple literal login name.
    #[error(
        "Username must start with a letter, digit or underscore and contain only letters, digits, underscore, dot or hyphen (maximum 64 characters)"
    )]
    Username,
    /// Port must be decimal and nonzero.
    #[error("Port must be empty or a decimal number from 1 to 65535")]
    Port,
}

impl Connection {
    /// Validates literal form fields; empty username/port inherit the user's SSH config.
    ///
    /// # Errors
    /// Rejects option-like destinations, shell syntax, combined `user@host`, URIs,
    /// invalid usernames and ports. Whitespace is not silently trimmed.
    pub fn new(host: &str, username: &str, port: &str) -> Result<Self, ConnectionError> {
        let simple_host = host
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
            && host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
        if host.len() > 253 || !(simple_host || host.parse::<std::net::Ipv6Addr>().is_ok()) {
            return Err(ConnectionError::Host);
        }
        if !username.is_empty()
            && (username.len() > 64
                || !username
                    .bytes()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
                || !username
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)))
        {
            return Err(ConnectionError::Username);
        }
        let port = if port.is_empty() {
            None
        } else {
            if !port.bytes().all(|c| c.is_ascii_digit()) {
                return Err(ConnectionError::Port);
            }
            Some(
                port.parse::<u16>()
                    .ok()
                    .filter(|p| *p != 0)
                    .ok_or(ConnectionError::Port)?,
            )
        };
        Ok(Self {
            host: host.into(),
            username: (!username.is_empty()).then(|| username.into()),
            port,
        })
    }

    /// Direct local SSH argv. No remote command is appended; normal config applies.
    /// `--` terminates option parsing, in addition to destination validation.
    pub fn args(&self) -> Vec<String> {
        let mut args = vec!["-t".into()];
        if let Some(username) = &self.username {
            args.extend(["-l".into(), username.clone()]);
        }
        if let Some(port) = self.port {
            args.extend(["-p".into(), port.to_string()]);
        }
        args.extend(["--".into(), self.host.clone()]);
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_argv_inherits_config_without_policy_overrides() -> Result<(), ConnectionError> {
        assert_eq!(
            Connection::new("dev-alias", "", "")?.args(),
            ["-t", "--", "dev-alias"]
        );
        assert_eq!(
            Connection::new("::1", "dev.user", "65535")?.args(),
            ["-t", "-l", "dev.user", "-p", "65535", "--", "::1"]
        );
        assert!(Connection::new("127.0.0.1", "_dev", "1").is_ok());
        Ok(())
    }

    #[test]
    fn rejects_options_shell_syntax_and_ambiguous_destinations() {
        for host in [
            "",
            "-oProxyCommand=calc",
            "dev;calc",
            "$(calc)",
            "a b",
            "a\n",
            "user@host",
            "ssh://host",
            "*.dev",
            "a/b",
            "a\"b",
            "a\\b",
            "[::1]",
        ] {
            assert_eq!(Connection::new(host, "", ""), Err(ConnectionError::Host));
        }
        for user in ["-oX", "a b", "a@b", "a;calc", "a\n", "$(calc)", "a\"b"] {
            assert_eq!(
                Connection::new("dev", user, ""),
                Err(ConnectionError::Username)
            );
        }
        for port in ["0", "65536", "-1", "+22", " 22", "22 ", "22 -oX", "2.2"] {
            assert_eq!(Connection::new("dev", "", port), Err(ConnectionError::Port));
        }
        assert_eq!(
            Connection::new(&"a".repeat(254), "", ""),
            Err(ConnectionError::Host)
        );
        assert_eq!(
            Connection::new("dev", &"a".repeat(65), ""),
            Err(ConnectionError::Username)
        );
    }
}
