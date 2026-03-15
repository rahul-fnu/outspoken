use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const HF_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

const SILERO_VAD_URL: &str =
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx";
const SILERO_VAD_FILENAME: &str = "silero_vad.onnx";
const SILERO_VAD_SIZE: u64 = 2_000_000;

/// Cached flag: true once we've confirmed at least one whisper model is downloaded.
static MODELS_CHECKED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub description: String,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedModel {
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub path: String,
    pub version: String,
    pub downloaded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub model_name: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: f64,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    Downloading,
    Completed,
    Cancelled,
    Failed,
}

pub fn available_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "tiny".into(),
            filename: "ggml-tiny.bin".into(),
            size_bytes: 75_000_000,
            description: "Tiny (~75MB) - Fastest, lowest accuracy. CPU: ~1x realtime".into(),
            recommended: false,
        },
        ModelInfo {
            name: "base".into(),
            filename: "ggml-base.bin".into(),
            size_bytes: 142_000_000,
            description: "Base (~142MB) - Fast, good accuracy. CPU: ~2x realtime".into(),
            recommended: false,
        },
        ModelInfo {
            name: "small".into(),
            filename: "ggml-small.bin".into(),
            size_bytes: 466_000_000,
            description: "Small (~466MB) - Balanced speed and accuracy. CPU: ~4x realtime".into(),
            recommended: false,
        },
        ModelInfo {
            name: "medium".into(),
            filename: "ggml-medium.bin".into(),
            size_bytes: 1_500_000_000,
            description: "Medium (~1.5GB) - High accuracy, slow on CPU. CPU: ~10x realtime".into(),
            recommended: false,
        },
        ModelInfo {
            name: "large-v3-turbo-q5_0".into(),
            filename: "ggml-large-v3-turbo-q5_0.bin".into(),
            size_bytes: 400_000_000,
            description: "Large V3 Turbo Q5_0 (~400MB) - Best English model for CPU. Only 4 decoder layers, quantized. CPU: ~3x realtime".into(),
            recommended: true,
        },
        ModelInfo {
            name: "large-v3-turbo".into(),
            filename: "ggml-large-v3-turbo.bin".into(),
            size_bytes: 800_000_000,
            description: "Large V3 Turbo (~800MB) - Full precision turbo model. CPU: ~5x realtime".into(),
            recommended: false,
        },
    ]
}

fn models_dir() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir().ok_or("Could not determine platform data directory")?;
    let dir = data_dir.join("outspoken").join("models");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create models directory: {e}"))?;
    Ok(dir)
}

fn db_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir().ok_or("Could not determine platform data directory")?;
    let dir = data_dir.join("outspoken");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create data directory: {e}"))?;
    Ok(dir.join("models.db"))
}

fn open_db() -> Result<Connection, String> {
    let path = db_path()?;
    let conn =
        Connection::open(&path).map_err(|e| format!("Failed to open database: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS models (
            name TEXT PRIMARY KEY,
            filename TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            path TEXT NOT NULL,
            version TEXT NOT NULL,
            downloaded_at TEXT NOT NULL
        );",
    )
    .map_err(|e| format!("Failed to initialize database: {e}"))?;
    Ok(conn)
}

