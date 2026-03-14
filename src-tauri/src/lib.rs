mod models;
mod transcription;

use std::collections::HashMap;
use std::sync::Arc;

use models::{CancellationMap, ProgressMap};
use tokio::sync::Mutex;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Outspoken.", name)
}

#[tauri::command]
fn list_available_models() -> Vec<models::ModelInfo> {
    models::available_models()
}

#[tauri::command]
fn list_models() -> Result<Vec<models::DownloadedModel>, String> {
    models::list_downloaded_models()
}

#[tauri::command]
async fn download_model(
    model_name: String,
    progress_map: tauri::State<'_, ProgressMap>,
    cancellation_map: tauri::State<'_, CancellationMap>,
) -> Result<models::DownloadedModel, String> {
    models::download_model(
        model_name,
        progress_map.inner().clone(),
        cancellation_map.inner().clone(),
    )
    .await
}

#[tauri::command]
async fn cancel_download(
    model_name: String,
    cancellation_map: tauri::State<'_, CancellationMap>,
) -> Result<(), String> {
    let map = cancellation_map.lock().await;
    if let Some(token) = map.get(&model_name) {
        token.cancel();
        Ok(())
    } else {
        Err(format!("No active download for model: {model_name}"))
    }
}

#[tauri::command]
async fn get_download_progress(
    model_name: String,
    progress_map: tauri::State<'_, ProgressMap>,
) -> Result<models::DownloadProgress, String> {
    let map = progress_map.lock().await;
    map.get(&model_name)
        .cloned()
        .ok_or_else(|| format!("No download progress for model: {model_name}"))
}

#[tauri::command]
fn delete_model(name: String) -> Result<(), String> {
    models::delete_model(&name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let progress_map: ProgressMap = Arc::new(Mutex::new(HashMap::new()));
    let cancellation_map: CancellationMap = Arc::new(Mutex::new(HashMap::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(progress_map)
        .manage(cancellation_map)
        .invoke_handler(tauri::generate_handler![
            greet,
            list_available_models,
            list_models,
            download_model,
            cancel_download,
            get_download_progress,
            delete_model,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
