//! Build the Phase 7 static browser artifact.
//!
//! The output is deliberately ordinary static files under `target/web-dist`:
//! copied site assets plus wasm-bindgen's `--target web` glue. Browser APIs stay
//! in `platform-web`; this tool only orchestrates reproducible packaging.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root()?;
    let dist = root.join("target/web-dist");
    let generated = dist.join("generated");

    recreate_dir(&dist)?;
    fs::create_dir_all(&generated)?;
    copy_dir(&root.join("crates/platform-web/web"), &dist)?;

    run(Command::new("cargo").current_dir(&root).args([
        "build",
        "-p",
        "platform-web",
        "--target",
        "wasm32-unknown-unknown",
        "--release",
    ]))?;
    run(Command::new("wasm-bindgen")
        .current_dir(&root)
        .arg(root.join("target/wasm32-unknown-unknown/release/platform_web.wasm"))
        .args(["--out-dir", "target/web-dist/generated", "--target", "web"]))?;

    let wasm_size = fs::metadata(generated.join("platform_web_bg.wasm"))?.len();
    let js_size = fs::metadata(generated.join("platform_web.js"))?.len();
    fs::write(
        dist.join("assets/manifest.json"),
        format!(
            concat!(
                "{{\n",
                "  \"version\": \"phase-7-static-scaffold\",\n",
                "  \"generated/platform_web.js\": {{ \"bytes\": {} }},\n",
                "  \"generated/platform_web_bg.wasm\": {{ \"bytes\": {} }}\n",
                "}}\n"
            ),
            js_size, wasm_size
        ),
    )?;

    println!("built {}", dist.display());
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

fn recreate_dir(path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    fs::create_dir_all(path)
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            fs::create_dir_all(&target)?;
            copy_dir(&source, &target)?;
        } else {
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("command failed with {status}: {command:?}").into());
    }
    Ok(())
}
