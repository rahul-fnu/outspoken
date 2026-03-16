use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use outspoken_lib::audio;
use outspoken_lib::models;
use outspoken_lib::transcription::{TranscriptionConfig, TranscriptionService};
use outspoken_lib::vad::VadSegmenter;

#[derive(Parser)]
#[command(name = "outspoken", version, about = "AI-powered dictation from the terminal")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Record from mic, transcribe, print to stdout, exit
    Dictate {
        /// Model to use (auto-downloads if missing)
        #[arg(long, default_value = "large-v3-turbo-q5_0")]
        model: String,

        /// Also copy result to clipboard
        #[arg(long)]
        copy: bool,

        /// Output as JSON with segments, timestamps, duration
        #[arg(long)]
        json: bool,

        /// Disable voice activity detection
        #[arg(long)]
        no_vad: bool,

        /// Disable self-correction detection
        #[arg(long)]
        no_corrections: bool,

        /// Audio input device name
        #[arg(long)]
        device: Option<String>,

        /// Stream partial transcriptions to stderr during recording
        #[arg(long)]
        stream: bool,
    },

    /// Continuous mode - transcribe each utterance as a new line
    Listen {
        /// Model to use (auto-downloads if missing)
        #[arg(long, default_value = "large-v3-turbo-q5_0")]
        model: String,

        /// Also copy result to clipboard
        #[arg(long)]
        copy: bool,

        /// Output as JSON with segments, timestamps, duration
        #[arg(long)]
        json: bool,

        /// Disable voice activity detection
        #[arg(long)]
        no_vad: bool,

        /// Disable self-correction detection
        #[arg(long)]
        no_corrections: bool,

        /// Audio input device name
        #[arg(long)]
        device: Option<String>,

        /// Seconds of silence before finalizing utterance
        #[arg(long, default_value = "2")]
        silence_timeout: f32,
    },

    /// Manage configuration: models, devices
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// MCP server for Claude Code plugin integration
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },

    /// Print version information
    Version,
}

#[derive(Subcommand)]
enum McpAction {
    /// Start the MCP JSON-RPC server on stdio
    Serve,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// List available and downloaded models
    Models,

    /// Download a model
    Download {
        /// Model name to download
        model: String,
    },

