use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::audio;
use crate::models;
use crate::transcription::{TranscriptionConfig, TranscriptionService};
use crate::vad::VadSegmenter;

#[derive(Debug, Clone, PartialEq)]
enum McpState {
    Idle,
    Recording,
    Transcribing,
}

impl McpState {
    fn as_str(&self) -> &'static str {
        match self {
            McpState::Idle => "idle",
            McpState::Recording => "recording",
            McpState::Transcribing => "transcribing",
        }
    }
}

struct McpServer {
    state: Arc<Mutex<McpState>>,
    transcription_service: Arc<Mutex<Option<TranscriptionService>>>,
    loaded_model_name: Arc<Mutex<Option<String>>>,
}

impl McpServer {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(McpState::Idle)),
            transcription_service: Arc::new(Mutex::new(None)),
            loaded_model_name: Arc::new(Mutex::new(None)),
        }
    }

    fn ensure_model_loaded(&self) -> Result<(), String> {
        let svc = self.transcription_service.lock().map_err(|e| format!("Lock error: {e}"))?;
        if svc.is_some() {
            return Ok(());
        }
        drop(svc);

        let default_model = "large-v3-turbo-q5_0";
        let downloaded = models::list_downloaded_models()?;

        let model_path = if let Some(m) = downloaded.first() {
            let path = PathBuf::from(&m.path);
            if path.exists() {
                path
            } else {
                return Err(format!("Model file not found: {}", m.path));
            }
        } else {
            eprintln!("No models found, auto-downloading '{default_model}'...");
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {e}"))?;
            let progress_map: models::ProgressMap =
                Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
            let cancellation_map: models::CancellationMap =
                Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
            let model = rt.block_on(models::download_model(
                default_model.to_string(),
                progress_map,
                cancellation_map,
            ))?;
            eprintln!("Download complete.");
            PathBuf::from(model.path)
        };

        let model_name = model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(default_model)
            .to_string();

        let config = TranscriptionConfig::default();
        let service = TranscriptionService::new(&model_path, config)?;

        let mut svc = self.transcription_service.lock().map_err(|e| format!("Lock error: {e}"))?;
        *svc = Some(service);
        let mut name = self.loaded_model_name.lock().map_err(|e| format!("Lock error: {e}"))?;
        *name = Some(model_name);
        Ok(())
    }

    fn record_audio(&self, timeout_secs: f64) -> Result<Vec<f32>, String> {
        {
            let mut state = self.state.lock().map_err(|e| format!("Lock error: {e}"))?;
            *state = McpState::Recording;
        }

        let recording = audio::start_capture(&None, None)?;

        let silence_threshold: f32 = 0.01;
        let silence_timeout: f32 = 2.0;
        let silence_samples = (silence_timeout * 16_000.0) as usize;
        let start = Instant::now();
        let mut had_speech = false;
        let mut silent_count: usize = 0;

        loop {
            if start.elapsed().as_secs_f64() >= timeout_secs {
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(100));

            let buf = recording
                .buffer
                .lock()
                .map_err(|e| format!("Lock error: {e}"))?;
            let len = buf.len();

            if len == 0 {
                continue;
            }

            let check_len = 1600.min(len);
            let tail = &buf[len - check_len..];
            let rms: f32 = (tail.iter().map(|s| s * s).sum::<f32>() / check_len as f32).sqrt();

            if rms > silence_threshold {
                had_speech = true;
                silent_count = 0;
            } else if had_speech {
                silent_count += 1600;
                if silent_count >= silence_samples {
                    break;
                }
            }

            drop(buf);
        }

        recording.is_recording.store(false, Ordering::Relaxed);
        let buffer = recording
            .buffer
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?
            .clone();

        {
            let mut state = self.state.lock().map_err(|e| format!("Lock error: {e}"))?;
            *state = McpState::Idle;
        }

        Ok(buffer)
    }

    fn transcribe(&self, audio_data: &[f32]) -> Result<(String, u64), String> {
        {
            let mut state = self.state.lock().map_err(|e| format!("Lock error: {e}"))?;
            *state = McpState::Transcribing;
        }

        let svc = self.transcription_service.lock().map_err(|e| format!("Lock error: {e}"))?;
        let service = svc.as_ref().ok_or("No model loaded")?;
        let mut vad = VadSegmenter::new()?;
        let result = service.transcribe_with_vad(audio_data, &mut vad);

        {
            let mut state = self.state.lock().map_err(|e| format!("Lock error: {e}"))?;
            *state = McpState::Idle;
        }

        let result = result?;
        Ok((result.text, result.duration_ms))
    }

    fn handle_dictate(&self, params: &Value) -> Result<Value, String> {
        let timeout_secs = params
            .get("timeout_secs")
            .and_then(|v| v.as_f64())
            .unwrap_or(30.0);

        self.ensure_model_loaded()?;
        let audio = self.record_audio(timeout_secs)?;
        let (text, duration_ms) = self.transcribe(&audio)?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({
                    "text": text,
                    "duration_ms": duration_ms
                })).unwrap()
            }]
        }))
    }

    fn handle_listen(&self, params: &Value) -> Result<Value, String> {
        let duration_secs = params
            .get("duration_secs")
            .and_then(|v| v.as_f64())
            .unwrap_or(60.0);

        self.ensure_model_loaded()?;

        let segment_duration = 5.0_f64;
        let start = Instant::now();
        let mut utterances: Vec<Value> = Vec::new();

        while start.elapsed().as_secs_f64() < duration_secs {
            let remaining = duration_secs - start.elapsed().as_secs_f64();
            if remaining <= 0.0 {
                break;
            }
            let record_time = remaining.min(segment_duration);
            let timestamp_ms = start.elapsed().as_millis() as u64;

            match self.record_audio(record_time) {
                Ok(audio) => {
                    if audio.is_empty() {
                        continue;
                    }
                    match self.transcribe(&audio) {
                        Ok((text, _)) => {
                            if !text.is_empty() {
                                utterances.push(json!({
                                    "text": text,
                                    "timestamp_ms": timestamp_ms
                                }));
                            }
                        }
                        Err(e) => {
                            eprintln!("Transcription error during listen: {e}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Recording error during listen: {e}");
                    break;
                }
            }
        }

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({
                    "utterances": utterances
                })).unwrap()
            }]
        }))
    }

    fn handle_get_status(&self) -> Result<Value, String> {
        let state = self.state.lock().map_err(|e| format!("Lock error: {e}"))?;
        let model_name = self.loaded_model_name.lock().map_err(|e| format!("Lock error: {e}"))?;

        let downloaded = models::list_downloaded_models().unwrap_or_default();
        let models_info: Vec<Value> = downloaded
            .iter()
            .map(|m| json!({ "name": m.name, "size_bytes": m.size_bytes }))
            .collect();

        Ok(json!({
            "content": [{
                "type": "text",
                "text": serde_json::to_string(&json!({
                    "state": state.as_str(),
                    "loaded_model": *model_name,
                    "available_models": models_info
                })).unwrap()
            }]
        }))
    }

    fn handle_request(&self, request: &Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        // Notifications (no id) don't get a response
        if id.is_null() && (method == "notifications/initialized" || method == "notifications/cancelled") {
            return Value::Null;
        }

        match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "outspoken",
                        "version": "0.1.0"
                    }
                }
            }),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [
                        {
                            "name": "dictate",
                            "description": "Record and transcribe a single utterance from the microphone",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "timeout_secs": {
                                        "type": "number",
                                        "description": "Maximum recording time in seconds (default: 30)"
                                    }
                                }
                            }
                        },
                        {
                            "name": "listen",
                            "description": "Start continuous listening and transcribe each utterance",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "duration_secs": {
                                        "type": "number",
                                        "description": "How long to listen in seconds (default: 60)"
                                    }
                                }
                            }
                        },
                        {
                            "name": "get_status",
                            "description": "Get current state (idle, recording, transcribing) and loaded model info",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        }
                    ]
                }
            }),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or(json!({}));
                let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                let result = match tool_name {
                    "dictate" => self.handle_dictate(&arguments),
                    "listen" => self.handle_listen(&arguments),
                    "get_status" => self.handle_get_status(),
                    _ => Err(format!("Unknown tool: {tool_name}")),
                };

                match result {
                    Ok(content) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": content
                    }),
                    Err(e) => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": e
                            }],
                            "isError": true
                        }
                    }),
                }
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {method}")
                }
            }),
        }
    }
}

/// Run the MCP stdio server. Reads JSON-RPC messages from stdin, writes responses to stdout.
pub fn run_mcp_server() -> Result<(), String> {
    let server = McpServer::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("Failed to read stdin: {e}"))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let error_response = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {e}")
                    }
                });
                let response_str = serde_json::to_string(&error_response).unwrap();
                writeln!(stdout, "{response_str}").map_err(|e| format!("Write error: {e}"))?;
                stdout.flush().map_err(|e| format!("Flush error: {e}"))?;
                continue;
            }
        };

        let response = server.handle_request(&request);

        // Null response means it was a notification — no reply needed
        if response.is_null() {
            continue;
        }

        let response_str = serde_json::to_string(&response).map_err(|e| format!("Serialize error: {e}"))?;
        writeln!(stdout, "{response_str}").map_err(|e| format!("Write error: {e}"))?;
        stdout.flush().map_err(|e| format!("Flush error: {e}"))?;
    }

    Ok(())
}
