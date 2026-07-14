//! The `F12` debug dump: write a snapshot of the running shell into
//! `./dump/<UTC datetime>/` — a screenshot of the active Map, POV, or Split
//! presentation plus `state.txt`, a plain-text report of everything useful
//! for diagnosing a problem after the fact: mode, focus, exact pane/layout
//! rectangles, focused hover, traveler/camera state (position, yaw, pitch,
//! and the forward vector in POV), steering and anchors, frame telemetry, the
//! cell under the player, the covering region's
//! dependency-hash chain (ADR 0008), the vault counters, and the `WER_*`
//! environment.
//!
//! Debug output only, never an input. The map screenshot re-runs the CPU
//! composer (the headless `--screenshot` path) so the pixels are faithful
//! even while the GPU-composed map is active; each visible POV pane goes
//! through the offscreen [`renderer::pov::PovCapture`] (ADR 0021), and Split
//! plus the panel is assembled with the same shared layout used live. The live
//! renderer still exposes no readback of any kind (ADR 0017). The
//! wall-clock directory name is presentation-only and never an identity
//! (ADR 0003 posture); nothing written here feeds back into world state,
//! hashing, or persistence.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use viewer_host::inspect::{pick_map_organism_info, sample_cell};
use viewer_host::layout::{PixelRect, PresentationMode, ViewKind};
use viewer_host::map::MapBackend;
use viewer_host::panel::PanelDocument;
use world_core::layer::layer_decl;
use world_core::PossibilityDomain;
use world_runtime::Pass;

use pov_host::{self as pov, PovChunkManager, PovOrganismManager};

use crate::{App, NativeFrameRects, CLEAR_COLOR};

/// The linear [`CLEAR_COLOR`] encoded for an sRGB screenshot target.
const CLEAR_RGBA8: [u8; 4] = [39, 39, 56, 255];
/// Same final-pass focus color as `renderer::Renderer`.
const FOCUS_RGBA8: [u8; 4] = [72, 190, 255, 255];
const FOCUS_THICKNESS: u32 = 3;

#[derive(Debug, Clone, Copy)]
struct RgbaImage<'a> {
    pixels: &'a [u8],
    size: (u32, u32),
}

fn rgba_len(width: u32, height: u32) -> Result<usize, String> {
    usize::try_from(u64::from(width) * u64::from(height) * 4)
        .map_err(|_| format!("RGBA dimensions {width}x{height} exceed address space"))
}

/// Nearest-neighbor RGBA blit used only to assemble file-bound debug pixels.
fn blit_rgba(
    destination: &mut [u8],
    destination_size: (u32, u32),
    source: &[u8],
    source_size: (u32, u32),
    rectangle: PixelRect,
) -> Result<(), String> {
    let (destination_width, destination_height) = destination_size;
    let (source_width, source_height) = source_size;
    if destination.len() != rgba_len(destination_width, destination_height)? {
        return Err(String::from(
            "destination RGBA length does not match dimensions",
        ));
    }
    if source.len() != rgba_len(source_width, source_height)? {
        return Err(String::from("source RGBA length does not match dimensions"));
    }
    if source_width == 0
        || source_height == 0
        || rectangle.width == 0
        || rectangle.height == 0
        || rectangle.right() > destination_width
        || rectangle.bottom() > destination_height
    {
        return Err(String::from(
            "RGBA blit rectangle or source is empty/out of bounds",
        ));
    }

    for destination_y in 0..rectangle.height {
        let source_y = u32::try_from(
            (2 * u64::from(destination_y) + 1) * u64::from(source_height)
                / (2 * u64::from(rectangle.height)),
        )
        .map_err(|_| String::from("source row exceeds u32"))?;
        for destination_x in 0..rectangle.width {
            let source_x = u32::try_from(
                (2 * u64::from(destination_x) + 1) * u64::from(source_width)
                    / (2 * u64::from(rectangle.width)),
            )
            .map_err(|_| String::from("source column exceeds u32"))?;
            let source_offset = usize::try_from(
                (u64::from(source_y) * u64::from(source_width) + u64::from(source_x)) * 4,
            )
            .map_err(|_| String::from("source pixel offset exceeds address space"))?;
            let x = rectangle.x + destination_x;
            let y = rectangle.y + destination_y;
            let destination_offset =
                usize::try_from((u64::from(y) * u64::from(destination_width) + u64::from(x)) * 4)
                    .map_err(|_| String::from("destination pixel offset exceeds address space"))?;
            destination[destination_offset..destination_offset + 4]
                .copy_from_slice(&source[source_offset..source_offset + 4]);
        }
    }
    Ok(())
}

