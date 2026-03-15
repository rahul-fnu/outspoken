

```rust
mod audio;
mod audio_level;
mod hotkey;
mod models;
mod transcription;
mod tray;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use audio_level::{AudioLevelMonitor, SilenceConfig};
use hotkey::{HotkeyConfig, HotkeyConfigState};
use models::{CancellationMap, ProgressMap};
use tokio::sync::Mutex;
use transcription::{SupportedLanguage, TranscriptionConfig, TranscriptionResult, TranscriptionService};
use tray::{TrayRecordingState, TrayState};

type AudioState = Arc<std::sync::Mutex<Option<audio::RecordingState>>>;
type SelectedDevice = Arc<std::sync::Mutex<Option<String>>>;
type SilenceConfigState = Arc<std::sync::Mutex<SilenceConfig>>;
type TranscriptionServiceState = Arc<std::sync::Mutex<Option<TranscriptionService>>>;

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
    app_handle: tauri::AppHandle,
    audio_state: tauri::State<'_, AudioState>,
    selected_device: tauri::State<'_, SelectedDevice>,
    silence_config: tauri::State<'_, SilenceConfigState>,
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

    let config = silence_config
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?
        .clone();

    // Create level monitor and wrap in a callback for the audio stream.
    let monitor = Arc::new(std::sync::Mutex::new(AudioLevelMonitor::new(
        app_handle, config,
    )));
    let sample_callback: audio::SampleCallback = Arc::new(move |samples: &[f32]| {
        if let Ok(mut mon) = monitor.lock() {
            mon.process_samples(samples);
        }
    });

    let recording = audio::start_capture(&device_name, Some(sample_callback))?;
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

#[tauri::command]
fn set_silence_config(
    threshold_db: f32,
    duration_secs: f32,
    silence_config: tauri::State<'_, SilenceConfigState>,
) -> Result<(), String> {
    let mut config = silence_config
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    config.threshold_db = threshold_db;
    config.duration_secs = duration_secs;
    Ok(())
}

#[tauri::command]
async fn load_transcription_model(
    model_name: String,
    config: Option<TranscriptionConfig>,
    service_state: tauri::State<'_, TranscriptionServiceState>,
) -> Result<(), String> {
    // Look up the model path from downloaded models.
    let downloaded = models::list_downloaded_models()?;
    let model = downloaded
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Model not downloaded: {model_name}"))?;

    let model_path = PathBuf::from(&model.path);
    if !model_path.exists() {
        return Err(format!("Model file not found: {}", model.path));
    }

    let cfg = config.unwrap_or_default();
    let svc = tokio::task::spawn_blocking(move || {
        TranscriptionService::new(&model_path, cfg)
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))??;

    let mut state = service_state
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    *state = Some(svc);
    Ok(())
}

#[tauri::command]
fn list_supported_languages() -> Vec<SupportedLanguage> {
    transcription::supported_languages()
}

#[tauri::command]
async fn transcribe_recording(
    audio_data: Vec<f32>,
    service_state: tauri::State<'_, TranscriptionServiceState>,
) -> Result<TranscriptionResult, String> {
    let service = {
        let state = service_state
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        state
            .as_ref()
            .ok_or("No transcription model loaded. Call load_transcription_model first.")?
            .clone()
    };

    let result = tokio::task::spawn_blocking(move || service.transcribe(&audio_data))
        .await
        .map_err(|e| format!("Task join error: {e}"))??;

    Ok(result)
}

#[tauri::command]
fn set_tray_state(
    app_handle: tauri::AppHandle,
    state: String,
) -> Result<(), String> {
    let tray_state = match state.as_str() {
        "idle" => TrayState::Idle,
        "recording" => TrayState::Recording,
        "processing" => TrayState::Processing,
        _ => return Err(format!("Invalid tray state: {state}")),
    };
    tray::update_tray_state(&app_handle, tray_state);
    Ok(())
}

#[tauri::command]
fn get_hotkey(
    hotkey_config: tauri::State<'_, HotkeyConfigState>,
) -> Result<String, String> {
    let config = hotkey_config
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    Ok(config.shortcut.clone())
}

#[tauri::command]
fn set_hotkey(
    app_handle: tauri::AppHandle,
    shortcut: String,
    hotkey_config: tauri::State<'_, HotkeyConfigState>,
) -> Result<(), String> {
    // Try to register the new shortcut first
    hotkey::register_hotkey(&app_handle, &shortcut)?;

    // If registration succeeded, update the stored config
    let mut config = hotkey_config
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    config.shortcut = shortcut;
    Ok(())
}

#[tauri::command]
fn unregister_hotkey(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    hotkey::unregister_all(&app_handle)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let progress_map: ProgressMap = Arc::new(Mutex::new(HashMap::new()));
    let cancellation_map: CancellationMap = Arc::new(Mutex::new(HashMap::new()));
    let audio_state: AudioState = Arc::new(std::sync::Mutex::new(None));
    let selected_device: SelectedDevice = Arc::new(std::sync::Mutex::new(None));
    let silence_config: SilenceConfigState =
        Arc::new(std::sync::Mutex::new(SilenceConfig::default()));
    let transcription_service: TranscriptionServiceState =
        Arc::new(std::sync::Mutex::new(None));
    let tray_recording_state: TrayRecordingState =
        Arc::new(std::sync::atomic::AtomicU8::new(TrayState::Idle as u8));
    let hotkey_config: HotkeyConfigState =
        std::sync::Mutex::new(HotkeyConfig::default());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(progress_map)
        .manage(cancellation_map)
        .manage(audio_state)
        .manage(selected_device)
        .manage(silence_config)
        .manage(transcription_service)
        .manage(tray_recording_state)
        .manage(hotkey_config)
        .setup(|app| {
            tray::setup_tray(app.handle())?;
            // Register the default global hotkey
            if let Err(e) = hotkey::register_hotkey(app.handle(), "Ctrl+Shift+Space") {
                eprintln!("Failed to register default hotkey: {e}");
            }
            Ok(())
        })
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
            set_silence_config,
            load_transcription_model,
            transcribe_recording,
            list_supported_languages,
            set_tray_state,
            get_hotkey,
            set_hotkey,
            unregister_hotkey,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```
