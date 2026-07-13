//! Serve the Phase 7 static browser artifact for local testing.
//!
//! `index.html` loads ES modules and wasm, which browsers refuse from
//! `file://` URLs (CORS), so the viewer needs an HTTP origin. This is a
//! deliberately tiny std-only static file server — no new dependencies, no
//! configuration — that serves `target/web-dist` (or a given directory) with
//! the details a generic one-liner gets wrong:
//!
//! - correct MIME types, including `application/wasm` so streaming
//!   instantiation works;
//! - `Cross-Origin-Opener-Policy` / `Cross-Origin-Embedder-Policy` headers,
//!   so the page is cross-origin isolated and the shared-memory worker mode
//!   (phase-7-plan.md, `worker:shared`) is testable locally;
//! - `Cache-Control: no-store`, so an edit + `web-build` shows up on reload.
//!
//! Usage: `cargo run --bin web-serve [-- [port] [dir]]` — defaults to port
//! 8080 and `target/web-dist`, then open <http://localhost:8080>. Local debug
//! tooling only: it binds the loopback interface and is not a deployment
//! server (the artifact deploys as plain static files; phase-7-plan.md §9.10).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let port: u16 = match args.next() {
        Some(raw) => raw
            .parse()
            .map_err(|_| format!("port must be a number, got {raw:?}"))?,
        None => 8080,
    };
    let root = PathBuf::from(
        args.next()
            .unwrap_or_else(|| String::from("target/web-dist")),
    );
    if !root.join("index.html").is_file() {
        return Err(format!(
            "{} has no index.html — run `cargo run --bin web-build` first",
            root.display()
        )
        .into());
    }

    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!(
        "serving {} at http://localhost:{port}/ (cross-origin isolated; Ctrl+C to stop)",
        root.display()
    );
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let root = root.clone();
        // Thread-per-connection: plenty for a local debug page.
        std::thread::spawn(move || {
            if let Err(err) = handle(stream, &root) {
                eprintln!("web-serve: {err}");
            }
        });
    }
    Ok(())
}

/// Serve one request. GET and HEAD only; every response closes the
/// connection (simplest correct HTTP/1.1: `Connection: close`).
fn handle(mut stream: TcpStream, root: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // Drain the headers so well-behaved clients see a clean close.
    let mut header = String::new();
    while reader.read_line(&mut header)? > 2 {
        header.clear();
    }

    let mut parts = request_line.split_whitespace();
    let (method, target) = match (parts.next(), parts.next()) {
        (Some(method @ ("GET" | "HEAD")), Some(target)) => (method, target),
        (Some(_), Some(_)) => {
            return respond(
                &mut stream,
                "405 Method Not Allowed",
                "text/plain",
                b"GET only",
                true,
            )
        }
        _ => return Ok(()),
    };

    let Some(path) = resolve(root, target) else {
        return respond(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found",
            method == "GET",
        );
    };
    let mut body = Vec::new();
    std::fs::File::open(&path)?.read_to_end(&mut body)?;
    respond(
        &mut stream,
        "200 OK",
        mime_type(&path),
        &body,
        method == "GET",
    )
}

/// Write status, the fixed header set, and (for GET) the body.
fn respond(
    stream: &mut TcpStream,
    status: &str,
    mime: &str,
    body: &[u8],
    include_body: bool,
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\n\
         Content-Type: {mime}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Cross-Origin-Opener-Policy: same-origin\r\n\
         Cross-Origin-Embedder-Policy: require-corp\r\n\
         Connection: close\r\n\r\n",
        body.len()
    )?;
    if include_body {
        stream.write_all(body)?;
    }
    stream.flush()
}

/// Map a request target onto a file under `root`, or `None` for 404. Query
/// strings and fragments are stripped; `.` segments collapse; any `..`
/// segment or percent-escape is rejected outright (the artifact's paths are
/// plain ASCII, so refusing to decode is traversal safety for free);
/// directories serve their `index.html`.
fn resolve(root: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next().unwrap_or("");
    let mut clean = PathBuf::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            segment if segment.contains('%') => return None,
            segment => clean.push(segment),
        }
    }
    let mut full = root.join(clean);
    if full.is_dir() {
        full = full.join("index.html");
    }
    full.is_file().then_some(full)
}

/// Content type by extension — the handful the artifact actually contains,
/// `application/wasm` being the one a generic server most often misses.
fn mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_maps_paths_and_blocks_traversal() {
        let dir = std::env::temp_dir().join(format!("web-serve-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("help")).expect("test dir");
        std::fs::write(dir.join("index.html"), "root").expect("write");
        std::fs::write(dir.join("help/index.html"), "help").expect("write");
        std::fs::write(dir.join("app.wasm"), "wasm").expect("write");

        assert_eq!(resolve(&dir, "/"), Some(dir.join("index.html")));
        assert_eq!(resolve(&dir, "/index.html"), Some(dir.join("index.html")));
        assert_eq!(resolve(&dir, "/help/"), Some(dir.join("help/index.html")));
        assert_eq!(resolve(&dir, "/help"), Some(dir.join("help/index.html")));
        assert_eq!(
            resolve(&dir, "/app.wasm?v=1#frag"),
            Some(dir.join("app.wasm"))
        );
        assert_eq!(resolve(&dir, "/./help/../index.html"), None, "traversal");
        assert_eq!(resolve(&dir, "/%2e%2e/secret"), None, "escapes rejected");
        assert_eq!(resolve(&dir, "/missing.js"), None);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn mime_types_cover_the_artifact() {
        assert_eq!(
            mime_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            mime_type(Path::new("a/app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            mime_type(Path::new("platform_web_bg.wasm")),
            "application/wasm"
        );
        assert_eq!(mime_type(Path::new("manifest.json")), "application/json");
        assert_eq!(
            mime_type(Path::new("unknown.bin")),
            "application/octet-stream"
        );
    }
}
