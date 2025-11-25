mod api;
mod jj;
mod mcp;
mod review;

use anyhow::Result;
use clap::{Parser, Subcommand};

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
    }

    Ok(())
}
