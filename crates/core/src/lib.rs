use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};
use tokio::{net::UnixStream, stream};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Message {
    Search(PathBuf),             // Set the search term
    Get(usize),                  // Get the next n things matching the search term
    Track(PathBuf, TrackArgs),   // Track a directory or file
    Untrack(PathBuf, TrackArgs), // Untrack a directory or file
    Paths(Vec<PathBuf>),         // A response with some paths
    Confirmation,                // Confirmation with no data
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct TrackArgs {
    pub recursive: bool,
    pub follow_symlinks: bool,
}

impl Default for TrackArgs {
    fn default() -> Self {
        TrackArgs {
            recursive: true,
            follow_symlinks: false,
        }
    }
}

pub async fn connect_to_daemon() -> anyhow::Result<UnixStream> {
    let bind_path = PathBuf::from_str("/tmp/vs.sock")?;
    UnixStream::connect(bind_path)
        .await
        .context("Failed to connect to daemon")
}
