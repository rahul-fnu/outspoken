mod audio;
mod models;
mod transcription;

use std::collections::HashMap;
use std::sync::Arc;

use models::{CancellationMap, ProgressMap};
use tokio::sync::Mutex;

type AudioState = Arc<std::sync::Mutex<Option<audio::RecordingState>>>;
type SelectedDevice = Arc<std::sync::Mutex<Option<String>>>;

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

#[tauri::command]
fn list_audio_devices() -> Result<Vec<audio::AudioDeviceInfo>, String> {
    audio::list_devices()
}

#[tauri::command]
fn select_audio_device(
    device_name: Option<String>,
    selected_device: tauri::State<'_, SelectedDevice>,
) -> Result<(), String> {
    let mut dev = selected_device
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    *dev = device_name;
    Ok(())
}

#[tauri::command]
fn start_recording(
    audio_state: tauri::State<'_, AudioState>,
    selected_device: tauri::State<'_, SelectedDevice>,
) -> Result<(), String> {
    let mut state = audio_state
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;

    if state.is_some() {
        return Err("Already recording".into());
    }

    let device_name = selected_device
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?
        .clone();

    let recording = audio::start_capture(&device_name)?;
    *state = Some(recording);
    Ok(())
}

#[tauri::command]
fn stop_recording(
    audio_state: tauri::State<'_, AudioState>,
) -> Result<Vec<f32>, String> {
    let mut state = audio_state
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;

    let recording = state.take().ok_or("Not currently recording")?;

    // Signal to stop collecting samples.
    recording
        .is_recording
        .store(false, std::sync::atomic::Ordering::Relaxed);

    // Extract the buffer. The stream will be dropped when RecordingState is dropped.
    let buffer = recording
        .buffer
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?
        .clone();

    Ok(buffer)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let progress_map: ProgressMap = Arc::new(Mutex::new(HashMap::new()));
    let cancellation_map: CancellationMap = Arc::new(Mutex::new(HashMap::new()));
    let audio_state: AudioState = Arc::new(std::sync::Mutex::new(None));
    let selected_device: SelectedDevice = Arc::new(std::sync::Mutex::new(None));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(progress_map)
        .manage(cancellation_map)
        .manage(audio_state)
        .manage(selected_device)
        .invoke_handler(tauri::generate_handler![
            greet,
            list_available_models,
            list_models,
            download_model,
            cancel_download,
            get_download_progress,
            delete_model,
            list_audio_devices,
            select_audio_device,
            start_recording,
            stop_recording,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
