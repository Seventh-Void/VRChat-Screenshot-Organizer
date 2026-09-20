fn main() {
    // Force re-run when frontend files change (Tauri embeds these at build time)
    println!("cargo:rerun-if-changed=../frontend/index.html");
    println!("cargo:rerun-if-changed=../frontend/");
    tauri_build::build()
}
