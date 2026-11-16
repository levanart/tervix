use portable_pty::CommandBuilder;
use std::path::PathBuf;
use tervix_application::{
    terminal::Profile,
    wsl::{DistributionName, Launch},
};

pub(crate) fn executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}

pub(crate) fn command(profile: Profile) -> Result<CommandBuilder, String> {
    if !cfg!(windows) {
        return Err("This terminal adapter currently supports Windows only".into());
    }
    let powershell = || {
        executable("pwsh.exe").or_else(|| {
            std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .map(|p| p.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
                .filter(|p| p.is_file())
        })
    };
    let cmd = || {
        std::env::var_os("COMSPEC")
            .map(PathBuf::from)
            .or_else(|| executable("cmd.exe"))
    };
    let mut result = match profile {
        Profile::DockerExec(_) => {
            return Err("Docker exec requires the Engine terminal provider".into());
        }
        Profile::Default => CommandBuilder::new(
            powershell()
                .or_else(cmd)
                .ok_or("No PowerShell or cmd installed")?,
        ),
        Profile::PowerShell => {
            CommandBuilder::new(powershell().ok_or("PowerShell is unavailable")?)
        }
        Profile::Cmd => CommandBuilder::new(cmd().ok_or("cmd.exe is unavailable")?),
        Profile::Wsl(distribution) => {
            return wsl_command(Launch {
                distribution: distribution
                    .map(DistributionName::new)
                    .transpose()
                    .map_err(|e| e.to_string())?,
                linux_cwd: None,
                argv: Vec::new(),
            });
        }
        Profile::WslLaunch(launch) => return wsl_command(launch),
        Profile::Ssh(connection) => {
            let program = executable("ssh.exe").or_else(|| {
                std::env::var_os("SystemRoot")
                    .map(PathBuf::from)
                    .map(|p| p.join("System32/OpenSSH/ssh.exe"))
                    .filter(|p| p.is_file())
            }).ok_or("Windows OpenSSH (ssh.exe) is unavailable. Install the OpenSSH Client optional feature or add it to PATH, then connect again.")?;
            let mut command = CommandBuilder::new(program);
            command.args(connection.args());
            return Ok(command);
        }
    };
    result.arg("/Q");
    // Inspect the resolved executable, not the requested profile.
    let program = result.get_argv()[0].to_string_lossy().to_ascii_lowercase();
    if program.contains("powershell") || program.contains("pwsh") {
        result = CommandBuilder::new(result.get_argv()[0].clone());
        result.arg("-NoLogo");
    }
    Ok(result)
}

fn wsl_command(launch: Launch) -> Result<CommandBuilder, String> {
    launch.validate().map_err(|e| e.to_string())?;
    let distros = super::wsl::discover_wsl()?;
    if distros.is_empty() {
        return Err(
            "WSL has no installed distributions. Install a distribution and restart this pane."
                .into(),
        );
    }
    if launch
        .distribution
        .as_ref()
        .is_some_and(|name| !distros.iter().any(|d| d == name.as_str()))
    {
        return Err("The selected WSL distribution is unavailable".into());
    }
    let mut command = CommandBuilder::new(executable("wsl.exe").ok_or("WSL is unavailable")?);
    command.args(wsl_args(&launch));
    Ok(command)
}

fn wsl_args(launch: &Launch) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(name) = &launch.distribution {
        args.extend(["--distribution".into(), name.as_str().into()]);
    }
    if let Some(cwd) = &launch.linux_cwd {
        args.extend(["--cd".into(), cwd.clone()]);
    }
    if !launch.argv.is_empty() {
        // `--` alone still uses WSL's default shell and can interpret metacharacters.
        // --exec is required for literal Linux argv (and consumes the remaining args).
        args.push("--exec".into());
        args.extend(launch.argv.iter().cloned());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(windows)]
    fn native_ssh_command_uses_exact_argv_or_reports_unavailable()
    -> Result<(), Box<dyn std::error::Error>> {
        let connection =
            tervix_application::ssh::Connection::new("dev-alias", "developer", "2222")?;
        match command(Profile::Ssh(connection)) {
            Ok(command) => {
                let args: Vec<_> = command
                    .get_argv()
                    .iter()
                    .skip(1)
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect();
                assert_eq!(
                    args,
                    ["-t", "-l", "developer", "-p", "2222", "--", "dev-alias"]
                );
            }
            Err(error) => assert!(error.contains("OpenSSH") && error.contains("unavailable")),
        }
        Ok(())
    }
    #[test]
    fn explicit_distribution_cwd_and_command_are_separate_arguments()
    -> Result<(), Box<dyn std::error::Error>> {
        let launch = Launch {
            distribution: Some(DistributionName::new("Development Linux")?),
            linux_cwd: Some("/home/user/a b".into()),
            argv: vec![
                "/usr/bin/printf".into(),
                "%s".into(),
                "$(touch /tmp/no); &".into(),
            ],
        };
        launch.validate()?;
        assert_eq!(
            wsl_args(&launch),
            [
                "--distribution",
                "Development Linux",
                "--cd",
                "/home/user/a b",
                "--exec",
                "/usr/bin/printf",
                "%s",
                "$(touch /tmp/no); &"
            ]
        );
        Ok(())
    }
}
