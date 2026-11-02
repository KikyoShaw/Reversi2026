use std::sync::Arc;
use tauri::{Manager, State};

mod game;
mod network;
mod session;
mod storage;

use session::{GameAction, Hint, Hub, Settings, Snapshot};

#[tauri::command]
fn get_snapshot(hub: State<'_, Arc<Hub>>) -> Snapshot { hub.snapshot() }

#[tauri::command]
fn new_game(hub: State<'_, Arc<Hub>>, settings: Settings) -> Result<Snapshot, String> { hub.new_game(settings) }

#[tauri::command]
fn update_preferences(hub: State<'_, Arc<Hub>>, sound: bool, show_hints: bool) -> Result<Snapshot, String> {
    hub.preferences(sound, show_hints)
}

#[tauri::command]
fn game_action(hub: State<'_, Arc<Hub>>, action: GameAction, revision: u64, session_id: u64, game_id: u64, turn_token: u64) -> Result<Snapshot, String> {
    hub.action(action, revision, session_id, game_id, turn_token)
}

#[tauri::command]
async fn get_hint(hub: State<'_, Arc<Hub>>, revision: u64, session_id: u64, game_id: u64, turn_token: u64) -> Result<Hint, String> {
    let hub = hub.inner().clone();
    hub.hint(revision, session_id, game_id, turn_token).await
}

#[tauri::command]
async fn host_lan(hub: State<'_, Arc<Hub>>, port: u16, settings: Settings) -> Result<Snapshot, String> {
    let hub = hub.inner().clone();
    if port == 0 { return Err("端口应为 1～65535".into()); }
    settings.validate()?;
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await
        .map_err(|error| format!("无法创建房间，请检查端口是否被占用：{error}"))?;
    hub.begin_host(listener, settings, port)
}

#[tauri::command]
fn join_lan(hub: State<'_, Arc<Hub>>, address: String, port: u16, player_name: String) -> Result<Snapshot, String> {
    hub.begin_join(address, port, player_name)
}

#[tauri::command]
fn leave_lan(hub: State<'_, Arc<Hub>>) -> Result<Snapshot, String> { hub.leave() }

#[tauri::command]
fn respond_request(hub: State<'_, Arc<Hub>>, id: u64, accept: bool, revision: u64, session_id: u64, game_id: u64, turn_token: u64) -> Result<Snapshot, String> {
    hub.respond(id, accept, revision, session_id, game_id, turn_token)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let hub = Hub::new(app.handle().clone(), app.path().app_config_dir()?);
            app.manage(hub.clone());
            hub.start_clock();
            hub.refresh();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot, new_game, update_preferences, game_action, get_hint,
            host_lan, join_lan, leave_lan, respond_request,
        ])
        .build(tauri::generate_context!())
        .expect("无法启动黑白棋")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event { app.state::<Arc<Hub>>().shutdown(); }
        });
}