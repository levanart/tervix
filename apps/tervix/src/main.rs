//! Composition root for the native layout spike and headless diagnostic demo.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod bootstrap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [arg] if arg == "--layout-demo" => bootstrap::layout_demo()?,
        [] => {
            let monitoring = tervix_application::monitoring::MonitoringService::start(
                Some(Box::new(tervix_system::NativeTelemetry::default())),
                Default::default(),
            )?;
            let discovery = tervix_terminal::NativeWslDiscovery::new()
                .map(|worker| tervix_application::wsl::DistributionCatalog::new(Box::new(worker)))
                .map_err(|error| error.to_string());
            let docker: std::sync::Arc<dyn tervix_application::docker::DockerService> =
                std::sync::Arc::new(tervix_docker::NativeDocker::new()?);
            tervix_ui::run(
                std::sync::Arc::new(tervix_docker::EngineTerminal {
                    local: std::sync::Arc::new(tervix_terminal::NativeTerminal),
                    docker: docker.clone(),
                }),
                monitoring,
                discovery,
                docker,
                tervix_platform_windows::begin_resize,
            )?;
        }
        [arg] if arg == "--help" || arg == "-h" => {
            println!("Usage: tervix [--layout-demo | --help]")
        }
        _ => return Err("unknown arguments; use --help".into()),
    }
    Ok(())
}
