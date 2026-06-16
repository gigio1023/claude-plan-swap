//! Command-line surface for the binary.
//!
//! This module owns only argument parsing. It deliberately delegates behavior to
//! [`App`](crate::app::App) so clap-specific structs do not become the place
//! where business rules accumulate.

use crate::app::App;
use crate::domain::{AccountKind, RoutingMode};
use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Save the active Claude Code credential as a named account.
    Setup {
        /// Account name, for example team, enterprise, team-2, or backup.
        name: String,
        /// Override the account kind. If omitted, claude auth status is used.
        #[arg(long, value_enum)]
        kind: Option<AccountKind>,
    },
    /// Switch the active Claude Code credential to a saved account.
    Switch {
        name: String,
        #[arg(short, long)]
        yes: bool,
    },
    /// Switch back to the previous account.
    Toggle {
        #[arg(short, long)]
        yes: bool,
    },
    /// List saved accounts.
    List,
    /// Remove a saved account.
    Remove { name: String },
    /// Print the current account name.
    Current,
    /// Show state, config, and cached quota information.
    Status,
    /// Read or update statusline alert settings.
    Config {
        #[arg(long)]
        alert_at: Option<u8>,
        #[arg(long, value_enum)]
        mode: Option<RoutingMode>,
    },
    /// Install the Claude Code statusLine wrapper.
    Install,
    /// Remove the Claude Code statusLine wrapper and restore the prior command.
    Uninstall,
    /// Internal command used by Claude Code statusLine.
    Statusline,
}

/// Parse CLI input and execute the selected command.
///
/// Keeping this as a single public entrypoint prevents the rest of the crate
/// from exporting clap-specific details as public API.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let app = App::new()?;
    app.handle(cli.command)
}
