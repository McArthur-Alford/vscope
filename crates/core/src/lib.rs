use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Message {
    Search(String),            // Set the search term
    Get(usize),                // Get the next n things matching the search term
    Track(PathBuf, TrackArgs), // Track a directory or file
    Untrack(PathBuf),          // Untrack a directory or file
    Paths(Vec<PathBuf>),       // A response with some paths
    Confirmation,              // Confirmation with no data
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
