//! Phase 7 browser sign-off harness.
//!
//! This intentionally runs the browser-specific gates that are cheap and stable
//! locally. Full workspace CI still owns exhaustive native/clippy coverage.
//!
//! Pass `--record-layout crates/platform-web/web/baselines/native-web-alignment-m0-layout.json`
//! to additionally launch the built artifact and capture the Milestone 0
//! viewport characterization with `agent-browser`. The fixed destination keeps
//! the artifact exercised by the smoke gate identical to the file just written.
//! The default sign-off remains browserless so CI does not require that local
//! debugging tool.

use std::env;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root()?;
    let record_layout = record_layout_path(&root)?;
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
    if let Some(path) = record_layout {
        capture_layout(&root, &path)?;
        // The capture target normally lives in the source web tree, while
        // `web-build` copied the pre-capture placeholder. Refresh the artifact
        // before its static schema gate reads the new evidence.
        run(Command::new("cargo")
            .current_dir(&root)
            .args(["run", "--bin", "web-build"]))?;
    }
    run(Command::new("node")
        .current_dir(&root)
        .args(["crates/platform-web/web/smoke.mjs", "target/web-dist"]))?;
    println!("web signoff ok");
    Ok(())
}

fn record_layout_path(root: &Path) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None => Ok(None),
        Some("--record-layout") => {
            let raw = args
                .next()
                .ok_or("--record-layout requires an output path")?;
            if args.next().is_some() {
                return Err("unexpected arguments after --record-layout <path>".into());
            }
            let path = PathBuf::from(raw);
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            let canonical =
                root.join("crates/platform-web/web/baselines/native-web-alignment-m0-layout.json");
            if path != canonical {
                return Err(format!(
                    "--record-layout must write the canonical smoke fixture {}",
                    canonical.display()
                )
                .into());
            }
            Ok(Some(path))
        }
        Some(other) => Err(format!("unknown argument {other:?}").into()),
    }
}

fn capture_layout(root: &Path, output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    run(Command::new("agent-browser").arg("--version"))?;
    let port = TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port();
    let server = Command::new("cargo")
        .current_dir(root)
        .args([
            "run",
            "--quiet",
            "--bin",
            "web-serve",
            "--",
            &port.to_string(),
            "target/web-dist",
        ])
        .stdout(Stdio::null())
        .spawn()?;
    let _server = ChildGuard(server);
    let deadline = Instant::now() + Duration::from_secs(15);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        if Instant::now() >= deadline {
            return Err(format!("web-serve did not start on port {port}").into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    run(Command::new("node").current_dir(root).args([
        "crates/platform-web/web/capture-layout.mjs",
        &format!("http://127.0.0.1:{port}/"),
        output
            .to_str()
            .ok_or("layout output path is not valid UTF-8")?,
    ]))?;
    Ok(())
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