    /// List audio input devices
    Devices,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Dictate {
            model,
            copy,
            json,
            no_vad,
            no_corrections: _,
            device,
            stream,
        } => {
            if let Err(e) = run_dictate(&model, copy, json, no_vad, &device, stream) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Listen {
            model,
            copy,
            json,
            no_vad,
            no_corrections: _,
            device,
            silence_timeout,
        } => {
            if let Err(e) = run_listen(&model, copy, json, no_vad, &device, silence_timeout) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Config { action } => {
            if let Err(e) = run_config(action) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Mcp { action } => match action {
            McpAction::Serve => {
                if let Err(e) = outspoken_lib::mcp::run_mcp_server() {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        },
        Commands::Version => {
            println!("outspoken {}", env!("CARGO_PKG_VERSION"));
        }
    }
}

fn ensure_model(model_name: &str) -> Result<PathBuf, String> {
    let downloaded = models::list_downloaded_models()?;
    if let Some(m) = downloaded.iter().find(|m| m.name == model_name) {
        let path = PathBuf::from(&m.path);
        if path.exists() {
            return Ok(path);
        }
    }

    // Auto-download if not found
    eprintln!("Model '{model_name}' not found locally, downloading...");
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("Failed to create runtime: {e}"))?;
    let progress_map: models::ProgressMap =
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let cancellation_map: models::CancellationMap =
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

    let model = rt.block_on(models::download_model(
        model_name.to_string(),
        progress_map,
        cancellation_map,
    ))?;
    eprintln!("Download complete.");
    Ok(PathBuf::from(model.path))
}

fn load_service(model_name: &str) -> Result<TranscriptionService, String> {
    let model_path = ensure_model(model_name)?;
    let config = TranscriptionConfig::default();
    TranscriptionService::new(&model_path, config)
}

fn run_dictate(
    model: &str,
    copy: bool,
    json: bool,
    no_vad: bool,
    device: &Option<String>,
    stream: bool,
) -> Result<(), String> {
    let service = load_service(model)?;

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .map_err(|e| format!("Failed to set Ctrl+C handler: {e}"))?;

    eprintln!("Recording... press Ctrl+C to stop and transcribe.");
    let recording = audio::start_capture(device, None)?;

    if stream {
        let stream_running = running.clone();
        let stream_buffer = recording.buffer.clone();
        let stream_service = load_service(model)?;
        std::thread::spawn(move || {
            while stream_running.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if !stream_running.load(Ordering::SeqCst) {
                    break;
                }
                let snapshot = match stream_buffer.lock() {
                    Ok(buf) => buf.clone(),
                    Err(_) => continue,
                };
                if snapshot.is_empty() {
                    continue;
                }
                if let Ok(result) = stream_service.transcribe(&snapshot) {
                    let text = result.text.trim();
                    if !text.is_empty() {
                        eprint!("\r\x1b[2K{}", text);
                        let _ = std::io::stderr().flush();
                    }
                }
            }
        });
    }

    // Wait for Ctrl+C
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if stream {
        eprint!("\r\x1b[2K");
        let _ = std::io::stderr().flush();
    }

    // Stop recording
    recording
        .is_recording
        .store(false, Ordering::Relaxed);
    let buffer = recording
        .buffer
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?
        .clone();

    if buffer.is_empty() {
        return Err("No audio recorded".into());
    }

    eprintln!("Transcribing...");
    let result = if no_vad {
        service.transcribe(&buffer)?
    } else {
        let mut vad = VadSegmenter::new()?;
        service.transcribe_with_vad(&buffer, &mut vad)?
    };

    if json {
        let output = serde_json::json!({
            "text": result.text,
            "segments": result.segments,
            "language": result.language,
            "duration_ms": result.duration_ms,
        });
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("{}", result.text);
    }

    if copy {
        copy_to_clipboard(&result.text)?;
        eprintln!("Copied to clipboard.");
    }

    Ok(())
}

fn run_listen(
    model: &str,
    copy: bool,
    json: bool,
    no_vad: bool,
    device: &Option<String>,
    silence_timeout: f32,
) -> Result<(), String> {
    let service = load_service(model)?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .map_err(|e| format!("Failed to set Ctrl+C handler: {e}"))?;

    eprintln!("Listening... press Ctrl+C to stop.");

    let silence_samples = (silence_timeout * 16_000.0) as usize;
    let silence_threshold: f32 = 0.01;

    while running.load(Ordering::SeqCst) {
        let recording = audio::start_capture(device, None)?;

        // Wait for speech then silence, or Ctrl+C
        let mut had_speech = false;
        let mut silent_count: usize = 0;

        loop {
            if !running.load(Ordering::SeqCst) {
                recording
                    .is_recording
                    .store(false, Ordering::Relaxed);
                return Ok(());
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

            // Check the last 1600 samples (100ms at 16kHz) for energy
            let check_len = 1600.min(len);
            let tail = &buf[len - check_len..];
            let rms: f32 = (tail.iter().map(|s| s * s).sum::<f32>() / check_len as f32).sqrt();

            if rms > silence_threshold {
                had_speech = true;
                silent_count = 0;
            } else if had_speech {
                // Count silent samples (approximate from buffer growth)
                silent_count += 1600;
                if silent_count >= silence_samples {
                    break;
                }
            }

            drop(buf);
        }

        // Stop recording and transcribe
        recording
            .is_recording
            .store(false, Ordering::Relaxed);
        let buffer = recording
            .buffer
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?
            .clone();

        if buffer.is_empty() {
            continue;
        }

        let result = if no_vad {
            service.transcribe(&buffer)?
        } else {
            let mut vad = VadSegmenter::new()?;
            service.transcribe_with_vad(&buffer, &mut vad)?
        };
        if result.text.trim().is_empty() {
            continue;
        }

        if json {
            let output = serde_json::json!({
                "text": result.text,
                "segments": result.segments,
                "language": result.language,
                "duration_ms": result.duration_ms,
            });
            println!("{}", serde_json::to_string(&output).unwrap());
        } else {
            println!("{}", result.text);
        }

        if copy {
            let _ = copy_to_clipboard(&result.text);
        }
    }

    Ok(())
}

fn run_config(action: ConfigAction) -> Result<(), String> {
    match action {
        ConfigAction::Models => {
            let available = models::available_models();
            let downloaded = models::list_downloaded_models().unwrap_or_default();
            let downloaded_names: Vec<&str> = downloaded.iter().map(|m| m.name.as_str()).collect();

            println!("{:<30} {:<12} {}", "MODEL", "SIZE", "STATUS");
            println!("{}", "-".repeat(56));
            for model in &available {
                let status = if downloaded_names.contains(&model.name.as_str()) {
                    "downloaded"
                } else {
                    "not downloaded"
                };
                let size = format_bytes(model.size_bytes);
                println!("{:<30} {:<12} {}", model.name, size, status);
            }
            Ok(())
        }
        ConfigAction::Download { model } => {
            eprintln!("Downloading model '{model}'...");
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| format!("Failed to create runtime: {e}"))?;
            let progress_map: models::ProgressMap =
                Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
            let cancellation_map: models::CancellationMap =
                Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

            let result = rt.block_on(models::download_model(
                model,
                progress_map,
                cancellation_map,
            ))?;
            println!("Downloaded: {} ({})", result.name, format_bytes(result.size_bytes));
            Ok(())
        }
        ConfigAction::Devices => {
            let devices = audio::list_devices()?;
            if devices.is_empty() {
                println!("No audio input devices found.");
                return Ok(());
            }
            for dev in &devices {
                let marker = if dev.is_default { " (default)" } else { "" };
                println!("{}{}", dev.name, marker);
            }
            Ok(())
        }
    }
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    use arboard::Clipboard;
    let mut clipboard = Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| format!("Clipboard error: {e}"))?;
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    }
}
