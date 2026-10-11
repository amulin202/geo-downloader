fn main() {
    // Rebuild native resources when branding changes; tauri-build only watches
    // configuration and capabilities by default.
    println!("cargo:rerun-if-changed=icons");
  tauri_build::build()
}
