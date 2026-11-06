fn main() {
    println!("cargo:rerun-if-env-changed=APTABASE_KEY");
    println!("cargo:rerun-if-env-changed=APTABASE_URL");

    #[cfg(target_os = "windows")]
    println!("cargo:rustc-link-arg-bins=/STACK:8388608");

    tauri_build::build()
}
