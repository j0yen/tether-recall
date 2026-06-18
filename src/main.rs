//! `wm-tether-recall` — fleet bus proxy for recall memory queries.
//!
//! Bridges the laptop's local recall store to the work node over the NATS fleet
//! bus. Read-only: the responder only ever queries recall; write operations are
//! refused.
//!
//! # Subcommands
//! - `query <text>` — publish a recall query request and print ranked hits
//! - `serve` — run the responder daemon on the laptop
//! - `status` — check responder reachability and last round-trip time

#![deny(unsafe_code)]
#![warn(missing_docs, clippy::pedantic, clippy::nursery)]

use anyhow::Result;
use clap::{Parser, Subcommand};

mod protocol;
mod requester;
mod responder;

use protocol::{DEFAULT_LIMIT, DEFAULT_TIMEOUT_SECS};

/// Fleet bus proxy for recall memory queries.
#[derive(Debug, Parser)]
#[command(
    name = "wm-tether-recall",
    version,
    about = "Bridges the laptop's recall store to the work node over the NATS fleet bus"
)]
struct Cli {
    /// NATS server URL (default: nats://localhost:4222)
    #[arg(long, env = "WM_NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    #[command(subcommand)]
    command: Command,
}

/// Available subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Query the recall store via the fleet bus (requester side).
    Query {
        /// The query text to search for.
        text: String,
        /// Filter by memory kind (e.g. reflective, semantic, procedural).
        #[arg(long)]
        kind: Option<String>,
        /// Maximum number of results to return (capped at 50).
        #[arg(long, default_value_t = DEFAULT_LIMIT)]
        limit: usize,
        /// Use hybrid search (FTS5 + vector).
        #[arg(long)]
        hybrid: bool,
        /// Timeout in seconds to wait for a response.
        #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS)]
        timeout: u64,
    },
    /// Run the responder daemon (laptop side — serves recall queries over the bus).
    Serve {
        /// Path to the `recall` binary (default: searches PATH).
        #[arg(long, env = "WM_RECALL_BIN")]
        recall_bin: Option<String>,
        /// Maximum snippet length in characters.
        #[arg(long, default_value_t = 300)]
        snippet_max: usize,
        /// Maximum hits per response (hard cap regardless of requester limit).
        #[arg(long, default_value_t = 50)]
        limit_cap: usize,
    },
    /// Report responder reachability and last round-trip time.
    Status {
        /// Timeout in seconds to wait for a ping response.
        #[arg(long, default_value_t = 5)]
        timeout: u64,
    },
}

fn main() -> Result<()> {
    // SIGPIPE fix: prevent panic on broken pipe (e.g. `wm-tether-recall query … | head`)
    // SAFETY NOTE: this is a safe C-library call via the sigpipe crate; deny(unsafe_code)
    // applies to our code, not the crate internals.
    sigpipe::reset();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async move {
        match cli.command {
            Command::Query {
                text,
                kind,
                limit,
                hybrid,
                timeout,
            } => {
                requester::run_query(
                    &cli.nats_url,
                    &text,
                    kind.as_deref(),
                    limit,
                    hybrid,
                    timeout,
                )
                .await
            }
            Command::Serve {
                recall_bin,
                snippet_max,
                limit_cap,
            } => {
                responder::run_serve(&cli.nats_url, recall_bin.as_deref(), snippet_max, limit_cap)
                    .await
            }
            Command::Status { timeout } => requester::run_status(&cli.nats_url, timeout).await,
        }
    })
}
