use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, str::FromStr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    stream,
};

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

pub struct Connection {
    stream: UnixStream,
}

impl Connection {
    pub async fn send_message(&mut self, message: Message) -> anyhow::Result<()> {
        let message = bincode::serialize(&message)?;
        self.stream.write_all(&message).await?;
        self.stream.flush().await?;
        Ok(())
    }
    pub async fn receive_message(&mut self) -> anyhow::Result<Message> {
        let mut buf = Vec::new();
        while let Ok(_) = self.stream.read_buf(&mut buf).await {}
        bincode::deserialize::<Message>(&buf).context("Failed to deserialize")
    }

    pub async fn communicate(&mut self, message: Message) -> anyhow::Result<Message> {
        self.send_message(message).await?;
        self.receive_message().await
    }
}

pub async fn connect_to_daemon() -> anyhow::Result<Connection> {
    let bind_path = PathBuf::from_str("/tmp/vs.sock")?;
    let stream = UnixStream::connect(bind_path)
        .await
        .context("Failed to connect to daemon")?;
    Ok(Connection { stream })
}
