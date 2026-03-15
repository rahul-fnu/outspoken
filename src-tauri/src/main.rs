// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Check for `mcp serve` subcommand
    if args.len() >= 3 && args[1] == "mcp" && args[2] == "serve" {
        if let Err(e) = outspoken_lib::mcp::run_mcp_server() {
            eprintln!("MCP server error: {e}");
            std::process::exit(1);
        }
        return;
    }

    outspoken_lib::run()
}
