//! The `F12` debug dump: write a snapshot of the running shell into
//! `./dump/<UTC datetime>/` — a screenshot of the active presentation (map
//! or POV) plus `state.txt`, a plain-text report of everything useful for
//! diagnosing a problem after the fact: player/camera state (position, yaw,
//! pitch, and the forward vector in POV), steering and anchors, frame
//! telemetry, the cell under the player, the covering region's
//! dependency-hash chain (ADR 0008), the vault counters, and the `WER_*`
//! environment.
//!
//! Debug output only, never an input. The map screenshot re-runs the CPU
//! composer (the headless `--screenshot` path) so the pixels are faithful
//! even while the GPU-composed map is active; the POV screenshot goes
//! through the offscreen [`renderer::pov::PovCapture`] (ADR 0021) — the
//! live renderer still exposes no readback of any kind (ADR 0017). The
//! wall-clock directory name is presentation-only and never an identity
//! (ADR 0003 posture); nothing written here feeds back into world state,
//! hashing, or persistence.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use viewer_host::layout::PresentationMode;
use viewer_host::map::MapBackend;
use world_core::layer::layer_decl;
use world_core::PossibilityDomain;
use world_runtime::Pass;

use crate::panel::PanelInfo;
use crate::pov::{self, PovChunkManager, PovOrganismManager};
use crate::{App, CLEAR_COLOR, ORGANISM_INFO_ZOOM};

/// Encode an RGBA8 buffer as binary PPM (P6, alpha dropped) — the repo's
/// image format everywhere (`--screenshot`, `--pov-script`, and the dump).
pub(crate) fn write_ppm(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
    let mut out = Vec::with_capacity(rgba.len() / 4 * 3 + 32);
    out.extend_from_slice(format!("P6\n{width} {height}\n255\n").as_bytes());
    for px in rgba.chunks_exact(4) {
        out.extend_from_slice(&px[..3]);
    }
    std::fs::write(path, out).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Seconds since the Unix epoch from the system clock (0 if the clock sits
/// before the epoch — the dump must never panic over a broken clock).
fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Gregorian `(year, month, day)` from days since the Unix epoch — Howard
/// Hinnant's `civil_from_days`. Hand-rolled because the workspace carries no
/// date crate and the wall clock is presentation-only here.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y, m as u32, d as u32)
}

/// UTC date/time as a filesystem-safe `YYYY-MM-DD_HH-MM-SS` directory name.
fn utc_stamp(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let tod = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}",
        tod / 3600,
        (tod / 60) % 60,
        tod % 60
    )
}

/// UTC date/time as ISO 8601 for the report body.
fn utc_iso(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let tod = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod / 60) % 60,
        tod % 60
    )
}

/// Create `<base>/<UTC datetime>/`, suffixing `-1`, `-2`, … when several
/// dumps land within the same clock second.
fn create_dump_dir(base: &Path, secs: i64) -> Result<PathBuf, String> {
    std::fs::create_dir_all(base).map_err(|e| format!("create {}: {e}", base.display()))?;
    let stamp = utc_stamp(secs);
    for n in 0..100u32 {
        let dir = if n == 0 {
            base.join(&stamp)
        } else {
            base.join(format!("{stamp}-{n}"))
        };
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("create {}: {e}", dir.display())),
        }
    }
    Err(String::from("dump directory name collision"))
}

impl App {
    /// One-shot `F12` handler: write the dump, log where it went (or why it
    /// failed). Never panics — a diagnostics feature must not take down the
    /// session it is diagnosing.
    pub(crate) fn debug_dump(&mut self) -> Result<PathBuf, String> {
        let start = std::time::Instant::now();
        let result = self.write_debug_dump();
        match &result {
            Ok(dir) => log::info!(
                "debug dump written to {} ({:.2}s)",
                dir.display(),
                start.elapsed().as_secs_f64()
            ),
            Err(err) => log::error!("debug dump failed: {err}"),
        }
        result
    }

    fn write_debug_dump(&mut self) -> Result<PathBuf, String> {
        self.write_debug_dump_into(Path::new("dump"))
    }

