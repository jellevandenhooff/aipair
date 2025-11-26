mod api;
mod jj;
mod mcp;
mod review;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(name = "aipair")]
#[command(about = "Code review tool for AI pair programming")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the web server (includes MCP endpoint at /mcp)
    Serve {
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
    /// Initialize aipair in the current directory
    Init {
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("aipair=debug".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port } => {
            api::serve(port).await?;
        }
        Commands::Init { port } => {
            init(port)?;
        }
    }

    Ok(())
}

fn init(port: u16) -> Result<()> {
    let mcp_json = Path::new(".mcp.json");

    if mcp_json.exists() {
        println!("Warning: .mcp.json already exists, overwriting");
    }

    let config = serde_json::json!({
        "mcpServers": {
            "aipair": {
                "type": "http",
                "url": format!("http://localhost:{}/mcp", port)
            }
        }
    });

    fs::write(mcp_json, serde_json::to_string_pretty(&config)?)?;
    println!("Created .mcp.json");

    // Create .aipair directory
    let aipair_dir = Path::new(".aipair");
    if !aipair_dir.exists() {
        fs::create_dir_all(aipair_dir)?;
        println!("Created .aipair/");
    }

    println!();
    println!("Initialization complete. Next steps:");
    println!("  1. Start the server: aipair serve");
    println!("  2. Open the web UI at http://localhost:{}", port);

    Ok(())
}
