mod api;
mod jj;
mod line_mapper;
mod review;
mod session;
mod terminal;
mod timeline;
mod todo;

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
    /// Start the web server
    Serve {
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Initialize aipair in the current directory
    Init,
    /// Manage sessions
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },
    /// Push changes to main repo (from session clone)
    Push {
        #[arg(short, long)]
        message: String,
        /// Revision to set the session bookmark to before pushing
        #[arg(long)]
        rev: Option<String>,
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
    New {
        name: String,
        /// Base bookmark to branch from (default: main)
        #[arg(long, default_value = "main")]
        base: String,
    },
    /// List all sessions
    List,
    /// Merge a session into main
    Merge { name: String },
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
        Commands::Init => {
            init()?;
        }
        Commands::Session { command } => match command {
            SessionCommands::New { name, base } => {
                session::session_new(&name, &base)?;
            }
            SessionCommands::List => {
                session::session_list()?;
            }
            SessionCommands::Merge { name } => {
                session::session_merge(&name)?;
            }
        },
        Commands::Push { message, rev } => {
            session::push(&message, rev.as_deref())?;
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

fn init() -> Result<()> {
    let jj = jj::Jj::discover()?;

    // Create .aipair directory
    let aipair_dir = Path::new(".aipair");
    if !aipair_dir.exists() {
        fs::create_dir_all(aipair_dir)?;
        println!("Created .aipair/");
    }

    // Create 'main' bookmark if it doesn't exist
    if jj.get_bookmark("main")?.is_none() {
        jj.bookmark_create("main", "@")?;
        println!("Created 'main' bookmark at current change");
    }

    // Add .aipair to .gitignore
    setup_gitignore()?;

    // Untrack .aipair if it's currently tracked
    untrack_aipair();

    // Add session workflow to CLAUDE.md
    setup_claude_md()?;

    println!();
    println!("Initialization complete. Next steps:");
    println!("  1. Start the server: aipair serve");

    Ok(())
}

fn setup_gitignore() -> Result<()> {
    let gitignore = Path::new(".gitignore");
    if gitignore.exists() {
        let content = fs::read_to_string(gitignore)?;
        // Check if .aipair is already ignored (as a whole line)
        if content.lines().any(|line| line.trim() == ".aipair" || line.trim() == ".aipair/") {
            return Ok(());
        }
        let mut new_content = content;
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(".aipair\n");
        fs::write(gitignore, new_content)?;
    } else {
        fs::write(gitignore, ".aipair\n")?;
    }
    println!("Added .aipair to .gitignore");
    Ok(())
}

fn untrack_aipair() {
    // Silently remove .aipair from git tracking if present
    let _ = std::process::Command::new("git")
        .args(["rm", "-r", "--cached", "--quiet", ".aipair"])
        .output();
}

fn setup_claude_md() -> Result<()> {
    let claude_md = Path::new("CLAUDE.md");

    let section = r#"
## Session Workflow (aipair)

### Commands (run from session clone directory)
- `aipair push -m "summary"` — push changes for review
- `aipair pull` — pull latest main and rebase
- `aipair feedback` — show pending review comments
- `aipair respond <change-id> <thread-id> "message" [--resolve]` — reply to a review thread
- `aipair status` — show session info

### Workflow
1. Make changes, then push: `aipair push -m "description"`
2. Check for feedback: `aipair feedback`
3. Address comments, respond: `aipair respond <change-id> <thread-id> "Fixed" --resolve`
4. Push again: `aipair push -m "Address feedback"`
5. Repeat until all threads resolved
"#;

    if claude_md.exists() {
        let content = fs::read_to_string(claude_md)?;
        if content.to_lowercase().contains("session workflow (aipair)") {
            return Ok(());
        }
        let mut new_content = content;
        new_content.push_str(section);
        fs::write(claude_md, new_content)?;
    } else {
        fs::write(claude_md, format!("# Project Guidelines\n{section}"))?;
    }

    println!("Added session workflow instructions to CLAUDE.md");
    Ok(())
}
