#![cfg(windows)]
//! Real Windows PTY contract tests; no fake shell output.
use std::time::{Duration, Instant};
use tervix_application::terminal::{
    GridSize, Lifecycle, Profile, Snapshot, TerminalProvider, TerminalSession,
};
use tervix_terminal::NativeTerminal;

fn wait(
    session: &mut dyn TerminalSession,
    predicate: impl Fn(&Snapshot) -> bool,
) -> Result<Snapshot, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last = None;
    while Instant::now() < deadline {
        if let Some(frame) = session.poll() {
            if predicate(&frame) {
                return Ok(frame);
            }
            if let Lifecycle::Failed(error) = &frame.lifecycle {
                return Err(error.clone().into());
            }
            if matches!(frame.lifecycle, Lifecycle::Exited(_) | Lifecycle::Closed) {
                return Err(format!(
                    "PTY ended before expected state: {:?}; screen={:?}",
                    frame.lifecycle,
                    output(&frame)
                )
                .into());
            }
            last = Some(frame.lifecycle);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(format!("Timed out waiting for PTY state; last={last:?}").into())
}
fn output(frame: &Snapshot) -> String {
    frame.cells.iter().map(|c| c.text.as_str()).collect()
}

#[test]
fn cmd_input_output_resize_exit_and_cancel() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = NativeTerminal.start(Profile::Cmd, GridSize::new(100, 25))?;
    let identity = session.id();
    wait(&mut *session, |s| matches!(s.lifecycle, Lifecycle::Running))?;
    session.input(b"echo TERVIX_REAL_CONPTY\r".to_vec())?;
    wait(&mut *session, |s| {
        output(s).matches("TERVIX_REAL_CONPTY").count() >= 2
    })?;
    let resized = GridSize::new(72, 18);
    session.resize(resized)?;
    wait(&mut *session, |s| s.size == resized)?;
    assert_eq!(session.id(), identity);
    session.input(b"exit 7\r".to_vec())?;
    wait(&mut *session, |s| s.lifecycle == Lifecycle::Exited(7))?;
    let mut cancelled = NativeTerminal.start(Profile::Cmd, GridSize::new(80, 24))?;
    wait(&mut *cancelled, |s| s.lifecycle == Lifecycle::Running)?;
    cancelled.close();
    wait(&mut *cancelled, |s| s.lifecycle == Lifecycle::Closed)?;
    Ok(())
}

#[test]
fn discovered_default_powershell_is_interactive() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = NativeTerminal.start(Profile::PowerShell, GridSize::new(100, 25))?;
    wait(&mut *session, |s| s.lifecycle == Lifecycle::Running)?;
    session.input(b"Write-Output ('TERVIX_' + 'POWERSHELL_OK')\r".to_vec())?;
    wait(&mut *session, |s| {
        output(s).contains("TERVIX_POWERSHELL_OK")
    })?;
    session.close();
    wait(&mut *session, |s| s.lifecycle == Lifecycle::Closed)?;
    Ok(())
}

#[test]
fn invalid_distribution_is_a_real_failed_session() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = NativeTerminal.start(
        Profile::Wsl(Some("tervix-nonexistent-test-distribution".into())),
        GridSize::new(80, 24),
    )?;
    let frame = wait(&mut *session, |s| {
        matches!(s.lifecycle, Lifecycle::Failed(_))
    })?;
    assert!(matches!(frame.lifecycle, Lifecycle::Failed(_)));
    Ok(())
}

#[test]
fn concurrent_shell_close_is_closed_and_does_not_cancel_its_neighbor()
-> Result<(), Box<dyn std::error::Error>> {
    let mut survivor = NativeTerminal.start(Profile::Cmd, GridSize::new(100, 25))?;
    wait(&mut *survivor, |s| s.lifecycle == Lifecycle::Running)?;
    // Exercise writer/owner cancellation scheduling repeatedly, with another
    // console alive throughout. No process-wide lock or suite serialization.
    for _ in 0..12 {
        let mut closing = NativeTerminal.start(Profile::PowerShell, GridSize::new(100, 25))?;
        wait(&mut *closing, |s| s.lifecycle == Lifecycle::Running)?;
        closing.input(b"Write-Output ('CLOSE_' + 'READY')\r".to_vec())?;
        wait(&mut *closing, |s| output(s).contains("CLOSE_READY"))?;
        closing.close();
        wait(&mut *closing, |s| s.lifecycle == Lifecycle::Closed)?;
    }
    survivor.input(b"echo NEIGHBOR_ALIVE & exit 0\r".to_vec())?;
    let frame = wait(&mut *survivor, |s| s.lifecycle == Lifecycle::Exited(0))?;
    assert!(output(&frame).contains("NEIGHBOR_ALIVE"));
    Ok(())
}