pub fn list_downloaded_models() -> Result<Vec<DownloadedModel>, String> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare("SELECT name, filename, size_bytes, path, version, downloaded_at FROM models")
        .map_err(|e| format!("Query error: {e}"))?;

    let models = stmt
        .query_map([], |row| {
            Ok(DownloadedModel {
                name: row.get(0)?,
                filename: row.get(1)?,
                size_bytes: row.get(2)?,
                path: row.get(3)?,
                version: row.get(4)?,
                downloaded_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Query error: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Row error: {e}"))?;

    Ok(models)
}

fn save_model_to_db(model: &DownloadedModel) -> Result<(), String> {
    let conn = open_db()?;
    conn.execute(
        "INSERT OR REPLACE INTO models (name, filename, size_bytes, path, version, downloaded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            model.name,
            model.filename,
            model.size_bytes,
            model.path,
            model.version,
            model.downloaded_at,
        ],
    )
    .map_err(|e| format!("Failed to save model: {e}"))?;
    Ok(())
}

fn remove_model_from_db(name: &str) -> Result<(), String> {
    let conn = open_db()?;
    conn.execute("DELETE FROM models WHERE name = ?1", rusqlite::params![name])
        .map_err(|e| format!("Failed to remove model from database: {e}"))?;
    Ok(())
}

pub type ProgressMap = Arc<Mutex<HashMap<String, DownloadProgress>>>;
pub type CancellationMap = Arc<Mutex<HashMap<String, CancellationToken>>>;

pub async fn download_model(
    model_name: String,
    progress_map: ProgressMap,
    cancellation_map: CancellationMap,
) -> Result<DownloadedModel, String> {
    let info = available_models()
        .into_iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Unknown model: {model_name}"))?;

    let dir = models_dir()?;
    let file_path = dir.join(&info.filename);
    let url = format!("{HF_BASE_URL}/{}", info.filename);

    let cancel_token = CancellationToken::new();
    {
        let mut map = cancellation_map.lock().await;
        map.insert(model_name.clone(), cancel_token.clone());
    }

    // Initialize progress
    {
        let mut map = progress_map.lock().await;
        map.insert(
            model_name.clone(),
            DownloadProgress {
                model_name: model_name.clone(),
                downloaded_bytes: 0,
                total_bytes: 0,
                progress_percent: 0.0,
                status: DownloadStatus::Downloading,
            },
        );
    }

    let client = Client::new();
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?;

    if !response.status().is_success() {
        update_progress_status(&progress_map, &model_name, DownloadStatus::Failed).await;
        return Err(format!(
            "Download failed with status: {}",
            response.status()
        ));
    }

    let total_bytes = response.content_length().unwrap_or(info.size_bytes);
    {
        let mut map = progress_map.lock().await;
        if let Some(p) = map.get_mut(&model_name) {
            p.total_bytes = total_bytes;
        }
    }

    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&file_path)
        .await
        .map_err(|e| format!("Failed to create file: {e}"))?;

    let mut downloaded: u64 = 0;

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                drop(file);
                let _ = tokio::fs::remove_file(&file_path).await;
                update_progress_status(&progress_map, &model_name, DownloadStatus::Cancelled).await;
                {
                    let mut map = cancellation_map.lock().await;
                    map.remove(&model_name);
                }
                return Err("Download cancelled".into());
            }
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        use tokio::io::AsyncWriteExt;
                        file.write_all(&bytes)
                            .await
                            .map_err(|e| format!("Failed to write to file: {e}"))?;
                        downloaded += bytes.len() as u64;

                        let percent = if total_bytes > 0 {
                            (downloaded as f64 / total_bytes as f64) * 100.0
                        } else {
                            0.0
                        };

                        let mut map = progress_map.lock().await;
                        if let Some(p) = map.get_mut(&model_name) {
                            p.downloaded_bytes = downloaded;
                            p.progress_percent = percent;
                        }
                    }
                    Some(Err(e)) => {
                        drop(file);
                        let _ = tokio::fs::remove_file(&file_path).await;
                        update_progress_status(&progress_map, &model_name, DownloadStatus::Failed).await;
                        return Err(format!("Download stream error: {e}"));
                    }
                    None => break, // Download complete
                }
            }
        }
    }

    // Get actual file size
    let metadata = tokio::fs::metadata(&file_path)
        .await
        .map_err(|e| format!("Failed to read file metadata: {e}"))?;

    let now: DateTime<Utc> = Utc::now();
    let model = DownloadedModel {
        name: model_name.clone(),
        filename: info.filename,
        size_bytes: metadata.len(),
        path: file_path.to_string_lossy().into_owned(),
        version: "1.0".into(),
        downloaded_at: now.to_rfc3339(),
    };

    save_model_to_db(&model)?;

    update_progress_status(&progress_map, &model_name, DownloadStatus::Completed).await;
    {
        let mut map = cancellation_map.lock().await;
        map.remove(&model_name);
    }

    Ok(model)
}