    fn write_debug_dump_into(&mut self, base: &Path) -> Result<PathBuf, String> {
        let dir = create_dump_dir(base, unix_seconds())?;
        let screenshot = match self.controller.layout().mode {
            PresentationMode::Map | PresentationMode::Split => self.dump_map_screenshot(&dir),
            PresentationMode::Pov => self.dump_pov_screenshot(&dir),
        };
        let report = self
            .dump_report(&screenshot)
            .map_err(|e| format!("format report: {e}"))?;
        let path = dir.join("state.txt");
        std::fs::write(&path, report).map_err(|e| format!("write {}: {e}", path.display()))?;
        // A failed screenshot still leaves state.txt behind (with the error
        // recorded in it); surface the failure only after the report landed.
        screenshot?;
        Ok(dir)
    }

    /// The map screenshot: the live CPU compose path (`frame`'s non-GPU
    /// branch), forced regardless of the GPU toggle so the dump is the full
    /// composed map + panel, not the GPU path's sparse overlay strip. The
    /// panel's cursor readout is pinned at the player, like `--screenshot`.
    fn dump_map_screenshot(&mut self, dir: &Path) -> Result<String, String> {
        let decor = self.build_decor();
        let preferences = self.controller.map_preferences();
        let traveler = self.controller.world().traveler().position;
        self.composer.set_zoom(preferences.zoom);
        self.composer.compose(
            self.controller.world().map(),
            traveler,
            preferences.channel,
            preferences.overlays,
            self.controller.world().anchors(),
            &decor,
        );
        let organism = if preferences.zoom >= ORGANISM_INFO_ZOOM {
            Self::pick_organism(self.controller.world().map(), traveler)
        } else {
            None
        };
        let world = self.controller.world();
        let capture = self.controller.capture_preferences();
        let info = PanelInfo {
            fps: self.fps,
            update_ms: self.update_ms,
            compose_ms: self.compose_ms,
            render_ms: self.render_ms,
            upload_kb: self.upload_kb,
            gpu_compose: false,
            tier: self.tier.name(),
            cache_ceiling_bytes: world.map().config().max_field_cache_bytes,
            pass_ms: self.pass_ms,
            workers: self.executor.parallelism(),
            stats: self.last_stats,
            regen_totals: &self.regen_totals,
            macro_tiles: world.map().macro_cache().len(),
            rosters: world.map().roster_cache().len(),
            organisms: world.map().organism_count(),
            jobs_in_flight: world.map().jobs_in_flight(),
            pinned_violations: self.composer.pinned_violations,
            channel: preferences.channel,
            player: traveler,
            bias: world.bias(),
            anchors: world.anchors(),
            capture_category: capture.category.name(),
            capture_polarity: capture.polarity,
            transition_mode: world.transition_mode(),
            vault: self.vault_panel_info(),
            zoom: preferences.zoom,
            cursor: Some(Self::sample_cursor(world.map(), traveler)),
            organism,
        };
        let (width, height) = self.hud.size();
        let pixels = self.hud.compose(self.composer.pixels(), &info);
        write_ppm(&dir.join("screenshot.ppm"), pixels, width, height)?;
        Ok(format!(
            "screenshot.ppm ({width}x{height} map+panel, CPU-composed, channel {}, zoom x{})",
            preferences.channel.name(),
            preferences.zoom
        ))
    }

