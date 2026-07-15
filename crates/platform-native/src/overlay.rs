//! DOM overlay dock host (docs/wry-overlay/implementation-plan.md M2).
//!
//! Two wry child webviews — the control toolbar strip on top and the
//! information-panel dock on the bottom — render the same shared UI assets
//! as the browser shell (`crates/platform-web/web/assets/ui`), wired through
//! `bridge-ipc.js` instead of the wasm facade. The M0 spike proved X11 cannot
//! alpha-composite a child webview over the parent surface
//! (`docs/wry-overlay/spike-notes.md`), so both webviews are bounded to
//! non-overlapping dock strips and the wgpu deck keeps the space between:
//! the renderer's one acquire/submit/present is untouched and no readback
//! exists (ADR 0017). Semantics stay in `viewer-host` (ADR 0028): upward IPC
//! carries primitive DOM facts and shared action ids; downward pushes carry
//! `viewer_host::dto` JSON and the cached `PanelDocument`.

use std::fmt;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use viewer_host::input::Modifiers;
use viewer_host::PixelRect;

/// Physical height of the toolbar strip webview (two wrapped control rows).
const TOOLBAR_HEIGHT: u32 = 116;
/// Panel dock share of the window height, mirroring the browser shell's
/// default 7:3 viewer/panel row split.
const PANEL_SHARE_NUM: u32 = 3;
const PANEL_SHARE_DEN: u32 = 10;
const PANEL_MIN_HEIGHT: u32 = 200;
/// Panel pushes are floored to this interval; revisions already gate on
/// semantic change, so this only bounds worst-case hover churn.
const PANEL_PUSH_INTERVAL: Duration = Duration::from_millis(100);

/// Static UI assets served through the `wer://` custom protocol. The shared
/// modules are included straight from the platform-web asset tree — one
/// source for both shells is the point of the overlay (plan M1/M2).
/// `WER_UI_DIR=<repo root>` overrides from disk for live editing.
const UI_ASSETS: &[(&str, &str, &str)] = &[
    (
        "assets/app.css",
        "text/css",
        include_str!("../../platform-web/web/assets/app.css"),
    ),
    (
        "assets/bridge-ipc.js",
        "text/javascript",
        include_str!("../../platform-web/web/assets/bridge-ipc.js"),
    ),
    (
        "assets/ui/panel-dock.js",
        "text/javascript",
        include_str!("../../platform-web/web/assets/ui/panel-dock.js"),
    ),
    (
        "assets/ui/toolbar.js",
        "text/javascript",
        include_str!("../../platform-web/web/assets/ui/toolbar.js"),
    ),
    (
        "assets/ui/keys.js",
        "text/javascript",
        include_str!("../../platform-web/web/assets/ui/keys.js"),
    ),
    (
        "assets/ui/diagnostics.js",
        "text/javascript",
        include_str!("../../platform-web/web/assets/ui/diagnostics.js"),
    ),
    (
        "native/toolbar.html",
        "text/html",
        include_str!("../ui/toolbar.html"),
    ),
    (
        "native/panel.html",
        "text/html",
        include_str!("../ui/panel.html"),
    ),
    (
        "native/toolbar.js",
        "text/javascript",
        include_str!("../ui/toolbar.js"),
    ),
    (
        "native/panel.js",
        "text/javascript",
        include_str!("../ui/panel.js"),
    ),
    (
        "native/overlay.css",
        "text/css",
        include_str!("../ui/overlay.css"),
    ),
];

/// One upward message from either overlay page, already decoded.
#[derive(Debug)]
pub enum OverlayEvent {
    /// A page finished booting and wants its initial pushes.
    Ready { pane: OverlayPane },
    /// A toolbar control dispatched a shared action id.
    Action { id: String, value: String },
    /// A window-level key event captured by an overlay page.
    Key {
        code: String,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayPane {
    Toolbar,
    Panel,
}

/// The live overlay: two dock webviews plus the push/pull plumbing.
pub struct OverlayHost {
    toolbar: wry::WebView,
    panel: wry::WebView,
    // Each pane keeps its own web context: separate custom-protocol scheme
    // registries (WebKitGTK registers schemes per context) and separate
    // storage directories (a shared default context makes libsoup fight over
    // one cookie database).
    _toolbar_context: wry::WebContext,
    _panel_context: wry::WebContext,
    events: mpsc::Receiver<OverlayEvent>,
    last_presentation: Option<String>,
    last_panel_revision: Option<u64>,
    last_panel_push: Option<Instant>,
}

impl fmt::Debug for OverlayHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayHost")
            .field("last_panel_revision", &self.last_panel_revision)
            .finish_non_exhaustive()
    }
}

