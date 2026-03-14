mod models;

use std::sync::Arc;

use models::{available_models, DownloadProgress, DownloadedModel, ModelInfo, ModelManager};
use tauri::State;

type ManagerState = Arc<ModelManager>;

#[tauri::command]
async fn list_models(manager: State<'_, ManagerState>) -> Result<Vec<ModelInfo>, String> {
    let downloaded = manager
        .list_downloaded()
        .await
        .map_err(|e| e.to_string())?;
    let downloaded_names: Vec<String> = downloaded.iter().map(|m| m.name.clone()).collect();

    let mut models = available_models();
    // Mark which ones are downloaded by including all available
    // Frontend can cross-reference with downloaded list
    models.retain(|_| true);
    let _ = downloaded_names;
    Ok(models)
}

#[tauri::command]
async fn list_downloaded_models(
    manager: State<'_, ManagerState>,
) -> Result<Vec<DownloadedModel>, String> {
    manager.list_downloaded().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn download_model(
    manager: State<'_, ManagerState>,
    model_name: String,
) -> Result<(), String> {
    manager
        .download_model(&model_name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_download(
    manager: State<'_, ManagerState>,
    model_name: String,
) -> Result<(), String> {
    manager
        .cancel_download(&model_name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_model(
    manager: State<'_, ManagerState>,
    model_name: String,
) -> Result<(), String> {
    manager
        .delete_model(&model_name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_download_progress(
    manager: State<'_, ManagerState>,
    model_name: String,
) -> Result<Option<DownloadProgress>, String> {
    Ok(manager.get_download_progress(&model_name).await)
}

#[tauri::command]
async fn get_all_download_progress(
    manager: State<'_, ManagerState>,
) -> Result<Vec<DownloadProgress>, String> {
    Ok(manager.get_all_progress().await)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let model_manager = ModelManager::new().expect("Failed to initialize model manager");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(model_manager)
        .invoke_handler(tauri::generate_handler![
            list_models,
            list_downloaded_models,
            download_model,
            cancel_download,
            delete_model,
            get_download_progress,
            get_all_download_progress,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
