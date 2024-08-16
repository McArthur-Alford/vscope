use rayon::prelude::*;
use std::{
    fs::read_dir,
    io,
    path::{Path, PathBuf},
    str::FromStr,
};

fn crawl_directory<T: Send + Sync>(dir: PathBuf, f: fn(PathBuf) -> T) -> io::Result<Vec<T>> {
    println!("{}", dir.display());
    if dir.is_dir() {
        let entries = read_dir(dir)?;
        let results = entries
            .par_bridge()
            .filter_map(|p| p.ok())
            .map(|p| p.path())
            .map(|p| crawl_directory(p, f))
            .filter_map(|p| p.ok())
            .flatten()
            .collect::<Vec<_>>();
        Ok(results)
    } else {
        Ok(Vec::new())
    }
}

fn main() {
    crawl_directory(PathBuf::from_str(".").unwrap(), |t| t);
}
