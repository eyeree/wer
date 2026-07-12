//! `wer-atlas` — export, import, validate, and list atlas bundles and vault
//! stores (phase-5-plan.md §5.3, §11): the file-based proof of the sharing
//! model. Bundles carry only the shareable tier (quantized integers +
//! strings, ADR 0013), merge lawfully (ADR 0014), and need no server.
//!
//! Usage:
//!     wer-atlas export <store-dir> <bundle-file>
//!     wer-atlas import <bundle-file> <store-dir>
//!     wer-atlas check  <bundle-file>
//!     wer-atlas list   <store-dir>

use std::process::ExitCode;

use tools::{check_bundle, decode_bundle, encode_bundle, FileStorage};
use world_runtime::{Vault, VaultError};

fn open_vault(dir: &str) -> Result<Vault<FileStorage>, VaultError> {
    let store = FileStorage::open(dir).map_err(VaultError::from)?;
    Vault::open(store)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: wer-atlas export <store-dir> <bundle-file>\n\
                 \x20      wer-atlas import <bundle-file> <store-dir>\n\
                 \x20      wer-atlas check  <bundle-file>\n\
                 \x20      wer-atlas list   <store-dir>";
    let arg_strs: Vec<&str> = args.iter().map(String::as_str).collect();
    match arg_strs.as_slice() {
        ["export", store_dir, bundle_file] => {
            let vault = open_vault(store_dir).map_err(|e| e.to_string())?;
            for issue in vault.issues() {
                eprintln!("note: {issue}");
            }
            let bundle = vault.export();
            let counts = (
                bundle.discoveries.len(),
                bundle.routes.len(),
                bundle.preserves.len(),
            );
            let bytes = encode_bundle(bundle);
            std::fs::write(bundle_file, &bytes).map_err(|e| format!("write {bundle_file}: {e}"))?;
            println!(
                "exported {} discoveries, {} routes, {} preserves ({} bytes) to {bundle_file}",
                counts.0,
                counts.1,
                counts.2,
                bytes.len()
            );
            Ok(())
        }
        ["import", bundle_file, store_dir] => {
            let bytes =
                std::fs::read(bundle_file).map_err(|e| format!("read {bundle_file}: {e}"))?;
            let (_, bundle) = decode_bundle(&bytes).map_err(|e| e.to_string())?;
            let mut vault = open_vault(store_dir).map_err(|e| e.to_string())?;
            let stats = vault.import(&bundle);
            let flush = vault.flush_all();
            for issue in vault.issues() {
                eprintln!("note: {issue}");
            }
            println!(
                "imported into {store_dir}: {} added, {} merged, {} unchanged, {} rejected \
                 ({} records written)",
                stats.added, stats.merged, stats.unchanged, stats.rejected, flush.flushed
            );
            if stats.rejected > 0 {
                return Err(format!("{} records rejected", stats.rejected));
            }
            Ok(())
        }
        ["check", bundle_file] => {
            let bytes =
                std::fs::read(bundle_file).map_err(|e| format!("read {bundle_file}: {e}"))?;
            let check = check_bundle(&bytes).map_err(|e| e.to_string())?;
            println!(
                "bundle {bundle_file}: format v{}, world v{}, {} discoveries, {} routes, \
                 {} preserves",
                check.envelope.format_version,
                check.envelope.world_version,
                check.counts.0,
                check.counts.1,
                check.counts.2
            );
            for finding in &check.findings {
                eprintln!("  finding: {finding}");
            }
            if check.passed() {
                println!("bundle is valid");
                Ok(())
            } else {
                Err(format!("{} findings", check.findings.len()))
            }
        }
        ["list", store_dir] => {
            let vault = open_vault(store_dir).map_err(|e| e.to_string())?;
            println!(
                "store {store_dir}: {} discoveries, {} routes, {} preserves, {} regions seen",
                vault.discoveries().len(),
                vault.routes().len(),
                vault.preserves().len(),
                vault.seen_count()
            );
            for (id, r) in vault.discoveries() {
                println!("  disc  {id:#018x}  {:?}  {}", r.source, r.name);
            }
            for (id, r) in vault.routes() {
                println!(
                    "  route {id:#018x}  {} nodes, usage {}  {}",
                    r.nodes.len(),
                    r.usage,
                    r.name
                );
            }
            for (id, r) in vault.preserves() {
                println!(
                    "  pres  {id:#018x}  {} regions  {}",
                    r.regions.len(),
                    r.name
                );
            }
            for issue in vault.issues() {
                eprintln!("  issue: {issue}");
            }
            Ok(())
        }
        _ => Err(usage.into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
