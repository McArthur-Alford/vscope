use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Message {
    Search(String),      // Set the search term
    Get(usize),          // Get the next n things matching the search term
    Track(PathBuf),      // Track a directory or file
    Untrack(PathBuf),    // Untrack a directory or file
    Paths(Vec<PathBuf>), // A response with some paths
    Confirmation,        // Confirmation with no data
}