/// Assemble a file-bound Map/POV/Split diagnostic surface from CPU Map bytes,
/// an ADR 0021 offscreen POV capture, and the native rasterization of the one
/// shared panel document. Destination rectangles come only from the live
/// [`NativeFrameRects`] resolver used by rendering and picking.
fn compose_debug_surface(
    surface_size: (u32, u32),
    rectangles: NativeFrameRects,
    map: Option<RgbaImage<'_>>,
    pov: Option<RgbaImage<'_>>,
    panel: RgbaImage<'_>,
) -> Result<Vec<u8>, String> {
    let mut rgba = vec![0; rgba_len(surface_size.0, surface_size.1)?];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.copy_from_slice(&CLEAR_RGBA8);
    }
    match (map, rectangles.views.map_content) {
        (Some(image), Some(destination)) => blit_rgba(
            &mut rgba,
            surface_size,
            image.pixels,
            image.size,
            destination,
        )?,
        (None, None) => {}
        _ => return Err(String::from("Map image and shared layout disagree")),
    }
    match (pov, rectangles.views.pov_pane) {
        (Some(image), Some(destination)) => blit_rgba(
            &mut rgba,
            surface_size,
            image.pixels,
            image.size,
            destination,
        )?,
        (None, None) => {}
        _ => return Err(String::from("POV image and shared layout disagree")),
    }
    blit_rgba(
        &mut rgba,
        surface_size,
        panel.pixels,
        panel.size,
        rectangles.panel,
    )?;

    if rectangles.views.mode == PresentationMode::Split {
        let focused = rectangles
            .views
            .focus_border(rectangles.views.focused)
            .ok_or_else(|| String::from("Split focus has no visible pane"))?;
        paint_border(
            &mut rgba,
            surface_size,
            focused,
            FOCUS_THICKNESS,
            FOCUS_RGBA8,
        )?;
    }
    Ok(rgba)
}

pub(crate) fn compose_capture_surface(
    surface_size: (u32, u32),
    rectangles: NativeFrameRects,
    map: Option<(&[u8], (u32, u32))>,
    pov: Option<(&[u8], (u32, u32))>,
    panel: (&[u8], (u32, u32)),
) -> Result<Vec<u8>, String> {
    compose_debug_surface(
        surface_size,
        rectangles,
        map.map(|(pixels, size)| RgbaImage { pixels, size }),
        pov.map(|(pixels, size)| RgbaImage { pixels, size }),
        RgbaImage {
            pixels: panel.0,
            size: panel.1,
        },
    )
}

