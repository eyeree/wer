//! Phase 7 browser sign-off harness.
//!
//! This intentionally runs the browser-specific gates that are cheap and stable
//! locally. Full workspace CI still owns exhaustive native/clippy coverage.

use std::env;
use std::io;
use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root()?;
    run(Command::new("cargo")
        .current_dir(&root)
        .args(["fmt", "--all", "--", "--check"]))?;
    run(Command::new("cargo")
        .current_dir(&root)
        .env("RUSTFLAGS", "-D warnings")
        .args(["test", "-p", "platform-web"]))?;
    run(Command::new("cargo")
        .current_dir(&root)
        .env("RUSTFLAGS", "-D warnings")
        .args([
            "check",
            "-p",
            "platform-web",
            "--target",
            "wasm32-unknown-unknown",
        ]))?;
    run(Command::new("cargo")
        .current_dir(&root)
        .args(["run", "--bin", "web-build"]))?;
    run(Command::new("node")
        .current_dir(&root)
        .args(["crates/platform-web/web/smoke.mjs", "target/web-dist"]))?;
    println!("web signoff ok");
    Ok(())
}

fn workspace_root() -> io::Result<PathBuf> {
    let mut dir = env::current_dir()?;
    loop {
        if dir.join("Cargo.toml").is_file() && dir.join("crates").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "could not find workspace root",
            ));
        }
    }
}

fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("command failed with {status}: {command:?}").into());
    }
    Ok(())
}
