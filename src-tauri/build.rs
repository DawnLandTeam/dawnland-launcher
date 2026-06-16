fn main() {
    println!("cargo:rerun-if-env-changed=APTABASE_KEY");
    println!("cargo:rerun-if-env-changed=APTABASE_URL");
    tauri_build::build()
}
