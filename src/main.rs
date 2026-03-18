use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "outspoken", version, about = "AI-powered dictation daemon — press Option+D to dictate")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Commands {
    /// Start the dictation daemon (Option+D to record, Option+D again to transcribe + type)
    Start {
        /// Model to use (auto-downloads if missing)
        #[arg(long, default_value = "large-v3-turbo")]
        model: String,
    },
    /// Install as a LaunchAgent (auto-start on login)
    Install,
    /// Uninstall the LaunchAgent
    Uninstall,
    /// Show daemon status
    Status,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { model } => {
            if let Err(e) = run_daemon(&model) {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Install => {
            println!("install: not yet implemented");
        }
        Commands::Uninstall => {
            println!("uninstall: not yet implemented");
        }
        Commands::Status => {
            println!("status: not yet implemented");
        }
    }
}

fn run_daemon(model_name: &str) -> Result<(), String> {
    // Ensure model is available
    let model_path = ensure_model(model_name)?;

    // Load transcription service
    let config = outspoken_lib::transcription::TranscriptionConfig::default();
    let transcriber = outspoken_lib::transcription::TranscriptionService::new(&model_path, config)?;

    // Set up hotkey channel
    let (hotkey_tx, hotkey_rx) = mpsc::channel();

    // Set up shutdown
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_ctrlc = shutdown.clone();
    ctrlc::set_handler(move || {
        shutdown_ctrlc.store(true, Ordering::SeqCst);
    })
    .map_err(|e| format!("Failed to set Ctrl+C handler: {e}"))?;

    // Start hotkey listener (Option+D)
    start_hotkey_listener(hotkey_tx, shutdown.clone())?;

    // Create platform components
    let audio = Box::new(outspoken_lib::platform::macos::MacAudioCapture::new());
    let injector = Box::new(outspoken_lib::platform::macos::MacTextInjector::new());
    let indicator = Box::new(outspoken_lib::platform::macos::MacStatusIndicator::new());

    eprintln!("outspoken daemon running. Press Option+D to start dictating. Ctrl+C to quit.");

    let mut daemon = outspoken_lib::daemon::Daemon::new(
        audio,
        transcriber,
        injector,
        indicator,
        hotkey_rx,
        shutdown,
    );

    daemon.run()
}

fn start_hotkey_listener(tx: mpsc::Sender<()>, shutdown: Arc<AtomicBool>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        use outspoken_lib::hotkey_listener::{HotkeyListener, MacHotkeyListener};
        let tx_clone = tx.clone();
        let mut listener = MacHotkeyListener::new(move || {
            let _ = tx_clone.send(());
        });
        listener.start().map_err(|e| format!("Failed to start hotkey listener: {e}"))?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        // On non-macOS, listen for Enter key on stdin as fallback
        let tx_clone = tx.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            for _line in stdin.lock().lines() {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                let _ = tx_clone.send(());
            }
        });
    }

    Ok(())
}

fn ensure_model(model_name: &str) -> Result<PathBuf, String> {
    let downloaded = outspoken_lib::models::list_downloaded_models()?;
    if let Some(m) = downloaded.iter().find(|m| m.name == model_name) {
        let path = PathBuf::from(&m.path);
        if path.exists() {
            return Ok(path);
        }
    }

    eprintln!("Model '{model_name}' not found. Downloading...");
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("Runtime error: {e}"))?;
    let progress_map: outspoken_lib::models::ProgressMap =
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));
    let cancellation_map: outspoken_lib::models::CancellationMap =
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

    let result = rt.block_on(outspoken_lib::models::download_model(
        model_name.to_string(),
        progress_map,
        cancellation_map,
    ))?;

    eprintln!("Download complete.");
    Ok(PathBuf::from(result.path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_start_command() {
        let cli = Cli::parse_from(["outspoken", "start"]);
        assert_eq!(cli.command, Commands::Start { model: "large-v3-turbo".to_string() });
    }

    #[test]
    fn test_install_command() {
        let cli = Cli::parse_from(["outspoken", "install"]);
        assert_eq!(cli.command, Commands::Install);
    }

    #[test]
    fn test_uninstall_command() {
        let cli = Cli::parse_from(["outspoken", "uninstall"]);
        assert_eq!(cli.command, Commands::Uninstall);
    }

    #[test]
    fn test_status_command() {
        let cli = Cli::parse_from(["outspoken", "status"]);
        assert_eq!(cli.command, Commands::Status);
    }

    #[test]
    fn test_no_command_fails() {
        let result = Cli::try_parse_from(["outspoken"]);
        assert!(result.is_err());
    }
}
