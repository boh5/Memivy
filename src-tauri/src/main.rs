mod backup;
mod capture_panel;
mod cleanup;
mod desktop;
mod mcp;
mod prototype;
mod storage;
mod voice;
mod workspace;
fn main() {
    let mut context = tauri::generate_context!();
    if std::env::var("MEMIVY_PHASE1_PROTOTYPE").as_deref() == Ok("1") {
        *context.config_mut() = serde_json::from_str(include_str!("../tauri.prototype.conf.json"))
            .expect("bundled prototype config must be valid");
        prototype::run(context);
    } else {
        workspace::run(context);
    }
}
