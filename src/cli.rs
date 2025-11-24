use anyhow::Result;
use clap::Subcommand;

use crate::jj::Jj;
use crate::review::{Author, ReviewStore, ThreadStatus};

#[derive(Subcommand)]
pub enum ReviewCommands {
    /// List reviews with open threads
    List,
    /// Show a review with diff and all comments
    Show {
        /// Change ID to show
        change_id: String,
    },
    /// Reply to a thread
    Reply {
        /// Change ID
        change_id: String,
        /// Thread ID
        thread_id: String,
        /// Reply message
        message: String,
    },
    /// Mark a thread as resolved
    Resolve {
        /// Change ID
        change_id: String,
        /// Thread ID
        thread_id: String,
    },
}

pub async fn handle_review_command(cmd: ReviewCommands) -> Result<()> {
    let jj = Jj::discover()?;
    let store = ReviewStore::new(jj.repo_path());

    match cmd {
        ReviewCommands::List => {
            let reviews = store.list_with_open_threads()?;
            if reviews.is_empty() {
                println!("No reviews with open threads.");
                return Ok(());
            }

            println!("Reviews with open comments:\n");
            for review in reviews {
                let open_count = review
                    .threads
                    .iter()
                    .filter(|t| t.status == ThreadStatus::Open)
                    .count();
                println!(
                    "  {} - {} open thread(s)",
                    review.change_id, open_count
                );
                for thread in review.threads.iter().filter(|t| t.status == ThreadStatus::Open) {
                    println!(
                        "    [{}] {}:{}-{} ({} comment(s))",
                        thread.id,
                        thread.file,
                        thread.line_start,
                        thread.line_end,
                        thread.comments.len()
                    );
                }
            }
        }
        ReviewCommands::Show { change_id } => {
            let review = store
                .get(&change_id)?
                .ok_or_else(|| anyhow::anyhow!("No review found for change: {}", change_id))?;

            // Show the diff
            let diff = jj.diff(&change_id, Some(&review.base))?;
            println!("=== Diff for {} (base: {}) ===\n", change_id, review.base);
            println!("{}", diff.raw);

            // Show threads
            if review.threads.is_empty() {
                println!("\n=== No comments ===");
            } else {
                println!("\n=== Comments ===\n");
                for thread in &review.threads {
                    let status = match thread.status {
                        ThreadStatus::Open => "OPEN",
                        ThreadStatus::Resolved => "RESOLVED",
                    };
                    println!(
                        "[{}] {}:{}-{} ({})",
                        thread.id, thread.file, thread.line_start, thread.line_end, status
                    );
                    for comment in &thread.comments {
                        let author = match comment.author {
                            Author::User => "user",
                            Author::Claude => "claude",
                        };
                        println!("  {}: {}", author, comment.text);
                    }
                    println!();
                }
            }
        }
        ReviewCommands::Reply {
            change_id,
            thread_id,
            message,
        } => {
            let review = store.reply_to_thread(&change_id, &thread_id, Author::Claude, &message)?;
            println!("Replied to thread {} in review {}", thread_id, change_id);

            // Show the updated thread
            if let Some(thread) = review.threads.iter().find(|t| t.id == thread_id) {
                println!("\nThread {}:", thread_id);
                for comment in &thread.comments {
                    let author = match comment.author {
                        Author::User => "user",
                        Author::Claude => "claude",
                    };
                    println!("  {}: {}", author, comment.text);
                }
            }
        }
        ReviewCommands::Resolve { change_id, thread_id } => {
            store.resolve_thread(&change_id, &thread_id)?;
            println!(
                "Resolved thread {} in review {}",
                thread_id, change_id
            );
        }
    }

    Ok(())
}
