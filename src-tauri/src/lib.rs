//! TLX Control — backend Tauri: stato dell'applicazione, comandi IPC, eventi verso la UI.
//! Contratto: docs/IPC.md.

mod config;
mod ipc;
mod plan;
mod state;

use state::App;
use std::sync::Arc;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Secondo doppio clic: porta in primo piano la finestra già aperta.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
            let st = Arc::new(App::new(app.handle().clone(), dir.join("config.json")));
            app.manage(st.clone());
            // Connessione automatica al profilo attivo: l'operatore non deve fare nulla.
            tauri::async_runtime::spawn(async move {
                st.autoconnect().await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::get_config,
            ipc::save_config,
            ipc::export_config,
            ipc::import_config,
            ipc::import_legacy_presets,
            ipc::export_log,
            ipc::list_serial_ports,
            ipc::connect,
            ipc::disconnect,
            ipc::get_snapshot,
            ipc::refresh_routing,
            ipc::apply_routes,
            ipc::link,
            ipc::disconnect_port,
            ipc::disconnect_all,
            ipc::execute_preset,
            ipc::snapshot_to_preset,
            ipc::undo_last,
            ipc::send_raw,
            ipc::read_diagnostics,
            ipc::clear_log,
            ipc::get_log,
        ])
        .build(tauri::generate_context!())
        .expect("avvio di TLX Control fallito")
        .run(|app, event| {
            // Chiusura della finestra = chiusura di tutto: XQUIT e stop del simulatore.
            if let tauri::RunEvent::Exit = event {
                if let Some(st) = app.try_state::<Arc<App>>() {
                    let st = st.inner().clone();
                    tauri::async_runtime::block_on(async move {
                        // 3 s: l'`XQUIT` ha un timeout proprio di 500 ms e su una LAN si
                        // conclude subito, ma un comando di stato su una matrice grande può
                        // essere in volo. Non si attende `ClientConfig::max_cmd_timeout()`
                        // (≈16 s) perché chiudendo la finestra l'operatore vuole che si
                        // chiuda: il ricevitore UDP viene fermato da `shutdown()` prima
                        // dell'attesa, quindi la porta 17564 resta libera comunque, e una
                        // sessione troncata la matrice la recupera col suo `--idle`.
                        let _ =
                            tokio::time::timeout(std::time::Duration::from_secs(3), st.shutdown())
                                .await;
                    });
                }
            }
        });
}
