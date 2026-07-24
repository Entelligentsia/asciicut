// Thin native entry point. All app wiring lives in the library `run()` so the
// entry stays mobile-ready (the mobile toolchains call the exported `run`
// directly) and unit-testable. Windows release builds hide the console via the
// `windows_subsystem` attribute; it is a no-op on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    asciicut_desktop_lib::run();
}
