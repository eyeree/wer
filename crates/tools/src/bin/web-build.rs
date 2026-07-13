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
    generate_world_model_docs(&root, &dist)?;

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

fn generate_world_model_docs(root: &Path, dist: &Path) -> io::Result<()> {
    let markdown = fs::read_to_string(root.join("docs/world-model.md"))?;
    let docs_dir = dist.join("docs");
    fs::create_dir_all(&docs_dir)?;
    fs::write(
        docs_dir.join("world-model.html"),
        render_doc_page("World Model", &markdown),
    )
}

fn render_doc_page(title: &str, markdown: &str) -> String {
    let mut body = String::new();
    let mut paragraph = String::new();
    let mut in_code = false;
    let mut in_table = false;

    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            flush_paragraph(&mut body, &mut paragraph);
            if in_table {
                body.push_str("</tbody></table>\n");
                in_table = false;
            }
            if in_code {
                body.push_str("</code></pre>\n");
                in_code = false;
            } else {
                body.push_str("<pre><code>");
                in_code = true;
            }
            continue;
        }
        if in_code {
            body.push_str(&escape_html(line));
            body.push('\n');
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush_paragraph(&mut body, &mut paragraph);
            if in_table {
                body.push_str("</tbody></table>\n");
                in_table = false;
            }
            continue;
        }
        if let Some(level) = heading_level(trimmed) {
            flush_paragraph(&mut body, &mut paragraph);
            if in_table {
                body.push_str("</tbody></table>\n");
                in_table = false;
            }
            let text = trimmed[level + 1..].trim();
            body.push_str(&format!(
                "<h{level} id=\"{}\">{}</h{level}>\n",
                slug(text),
                inline_markdown(text)
            ));
            continue;
        }
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            flush_paragraph(&mut body, &mut paragraph);
            if !in_table {
                body.push_str("<table><tbody>\n");
                in_table = true;
            }
            if !trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                body.push_str("<tr>");
                for cell in trimmed.trim_matches('|').split('|') {
                    body.push_str("<td>");
                    body.push_str(&inline_markdown(cell.trim()));
                    body.push_str("</td>");
                }
                body.push_str("</tr>\n");
            }
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    flush_paragraph(&mut body, &mut paragraph);
    if in_table {
        body.push_str("</tbody></table>\n");
    }
    if in_code {
        body.push_str("</code></pre>\n");
    }

    format!(
        concat!(
            "<!doctype html>\n<html lang=\"en\">\n<head>\n",
            "  <meta charset=\"utf-8\" />\n",
            "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n",
            "  <title>{}</title>\n",
            "  <link rel=\"stylesheet\" href=\"../assets/app.css\" />\n",
            "</head>\n<body>\n",
            "  <header class=\"site-header\"><a class=\"brand\" href=\"../\">Infinite World Exploration</a>",
            "<nav aria-label=\"Site\"><a href=\"../\">Viewer</a><a href=\"./world-model.html\">World Model</a><a href=\"../help/\">Help</a></nav></header>\n",
            "  <main class=\"doc-page\">\n{}\n  </main>\n",
            "</body>\n</html>\n"
        ),
        escape_html(title),
        body
    )
}

fn heading_level(line: &str) -> Option<usize> {
    let count = line.chars().take_while(|c| *c == '#').count();
    (1..=6)
        .contains(&count)
        .then_some(count)
        .filter(|&level| line.as_bytes().get(level) == Some(&b' '))
}

fn flush_paragraph(body: &mut String, paragraph: &mut String) {
    if paragraph.is_empty() {
        return;
    }
    body.push_str("<p>");
    body.push_str(&inline_markdown(paragraph));
    body.push_str("</p>\n");
    paragraph.clear();
}

fn inline_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut code = false;
    for part in text.split('`') {
        if code {
            out.push_str("<code>");
            out.push_str(&escape_html(part));
            out.push_str("</code>");
        } else {
            out.push_str(&escape_html(part));
        }
        code = !code;
    }
    out
}

fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if matches!(c, ' ' | '-' | '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn run(command: &mut Command) -> Result<(), Box<dyn std::error::Error>> {
    let status = command.status()?;
    if !status.success() {
        return Err(format!("command failed with {status}: {command:?}").into());
    }
    Ok(())
}
