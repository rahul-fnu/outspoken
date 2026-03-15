use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub language: String,
    pub auto_start: bool,
    pub hotkey: String,
    pub silence_threshold_db: f32,
    pub silence_duration_secs: f32,
    pub audio_input_device: Option<String>,
    pub active_model: Option<String>,
    pub strip_filler_words: bool,
    pub personal_dictionary: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            auto_start: false,
            hotkey: "Ctrl+Shift+Space".into(),
            silence_threshold_db: -40.0,
            silence_duration_secs: 3.0,
            audio_input_device: None,
            active_model: None,
            strip_filler_words: false,
            personal_dictionary: Vec::new(),
        }
    }
}

pub type SettingsState = Mutex<AppSettings>;

fn settings_db_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir().ok_or("Could not determine platform data directory")?;
    let dir = data_dir.join("outspoken");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create data directory: {e}"))?;
    Ok(dir.join("settings.db"))
}

fn open_settings_db() -> Result<Connection, String> {
    let path = settings_db_path()?;
    let conn = Connection::open(&path)
        .map_err(|e| format!("Failed to open settings database: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
    .map_err(|e| format!("Failed to initialize settings table: {e}"))?;
    Ok(conn)
}

fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        rusqlite::params![key],
        |row| row.get(0),
    )
    .ok()
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )
    .map_err(|e| format!("Failed to save setting: {e}"))?;
    Ok(())
}

pub fn load_settings() -> AppSettings {
    let conn = match open_settings_db() {
        Ok(c) => c,
        Err(_) => return AppSettings::default(),
    };

    let defaults = AppSettings::default();

    AppSettings {
        language: get_setting(&conn, "language").unwrap_or(defaults.language),
        auto_start: get_setting(&conn, "auto_start")
            .map(|v| v == "true")
            .unwrap_or(defaults.auto_start),
        hotkey: get_setting(&conn, "hotkey").unwrap_or(defaults.hotkey),
        silence_threshold_db: get_setting(&conn, "silence_threshold_db")
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.silence_threshold_db),
        silence_duration_secs: get_setting(&conn, "silence_duration_secs")
            .and_then(|v| v.parse().ok())
            .unwrap_or(defaults.silence_duration_secs),
        audio_input_device: get_setting(&conn, "audio_input_device"),
        active_model: get_setting(&conn, "active_model"),
        strip_filler_words: get_setting(&conn, "strip_filler_words")
            .map(|v| v == "true")
            .unwrap_or(defaults.strip_filler_words),
        personal_dictionary: get_setting(&conn, "personal_dictionary")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default(),
        openai_api_key: get_setting(&conn, "openai_api_key").unwrap_or_default(),
        anthropic_api_key: get_setting(&conn, "anthropic_api_key").unwrap_or_default(),
    }
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let conn = open_settings_db()?;
    set_setting(&conn, "language", &settings.language)?;
    set_setting(&conn, "auto_start", &settings.auto_start.to_string())?;
    set_setting(&conn, "hotkey", &settings.hotkey)?;
    set_setting(&conn, "silence_threshold_db", &settings.silence_threshold_db.to_string())?;
    set_setting(&conn, "silence_duration_secs", &settings.silence_duration_secs.to_string())?;
    set_setting(
        &conn,
        "audio_input_device",
        settings.audio_input_device.as_deref().unwrap_or(""),
    )?;
    set_setting(
        &conn,
        "active_model",
        settings.active_model.as_deref().unwrap_or(""),
    )?;
    set_setting(&conn, "strip_filler_words", &settings.strip_filler_words.to_string())?;
    let dict_json = serde_json::to_string(&settings.personal_dictionary)
        .map_err(|e| format!("Failed to serialize dictionary: {e}"))?;
    set_setting(&conn, "personal_dictionary", &dict_json)?;
    set_setting(&conn, "openai_api_key", &settings.openai_api_key)?;
    set_setting(&conn, "anthropic_api_key", &settings.anthropic_api_key)?;
    Ok(())
}