pub fn delete_model(name: &str) -> Result<(), String> {
    let conn = open_db()?;
    let path: Option<String> = conn
        .query_row(
            "SELECT path FROM models WHERE name = ?1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .ok();

    if let Some(path) = path {
        let p = PathBuf::from(&path);
        if p.exists() {
            std::fs::remove_file(&p)
                .map_err(|e| format!("Failed to delete model file: {e}"))?;
        }
    }

    remove_model_from_db(name)?;
    Ok(())
}

async fn update_progress_status(
    progress_map: &ProgressMap,
    model_name: &str,
    status: DownloadStatus,
) {
    let mut map = progress_map.lock().await;
    if let Some(p) = map.get_mut(model_name) {
        p.status = status;
    }
}

/// Returns the recommended model info.
pub fn recommended_model() -> ModelInfo {
    available_models()
        .into_iter()
        .find(|m| m.recommended)
        .expect("No recommended model in registry")
}

/// Returns the path for a downloaded model, or None if not downloaded.
pub fn get_model_path(model_name: &str) -> Result<Option<PathBuf>, String> {
    let downloaded = list_downloaded_models()?;
    if let Some(m) = downloaded.iter().find(|m| m.name == model_name) {
        let p = PathBuf::from(&m.path);
        if p.exists() {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

/// Returns true if any whisper model is already downloaded.
pub fn has_any_model() -> Result<bool, String> {
    if MODELS_CHECKED.load(Ordering::Relaxed) {
        return Ok(true);
    }
    let downloaded = list_downloaded_models()?;
    let has = !downloaded.is_empty();
    if has {
        MODELS_CHECKED.store(true, Ordering::Relaxed);
    }
    Ok(has)
}

/// Progress callback type: receives (downloaded_bytes, total_bytes).
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send + Sync>;

/// Download a model with a simple progress callback (no cancellation maps needed).
/// Used by the CLI and auto-download logic.
pub async fn download_model_with_callback(
    model_name: &str,
    on_progress: Option<ProgressCallback>,
) -> Result<DownloadedModel, String> {
    let info = available_models()
        .into_iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Unknown model: {model_name}"))?;

    let dir = models_dir()?;
    let file_path = dir.join(&info.filename);

    // If already downloaded, return it
    if let Some(existing) = get_model_path(model_name)? {
        let metadata = std::fs::metadata(&existing)
            .map_err(|e| format!("Failed to read metadata: {e}"))?;
        return Ok(DownloadedModel {
            name: model_name.to_string(),
            filename: info.filename,
            size_bytes: metadata.len(),
            path: existing.to_string_lossy().into_owned(),
            version: "1.0".into(),
            downloaded_at: Utc::now().to_rfc3339(),
        });
    }

    let url = format!("{HF_BASE_URL}/{}", info.filename);
    let client = Client::new();
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    let total_bytes = response.content_length().unwrap_or(info.size_bytes);
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&file_path)
        .await
        .map_err(|e| format!("Failed to create file: {e}"))?;

    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("Download stream error: {e}"))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes)
            .await
            .map_err(|e| format!("Failed to write to file: {e}"))?;
        downloaded += bytes.len() as u64;
        if let Some(ref cb) = on_progress {
            cb(downloaded, total_bytes);
        }
    }

    let metadata = tokio::fs::metadata(&file_path)
        .await
        .map_err(|e| format!("Failed to read file metadata: {e}"))?;

    let now: DateTime<Utc> = Utc::now();
    let model = DownloadedModel {
        name: model_name.to_string(),
        filename: info.filename,
        size_bytes: metadata.len(),
        path: file_path.to_string_lossy().into_owned(),
        version: "1.0".into(),
        downloaded_at: now.to_rfc3339(),
    };

    save_model_to_db(&model)?;
    MODELS_CHECKED.store(true, Ordering::Relaxed);

    Ok(model)
}

/// Ensure a specific model is downloaded, downloading it if missing.
/// Returns the path to the model file.
pub async fn ensure_model(
    model_name: &str,
    on_progress: Option<ProgressCallback>,
) -> Result<PathBuf, String> {
    if let Some(path) = get_model_path(model_name)? {
        return Ok(path);
    }
    let result = download_model_with_callback(model_name, on_progress).await?;
    Ok(PathBuf::from(result.path))
}

/// Ensure the recommended model is downloaded.
pub async fn ensure_recommended_model(
    on_progress: Option<ProgressCallback>,
) -> Result<PathBuf, String> {
    let rec = recommended_model();
    ensure_model(&rec.name, on_progress).await
}

/// Returns the path to the Silero VAD model, downloading if missing.
pub async fn ensure_vad_model(
    on_progress: Option<ProgressCallback>,
) -> Result<PathBuf, String> {
    let dir = models_dir()?;
    let file_path = dir.join(SILERO_VAD_FILENAME);

    if file_path.exists() {
        return Ok(file_path);
    }

    let client = Client::new();
    let response = client
        .get(SILERO_VAD_URL)
        .send()
        .await
        .map_err(|e| format!("VAD download request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "VAD download failed with status: {}",
            response.status()
        ));
    }

    let total_bytes = response.content_length().unwrap_or(SILERO_VAD_SIZE);
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&file_path)
        .await
        .map_err(|e| format!("Failed to create VAD file: {e}"))?;

    let mut downloaded: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("VAD download stream error: {e}"))?;
        use tokio::io::AsyncWriteExt;
        file.write_all(&bytes)
            .await
            .map_err(|e| format!("Failed to write VAD file: {e}"))?;
        downloaded += bytes.len() as u64;
        if let Some(ref cb) = on_progress {
            cb(downloaded, total_bytes);
        }
    }

    Ok(file_path)
}
