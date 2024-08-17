use crate::app_cli::AppCLI;
use crate::app_interactive::AppInteractive;
use anyhow::{Context, Result};
use clap::Parser;
use ratatui::backend::CrosstermBackend;
use ratatui::{style::Stylize, widgets::Widget, Terminal};
use std::io::stdout;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fs, io};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vs_core::{connect_to_daemon, Message, TrackArgs};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    search: PathBuf,

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
async fn main() -> Result<()> {
    let mut connection = connect_to_daemon().await?;
    let message = Message::Track(
        fs::canonicalize("./crates".to_owned())?,
        TrackArgs {
            recursive: true,
            follow_symlinks: false,
        },
    );
    println!("{:?}", connection.communicate(message).await);

    let message = Message::Search("magic".into());
    println!("{:?}", connection.communicate(message).await);
    return Ok(());

    let buffs = ["/mnt/d/", "/mnt/d/vs-scope"]
        .iter()
        .map(|path| PathBuf::from(path))
        .collect();

    let message = Message::Paths(buffs);

    let args = Args::parse();

    println!("{:?}", args);

    if !args.search.exists() {
        panic!("Provided search directory does not exist")
    }

    if args.inline {
        if args.list {
            AppCLI::render_list(&message).context("Failed to render list")
        } else {
            AppCLI::render(&message).context("Failed to render")
        }
    } else {
        let mut terminal = tui::init().context("Failed to init terminal")?;
        let app_result = AppInteractive::default().run(&mut terminal);
        tui::restore()?;

        app_result
            .map(|opt| opt.map(|path| print!("{}", path)))
            .context("App result was err")?;

        Ok(())
    }
}