    /// The POV screenshot: an offscreen [`renderer::pov::PovCapture`] at the
    /// window size (ADR 0021). The live renderer's chunks cannot be read
    /// back, so a fresh chunk manager re-meshes the ring inline against the
    /// live, already-settled map — meshing is deterministic, so this shows
    /// exactly what the window shows (the `--pov-script` snap loop).
    fn dump_pov_screenshot(&self, dir: &Path) -> Result<String, String> {
        let (width, height) = self
            .renderer
            .as_ref()
            .map_or((1024, 768), renderer::Renderer::size);
        let mut cap = renderer::pov::PovCapture::new(width, height)
            .map_err(|e| format!("pov capture init: {e}"))?;
        let camera = self.controller.pov_camera();
        let map = self.controller.world().map();
        let mut chunks = PovChunkManager::new();
        for _ in 0..256 {
            let (uploads, removes) = chunks.sync(
                map,
                (camera.pos.x, camera.pos.y),
                self.pov_radius,
                &world_runtime::InlineExecutor,
            );
            let done = uploads.is_empty() && chunks.is_idle();
            cap.apply(&uploads, &removes, None);
            if done {
                break;
            }
        }
        // The capture owns fresh GPU buffers, so build a fresh exact
        // organism replacement against the same newly meshed terrain ring.
        // This reads the live publication as-is; F12 never settles or mutates
        // the world, including a legitimately partial tier expansion.
        let mut organisms = PovOrganismManager::new();
        let organisms_changed = organisms.sync(
            map,
            &chunks,
            (camera.pos.x, camera.pos.y),
            pov::pov_fog_end(self.pov_radius),
        );
        cap.apply(&[], &[], organisms_changed.then(|| organisms.upload()));
        let aspect = width as f32 / height.max(1) as f32;
        // Time-frozen like every capture (3d-phase-3-plan.md §4.3), with the
        // live diagnostic toggles applied so the dump shows what the window
        // shows.
        let shadow = pov::shadow_frame(
            camera,
            &chunks,
            organisms.shadow_bounds(),
            pov::shadow_resolution(self.tier),
        );
        let params = pov::frame_params(
            camera,
            aspect,
            self.pov_radius,
            0.0,
            self.controller.pov_toggles(),
            shadow,
        );
        let rgba = cap.snapshot_at_scale(
            &params,
            CLEAR_COLOR,
            self.controller.pov_state().render_scale,
        );
        write_ppm(&dir.join("screenshot.ppm"), &rgba, width, height)?;
        let counts = organisms.counters();
        Ok(format!(
            "screenshot.ppm ({width}x{height} POV, offscreen capture, {} chunks meshed, {} organisms drawn / {} published / {} waiting for ground; {} distance-culled)",
            chunks.len(),
            counts.drawn(),
            counts.published,
            counts.waiting_for_ground,
            counts.distance_culled,
        ))
    }