fn paint_border(
    destination: &mut [u8],
    destination_size: (u32, u32),
    rectangle: PixelRect,
    thickness: u32,
    color: [u8; 4],
) -> Result<(), String> {
    if destination.len() != rgba_len(destination_size.0, destination_size.1)?
        || !PixelRect::new(0, 0, destination_size.0, destination_size.1).contains_rect(rectangle)
    {
        return Err(String::from("focus border is outside the debug surface"));
    }
    let thickness = thickness.min(rectangle.width).min(rectangle.height);
    for y in rectangle.y..rectangle.bottom() {
        for x in rectangle.x..rectangle.right() {
            let on_border = x - rectangle.x < thickness
                || rectangle.right() - x <= thickness
                || y - rectangle.y < thickness
                || rectangle.bottom() - y <= thickness;
            if on_border {
                let offset = usize::try_from(
                    (u64::from(y) * u64::from(destination_size.0) + u64::from(x)) * 4,
                )
                .map_err(|_| String::from("focus pixel offset exceeds address space"))?;
                destination[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
    Ok(())
}

fn rect_text(rect: Option<PixelRect>) -> String {
    rect.map_or_else(
        || String::from("<hidden>"),
        |rect| {
            format!(
                "x={} y={} width={} height={}",
                rect.x, rect.y, rect.width, rect.height
            )
        },
    )
}

const fn view_name(view: ViewKind) -> &'static str {
    match view {
        ViewKind::Map => "map",
        ViewKind::Pov => "pov",
    }
}

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
        let panel = self.debug_panel_document();
        let screenshot = self.dump_aligned_screenshot(&dir, &panel);
        let report = self
            .dump_report(&screenshot, &panel)
            .map_err(|e| format!("format report: {e}"))?;
        let path = dir.join("state.txt");
        std::fs::write(&path, report).map_err(|e| format!("write {}: {e}", path.display()))?;
        // A failed screenshot still leaves state.txt behind (with the error
        // recorded in it); surface the failure only after the report landed.
        screenshot?;
        Ok(dir)
    }

    fn debug_frame_rects(&self) -> Option<((u32, u32), NativeFrameRects)> {
        let surface_size = self
            .renderer
            .as_ref()
            .map_or((1024, 768), renderer::Renderer::size);
        let (source_width, source_height) = self.hud.size();
        let panel_width = source_width.checked_sub(source_height)?;
        NativeFrameRects::resolve(
            PixelRect::new(0, 0, surface_size.0, surface_size.1),
            source_height,
            panel_width,
            self.controller.layout(),
        )
        .map(|rectangles| (surface_size, rectangles))
    }

    /// Capture the current presentation with the same physical rectangles as
    /// the live surface. Map pixels intentionally use the shared CPU composer
    /// so they remain inspectable even while GPU atlas composition is active;
    /// POV pixels come only from a file-bound offscreen capture (ADR 0021).
    fn dump_aligned_screenshot(
        &mut self,
        dir: &Path,
        panel: &PanelDocument,
    ) -> Result<String, String> {
        let ((width, height), rectangles) = self
            .debug_frame_rects()
            .ok_or_else(|| String::from("surface is too small for views and information panel"))?;
        let mode = rectangles.views.mode;
        let preferences = self.controller.map_preferences();

        let map_rgba = if mode == PresentationMode::Pov {
            None
        } else {
            let decor = self.build_decor();
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
            Some(self.composer.pixels().to_vec())
        };

        let (pov_rgba, pov_note) = match rectangles.views.pov_pane {
            Some(pane) => {
                let (pixels, note) = self.capture_pov((pane.width, pane.height))?;
                (Some(pixels), Some(note))
            }
            None => (None, None),
        };
        let (panel_rgba, panel_width, panel_height) = {
            let (pixels, panel_width, panel_height, _) =
                self.hud.panel_image_for(panel.revision, &panel.sections);
            (pixels.to_vec(), panel_width, panel_height)
        };
        let map = map_rgba.as_deref().map(|pixels| RgbaImage {
            pixels,
            size: (self.composer.side(), self.composer.side()),
        });
        let pov = pov_rgba
            .as_deref()
            .zip(rectangles.views.pov_pane)
            .map(|(pixels, pane)| RgbaImage {
                pixels,
                size: (pane.width, pane.height),
            });
        let rgba = compose_debug_surface(
            (width, height),
            rectangles,
            map,
            pov,
            RgbaImage {
                pixels: &panel_rgba,
                size: (panel_width, panel_height),
            },
        )?;
        write_ppm(&dir.join("screenshot.ppm"), &rgba, width, height)?;

        let mut details = format!(
            "screenshot.ppm ({width}x{height} {}+panel, focus {}, map {}, POV {})",
            mode.as_str(),
            view_name(rectangles.views.focused),
            rect_text(rectangles.views.map_content),
            rect_text(rectangles.views.pov_pane),
        );
        if mode != PresentationMode::Pov {
            write!(
                details,
                "; CPU Map channel {}, zoom x{}",
                preferences.channel.name(),
                preferences.zoom
            )
            .expect("writing to String cannot fail");
        }
        if let Some(note) = pov_note {
            write!(details, "; {note}").expect("writing to String cannot fail");
        }
        Ok(details)
    }

    /// Offscreen POV pane used by both POV-only and Split dumps. The live
    /// renderer exposes no readback; a fresh deterministic mesh/organism ring
    /// is uploaded into [`renderer::pov::PovCapture`] for this file only.
    fn capture_pov(&self, size: (u32, u32)) -> Result<(Vec<u8>, String), String> {
        let (pov_width, pov_height) = size;
        let mut cap = renderer::pov::PovCapture::new(pov_width, pov_height)
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
        let aspect = pov_width as f32 / pov_height.max(1) as f32;
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
            CLEAR_COLOR,
            0.0,
            self.controller.pov_toggles(),
            shadow,
        );
        let pov_rgba = cap.snapshot_at_scale(
            &params,
            CLEAR_COLOR,
            self.controller.pov_state().render_scale,
        );
        let counts = organisms.counters();
        Ok((
            pov_rgba,
            format!(
                "{pov_width}x{pov_height} offscreen POV, {} chunks meshed, {} organisms drawn / {} published / {} waiting for ground; {} distance-culled",
                chunks.len(),
                counts.drawn(),
                counts.published,
                counts.waiting_for_ground,
                counts.distance_culled,
            ),
        ))
    }

    /// The `state.txt` body. Plain text, one `[section]` per concern, stable
    /// `key : value` lines — grep-friendly for humans and agents alike.
    fn dump_report(
        &self,
        screenshot: &Result<String, String>,
        panel: &PanelDocument,
    ) -> Result<String, std::fmt::Error> {
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

        writeln!(s, "[information_panel]")?;
        writeln!(s, "schema_version    : {}", panel.schema_version)?;
        writeln!(s, "revision          : {}", panel.revision)?;
        for section in &panel.sections {
            for field in section.fields.iter().filter(|field| field.visible) {
                writeln!(s, "{} : {}", field.id.as_str(), field.value)?;
            }
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
        writeln!(s, "mode_id           : {}", layout.mode.as_str())?;
        writeln!(s, "focused           : {}", view_name(layout.focused))?;
        writeln!(s, "split_ratio       : {:.3}", layout.split_ratio)?;
        if let Some(((width, height), rectangles)) = self.debug_frame_rects() {
            writeln!(s, "surface           : {width}x{height}")?;
            writeln!(
                s,
                "combined_rect     : {}",
                rect_text(Some(rectangles.combined))
            )?;
            writeln!(
                s,
                "view_deck_rect    : {}",
                rect_text(Some(rectangles.view_deck))
            )?;
            writeln!(
                s,
                "map_pane_rect     : {}",
                rect_text(rectangles.views.map_pane)
            )?;
            writeln!(
                s,
                "map_content_rect  : {}",
                rect_text(rectangles.views.map_content)
            )?;
            writeln!(
                s,
                "pov_pane_rect     : {}",
                rect_text(rectangles.views.pov_pane)
            )?;
            writeln!(
                s,
                "panel_rect        : {}",
                rect_text(Some(rectangles.panel))
            )?;
            writeln!(
                s,
                "focus_border_rect : {}",
                rect_text(rectangles.views.focus_border(rectangles.views.focused))
            )?;
        } else {
            writeln!(s, "surface           : <layout unavailable>")?;
        }
        writeln!(s, "hover             : {:?}", panel.model.hover)?;
        writeln!(s, "tier              : {}", self.tier.name())?;
        writeln!(s, "workers           : {}", self.executor.parallelism())?;
        writeln!(
            s,
            "gpu_compose       : {} (map dump screenshots are always CPU-composed)",
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
        writeln!(s, "traveler_world    : ({:.3}, {:.3})", player.0, player.1)?;
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
        writeln!(s, "{:#?}", sample_cell(world.map(), player))?;
        match pick_map_organism_info(world.map(), player, preferences.zoom) {
            Some(organism) => writeln!(s, "organism within one cell: {organism:#?}")?,
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

    fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let offset = usize::try_from((u64::from(y) * u64::from(width) + u64::from(x)) * 4)
            .expect("test image offset fits");
        rgba[offset..offset + 4]
            .try_into()
            .expect("complete test pixel")
    }

    #[test]
    fn nearest_blit_samples_destination_pixel_centers() {
        let source = [255, 0, 0, 255, 0, 255, 0, 255];
        let mut destination = vec![0; rgba_len(5, 3).expect("test dimensions")];
        blit_rgba(
            &mut destination,
            (5, 3),
            &source,
            (2, 1),
            PixelRect::new(1, 1, 3, 1),
        )
        .expect("valid blit");

        assert_eq!(pixel(&destination, 5, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&destination, 5, 1, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&destination, 5, 2, 1), [0, 255, 0, 255]);
        assert_eq!(pixel(&destination, 5, 3, 1), [0, 255, 0, 255]);
        assert_eq!(pixel(&destination, 5, 4, 2), [0, 0, 0, 0]);
    }

    #[test]
    fn split_dump_keeps_both_views_panel_focus_and_letterbox_disjoint() {
        let surface_size = (31, 11);
        let rectangles = NativeFrameRects::resolve(
            PixelRect::new(0, 0, surface_size.0, surface_size.1),
            8,
            4,
            viewer_host::layout::ViewLayout {
                mode: PresentationMode::Split,
                focused: ViewKind::Map,
                split_ratio: 0.5,
            },
        )
        .expect("visible panes");
        let map_pixel = [180, 40, 20, 255];
        let pov_pixel = [40, 180, 20, 255];
        let panel_pixel = [20, 80, 180, 255];
        let map_rgba = map_pixel.repeat(64);
        let pov_rgba = pov_pixel.repeat(64);
        let panel_rgba = panel_pixel.repeat(32);

        let composed = compose_debug_surface(
            surface_size,
            rectangles,
            Some(RgbaImage {
                pixels: &map_rgba,
                size: (8, 8),
            }),
            Some(RgbaImage {
                pixels: &pov_rgba,
                size: (8, 8),
            }),
            RgbaImage {
                pixels: &panel_rgba,
                size: (4, 8),
            },
        )
        .expect("valid Split composition");

        let (letterbox_x, letterbox_y) = (0..surface_size.1)
            .find_map(|y| {
                (0..surface_size.0)
                    .find(|&x| !rectangles.combined.contains(x, y))
                    .map(|x| (x, y))
            })
            .expect("aspect mismatch leaves letterbox pixels");
        assert_eq!(
            pixel(&composed, surface_size.0, letterbox_x, letterbox_y),
            CLEAR_RGBA8
        );
        assert_eq!(
            pixel(
                &composed,
                surface_size.0,
                rectangles.views.map_pane.unwrap().x + rectangles.views.map_pane.unwrap().width / 2,
                rectangles.views.map_pane.unwrap().y
                    + rectangles.views.map_pane.unwrap().height / 2,
            ),
            map_pixel
        );
        assert_eq!(
            pixel(
                &composed,
                surface_size.0,
                rectangles.views.pov_pane.unwrap().x + rectangles.views.pov_pane.unwrap().width / 2,
                rectangles.views.pov_pane.unwrap().y
                    + rectangles.views.pov_pane.unwrap().height / 2,
            ),
            pov_pixel
        );
        assert_eq!(
            pixel(
                &composed,
                surface_size.0,
                rectangles.panel.x + rectangles.panel.width / 2,
                rectangles.panel.y + rectangles.panel.height / 2,
            ),
            panel_pixel
        );
        let map_pane = rectangles.views.map_pane.unwrap();
        assert_eq!(
            pixel(&composed, surface_size.0, map_pane.x, map_pane.y),
            FOCUS_RGBA8
        );
        assert_eq!(rectangles.view_deck.right(), rectangles.panel.x);
    }

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
            "traveler_world",
            "[information_panel]",
            "view.mode",
            "[view]",
            "mode_id           : map",
            "focused           : map",
            "map_pane_rect",
            "panel_rect",
            "hover             :",
            "[pov_camera]",
            "forward",
            "[steering]",
            "[layer_dep_hash_chain]",
            "[vault]",
            "feature_hash",
        ] {
            assert!(report.contains(section), "report is missing {section:?}");
        }
        assert!(report.contains("CellInfo {"));
        assert!(report.contains("organism within one cell:"));

        // The state report follows the controller's aligned Split mode and
        // focus and records the exact same pane rectangles as composition.
        let available = app.services.pov_availability(true);
        app.controller.enqueue_service_notification(available);
        app.controller
            .enqueue_action(viewer_host::ViewerAction::SetPresentation(
                PresentationMode::Split,
            ));
        app.controller
            .enqueue_action(viewer_host::ViewerAction::FocusView(ViewKind::Pov));
        let split_tick = app.controller.tick(
            viewer_host::controller::TickInput {
                dt_seconds: 0.0,
                input: viewer_host::input::InputFrame::default(),
                platform: viewer_host::panel::PlatformTelemetry::default(),
            },
            &world_runtime::InlineExecutor,
            &mut app.services,
            &viewer_host::AnalyticGroundSampler,
        );
        app.last_tick_output = Some(split_tick);
        let split_panel = app.debug_panel_document();
        let split_report = app
            .dump_report(&Ok(String::from("split fixture")), &split_panel)
            .expect("format Split report");
        for field in [
            "mode_id           : split",
            "focused           : pov",
            "map_pane_rect     : x=",
            "map_content_rect  : x=",
            "pov_pane_rect     : x=",
            "focus_border_rect : x=",
            "hover             :",
        ] {
            assert!(
                split_report.contains(field),
                "Split report is missing {field:?}"
            );
        }

        // Diagnostic sampling is independent of presentation hover. Even a
        // sky/missing-geometry POV hover must leave the F12 report's player
        // cell diagnostics intact.
        let mut no_hover_panel = app.debug_panel_document();
        no_hover_panel.model.hover = viewer_host::inspect::HoverInfo::None;
        let no_hover_report = app
            .dump_report(&Ok(String::from("fixture")), &no_hover_panel)
            .expect("format no-hover report");
        assert!(no_hover_report.contains("CellInfo {"));
        assert!(no_hover_report.contains("organism within one cell:"));

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
