// Tauri v2 build script. `tauri_build::build()` runs the codegen that reads
// `tauri.conf.json`, wires the bundled `frontendDist`, and generates the
// capability/permission schemas consumed by `tauri::generate_context!()`.
fn main() {
    tauri_build::build();
}
