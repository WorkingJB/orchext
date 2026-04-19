#![forbid(unsafe_code)]

mod commands;
mod state;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::vault_open,
            commands::vault_info,
            commands::doc_list,
            commands::doc_read,
            commands::doc_write,
            commands::doc_delete,
            commands::token_list,
            commands::token_issue,
            commands::token_revoke,
            commands::audit_list,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mytex-desktop");
}
