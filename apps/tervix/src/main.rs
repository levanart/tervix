//! Composition root for the Phase 0 headless layout demonstration.

mod bootstrap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [arg] if arg == "--layout-demo" => bootstrap::layout_demo()?,
        [] => println!(
            "Tervix — Phase 0 foundation (no GUI or PTY yet).\nUse --layout-demo to exercise recursive tiling."
        ),
        [arg] if arg == "--help" || arg == "-h" => {
            println!("Usage: tervix [--layout-demo | --help]")
        }
        _ => return Err("unknown arguments; use --help".into()),
    }
    Ok(())
}
