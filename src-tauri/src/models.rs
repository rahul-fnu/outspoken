use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::Client;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const HF_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub name: String,
    pub filename: String,
    pub size_bytes: u64,
    pub description: String,
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
    pub is_complete: bool,
    pub error: Option<String>,
}

/// Available Whisper GGML models
pub fn available_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            name: "tiny".into(),
            filename: "ggml-tiny.bin".into(),
            size_bytes: 75_000_000,
            description: "Tiny model (~75MB) - Fastest, least accurate".into(),
        },
        ModelInfo {
            name: "base".into(),
            filename: "ggml-base.bin".into(),
            size_bytes: 142_000_000,
            description: "Base model (~142MB) - Good balance of speed and accuracy".into(),
        },
        ModelInfo {
            name: "small".into(),
            filename: "ggml-small.bin".into(),
            size_bytes: 466_000_000,
            description: "Small model (~466MB) - Better accuracy, slower".into(),
        },
        ModelInfo {
            name: "medium".into(),
            filename: "ggml-medium.bin".into(),
            size_bytes: 1_500_000_000,
            description: "Medium model (~1.5GB) - Best accuracy, slowest".into(),
        },
    ]
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("Model not found: {0}")]
    NotFound(String),
    #[error("Download cancelled")]
    Cancelled,
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Download already in progress: {0}")]
    AlreadyDownloading(String),
}

impl Serialize for ModelError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub struct ModelManager {
    db: Mutex<Connection>,
    models_dir: PathBuf,
    http_client: Client,
    active_downloads: Mutex<HashMap<String, DownloadProgress>>,
    cancellation_tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl ModelManager {
    pub fn new() -> Result<Arc<Self>, ModelError> {
        let models_dir = Self::models_directory();
        std::fs::create_dir_all(&models_dir)?;

        let db_path = models_dir.join("models.db");
        let conn = Connection::open(db_path)?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS models (
                name TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                path TEXT NOT NULL,
                version TEXT NOT NULL,
                downloaded_at TEXT NOT NULL
            )",
        )?;

        Ok(Arc::new(Self {
            db: Mutex::new(conn),
            models_dir,
            http_client: Client::new(),
            active_downloads: Mutex::new(HashMap::new()),
            cancellation_tokens: Mutex::new(HashMap::new()),
        }))
    }

