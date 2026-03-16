#[cfg(feature = "desktop")]
mod active_app;
pub mod ai;
pub mod audio;
#[cfg(feature = "desktop")]
mod audio_level;
pub mod audio_preprocess;
pub mod db;
pub mod history;
#[cfg(feature = "desktop")]
mod hotkey;
pub mod mcp;
pub mod models;
pub mod settings;
#[cfg(feature = "desktop")]
mod text_insert;
pub mod text_processing;
pub mod transcription;
#[cfg(feature = "desktop")]
mod tray;
pub mod vad;

#[cfg(feature = "desktop")]
use std::collections::HashMap;
#[cfg(feature = "desktop")]
use std::path::PathBuf;
#[cfg(feature = "desktop")]
use std::sync::Arc;

#[cfg(feature = "desktop")]
use audio_level::{AudioLevelMonitor, SilenceConfig};
#[cfg(feature = "desktop")]
use hotkey::{HotkeyConfig, HotkeyConfigState};
#[cfg(feature = "desktop")]
use models::{CancellationMap, ProgressMap};
#[cfg(feature = "desktop")]
use settings::{AppSettings, SettingsState};
#[cfg(feature = "desktop")]
use tokio::sync::Mutex;
#[cfg(feature = "desktop")]
use transcription::{SupportedLanguage, TranscriptionConfig, TranscriptionResult, TranscriptionService};
#[cfg(feature = "desktop")]
use tray::{TrayRecordingState, TrayState};

#[cfg(feature = "desktop")]
type AudioState = Arc<std::sync::Mutex<Option<audio::RecordingState>>>;
#[cfg(feature = "desktop")]
type SelectedDevice = Arc<std::sync::Mutex<Option<String>>>;
#[cfg(feature = "desktop")]
type SilenceConfigState = Arc<std::sync::Mutex<SilenceConfig>>;
#[cfg(feature = "desktop")]
type TranscriptionServiceState = Arc<std::sync::Mutex<Option<TranscriptionService>>>;

