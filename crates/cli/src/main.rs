use crate::app_cli::AppCLI;
use crate::app_interactive::AppInteractive;
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueHint};
use ratatui::{style::Stylize, widgets::Widget};
use std::path::PathBuf;
use std::str::FromStr;
use std::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vs_core::{connect_to_daemon, Connection, Message, TrackArgs};
use Message::Confirmation;

#[derive(Parser, Debug)]
#[command(name = "vs", version, about, long_about = None)]
#[clap(args_conflicts_with_subcommands = true)]
struct Cli {
    #[clap(flatten)]
    search: Option<SearchArgs>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Search(SearchArgs),
    Track {
        #[arg(value_hint = ValueHint::AnyPath)]
        path: PathBuf,

        #[clap(flatten)]
        args: SharedArgs,
    },
    Untrack {
        #[arg(value_hint = ValueHint::AnyPath)]
        path: PathBuf,

        #[clap(flatten)]
        args: SharedArgs,
    },
}

#[derive(Args, Debug)]
struct SharedArgs {
    /// Recurse into directories.
    #[arg(short, long, group = "recurse_grp")]
    recurse: bool,

    /// Recurse into symlinks.
    #[arg(short, long, requires = "recurse_grp")]
    symlinks: bool,
}

#[derive(Args, Debug)]
struct SearchArgs {
    query: String,
    
    #[clap(flatten)]
    args: SharedArgs,
    
    /// Show directories in output.
    #[arg(short, long)]
    directories: bool,

    /// Filter names using regex.
    #[arg(short, long, default_value = "*")]
    filter: String,

    /// Show <value> search results inline.
    #[arg(short, long)]
    inline: Option<usize>,

    /// Output results in a tabular format.
    #[arg(short, long)]
    list: bool,
}

mod app_cli;
mod app_interactive;
mod tui;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let mut connection = connect_to_daemon().await?;

    match &args.command {
        Some(Commands::Track { path, args }) => {
            if !path.exists() {
                return anyhow::bail!("Path does not exist");
            }
            println!("Tracking directory: {}", path.display());
            let message = Message::Track(
                fs::canonicalize(path.clone().to_owned())?,
                TrackArgs{recursive: args.recurse, follow_symlinks: args.symlinks},
            );
            let response = connection.communicate(message).await?;

            match response {
                Confirmation => Ok(()),
                _ => {
                    anyhow::bail!("Unexpected response");
                }
            }
        }
        Some(Commands::Untrack { path, args }) => {
            if !path.exists() {
                return anyhow::bail!("Path does not exist");
            }
            println!("No longer tracking directory: {}", path.display());

            let message = Message::Untrack(
                fs::canonicalize(path.clone().to_owned())?,
                TrackArgs{recursive: args.recurse, follow_symlinks: args.symlinks},
            );
            let response = connection.communicate(message).await?;

            match response {
                Confirmation => Ok(()),
                _ => {
                    anyhow::bail!("Unexpected response");
                }
            }
        }
        Some(Commands::Search(args)) => main_command(args, connection).await,
        _ => main_command(&args.search.unwrap(), connection).await,
    }
}

async fn main_command(args: &SearchArgs, mut connection: Connection) -> Result<()> {
    let search_message = Message::Search(args.query.clone());
    let response = connection.communicate(search_message).await?;
    
    match response {
        Confirmation => {
            if args.inline.is_some() {
                let message = Message::Get(args.inline.unwrap());
                let response = connection.communicate(message).await?;

                let paths = match response {
                    Message::Paths(paths) => paths,
                    _ => { anyhow::bail!("Unexpected response"); }
                };

                if args.list {
                    AppCLI::render_list(&paths).context("Failed to render list")
                } else {
                    AppCLI::render(&paths).context("Failed to render")
                }
            } else {
                let mut terminal = tui::init().context("Failed to init terminal")?;
                let app_result = AppInteractive::default().run(&mut terminal).await;
                tui::restore()?;

                app_result
                    .map(|opt| opt.map(|path| print!("{}", path)))
                    .context("App result was err")?;

                Ok(())
            }
        }
        _ => {
            Err(anyhow::anyhow!("Unexpected response"))
        }
    }
}