/// Resolve an asset path against `WER_UI_DIR` (repo root) for live editing,
/// falling back to the compiled-in copy.
fn asset_body(path: &str) -> Option<(String, Vec<u8>)> {
    let (_, mime, body) = UI_ASSETS.iter().find(|(name, _, _)| *name == path)?;
    if let Ok(root) = std::env::var("WER_UI_DIR") {
        let disk = if let Some(rest) = path.strip_prefix("assets/") {
            format!("{root}/crates/platform-web/web/assets/{rest}")
        } else {
            format!(
                "{root}/crates/platform-native/ui/{}",
                path.trim_start_matches("native/")
            )
        };
        if let Ok(bytes) = std::fs::read(&disk) {
            return Some(((*mime).to_string(), bytes));
        }
    }
    Some(((*mime).to_string(), body.as_bytes().to_vec()))
}

fn protocol_response(path: &str) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    match asset_body(path) {
        Some((mime, body)) => wry::http::Response::builder()
            .header("Content-Type", mime)
            .body(std::borrow::Cow::Owned(body))
            .expect("static overlay response"),
        None => wry::http::Response::builder()
            .status(404)
            .body(std::borrow::Cow::Owned(Vec::new()))
            .expect("static overlay response"),
    }
}

fn decode_event(body: &str) -> Option<OverlayEvent> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    match value.get("t")?.as_str()? {
        "ready" => Some(OverlayEvent::Ready {
            pane: match value.get("pane")?.as_str()? {
                "toolbar" => OverlayPane::Toolbar,
                "panel" => OverlayPane::Panel,
                _ => return None,
            },
        }),
        "action" => Some(OverlayEvent::Action {
            id: value.get("id")?.as_str()?.to_string(),
            value: value
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
        "key" => Some(OverlayEvent::Key {
            code: value.get("code")?.as_str()?.to_string(),
            pressed: value.get("pressed")?.as_bool()?,
            repeat: value
                .get("repeat")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            modifiers: Modifiers {
                shift: value
                    .get("shift")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                control: value
                    .get("control")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                alt: value.get("alt").and_then(|v| v.as_bool()).unwrap_or(false),
                super_key: value
                    .get("super_key")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            },
        }),
        _ => None,
    }
}

/// The three dock rectangles for a physical surface size.
#[derive(Debug, Clone, Copy)]
struct DockLayout {
    toolbar: wry::Rect,
    panel: wry::Rect,
    deck: PixelRect,
}

fn dock_layout(width: u32, height: u32) -> DockLayout {
    let toolbar_height = TOOLBAR_HEIGHT.min(height / 4).max(1);
    let panel_height = (height * PANEL_SHARE_NUM / PANEL_SHARE_DEN)
        .max(PANEL_MIN_HEIGHT)
        .min(height.saturating_sub(toolbar_height) / 2);
    let deck_top = toolbar_height;
    let deck_height = height
        .saturating_sub(toolbar_height)
        .saturating_sub(panel_height)
        .max(1);
    let rect = |y: u32, h: u32| wry::Rect {
        position: wry::dpi::PhysicalPosition::new(0, y as i32).into(),
        size: wry::dpi::PhysicalSize::new(width, h).into(),
    };
    DockLayout {
        toolbar: rect(0, toolbar_height),
        panel: rect(deck_top + deck_height, panel_height),
        deck: PixelRect::new(0, deck_top, width, deck_height),
    }
}

impl OverlayHost {
    /// Create both dock webviews as children of the winit window. Fails soft:
    /// the caller logs and continues with the bitmap panel + winit input.
    pub fn new(
        window: &std::sync::Arc<winit::window::Window>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        // Child webviews cannot attach to a Wayland surface (M0 finding);
        // fail with an actionable message instead of a wry internal error.
        // `build_event_loop` already prefers X11 for overlay runs, so this
        // only triggers with `WER_FORCE_WAYLAND=1` or without an X server.
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if matches!(
                window.window_handle().map(|handle| handle.as_raw()),
                Ok(RawWindowHandle::Wayland(_))
            ) {
                return Err(
                    "Wayland window: the overlay dock needs X11 (unset WER_FORCE_WAYLAND, \
                     or set WER_OVERLAY=0 for the bitmap panel)"
                        .to_string(),
                );
            }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            // GDK independently prefers Wayland whenever WAYLAND_DISPLAY is
            // set, but the child webviews must attach to the X11 window the
            // shell created (build_event_loop prefers X11 for overlay runs).
            // Restrict GDK before the first gtk::init or wry panics on the
            // backend mismatch.
            gtk::gdk::set_allowed_backends("x11");
            if gtk::init().is_err() {
                return Err("gtk initialization failed".to_string());
            }
        }
        let layout = dock_layout(width, height);
        let (sender, events) = mpsc::channel();
        let toolbar_sender = sender.clone();
        let context_dir = |pane: &str| {
            let dir = std::env::temp_dir().join("wer-overlay").join(pane);
            let _ = std::fs::create_dir_all(&dir);
            dir
        };
        let mut toolbar_context = wry::WebContext::new(Some(context_dir("toolbar")));
        let toolbar = wry::WebViewBuilder::new_with_web_context(&mut toolbar_context)
            .with_custom_protocol("wer".into(), |_id, request| {
                protocol_response(request.uri().path().trim_start_matches('/'))
            })
            .with_url("wer://ui/native/toolbar.html")
            .with_ipc_handler(move |request| {
                if let Some(event) = decode_event(request.body()) {
                    let _ = toolbar_sender.send(event);
                }
            })
            .with_bounds(layout.toolbar)
            .build_as_child(window)
            .map_err(|error| format!("toolbar webview: {error}"))?;
        let mut panel_context = wry::WebContext::new(Some(context_dir("panel")));
        let panel = wry::WebViewBuilder::new_with_web_context(&mut panel_context)
            .with_custom_protocol("wer".into(), |_id, request| {
                protocol_response(request.uri().path().trim_start_matches('/'))
            })
            .with_url("wer://ui/native/panel.html")
            .with_ipc_handler(move |request| {
                if let Some(event) = decode_event(request.body()) {
                    let _ = sender.send(event);
                }
            })
            .with_bounds(layout.panel)
            .build_as_child(window)
            .map_err(|error| format!("panel webview: {error}"))?;
        Ok(Self {
            toolbar,
            panel,
            _toolbar_context: toolbar_context,
            _panel_context: panel_context,
            events,
            last_presentation: None,
            last_panel_revision: None,
            last_panel_push: None,
        })
    }

    /// The wgpu deck rectangle between the dock strips.
    pub fn deck_rect(&self, width: u32, height: u32) -> PixelRect {
        dock_layout(width, height).deck
    }

    pub fn set_bounds(&self, width: u32, height: u32) {
        let layout = dock_layout(width, height);
        let _ = self.toolbar.set_bounds(layout.toolbar);
        let _ = self.panel.set_bounds(layout.panel);
    }

    /// Drain pending upward events (IPC handlers run on the GTK main thread
    /// during the pump; the shell consumes them at frame start).
    pub fn drain_events(&mut self) -> Vec<OverlayEvent> {
        let mut drained = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            drained.push(event);
        }
        drained
    }

    /// Service the platform webview loop. Call every `about_to_wait`.
    pub fn pump(&self) {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let mut drained = 0u32;
            while gtk::events_pending() && drained < 100 {
                gtk::main_iteration_do(false);
                drained += 1;
            }
        }
    }

    /// Initial pushes for a page that announced ready: descriptor tables plus
    /// whatever current state the other pushes would have deduplicated away.
    pub fn push_boot(&mut self, pane: OverlayPane) {
        match pane {
            OverlayPane::Toolbar => {
                let actions = viewer_host::dto::action_descriptors_json();
                let map = viewer_host::dto::map_descriptors_json();
                let script = format!(
                    r#"window.__werOverlayPush({{"kind":"descriptors","actions":{actions},"map":{map}}})"#
                );
                let _ = self.toolbar.evaluate_script(&script);
                if let Some(presentation) = self.last_presentation.take() {
                    self.push_presentation(&presentation);
                }
            }
            OverlayPane::Panel => {
                // Drop the revision gate so the next frame re-pushes.
                self.last_panel_revision = None;
                self.last_panel_push = None;
            }
        }
    }

    /// Push the per-frame presentation DTO to the toolbar when it changed.
    pub fn push_presentation(&mut self, json: &str) {
        if self.last_presentation.as_deref() == Some(json) {
            return;
        }
        let script =
            format!(r#"window.__werOverlayPush({{"kind":"presentation","presentation":{json}}})"#);
        let _ = self.toolbar.evaluate_script(&script);
        self.last_presentation = Some(json.to_string());
    }

    /// Push the shared panel document when its revision advanced (bounded by
    /// [`PANEL_PUSH_INTERVAL`]).
    pub fn push_panel_document(&mut self, revision: u64, json: &str) {
        if self.last_panel_revision == Some(revision) {
            return;
        }
        if let Some(last) = self.last_panel_push {
            if last.elapsed() < PANEL_PUSH_INTERVAL {
                return;
            }
        }
        let script =
            format!(r#"window.__werOverlayPush({{"kind":"panel-document","document":{json}}})"#);
        let _ = self.panel.evaluate_script(&script);
        self.last_panel_revision = Some(revision);
        self.last_panel_push = Some(Instant::now());
    }
}
