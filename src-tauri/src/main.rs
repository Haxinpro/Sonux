// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK rendering workarounds; see `linux_render` for the reasons.
    #[cfg(target_os = "linux")]
    sonux_lib::linux_render::configure();

    sonux_lib::run()
}
