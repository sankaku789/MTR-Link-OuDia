// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use mtr_oudia_application::DomainPort;
use mtr_oudia_domain::DomainLayer;
use mtr_oudia_infrastructure::StaticDomainPort;

/// Infrastructure の Port 実装を Application 経由で組み立てる。
fn compose_domain_layer() -> DomainLayer {
    StaticDomainPort.domain_layer()
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = compose_domain_layer();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
