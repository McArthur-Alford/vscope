use crate::app_cli::AppCLI;
use crate::app_interactive::AppInteractive;
use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand, ValueHint};
use ratatui::backend::CrosstermBackend;
use ratatui::{style::Stylize, widgets::Widget, Terminal};
use std::io::stdout;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fs, io};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vs_core::{connect_to_daemon, Message, TrackArgs};
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
    },
    Untrack {
        #[arg(value_hint = ValueHint::AnyPath)]
        path: PathBuf,
    },
}

#[derive(Args, Debug)]
struct SearchArgs {
    query: String,

    /// Recurse into directories and symlinks.
    #[arg(short, long)]
    recurse: bool,

    /// Recurse into symlinks.
    #[arg(short, long)]
    symlinks: bool,

    /// Show directories in output.
    #[arg(short, long)]
    directories: bool,

    /// Filter names using regex.
    #[arg(short, long, default_value = "*")]
    filter: String,

    /// Show search output inline.
    #[arg(short, long)]
    inline: bool,

    /// Output results in a tabular format.
    #[arg(short, long)]
    list: bool,
}

mod app_cli;
mod app_interactive;
mod tui;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // let mut connection = connect_to_daemon().await?;
    // let message = Message::Track(
    //     fs::canonicalize("./crates".to_owned())?,
    //     TrackArgs {
    //         recursive: true,
    //         follow_symlinks: false,
    //     },
    // );
    // let message = Message::Search("context".to_string());
    // println!("{:?}", connection.communicate(message).await);
    // return Ok(());

    let args = Cli::parse();

    let mut connection = connect_to_daemon().await?;

    match &args.command {
        Some(Commands::Track { path }) => {
            if !path.exists() {
                return anyhow::bail!("Path does not exist");
            }
            println!("Tracking directory: {}", path.display());
            let message = Message::Track(
                fs::canonicalize(path.clone().to_owned())?,
                TrackArgs::default(),
            );
            let response = connection.communicate(message).await?;

            match response {
                Confirmation => Ok(()),
                _ => {
                    anyhow::bail!("Unexpected response");
                }
            }
        }
        Some(Commands::Untrack { path }) => {
            if !path.exists() {
                return anyhow::bail!("Path does not exist");
            }
            println!("Untracking directory: {}", path.display());

            let message = Message::Untrack(
                fs::canonicalize(path.clone().to_owned())?,
                TrackArgs::default(),
            );
            let response = connection.communicate(message).await?;

            match response {
                Confirmation => Ok(()),
                _ => {
                    anyhow::bail!("Unexpected response");
                }
            }
        }
        Some(Commands::Search(args)) => main_command(args).await,
        _ => main_command(&args.search.unwrap()).await,
    }
}

async fn main_command(args: &SearchArgs) -> Result<()> {
    if args.inline {
        Ok(())
        // if args.list {
        //     AppCLI::render_list().context("Failed to render list")
        // } else {
        //     AppCLI::render(&message).context("Failed to render")
        // }
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