#[test]
fn immediate_close_acknowledges_even_before_startup() -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..8 {
        let mut session = NativeTerminal.start(Profile::Cmd, GridSize::new(80, 24))?;
        session.close();
        wait(&mut *session, |s| s.lifecycle == Lifecycle::Closed)?;
    }
    Ok(())
}

#[test]
fn cancellation_under_output_backpressure_and_final_bytes_on_exit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = NativeTerminal.start(Profile::Cmd, GridSize::new(80, 24))?;
    wait(&mut *session, |s| s.lifecycle == Lifecycle::Running)?;
    session.input(b"for /L %i in (1,1,100000) do @echo FLOOD_%i\r".to_vec())?;
    // Deliberately stop consuming immutable frames; byte parsing/backpressure must
    // stay bounded and cancellation must remain independent of full data queues.
    std::thread::sleep(Duration::from_millis(200));
    session.close();
    wait(&mut *session, |s| s.lifecycle == Lifecycle::Closed)?;
    let mut final_session = NativeTerminal.start(Profile::Cmd, GridSize::new(100, 25))?;
    wait(&mut *final_session, |s| s.lifecycle == Lifecycle::Running)?;
    final_session.input(b"echo TERVIX_FINAL_OUTPUT & exit 3\r".to_vec())?;
    let frame = wait(&mut *final_session, |s| s.lifecycle == Lifecycle::Exited(3))?;
    assert!(output(&frame).contains("TERVIX_FINAL_OUTPUT"));
    Ok(())
}

#[test]
#[ignore = "requires a usable default WSL distribution; run explicitly and report environment"]
fn wsl_default_shell_input_output() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = NativeTerminal.start(Profile::Wsl(None), GridSize::new(100, 25))?;
    wait(&mut *session, |s| s.lifecycle == Lifecycle::Running)?;
    session.input(b"printf 'TERVIX_%s\\n' WSL_OK\r".to_vec())?;
    wait(&mut *session, |s| output(s).contains("TERVIX_WSL_OK"))?;
    session.input(b"exit\r".to_vec())?;
    wait(&mut *session, |s| {
        matches!(s.lifecycle, Lifecycle::Exited(_))
    })?;
    Ok(())
}

#[test]
#[ignore = "explicitly launches a discovered WSL distro with direct Linux argv"]
fn wsl_selected_distribution_direct_argv_and_linux_cwd() -> Result<(), Box<dyn std::error::Error>> {
    use tervix_application::wsl::{DistributionName, Launch};
    let names = tervix_terminal::discover_wsl()?;
    let name = names
        .iter()
        .find(|name| !name.starts_with("docker-desktop"))
        .ok_or("no installed user WSL distributions")?;
    println!("Launching selected WSL distribution: {name}");
    let profile = Launch {
        distribution: Some(DistributionName::new(name.clone())?),
        linux_cwd: Some("/tmp".into()),
        argv: vec![
            "/usr/bin/printf".into(),
            "%s".into(),
            "TERVIX_DIRECT_WSL_OK; $(literal)".into(),
        ],
    }
    .profile()?;
    let mut session = NativeTerminal.start(profile, GridSize::new(100, 25))?;
    let frame = wait(&mut *session, |s| s.lifecycle == Lifecycle::Exited(0))?;
    assert!(output(&frame).contains("TERVIX_DIRECT_WSL_OK; $(literal)"));
    let profile = Launch {
        distribution: Some(DistributionName::new(name.clone())?),
        linux_cwd: Some("/tmp".into()),
        argv: vec!["/usr/bin/pwd".into()],
    }
    .profile()?;
    let mut session = NativeTerminal.start(profile, GridSize::new(100, 25))?;
    let frame = wait(&mut *session, |s| s.lifecycle == Lifecycle::Exited(0))?;
    assert!(output(&frame).contains("/tmp"));
    Ok(())
}
