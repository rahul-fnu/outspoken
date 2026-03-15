use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use futures_util::StreamExt;
use reqwest::Client;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::db;

// ---------------------------------------------------------------------------
// Encryption helpers
// ---------------------------------------------------------------------------

fn derive_encryption_key() -> [u8; 32] {
    let mut seed = String::from("outspoken-key-encryption");
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        seed.push_str(&hostname);
    }
    if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
        seed.push_str(&user);
    }
    // Add machine-id on Linux if available
    if let Ok(id) = std::fs::read_to_string("/etc/machine-id") {
        seed.push_str(id.trim());
    }
    let hash = Sha256::digest(seed.as_bytes());
    hash.into()
}

fn encrypt_string(plaintext: &str) -> Result<String, String> {
    let key = derive_encryption_key();
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init error: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("Encryption error: {e}"))?;
    // Store as base64: nonce + ciphertext
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(B64.encode(combined))
}

fn decrypt_string(encoded: &str) -> Result<String, String> {
    let key = derive_encryption_key();
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init error: {e}"))?;
    let combined = B64
        .decode(encoded)
        .map_err(|e| format!("Base64 decode error: {e}"))?;
    if combined.len() < 12 {
        return Err("Invalid encrypted data".into());
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed — key may have changed".to_string())?;
    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 error: {e}"))
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

fn open_ai_db() -> Result<rusqlite::Connection, String> {
    db::open_db()
}

// ---------------------------------------------------------------------------
// Key management commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub provider: String,
    pub is_set: bool,
    pub masked: String,
}

pub fn save_api_key(provider: &str, key: &str) -> Result<(), String> {
    if provider != "openai" && provider != "anthropic" {
        return Err(format!("Unknown provider: {provider}"));
    }
    let encrypted = encrypt_string(key)?;
    let conn = open_ai_db()?;
    conn.execute(
        "INSERT OR REPLACE INTO api_keys (provider, encrypted_key) VALUES (?1, ?2)",
        params![provider, encrypted],
    )
    .map_err(|e| format!("Failed to save API key: {e}"))?;
    Ok(())
}

pub fn delete_api_key(provider: &str) -> Result<(), String> {
    let conn = open_ai_db()?;
    conn.execute("DELETE FROM api_keys WHERE provider = ?1", params![provider])
        .map_err(|e| format!("Failed to delete API key: {e}"))?;
    Ok(())
}

pub fn get_api_key(provider: &str) -> Result<Option<String>, String> {
    let conn = open_ai_db()?;
    let result: Option<String> = conn
        .query_row(
            "SELECT encrypted_key FROM api_keys WHERE provider = ?1",
            params![provider],
            |row| row.get(0),
        )
        .ok();
    match result {
        Some(encrypted) => Ok(Some(decrypt_string(&encrypted)?)),
        None => Ok(None),
    }
}

pub fn list_api_keys() -> Result<Vec<ApiKeyInfo>, String> {
    let providers = ["openai", "anthropic"];
    let mut results = Vec::new();
    for p in &providers {
        let key = get_api_key(p)?;
        let (is_set, masked) = match &key {
            Some(k) if k.len() > 8 => {
                let masked = format!("{}...{}", &k[..4], &k[k.len() - 4..]);
                (true, masked)
            }
            Some(_) => (true, "****".to_string()),
            None => (false, String::new()),
        };
        results.push(ApiKeyInfo {
            provider: p.to_string(),
            is_set,
            masked,
        });
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Key validation
// ---------------------------------------------------------------------------

pub async fn validate_api_key(provider: &str, key: &str) -> Result<bool, String> {
    let client = Client::new();
    match provider {
        "openai" => {
            let resp = client
                .get("https://api.openai.com/v1/models")
                .header("Authorization", format!("Bearer {key}"))
                .send()
                .await
                .map_err(|e| format!("Network error: {e}"))?;
            Ok(resp.status().is_success())
        }
        "anthropic" => {
            let resp = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .body(r#"{"model":"claude-haiku-4-5-20251001","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#)
                .send()
                .await
                .map_err(|e| format!("Network error: {e}"))?;
            // 200 = valid, 401 = invalid key, other codes may indicate valid key but other issue
            Ok(resp.status().as_u16() != 401)
        }
        _ => Err(format!("Unknown provider: {provider}")),
    }
}

// ---------------------------------------------------------------------------
// AI Provider trait & implementations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRequest {
    pub text: String,
    pub prompt: String,
    pub provider: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChunkEvent {
    pub chunk: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResult {
    pub text: String,
    pub provider: String,
    pub model: String,
}

pub async fn process_ai_text(
    app_handle: AppHandle,
    request: AiRequest,
) -> Result<AiResult, String> {
    let api_key = get_api_key(&request.provider)?
        .ok_or_else(|| format!("No API key configured for {}", request.provider))?;

    match request.provider.as_str() {
        "openai" => process_openai(app_handle, &api_key, &request).await,
        "anthropic" => process_anthropic(app_handle, &api_key, &request).await,
        _ => Err(format!("Unknown provider: {}", request.provider)),
    }
}

async fn process_openai(
    app_handle: AppHandle,
    api_key: &str,
    request: &AiRequest,
) -> Result<AiResult, String> {
    let model = request
        .model
        .as_deref()
        .unwrap_or("gpt-4o-mini");
    let client = Client::new();

    let body = serde_json::json!({
        "model": model,
        "stream": true,
        "messages": [
            {"role": "system", "content": request.prompt},
            {"role": "user", "content": request.text}
        ]
    });

    let resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let status = resp.status();
    if status.as_u16() == 429 {
        return Err("Rate limited — please try again in a few seconds".into());
    }
    if status.as_u16() == 401 {
        return Err("Invalid API key — please re-enter your OpenAI key".into());
    }
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("OpenAI API error ({status}): {text}"));
    }

    // Stream SSE response incrementally
    let mut full_text = String::new();
    let mut line_buf = String::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream read error: {e}"))?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        line_buf.push_str(&chunk_str);

        // Process complete lines from buffer
        while let Some(newline_pos) = line_buf.find('\n') {
            let line = line_buf[..newline_pos].trim_end_matches('\r').to_string();
            line_buf = line_buf[newline_pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if data.trim() == "[DONE]" {
                    break;
                }
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                        full_text.push_str(content);
                        let _ = app_handle.emit(
                            "ai-chunk",
                            AiChunkEvent {
                                chunk: content.to_string(),
                                done: false,
                            },
                        );
                    }
                }
            }
        }
    }

    let _ = app_handle.emit(
        "ai-chunk",
        AiChunkEvent {
            chunk: String::new(),
            done: true,
        },
    );

    Ok(AiResult {
        text: full_text,
        provider: "openai".to_string(),
        model: model.to_string(),
    })
}

