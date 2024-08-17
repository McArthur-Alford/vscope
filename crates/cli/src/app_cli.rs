use crate::{tui, Args};
use std::io;
use std::path::PathBuf;
use clap::builder::Str;
use vs_core::Message;

#[derive(Debug, Default)]
pub struct AppCLI {
}

impl AppCLI {
    pub fn render(message: &Message) -> io::Result<()> {
        match message {
            Message::Paths(paths) => {
                for path in paths {
                    print!("{} ", path.display());
                }
                println!();
            },
            _ => { panic!("Unsupported response from daemon!")}
        }

        Ok(())
    }
    
    pub fn render_list(message: &Message) -> io::Result<()> {
        match message {
            Message::Paths(paths) => {
                const pathHeader:&str = "PATH";
                const typeHeader:&str = "TYPE";
                
                let mut maxPathLength = paths.iter()
                    .map(|path| path.display().to_string().len())
                    .max()
                    .unwrap();
                
                maxPathLength = 
                    if maxPathLength >= pathHeader.len() {maxPathLength} else { pathHeader.len() };
                
                println!("{:<width$} {}", pathHeader, typeHeader, width=maxPathLength);
                for path in paths {
                    println!("{:<width$} {}", path.display(), get_path_type_char(path), width=maxPathLength);
                }
            },
            _ => { panic!("Unsupported response from daemon!")}
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