//! Windows executable identity; no resource compiler is needed on other hosts.
fn main() {
    println!("cargo:rerun-if-changed=../../assets/icons/tervix.ico");
    // Test the Cargo target, not the host running this build script. On Linux,
    // winresource invokes llvm-rc for cargo-xwin's MSVC cross-compilation.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let result = winresource::WindowsResource::new()
            .set_icon("../../assets/icons/tervix.ico")
            .set("ProductName", "Tervix")
            .set("FileDescription", "Tervix developer workspace")
            .compile();
        if let Err(error) = result {
            eprintln!("Could not embed Windows application resources: {error}");
            std::process::exit(1);
        }
    }
}