#[cfg(feature = "desktop")]
#[tauri::command]
fn get_active_app() -> Result<active_app::ActiveAppInfo, String> {
    active_app::get_active_app()
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn list_available_models() -> Vec<models::ModelInfo> {
    models::available_models()
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn list_models() -> Result<Vec<models::DownloadedModel>, String> {
    models::list_downloaded_models()
}

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
#[tauri::command]
fn delete_model(name: String) -> Result<(), String> {
    models::delete_model(&name)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn list_audio_devices() -> Result<Vec<audio::AudioDeviceInfo>, String> {
    audio::list_devices()
}

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
#[tauri::command]
fn list_supported_languages() -> Vec<SupportedLanguage> {
    transcription::supported_languages()
}

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
#[tauri::command]
async fn transcribe_streaming_chunk(
    audio_state: tauri::State<'_, AudioState>,
    service_state: tauri::State<'_, TranscriptionServiceState>,
) -> Result<TranscriptionResult, String> {
    // Snapshot the current recording buffer without stopping the recording.
    let audio_data = {
        let state = audio_state
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        let recording = state
            .as_ref()
            .ok_or("Not currently recording")?;
        recording
            .buffer
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?
            .clone()
    };

    if audio_data.is_empty() {
        return Ok(TranscriptionResult {
            text: String::new(),
            segments: Vec::new(),
            language: String::new(),
            duration_ms: 0,
        });
    }

    let service = {
        let state = service_state
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        state
            .as_ref()
            .ok_or("No transcription model loaded.")?
            .clone()
    };

    let result = tokio::task::spawn_blocking(move || service.transcribe(&audio_data))
        .await
        .map_err(|e| format!("Task join error: {e}"))??;

    Ok(result)
}

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
#[tauri::command]
async fn insert_text(text: String) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    tokio::task::spawn_blocking(move || text_insert::insert_text(&text))
        .await
        .map_err(|e| format!("Task join error: {e}"))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn get_hotkey(
    hotkey_config: tauri::State<'_, HotkeyConfigState>,
) -> Result<String, String> {
    let config = hotkey_config
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    Ok(config.shortcut.clone())
}

#[cfg(feature = "desktop")]
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

#[cfg(feature = "desktop")]
#[tauri::command]
fn unregister_hotkey(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    hotkey::unregister_all(&app_handle)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn get_settings(
    settings_state: tauri::State<'_, SettingsState>,
) -> Result<AppSettings, String> {
    let settings = settings_state
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    Ok(settings.clone())
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn update_settings(
    new_settings: AppSettings,
    settings_state: tauri::State<'_, SettingsState>,
) -> Result<(), String> {
    settings::save_settings(&new_settings)?;
    let mut settings = settings_state
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    *settings = new_settings;
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn save_transcription(
    result: history::SaveTranscriptionInput,
) -> Result<history::Transcription, String> {
    history::save_transcription(&result)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn search_history(
    query: Option<String>,
    filters: Option<history::SearchFilters>,
) -> Result<Vec<history::Transcription>, String> {
    let mut f = filters.unwrap_or(history::SearchFilters {
        query: None,
        source_app: None,
        date_from: None,
        date_to: None,
        bookmarked_only: None,
        limit: None,
        offset: None,
    });
    if f.query.is_none() {
        f.query = query;
    }
    history::search_history(&f)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn delete_transcription(id: String) -> Result<(), String> {
    history::delete_transcription(&id)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn clear_history() -> Result<(), String> {
    history::clear_history()
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn toggle_bookmark(id: String) -> Result<bool, String> {
    history::toggle_bookmark(&id)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn get_history_stats() -> Result<history::HistoryStats, String> {
    history::get_stats()
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn export_history(filters: Option<history::SearchFilters>, format: String) -> Result<String, String> {
    let f = filters.unwrap_or(history::SearchFilters {
        query: None,
        source_app: None,
        date_from: None,
        date_to: None,
        bookmarked_only: None,
        limit: None,
        offset: None,
    });
    history::export_history(&f, &format)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn add_dictionary_entry(
    from_text: String,
    to_text: String,
    case_sensitive: bool,
) -> Result<text_processing::DictionaryEntry, String> {
    text_processing::add_entry(&from_text, &to_text, case_sensitive)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn remove_dictionary_entry(id: i64) -> Result<(), String> {
    text_processing::remove_entry(id)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn list_dictionary() -> Result<Vec<text_processing::DictionaryEntry>, String> {
    text_processing::list_entries()
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn process_transcription_text(
    text: String,
    strip_corrections: bool,
    strip_fillers: bool,
) -> Result<String, String> {
    let entries = text_processing::list_entries()?;
    Ok(text_processing::process_text(&text, strip_corrections, strip_fillers, &entries))
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn save_api_key(provider: String, key: String) -> Result<(), String> {
    // Validate key before saving
    let valid = ai::validate_api_key(&provider, &key).await?;
    if !valid {
        return Err(format!("Invalid API key for {provider}"));
    }
    ai::save_api_key(&provider, &key)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn delete_api_key(provider: String) -> Result<(), String> {
    ai::delete_api_key(&provider)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn validate_api_key(provider: String, key: String) -> Result<bool, String> {
    ai::validate_api_key(&provider, &key).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn list_api_keys() -> Result<Vec<ai::ApiKeyInfo>, String> {
    ai::list_api_keys()
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn process_ai_text(
    app_handle: tauri::AppHandle,
    request: ai::AiRequest,
) -> Result<ai::AiResult, String> {
    ai::process_ai_text(app_handle, request).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn list_ai_prompts() -> Result<Vec<ai::AiPrompt>, String> {
    ai::list_prompts()
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn save_custom_prompt(
    name: String,
    prompt: String,
    app_pattern: Option<String>,
) -> Result<ai::AiPrompt, String> {
    ai::save_custom_prompt(&name, &prompt, app_pattern.as_deref())
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn delete_custom_prompt(id: i64) -> Result<(), String> {
    ai::delete_custom_prompt(id)
}

#[cfg(feature = "desktop")]
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
    let app_settings: SettingsState =
        std::sync::Mutex::new(settings::load_settings());

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
        .manage(app_settings)
        .setup(|app| {
            tray::setup_tray(app.handle())?;
            // Register the default global hotkey
            if let Err(e) = hotkey::register_hotkey(app.handle(), "Ctrl+Shift+Space") {
                eprintln!("Failed to register default hotkey: {e}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_active_app,
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
            transcribe_streaming_chunk,
            list_supported_languages,
            set_tray_state,
            insert_text,
            get_hotkey,
            set_hotkey,
            unregister_hotkey,
            get_settings,
            update_settings,
            save_transcription,
            search_history,
            delete_transcription,
            clear_history,
            toggle_bookmark,
            get_history_stats,
            export_history,
            add_dictionary_entry,
            remove_dictionary_entry,
            list_dictionary,
            process_transcription_text,
            save_api_key,
            delete_api_key,
            validate_api_key,
            list_api_keys,
            process_ai_text,
            list_ai_prompts,
            save_custom_prompt,
            delete_custom_prompt,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
