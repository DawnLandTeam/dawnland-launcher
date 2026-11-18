use std::path::PathBuf;

const WINDOWS_MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;

/// Writes the Windows application manifest to `OUT_DIR` when missing or changed.
fn write_windows_manifest() -> std::io::Result<PathBuf> {
    let out_dir = std::env::var_os("OUT_DIR").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "OUT_DIR is not set")
    })?;
    let manifest_path = PathBuf::from(out_dir).join("windows.manifest");
    if std::fs::read(&manifest_path).ok().as_deref() != Some(WINDOWS_MANIFEST.as_bytes()) {
        std::fs::write(&manifest_path, WINDOWS_MANIFEST)?;
    }
    Ok(manifest_path)
}

fn main() {
    println!("cargo:rerun-if-env-changed=APTABASE_KEY");
    println!("cargo:rerun-if-env-changed=APTABASE_URL");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    if target_os == "windows" {
        match target_env.as_str() {
            "msvc" => {
                println!("cargo:rustc-link-arg-bins=/STACK:8388608");

                let manifest_path = match write_windows_manifest() {
                    Ok(path) => path,
                    Err(error) => {
                        eprintln!("failed to write Windows application manifest: {error}");
                        std::process::exit(1);
                    }
                };

                // Embed a complete application manifest (ComCtl32 v6, asInvoker, and PerMonitorV2
                // DPI awareness) across all MSVC link targets instead of tauri-build's bin-only
                // manifest, preventing 0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND) in test binaries.
                println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
                println!(
                    "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
                    manifest_path.display()
                );

                let attrs = tauri_build::Attributes::new()
                    .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
                if let Err(error) = tauri_build::try_build(attrs) {
                    eprintln!("failed to run tauri build script: {error:#}");
                    std::process::exit(1);
                }
            }
            "gnu" => {
                println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608");
                tauri_build::build();
            }
            _ => tauri_build::build(),
        }
    } else {
        tauri_build::build();
    }
}
