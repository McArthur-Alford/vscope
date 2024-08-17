use anyhow::{Context, Error};
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::{path::PathBuf, str::FromStr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    stream,
};

#[derive(Clone, Serialize, Deserialize, Debug)]
// #[serde(untagged)]
#[serde(tag = "t", content = "c")]
pub enum Message {
    Search(String),              // Set the search term
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
        let message = serde_json::to_vec(&message)?;
        let size = message.len();
        println!("{}", size);
        self.stream.write_u32(size as u32).await?;
        self.stream.flush().await?;
        self.stream.write_all(&message).await?;
        self.stream.flush().await?;
        Ok(())
    }
    pub async fn recieve_message(&mut self) -> anyhow::Result<Message> {
        let n = self.stream.read_u32().await?;
        let mut buf = vec![0; n as usize];
        let _ = self.stream.read_exact(&mut buf).await?;
        println!("{}", n);
        serde_json::from_slice::<Message>(&buf).context("Failed to deserialize")
    }

    pub async fn communicate(&mut self, message: Message) -> anyhow::Result<Message> {
        self.send_message(message).await?;
        self.recieve_message().await
    }
}

pub async fn connect_to_daemon() -> anyhow::Result<Connection> {
    let bind_path = PathBuf::from_str("/tmp/vs.sock")?;
    let stream = UnixStream::connect(bind_path)
        .await
        .context("Failed to connect to daemon")?;
    Ok(Connection { stream })
}
