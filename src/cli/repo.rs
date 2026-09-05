//! `runner repo [path]` command implementation — shows the configured
//! target repo, or sets it when a path is given.

use std::path::Path;

use crate::config;

pub fn set(path: &Path) -> Result<(), String> {
    let canonical = config::set_repo_path(path)?;
    println!("repo set: {}", canonical.display());
    Ok(())
}

pub fn show() -> Result<(), String> {
    match config::repo_path() {
        Some(path) => println!("{}", path.display()),
        None => println!("no repo configured — run `runner repo <path>`"),
    }
    Ok(())
}
