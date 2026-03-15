use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use outspoken_lib::models;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "outspoken", about = "AI-powered dictation from the terminal")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start dictation (records, transcribes, outputs text)
    Dictate {
        /// Model name to use for transcription (auto-downloads if missing)
        #[arg(long)]
        model: Option<String>,
    },
    /// List available models
    Models,
    /// Download a model
    Download {
        /// Model name to download
        name: Option<String>,
    },
}

fn make_progress_bar(total: u64, label: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{{spinner:.green}} {label} [{{bar:40.cyan/blue}}] {{bytes}}/{{total_bytes}} ({{eta}})"
            ))
            .unwrap()
            .progress_chars("#>-"),
    );
    pb
}

fn progress_callback(pb: Arc<ProgressBar>) -> models::ProgressCallback {
    Box::new(move |downloaded, total| {
        pb.set_length(total);
        pb.set_position(downloaded);
    })
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Dictate { model } => {
            if let Err(e) = cmd_dictate(model).await {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Models => {
            cmd_models();
        }
        Commands::Download { name } => {
            if let Err(e) = cmd_download(name).await {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}

async fn ensure_whisper_model(model_name: Option<&str>) -> Result<std::path::PathBuf, String> {
    let name = model_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| models::recommended_model().name);

    // Check if already downloaded
    if let Some(path) = models::get_model_path(&name)? {
        return Ok(path);
    }

    let info = models::available_models()
        .into_iter()
        .find(|m| m.name == name)
        .ok_or_else(|| format!("Unknown model: {name}"))?;

    let size_mb = info.size_bytes / 1_000_000;
    eprintln!("Downloading model '{name}' ({size_mb}MB)...");

    let pb = Arc::new(make_progress_bar(info.size_bytes, "Whisper model"));
    let model_path = models::ensure_model(&name, Some(progress_callback(pb.clone()))).await?;
    pb.finish_with_message("done");
    eprintln!("Model downloaded successfully.");

    Ok(model_path)
}

async fn ensure_vad() -> Result<std::path::PathBuf, String> {
    let vad_dir = dirs::data_dir()
        .ok_or("Could not determine data directory")?
        .join("outspoken")
        .join("models")
        .join("silero_vad.onnx");

    if vad_dir.exists() {
        return Ok(vad_dir);
    }

    eprintln!("Downloading Silero VAD model (~2MB)...");
    let pb = Arc::new(make_progress_bar(2_000_000, "VAD model    "));
    let path = models::ensure_vad_model(Some(progress_callback(pb.clone()))).await?;
    pb.finish_with_message("done");
    eprintln!("VAD model downloaded successfully.");

    Ok(path)
}

async fn cmd_dictate(model: Option<String>) -> Result<(), String> {
    // Ensure both models are downloaded
    let model_path = ensure_whisper_model(model.as_deref()).await?;
    let _vad_path = ensure_vad().await?;

    // Load the transcription model
    let config = outspoken_lib::transcription::TranscriptionConfig::default();
    let service = tokio::task::spawn_blocking({
        let config = config.clone();
        move || outspoken_lib::transcription::TranscriptionService::new(&model_path, config)
    })
    .await
    .map_err(|e| format!("Task join error: {e}"))??;

    eprintln!("Model loaded. Recording... (press Ctrl+C to stop)");

    // Start audio capture
    let recording = outspoken_lib::audio::start_capture(&None, None)?;

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| format!("Signal error: {e}"))?;

    // Stop recording
    recording
        .is_recording
        .store(false, std::sync::atomic::Ordering::Relaxed);

    let buffer = recording
        .buffer
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?
        .clone();

    if buffer.is_empty() {
        eprintln!("No audio captured.");
        return Ok(());
    }

    eprintln!("Transcribing...");

    let result = tokio::task::spawn_blocking(move || service.transcribe(&buffer))
        .await
        .map_err(|e| format!("Task join error: {e}"))??;

    // Output transcription to stdout
    println!("{}", result.text);

    Ok(())
}

fn cmd_models() {
    let available = models::available_models();
    let downloaded = models::list_downloaded_models().unwrap_or_default();
    let downloaded_names: Vec<_> = downloaded.iter().map(|m| m.name.as_str()).collect();

    println!("{:<25} {:>8}  {:>10}  {}", "NAME", "SIZE", "STATUS", "DESCRIPTION");
    println!("{}", "-".repeat(80));

    for m in &available {
        let size = format!("{}MB", m.size_bytes / 1_000_000);
        let status = if downloaded_names.contains(&m.name.as_str()) {
            "downloaded"
        } else {
            "-"
        };
        let rec = if m.recommended { " (recommended)" } else { "" };
        println!(
            "{:<25} {:>8}  {:>10}  {}{}",
            m.name, size, status, m.description, rec
        );
    }
}

async fn cmd_download(name: Option<String>) -> Result<(), String> {
    let model_name = name.unwrap_or_else(|| models::recommended_model().name);

    let info = models::available_models()
        .into_iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| format!("Unknown model: {model_name}"))?;

    if models::get_model_path(&model_name)?.is_some() {
        eprintln!("Model '{model_name}' is already downloaded.");
        return Ok(());
    }

    let size_mb = info.size_bytes / 1_000_000;
    eprintln!("Downloading '{model_name}' ({size_mb}MB)...");

    let pb = Arc::new(make_progress_bar(info.size_bytes, &model_name));
    models::ensure_model(&model_name, Some(progress_callback(pb.clone()))).await?;
    pb.finish_with_message("done");
    eprintln!("Download complete.");

    Ok(())
}
