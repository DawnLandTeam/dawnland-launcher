fn main() {
    println!("cargo:rerun-if-env-changed=APTABASE_KEY");
    println!("cargo:rerun-if-env-changed=APTABASE_URL");

    let is_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let is_msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");

    if is_windows {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }

    if is_windows && is_msvc {
        // Embed the ComCtl32 v6 manifest into all MSVC link targets (binaries,
        // lib/integration tests, and cdylib) instead of tauri-build's bin-only manifest,
        // preventing 0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND) in test binaries.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );

        let attrs = tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        if let Err(error) = tauri_build::try_build(attrs) {
            eprintln!("failed to run tauri build script: {error:#}");
            std::process::exit(1);
        }
    } else {
        tauri_build::build();
    }
}
