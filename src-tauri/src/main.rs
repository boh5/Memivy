mod backup;
mod capture_panel;
mod cleanup;
mod desktop;
mod errors;
mod i18n;
mod mcp;
mod models;
mod storage;
mod updates;
#[cfg(test)]
mod updates_tests;
mod voice;
mod workspace;
fn main() {
    workspace::run(tauri::generate_context!());
}