    fn models_directory() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("outspoken")
            .join("models")
    }

    pub async fn list_downloaded(&self) -> Result<Vec<DownloadedModel>, ModelError> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare(
            "SELECT name, filename, size_bytes, path, version, downloaded_at FROM models",
        )?;
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
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(models)
    }

    pub async fn download_model(
        self: &Arc<Self>,
        model_name: &str,
    ) -> Result<(), ModelError> {
        let model_info = available_models()
            .into_iter()
            .find(|m| m.name == model_name)
            .ok_or_else(|| ModelError::NotFound(model_name.to_string()))?;

        // Check if already downloading
        {
            let downloads = self.active_downloads.lock().await;
            if downloads.contains_key(model_name) {
                return Err(ModelError::AlreadyDownloading(model_name.to_string()));
            }
        }

        let cancel_token = CancellationToken::new();
        {
            let mut tokens = self.cancellation_tokens.lock().await;
            tokens.insert(model_name.to_string(), cancel_token.clone());
        }
        {
            let mut downloads = self.active_downloads.lock().await;
            downloads.insert(
                model_name.to_string(),
                DownloadProgress {
                    model_name: model_name.to_string(),
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    progress_percent: 0.0,
                    is_complete: false,
                    error: None,
                },
            );
        }

        let url = format!("{}/{}", HF_BASE_URL, model_info.filename);
        let file_path = self.models_dir.join(&model_info.filename);
        let model_name_owned = model_name.to_string();
        let manager = Arc::clone(self);

        tokio::spawn(async move {
            let result = manager
                .do_download(&url, &file_path, &model_name_owned, &cancel_token)
                .await;

            match result {
                Ok(()) => {
                    // Record in database
                    let now: DateTime<Utc> = Utc::now();
                    let path_str = file_path.to_string_lossy().to_string();
                    let file_size = tokio::fs::metadata(&file_path)
                        .await
                        .map(|m| m.len())
                        .unwrap_or(0);

                    if let Ok(db) = manager.db.lock().await.execute(
                        "INSERT OR REPLACE INTO models (name, filename, size_bytes, path, version, downloaded_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![
                            model_name_owned,
                            model_info.filename,
                            file_size as i64,
                            path_str,
                            "1.0",
                            now.to_rfc3339(),
                        ],
                    ) {
                        let _ = db;
                    }

                    let mut downloads = manager.active_downloads.lock().await;
                    if let Some(progress) = downloads.get_mut(&model_name_owned) {
                        progress.is_complete = true;
                        progress.progress_percent = 100.0;
                    }
                }
                Err(e) => {
                    // Clean up partial file
                    let _ = tokio::fs::remove_file(&file_path).await;
                    let mut downloads = manager.active_downloads.lock().await;
                    if let Some(progress) = downloads.get_mut(&model_name_owned) {
                        progress.error = Some(e.to_string());
                    }
                }
            }

            // Clean up cancellation token
            let mut tokens = manager.cancellation_tokens.lock().await;
            tokens.remove(&model_name_owned);
        });

        Ok(())
    }

    async fn do_download(
        &self,
        url: &str,
        file_path: &PathBuf,
        model_name: &str,
        cancel_token: &CancellationToken,
    ) -> Result<(), ModelError> {
        let response = self.http_client.get(url).send().await?.error_for_status()?;
        let total_size = response.content_length().unwrap_or(0);

        {
            let mut downloads = self.active_downloads.lock().await;
            if let Some(progress) = downloads.get_mut(model_name) {
                progress.total_bytes = total_size;
            }
        }

        let mut file = tokio::fs::File::create(file_path).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded: u64 = 0;

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    drop(file);
                    let _ = tokio::fs::remove_file(file_path).await;
                    return Err(ModelError::Cancelled);
                }
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            file.write_all(&bytes).await?;
                            downloaded += bytes.len() as u64;

                            let percent = if total_size > 0 {
                                (downloaded as f64 / total_size as f64) * 100.0
                            } else {
                                0.0
                            };

                            let mut downloads = self.active_downloads.lock().await;
                            if let Some(progress) = downloads.get_mut(model_name) {
                                progress.downloaded_bytes = downloaded;
                                progress.progress_percent = percent;
                            }
                        }
                        Some(Err(e)) => return Err(ModelError::Network(e)),
                        None => break,
                    }
                }
            }
        }

        file.flush().await?;
        Ok(())
    }

    pub async fn cancel_download(&self, model_name: &str) -> Result<(), ModelError> {
        let tokens = self.cancellation_tokens.lock().await;
        if let Some(token) = tokens.get(model_name) {
            token.cancel();
            Ok(())
        } else {
            Err(ModelError::NotFound(format!(
                "No active download for {}",
                model_name
            )))
        }
    }

    pub async fn delete_model(&self, model_name: &str) -> Result<(), ModelError> {
        let path = {
            let db = self.db.lock().await;
            let path: Option<String> = db
                .query_row(
                    "SELECT path FROM models WHERE name = ?1",
                    [model_name],
                    |row| row.get(0),
                )
                .ok();
            path
        };

        if let Some(path) = path {
            let _ = tokio::fs::remove_file(&path).await;
        }

        let db = self.db.lock().await;
        db.execute("DELETE FROM models WHERE name = ?1", [model_name])?;
        Ok(())
    }

    pub async fn get_download_progress(
        &self,
        model_name: &str,
    ) -> Option<DownloadProgress> {
        let downloads = self.active_downloads.lock().await;
        downloads.get(model_name).cloned()
    }

    pub async fn get_all_progress(&self) -> Vec<DownloadProgress> {
        let downloads = self.active_downloads.lock().await;
        downloads.values().cloned().collect()
    }
}
