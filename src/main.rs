mod api;
mod jj;
mod line_mapper;
mod mcp;
mod review;
mod session;
mod timeline;
mod todo;
mod topic;

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
    /// Manage sessions
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
    /// Push changes to main repo (from session clone)
    Push {
        #[arg(short, long)]
        message: String,
    },
    /// Pull latest from main repo (from session clone)
    Pull,
    /// Show session status
    Status,
    /// Show pending review feedback (run from session clone)
    Feedback,
    /// Respond to a review thread (run from session clone)
    Respond {
        /// Change ID (prefix ok) containing the thread
        change_id: String,
        /// Thread ID (prefix ok) to respond to
        thread_id: String,
        /// Your response message
        message: String,
        /// Resolve the thread after responding
        #[arg(long)]
        resolve: bool,
    },
}

#[derive(Subcommand)]
enum SessionCommands {
    /// Create a new session (clone + setup)
    New { name: String },
    /// List all sessions
    List,
    /// Merge a session into main
    Merge { name: String },
    /// Add aipair session workflow instructions to CLAUDE.md
    SetupClaude,
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
        Commands::Session { command } => match command {
            SessionCommands::New { name } => {
                session::session_new(&name)?;
            }
            SessionCommands::List => {
                session::session_list()?;
            }
            SessionCommands::Merge { name } => {
                session::session_merge(&name)?;
            }
            SessionCommands::SetupClaude => {
                session::session_setup_claude()?;
            }
        },
        Commands::Push { message } => {
            session::push(&message)?;
        }
        Commands::Pull => {
            session::pull()?;
        }
        Commands::Status => {
            session::status()?;
        }
        Commands::Feedback => {
            session::feedback()?;
        }
        Commands::Respond {
            change_id,
            thread_id,
            message,
            resolve,
        } => {
            session::respond(&change_id, &thread_id, &message, resolve)?;
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
