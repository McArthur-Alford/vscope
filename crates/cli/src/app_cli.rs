use crate::{tui_helper, Args};
use std::io;
use std::path::PathBuf;
use clap::builder::Str;
use vs_core::Message;

#[derive(Debug, Default)]
pub struct AppCLI {
}

impl AppCLI {
    pub fn render(paths: &Vec<PathBuf>) -> io::Result<()> {
        for path in paths {
            print!("{} ", path.display());
        }
        println!();

        Ok(())
    }
    
    pub fn render_list(paths: &Vec<PathBuf>) -> io::Result<()> {
        const PATH_HEADER:&str = "PATH";
        const TYPE_HEADER:&str = "TYPE";
        
        let mut maxPathLength = paths.iter()
            .map(|path| path.display().to_string().len())
            .max()
            .unwrap();
        
        maxPathLength = 
            if maxPathLength >= PATH_HEADER.len() {maxPathLength} else { PATH_HEADER.len() };
        
        println!("{:<width$} {}", PATH_HEADER, TYPE_HEADER, width=maxPathLength);
        for path in paths {
            println!("{:<width$} {}", path.display(), get_path_type_char(path), width = maxPathLength);
        }

        Ok(())
    }
}

fn get_path_type_char(path: &PathBuf) -> char {
    if path.is_dir() {
        'd'
    } else if path.is_file() {
        'f'
    } else if path.is_symlink() {
        's'
    } else {
        '?'
    }
}