    /// The `state.txt` body. Plain text, one `[section]` per concern, stable
    /// `key : value` lines — grep-friendly for humans and agents alike.
    fn dump_report(&self, screenshot: &Result<String, String>) -> Result<String, std::fmt::Error> {
        let mut s = String::new();
        let now = unix_seconds();
        writeln!(s, "wer debug dump")?;
        writeln!(s, "created_utc       : {}", utc_iso(now))?;
        writeln!(s, "unix_seconds      : {now}")?;
        writeln!(
            s,
            "algorithm_version : {}",
            world_core::WORLD_ALGORITHM_VERSION
        )?;
        writeln!(
            s,
            "args              : {:?}",
            std::env::args().collect::<Vec<_>>()
        )?;
        writeln!(
            s,
            "cwd               : {}",
            std::env::current_dir().map_or_else(|e| format!("<{e}>"), |p| p.display().to_string())
        )?;
        match screenshot {
            Ok(desc) => writeln!(s, "screenshot        : {desc}")?,
            Err(err) => writeln!(s, "screenshot        : FAILED: {err}")?,
        }
        writeln!(s)?;

        writeln!(s, "[view]")?;
        let layout = self.controller.layout();
        let pov_state = self.controller.pov_state();
        let preferences = self.controller.map_preferences();
        let world = self.controller.world();
        let mode = match layout.mode {
            PresentationMode::Map => "map",
            PresentationMode::Pov if pov_state.walk => "pov (3D walk camera)",
            PresentationMode::Pov => "pov (3D fly camera)",
            PresentationMode::Split => "split (map + POV)",
        };
        writeln!(s, "mode              : {mode}")?;
        let surface = self.renderer.as_ref().map_or_else(
            || String::from("<no renderer>"),
            |r| {
                let (w, h) = r.size();
                format!("{w}x{h}")
            },
        );
        writeln!(s, "surface           : {surface}")?;
        writeln!(s, "tier              : {}", self.tier.name())?;
        writeln!(s, "workers           : {}", self.executor.parallelism())?;
        writeln!(
            s,
            "gpu_compose       : {} (the dump screenshot is always CPU-composed)",
            preferences.backend == MapBackend::GpuAtlas
        )?;
        writeln!(s, "refinement        : {}", preferences.refinement)?;
        writeln!(s, "channel           : {}", preferences.channel.name())?;
        writeln!(s, "zoom              : x{}", preferences.zoom)?;
        let o = preferences.overlays;
        writeln!(
            s,
            "overlays          : grid={} rings={} pinned_flash={} organisms={} discovered={}",
            o.grid, o.rings, o.pinned_flash, o.organisms, o.discovered
        )?;
        writeln!(s)?;

        let traveler = world.traveler();
        let player = traveler.position;
        let (region, feature) = tools::probe_world_position(player.0, player.1);
        writeln!(s, "[position]")?;
        writeln!(s, "player_world      : ({:.3}, {:.3})", player.0, player.1)?;
        writeln!(
            s,
            "last_player       : ({:.3}, {:.3})",
            traveler.previous_position.0, traveler.previous_position.1
        )?;
        writeln!(
            s,
            "region            : x={} y={} level={}",
            region.x, region.y, region.level
        )?;
        let (ox, oy) = region.origin();
        writeln!(s, "region_origin     : ({ox}, {oy})")?;
        writeln!(s, "feature_hash      : {feature:#018x}")?;
        if let Some(hovered) = self.cursor_world() {
            writeln!(
                s,
                "cursor_world      : ({:.3}, {:.3})",
                hovered.0, hovered.1
            )?;
        }
        writeln!(s)?;

        // Camera state is always reported: in POV it *is* the player; in map
        // mode it is whatever the last POV session left behind.
        writeln!(s, "[pov_camera]")?;
        let cam = self.controller.pov_camera();
        let fwd = cam.forward();
        let right = cam.right();
        writeln!(
            s,
            "pos               : ({:.3}, {:.3}, {:.3})  (Z-up; Z = elevation)",
            cam.pos.x, cam.pos.y, cam.pos.z
        )?;
        writeln!(
            s,
            "yaw               : {:.2} deg ({:.5} rad; 0 = +X/east, 90 = +Y/north)",
            cam.yaw.to_degrees(),
            cam.yaw
        )?;
        writeln!(
            s,
            "pitch             : {:.2} deg ({:.5} rad)",
            cam.pitch.to_degrees(),
            cam.pitch
        )?;
        writeln!(
            s,
            "forward           : ({:.5}, {:.5}, {:.5})",
            fwd.x, fwd.y, fwd.z
        )?;
        writeln!(
            s,
            "right             : ({:.5}, {:.5}, {:.5})",
            right.x, right.y, right.z
        )?;
        writeln!(
            s,
            "move_mode         : {}",
            if cam.walk { "walk" } else { "fly" }
        )?;
        writeln!(s, "fly_speed         : {:.1} u/s", cam.speed)?;
        writeln!(s, "walk_speed        : {:.1} u/s", cam.walk_speed)?;
        if cam.walk {
            // The current ground and its source (mesh vs the analytic
            // frontier fallback, 3d-phase-2-plan.md §6.3).
            let (ground, mesh) =
                pov::walk_ground(&self.pov_chunks, world.map(), (cam.pos.x, cam.pos.y));
            writeln!(
                s,
                "ground            : {:.3} ({})",
                ground,
                if mesh { "mesh" } else { "analytic" }
            )?;
        }
        writeln!(s, "chunk_radius      : {} regions", self.pov_radius)?;
        writeln!(s, "resident_chunks   : {}", self.pov_chunks.len())?;
        let toggles = self.controller.pov_toggles();
        writeln!(
            s,
            "toggles           : shadow_ao {}, detail_normals {}, water {}",
            toggles.shadow_ao, toggles.detail_normals, toggles.water
        )?;
        writeln!(s, "render_scale      : {}", pov_state.render_scale)?;
        if layout.mode == PresentationMode::Map {
            writeln!(
                s,
                "note              : map mode — camera state is from the last POV session"
            )?;
        }
        writeln!(s)?;

        writeln!(s, "[steering]")?;
        writeln!(s, "bias              : {:?}", world.bias())?;
        writeln!(s, "transition_mode   : {}", world.transition_mode())?;
        let capture = self.controller.capture_preferences();
        writeln!(
            s,
            "capture           : {} / {:?}",
            capture.category.name(),
            capture.polarity
        )?;
        writeln!(
            s,
            "resonance         : strength {:.3}, {} nodes",
            self.last_stats.resonance_strength, self.last_stats.resonance_nodes
        )?;
        writeln!(s, "anchors           : {}", world.anchors().len())?;
        for (i, anchor) in world.anchors().iter().enumerate() {
            writeln!(s, "  [{i}] {anchor:?}")?;
        }
        writeln!(s)?;

        writeln!(s, "[frame]")?;
        writeln!(s, "fps               : {}", self.fps)?;
        writeln!(s, "update_ms         : {:.2}", self.update_ms)?;
        writeln!(s, "compose_ms        : {:.2}", self.compose_ms)?;
        writeln!(s, "present_ms        : {:.2}", self.render_ms)?;
        writeln!(s, "upload_kb_per_f   : {:.1}", self.upload_kb)?;
        write!(s, "pass_ms           :")?;
        for pass in Pass::ALL {
            write!(s, " {} {:.2}", pass.name(), self.pass_ms[pass.index()])?;
        }
        writeln!(s)?;
        write!(s, "regen_totals      :")?;
        for (layer, total) in self.regen_totals.iter().enumerate() {
            write!(s, " {} {total}", layer_decl(layer as u16).name)?;
        }
        writeln!(s)?;
        writeln!(s, "last_update_stats : {:#?}", self.last_stats)?;
        writeln!(s)?;

        writeln!(s, "[cell_at_player]")?;
        writeln!(s, "{:#?}", Self::sample_cursor(world.map(), player))?;
        match Self::pick_organism(world.map(), player) {
            Some(org) => writeln!(s, "organism within one cell: {org:#?}")?,
            None => writeln!(s, "organism within one cell: none")?,
        }
        writeln!(s)?;

        writeln!(s, "[possibility]  (player region's realized vector)")?;
        match world.map().get(region) {
            Some(state) => {
                writeln!(
                    s,
                    "stability {:.3}  revision {}",
                    state.stability, state.revision
                )?;
                for domain in PossibilityDomain::ALL {
                    writeln!(
                        s,
                        "  {:<12} {:.4}",
                        format!("{domain:?}"),
                        state.current.get(domain)
                    )?;
                }
            }
            None => writeln!(s, "region not resident")?,
        }
        writeln!(s)?;

        writeln!(s, "[layer_dep_hash_chain]  (player region; ADR 0008)")?;
        match world.map().layer_diagnostics(region) {
            Some(layers) => {
                for diag in &layers {
                    let decl = layer_decl(diag.layer);
                    writeln!(
                        s,
                        "  [{}] {:<10} rev {}  {}{}{}  expected {}  stored {}  buckets {:?}",
                        diag.layer,
                        decl.name,
                        decl.algorithm_revision,
                        if diag.is_stale() { "STALE" } else { "fresh" },
                        if diag.dirty { ", dirty" } else { "" },
                        if diag.in_flight { ", in-flight" } else { "" },
                        diag.expected
                            .map_or_else(|| String::from("(not ready)"), |h| format!("{h:#018x}")),
                        diag.stored
                            .map_or_else(|| String::from("(none)"), |h| format!("{h:#018x}")),
                        diag.buckets,
                    )?;
                }
            }
            None => writeln!(s, "  region not resident")?,
        }
        writeln!(s)?;

        writeln!(s, "[world]")?;
        writeln!(s, "stream_config     : {:?}", world.map().config())?;
        writeln!(
            s,
            "field_cache_bytes : {} (ceiling {})",
            self.last_stats.cache_bytes,
            world.map().config().max_field_cache_bytes
        )?;
        writeln!(s, "macro_tiles       : {}", world.map().macro_cache().len())?;
        writeln!(
            s,
            "rosters           : {}",
            world.map().roster_cache().len()
        )?;
        writeln!(s, "organisms         : {}", world.map().organism_count())?;
        writeln!(s, "jobs_in_flight    : {}", world.map().jobs_in_flight())?;
        writeln!(s, "pinned_violations : {}", self.composer.pinned_violations)?;
        writeln!(s)?;

        writeln!(s, "[vault]")?;
        writeln!(
            s,
            "dir               : {}",
            std::env::var("WER_VAULT_DIR").unwrap_or_else(|_| String::from("wer-vault"))
        )?;
        match self.services.vault.as_ref() {
            Some(v) => {
                writeln!(s, "open              : true")?;
                writeln!(s, "discoveries       : {}", v.discoveries().len())?;
                writeln!(s, "routes            : {}", v.routes().len())?;
                writeln!(s, "preserves         : {}", v.preserves().len())?;
                writeln!(s, "seen_regions      : {}", v.seen_count())?;
                writeln!(s, "dirty_records     : {}", v.dirty_records())?;
                writeln!(
                    s,
                    "issues            : {} ({} suppressed)",
                    v.issue_count(),
                    v.suppressed_issue_count()
                )?;
                for issue in v.issues() {
                    writeln!(s, "  - {issue}")?;
                }
            }
            None => writeln!(s, "open              : false (store directory unusable)")?,
        }
        writeln!(s, "last_flush_stats  : {:?}", self.services.vault_stats)?;
        writeln!(s, "path_tracking     : {}", world.path_tracking())?;
        writeln!(s, "route_attraction  : {}", world.route_attraction())?;
        writeln!(s, "route_recording   : {}", world.route_recording())?;
        writeln!(s)?;

        writeln!(s, "[env]")?;
        let mut vars: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| k.starts_with("WER_") || k.starts_with("WGPU_") || k == "RUST_LOG")
            .collect();
        vars.sort();
        if vars.is_empty() {
            writeln!(s, "(no WER_*/WGPU_*/RUST_LOG variables set)")?;
        }
        for (k, v) in vars {
            writeln!(s, "{k}={v}")?;
        }
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_names_match_known_instants() {
        // Fixtures cross-checked against `date -u`: the epoch, a century
        // boundary, and a leap day.
        assert_eq!(utc_stamp(0), "1970-01-01_00-00-00");
        assert_eq!(utc_stamp(946_684_800), "2000-01-01_00-00-00");
        assert_eq!(utc_stamp(1_583_020_799), "2020-02-29_23-59-59");
        assert_eq!(utc_iso(1_752_334_245), "2025-07-12T15:30:45Z");
    }

