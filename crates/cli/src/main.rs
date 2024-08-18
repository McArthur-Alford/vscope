use crate::app_cli::AppCLI;
use crate::app_interactive::AppInteractive;
use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueHint};
use ratatui::{style::Stylize, widgets::Widget};
use std::path::PathBuf;
use std::str::FromStr;
use std::{env, fs};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use vs_core::{connect_to_daemon, Connection, Message, TrackArgs};
use Message::Confirmation;

#[derive(Parser, Debug)]
#[command(name = "vs", version, about, long_about = None)]
#[clap(args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[clap(flatten)]
    search: Option<SearchArgs>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[clap(action)]
    Search(SearchArgs),
    #[clap(action)]
    Track {
        #[arg(value_hint = ValueHint::AnyPath)]
        path: PathBuf,

        /// Recurse into directories.
        #[arg(short, long)]
        recurse: bool,
        
        #[clap(flatten)]
        args: SharedArgs,
    },
    #[clap(action)]
    Untrack {
        #[arg(value_hint = ValueHint::AnyPath)]
        path: PathBuf,

        /// Recurse into directories.
        #[arg(short, long)]
        recurse: bool,

        #[clap(flatten)]
        args: SharedArgs,
    },
}

#[derive(Args, Debug)]
struct SharedArgs {
    /// Recurse into symlinks.
    #[arg(short, long)]
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

    /// Show <value> search results inline.
    #[arg(short, long, group="inline_grp")]
    inline: Option<usize>,

    /// Output results in a tabular format.
    #[arg(short, long, requires="inline_grp")]
    list: bool,
}

mod app_cli;
mod app_interactive;
mod tui_helper;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Cli::parse();

    let mut connection = connect_to_daemon().await?;

    match &args.command {
        Some(Commands::Track { path, args, recurse }) => {
            if !path.exists() {
                return anyhow::bail!("Path does not exist");
            }
            println!("Tracking directory: {}", path.display());
            let message = Message::Track(
                fs::canonicalize(path.clone().to_owned())?,
                TrackArgs {
                    recursive: *recurse,
                    follow_symlinks: args.symlinks,
                },
            );
            let response = connection.communicate(message).await?;

            match response {
                Confirmation => Ok(()),
                _ => {
                    anyhow::bail!("Unexpected response");
                }
            }
        }
        Some(Commands::Untrack { path, args, recurse }) => {
            if !path.exists() {
                return anyhow::bail!("Path does not exist");
            }
            println!("No longer tracking directory: {}", path.display());

            let message = Message::Untrack(
                fs::canonicalize(path.clone().to_owned())?,
                TrackArgs {
                    recursive: *recurse,
                    follow_symlinks: args.symlinks,
                },
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
    // let response = Confirmation;

    match response {
        Confirmation => {
            if args.inline.is_some() {
                let message = Message::Get(args.inline.unwrap());
                let response = connection.communicate(message).await?;

                let paths = match response {
                    Message::Paths(paths) => paths,
                    _ => {
                        anyhow::bail!("Unexpected response");
                    }
                };

                let pwd = env::current_dir()?;
                let paths = paths
                    .into_iter()
                    .filter(|p| p.starts_with(pwd.clone()))
                    .filter_map(|p| {
                        let stripped = p.strip_prefix(pwd.clone()).map(|p| p.to_path_buf());
                        stripped.ok()
                    })
                    .map(|p| {
                        let mut p2 = PathBuf::new();
                        p2.push(".");
                        p2.push(p);
                        p2
                    })
                    .filter(|p| p.is_file() || args.directories && p.is_dir() 
                        || args.args.symlinks && p.is_symlink())
                    .collect::<Vec<PathBuf>>();

                if args.list {
                    AppCLI::render_list(&paths).context("Failed to render list")
                } else {
                    AppCLI::render(&paths).context("Failed to render")
                }
            } else {
                let mut terminal = tui_helper::init().context("Failed to init terminal")?;
                let app_result = AppInteractive::default()
                    .run(&mut terminal, &mut connection, args).await;
                tui_helper::restore()?;

                app_result
                    .map(|opt| opt.map(|path| print!("{}", path)))
                    .context("App result was err")?;

                Ok(())
            }
        }
        _ => Err(anyhow::anyhow!("Unexpected response")),
    }
}