async fn process_anthropic(
    app_handle: AppHandle,
    api_key: &str,
    request: &AiRequest,
) -> Result<AiResult, String> {
    let model = request
        .model
        .as_deref()
        .unwrap_or("claude-haiku-4-5-20251001");
    let client = Client::new();

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "stream": true,
        "system": request.prompt,
        "messages": [
            {"role": "user", "content": request.text}
        ]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let status = resp.status();
    if status.as_u16() == 429 {
        return Err("Rate limited — please try again in a few seconds".into());
    }
    if status.as_u16() == 401 {
        return Err("Invalid API key — please re-enter your Anthropic key".into());
    }
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Anthropic API error ({status}): {text}"));
    }

    // Stream SSE response incrementally
    let mut full_text = String::new();
    let mut line_buf = String::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream read error: {e}"))?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        line_buf.push_str(&chunk_str);

        // Process complete lines from buffer
        while let Some(newline_pos) = line_buf.find('\n') {
            let line = line_buf[..newline_pos].trim_end_matches('\r').to_string();
            line_buf = line_buf[newline_pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                    let event_type = parsed["type"].as_str().unwrap_or("");
                    match event_type {
                        "content_block_delta" => {
                            if let Some(content) = parsed["delta"]["text"].as_str() {
                                full_text.push_str(content);
                                let _ = app_handle.emit(
                                    "ai-chunk",
                                    AiChunkEvent {
                                        chunk: content.to_string(),
                                        done: false,
                                    },
                                );
                            }
                        }
                        "message_stop" => break,
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = app_handle.emit(
        "ai-chunk",
        AiChunkEvent {
            chunk: String::new(),
            done: true,
        },
    );

    Ok(AiResult {
        text: full_text,
        provider: "anthropic".to_string(),
        model: model.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Prompt management
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiPrompt {
    pub id: i64,
    pub name: String,
    pub prompt: String,
    pub is_default: bool,
    pub app_pattern: Option<String>,
}

fn default_prompts() -> Vec<(String, String)> {
    vec![
        (
            "Clean up grammar".to_string(),
            "Clean up the grammar, spelling, and punctuation of the following text. Keep the same meaning and tone. Return only the corrected text.".to_string(),
        ),
        (
            "Make professional".to_string(),
            "Rewrite the following text in a professional, polished tone suitable for business communication. Keep the same meaning. Return only the rewritten text.".to_string(),
        ),
        (
            "Summarize".to_string(),
            "Summarize the following text concisely, capturing the key points. Return only the summary.".to_string(),
        ),
    ]
}

pub fn list_prompts() -> Result<Vec<AiPrompt>, String> {
    let mut prompts: Vec<AiPrompt> = default_prompts()
        .into_iter()
        .enumerate()
        .map(|(i, (name, prompt))| AiPrompt {
            id: -(i as i64 + 1), // negative IDs for defaults
            name,
            prompt,
            is_default: true,
            app_pattern: None,
        })
        .collect();

    let conn = open_ai_db()?;
    let mut stmt = conn
        .prepare("SELECT id, name, prompt, app_pattern FROM custom_prompts")
        .map_err(|e| format!("Query error: {e}"))?;
    let custom = stmt
        .query_map([], |row| {
            Ok(AiPrompt {
                id: row.get(0)?,
                name: row.get(1)?,
                prompt: row.get(2)?,
                is_default: false,
                app_pattern: row.get(3)?,
            })
        })
        .map_err(|e| format!("Query error: {e}"))?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

    prompts.extend(custom);
    Ok(prompts)
}

pub fn save_custom_prompt(
    name: &str,
    prompt: &str,
    app_pattern: Option<&str>,
) -> Result<AiPrompt, String> {
    let conn = open_ai_db()?;
    conn.execute(
        "INSERT INTO custom_prompts (name, prompt, app_pattern) VALUES (?1, ?2, ?3)",
        params![name, prompt, app_pattern],
    )
    .map_err(|e| format!("Failed to save prompt: {e}"))?;
    let id = conn.last_insert_rowid();
    Ok(AiPrompt {
        id,
        name: name.to_string(),
        prompt: prompt.to_string(),
        is_default: false,
        app_pattern: app_pattern.map(String::from),
    })
}

pub fn delete_custom_prompt(id: i64) -> Result<(), String> {
    if id < 0 {
        return Err("Cannot delete default prompts".into());
    }
    let conn = open_ai_db()?;
    conn.execute("DELETE FROM custom_prompts WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete prompt: {e}"))?;
    Ok(())
}
