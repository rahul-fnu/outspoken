use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "outspoken", version, about = "AI-powered dictation daemon")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Commands {
    /// Start the dictation daemon
    Start,
    /// Install the daemon as a system service
    Install,
    /// Uninstall the daemon system service
    Uninstall,
    /// Show daemon status
    Status,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start => {
            println!("running");
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_start_command() {
        let cli = Cli::parse_from(["outspoken", "start"]);
        assert_eq!(cli.command, Commands::Start);
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

    #[test]
    fn test_invalid_command_fails() {
        let result = Cli::try_parse_from(["outspoken", "bogus"]);
        assert!(result.is_err());
    }
}
