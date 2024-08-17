use crate::app_cli::AppCLI;
use crate::app_interactive::AppInteractive;
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use ratatui::backend::CrosstermBackend;
use ratatui::{style::Stylize, widgets::Widget, Terminal};
use std::io::stdout;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fs, io};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vs_core::{connect_to_daemon, Message, TrackArgs};
use crate::Commands::Search;

#[derive(Parser, Debug)]
#[command(name = "vs", version, about, long_about = None)]
struct Cli {    
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(name="s")]
    Search(SearchArgs),
    Track {
        path: PathBuf,
    },
    Untrack {
        path: PathBuf,
    },
}

#[derive(Args, Debug)]
struct SearchArgs {
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

    query: String,
}

mod app_cli;
mod app_interactive;
mod tui;

#[tokio::main]
async fn main() -> Result<()> {
    // let mut connection = connect_to_daemon().await?;
    // let message = Message::Track(
    //     fs::canonicalize("./crates".to_owned())?,
    //     TrackArgs {
    //         recursive: true,
    //         follow_symlinks: false,
    //     },
    // );
    // println!("{:?}", connection.communicate(message).await);

    let args = Cli::parse();

    println!("{:?}", args);

    match &args.command {
        Commands::Track {path} => {
            println!("Tracking directory: {}", path.display());
            Ok(())
            // Add your tracking logic here
        }
        Commands::Untrack {path} => {
            if !path.exists() {
                panic!("Provided search directory does not exist")
            }
            println!("Untracking directory: {}", path.display());
            Ok(())
            // Add your untracking logic here
        }
        Commands::Search(args) => {
            main_command(args).await
        }
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