    #[test]
    fn same_second_dumps_get_distinct_directories() {
        let tmp = std::env::temp_dir().join(format!("wer-dump-dirs-{}", std::process::id()));
        let a = create_dump_dir(&tmp, 1_752_334_245).expect("first dir");
        let b = create_dump_dir(&tmp, 1_752_334_245).expect("second dir");
        assert_eq!(a, tmp.join("2025-07-12_15-30-45"));
        assert_eq!(b, tmp.join("2025-07-12_15-30-45-1"));
        std::fs::remove_dir_all(&tmp).expect("cleanup");
    }

    /// The full map-mode dump against a freshly constructed shell (no
    /// window, no renderer, unsettled map — the dump must cope with all of
    /// that): both files land and the report carries its key sections.
    #[test]
    fn debug_dump_writes_screenshot_and_report() {
        let tmp = std::env::temp_dir().join(format!("wer-dump-smoke-{}", std::process::id()));
        // Keep the test's vault out of the repo tree (no other test reads
        // this variable; World::new consumes it at construction).
        std::env::set_var("WER_VAULT_DIR", tmp.join("vault"));
        let mut app = App::new(true, world_runtime::ResourceTier::Low);
        let dir = app
            .write_debug_dump_into(&tmp.join("dump"))
            .expect("dump succeeds");

        let report = std::fs::read_to_string(dir.join("state.txt")).expect("state.txt");
        for section in [
            "player_world",
            "[view]",
            "[pov_camera]",
            "forward",
            "[steering]",
            "[layer_dep_hash_chain]",
            "[vault]",
            "feature_hash",
        ] {
            assert!(report.contains(section), "report is missing {section:?}");
        }

        let shot = std::fs::read(dir.join("screenshot.ppm")).expect("screenshot.ppm");
        assert!(shot.starts_with(b"P6\n"), "screenshot must be binary PPM");
        std::fs::remove_dir_all(&tmp).expect("cleanup");
    }

    #[test]
    fn debug_dump_propagates_directory_creation_failure() {
        let tmp = std::env::temp_dir().join(format!("wer-dump-failure-{}", std::process::id()));
        std::fs::write(&tmp, b"not a directory").expect("create blocking file");
        let vault = tmp.with_extension("vault");
        std::env::set_var("WER_VAULT_DIR", &vault);
        let mut app = App::new(true, world_runtime::ResourceTier::Low);
        let error = app
            .write_debug_dump_into(&tmp)
            .expect_err("a file cannot contain a dump directory");
        assert!(error.contains("create"));
        drop(app);
        std::fs::remove_file(&tmp).expect("cleanup");
        if vault.exists() {
            std::fs::remove_dir_all(vault).expect("vault cleanup");
        }
    }
}
