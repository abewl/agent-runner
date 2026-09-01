//! `runner repo set|show` command implementations (F-02).

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
        None => println!("no repo configured — run `runner repo set <path>`"),
    }
    Ok(())
}
