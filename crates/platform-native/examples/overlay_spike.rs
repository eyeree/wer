//! M0 overlay feasibility spike — docs/wry-overlay/implementation-plan.md §6.
//!
//! Proves the risky physics of the wry-overlay architecture before any shared
//! code changes: a transparent wry child webview layered over the real
//! project `Renderer` (one acquire/submit/present, ADR 0028 frame model),
//! with instrumentation for the M0 questions:
//!
//! 1. compositing — does the transparent DOM layer composite over the wgpu
//!    surface without flicker/z-fights (animated Map underneath makes damage
//!    artifacts visible)?
//! 2. input latency — JS→Rust IPC round trip (immediate pong) and
//!    receive→frame-drain queue latency, p50/p95; DOM `setPointerCapture`
//!    drag semantics inside the webview.
//! 3. presentation-path perturbation — identical frame timing collection with
//!    `SPIKE_OVERLAY=0` (no webview at all) vs `1` for an A/B.
//! 4. GTK pump coexistence with the `ControlFlow::Wait` + redraw-chain pacer.
//! 5. webview startup time and memory overhead (self RSS + WebKit helper
//!    process RSS on Linux).
//!
//! Run: `cargo run -p platform-native --features overlay --example
//! overlay_spike`. Knobs: `SPIKE_OVERLAY=0|1` (default 1),
//! `SPIKE_SECONDS=<n>` (default 0 = until closed), `SPIKE_WINDOW=WxH`
//! (default 1280x800). A summary block prints on exit.
//!
//! This is spike code: it favors direct measurement over polish and is
//! expected to be deleted once M0 findings land in
//! docs/wry-overlay/spike-notes.md.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use renderer::{MapFramePane, MapFrameSource, MultiViewFrame, Renderer, SurfaceViewport};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Width of the translucent DOM panel strip on the right of the test page.
const PANEL_WIDTH: u32 = 320;
/// Side of the animated CPU map texture uploaded every frame.
const MAP_SIDE: u32 = 512;
/// Cap on retained per-frame samples so long runs stay bounded.
const MAX_SAMPLES: usize = 100_000;

const HTML: &str = r##"<!doctype html>
<html><head><meta charset="utf-8"><style>
  html,body{margin:0;height:100%;background:transparent;overflow:hidden;
    font:13px/1.5 system-ui,sans-serif;color:#e8ecf2;}
  #center{position:fixed;inset:0 320px 0 0;}
  #marker{position:absolute;inset:8px;border:1px dashed rgba(255,255,255,.35);
    pointer-events:none;}
  #panel{position:fixed;top:0;right:0;bottom:0;width:320px;
    background:rgba(16,20,28,.85);border-left:1px solid rgba(120,140,180,.4);
    padding:12px;box-sizing:border-box;}
  #stats{white-space:pre;font-family:monospace;font-size:12px;}
  #drag{width:120px;height:80px;background:#3b82f6;border-radius:6px;
    margin:12px 0;display:flex;align-items:center;justify-content:center;
    touch-action:none;user-select:none;cursor:grab;}
  p{font-size:12px;color:#9aa7bd;}
</style></head><body>
<div id="center"><div id="marker"></div></div>
<div id="panel">
  <h3>overlay spike</h3>
  <div id="stats">waiting…</div>
  <div id="drag">drag me</div>
  <p>The dashed region is transparent DOM: the animated wgpu map underneath
     must stay visible and animating. The blue box tests
     setPointerCapture drags. All input lands here and is forwarded to Rust
     over IPC.</p>
</div>
<script>
  const send = m => window.ipc.postMessage(JSON.stringify(m));
  const rtts = [];
  const pending = {};
  let pid = 0, moves = 0, dragging = false;
  window.__werPong = id => {
    const t = pending[id];
    if (t !== undefined) { rtts.push(performance.now() - t); delete pending[id]; }
  };
  const pct = (a, q) => {
    if (!a.length) return 0;
    const s = [...a].sort((x, y) => x - y);
    return s[Math.min(s.length - 1, Math.round((s.length - 1) * q))];
  };
  setInterval(() => { const id = ++pid; pending[id] = performance.now(); send({t:"ping", id}); }, 250);
  setInterval(() => {
    send({t:"rtt", n:rtts.length, p50:+pct(rtts,.5).toFixed(2), p95:+pct(rtts,.95).toFixed(2)});
    document.getElementById("stats").textContent =
      `pings   ${rtts.length}` +
      `\nrtt p50 ${pct(rtts,.5).toFixed(2)} ms` +
      `\nrtt p95 ${pct(rtts,.95).toFixed(2)} ms` +
      `\nmoves   ${moves}`;
  }, 1000);
  window.addEventListener("pointermove", e => { moves++; send({t:"move", x:e.clientX, y:e.clientY, b:e.buttons}); });
  window.addEventListener("keydown", e => send({t:"key", code:e.code}));
  const drag = document.getElementById("drag");
  drag.addEventListener("pointerdown", e => {
    dragging = true; drag.setPointerCapture(e.pointerId);
    send({t:"drag", phase:"down", x:e.clientX, y:e.clientY});
  });
  drag.addEventListener("pointermove", e => {
    if (dragging) send({t:"drag", phase:"move", x:e.clientX, y:e.clientY});
  });
  drag.addEventListener("pointerup", e => {
    dragging = false;
    send({t:"drag", phase:"up", x:e.clientX, y:e.clientY});
  });
  send({t:"ready", ua:navigator.userAgent});
</script>
</body></html>
"##;

/// One upward IPC message with its receive timestamp (for drain latency).
struct IpcMsg {
    received: Instant,
    value: serde_json::Value,
}

struct SpikeConfig {
    overlay: bool,
    seconds: u64,
    width: u32,
    height: u32,
}

impl SpikeConfig {
    fn from_env() -> Self {
        let overlay = std::env::var("SPIKE_OVERLAY")
            .map(|v| v != "0")
            .unwrap_or(true);
        let seconds = std::env::var("SPIKE_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let (width, height) = std::env::var("SPIKE_WINDOW")
            .ok()
            .and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            })
            .unwrap_or((1280, 800));
        Self {
            overlay,
            seconds,
            width,
            height,
        }
    }
}

#[derive(Default)]
struct Stats {
    frame_dt_us: Vec<u64>,
    render_us: Vec<u64>,
    queue_lat_us: Vec<u64>,
    rtt_last: Option<(u64, f64, f64)>,
    moves: u64,
    keys: u64,
    drags: u64,
    pings: u64,
    winit_cursor: u64,
    winit_key: u64,
    winit_button: u64,
    gtk_pump_max: u32,
    webview_create_ms: Option<f64>,
    rss_before_kb: Option<u64>,
    rss_after_kb: Option<u64>,
    resizes: u64,
}

fn push_capped(v: &mut Vec<u64>, sample: u64) {
    if v.len() < MAX_SAMPLES {
        v.push(sample);
    }
}

fn percentile_us(samples: &[u64], q: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)] as f64 / 1000.0
}

/// Current process resident-set size in KiB (Linux; `None` elsewhere).
fn self_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
}

/// Total RSS of WebKit helper processes (network/web processes live outside
/// this process on Linux, so self-RSS alone undercounts the overlay cost).
fn webkit_processes_rss_kb() -> u64 {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .filter(|s| s.chars().all(|c| c.is_ascii_digit()))
        else {
            continue;
        };
        let cmdline = std::fs::read_to_string(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if !cmdline.to_ascii_lowercase().contains("webkit") {
            continue;
        }
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
        if let Some(kb) = status
            .lines()
            .find(|l| l.starts_with("VmRSS:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
        {
            total += kb;
        }
    }
    total
}

/// Fill the animated Map source: a moving diagonal color sweep plus a bright
/// scanline bar, so any compositing stall/flicker under the DOM is obvious.
fn fill_map(rgba: &mut [u8], side: u32, t: f64) {
    let bar = ((t * 0.35).fract() * side as f64) as u32;
    for y in 0..side {
        for x in 0..side {
            let i = ((y * side + x) * 4) as usize;
            let phase = (x + y) as f64 / (side as f64 * 2.0) + t * 0.1;
            let h = phase.fract() * 6.0;
            let (r, g, b) = match h as u32 {
                0 => (255.0, h.fract() * 255.0, 0.0),
                1 => ((1.0 - h.fract()) * 255.0, 255.0, 0.0),
                2 => (0.0, 255.0, h.fract() * 255.0),
                3 => (0.0, (1.0 - h.fract()) * 255.0, 255.0),
                4 => (h.fract() * 255.0, 0.0, 255.0),
                _ => (255.0, 0.0, (1.0 - h.fract()) * 255.0),
            };
            let boost = if y.abs_diff(bar) < 4 { 80 } else { 0 };
            rgba[i] = (r as u32 + boost).min(255) as u8;
            rgba[i + 1] = (g as u32 + boost).min(255) as u8;
            rgba[i + 2] = (b as u32 + boost).min(255) as u8;
            rgba[i + 3] = 255;
        }
    }
}

struct App {
    config: SpikeConfig,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    webview: Rc<RefCell<Option<wry::WebView>>>,
    ipc_rx: Option<mpsc::Receiver<IpcMsg>>,
    map_rgba: Vec<u8>,
    started: Instant,
    last_frame: Option<Instant>,
    positioned: bool,
    stats: Stats,
}

impl App {
    fn new(config: SpikeConfig) -> Self {
        Self {
            config,
            window: None,
            renderer: None,
            webview: Rc::new(RefCell::new(None)),
            ipc_rx: None,
            map_rgba: vec![0; (MAP_SIDE * MAP_SIDE * 4) as usize],
            started: Instant::now(),
            last_frame: None,
            positioned: false,
            stats: Stats::default(),
        }
    }

    fn create_webview(&mut self, window: &Arc<Window>) {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            if gtk::init().is_err() {
                eprintln!("spike: gtk::init failed; running without overlay");
                return;
            }
        }
        self.stats.rss_before_kb = self_rss_kb();
        let (tx, rx) = mpsc::channel();
        self.ipc_rx = Some(rx);
        let pong_target = Rc::clone(&self.webview);
        let handler = move |req: wry::http::Request<String>| {
            let received = Instant::now();
            let Ok(value) = serde_json::from_str::<serde_json::Value>(req.body()) else {
                return;
            };
            // Pings are answered inside the handler so the JS-computed RTT
            // measures pure transport + dispatch, not our frame cadence.
            if value.get("t").and_then(|t| t.as_str()) == Some("ping") {
                if let Some(id) = value.get("id").and_then(|i| i.as_u64()) {
                    if let Some(view) = pong_target.borrow().as_ref() {
                        let _ = view.evaluate_script(&format!("__werPong({id})"));
                    }
                }
            }
            let _ = tx.send(IpcMsg { received, value });
        };
        let size = window.inner_size();
        let creation = Instant::now();
        let built = wry::WebViewBuilder::new()
            .with_transparent(true)
            .with_html(HTML)
            .with_ipc_handler(handler)
            .with_bounds(wry::Rect {
                position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                size: wry::dpi::PhysicalSize::new(size.width, size.height).into(),
            })
            .build_as_child(window);
        match built {
            Ok(view) => {
                self.stats.webview_create_ms = Some(creation.elapsed().as_secs_f64() * 1000.0);
                self.stats.rss_after_kb = self_rss_kb();
                *self.webview.borrow_mut() = Some(view);
                println!(
                    "spike: webview created in {:.1} ms",
                    self.stats.webview_create_ms.unwrap_or_default()
                );
            }
            Err(err) => eprintln!("spike: webview creation failed: {err}"),
        }
    }

    fn drain_ipc(&mut self) {
        let Some(rx) = &self.ipc_rx else { return };
        let now = Instant::now();
        while let Ok(msg) = rx.try_recv() {
            push_capped(
                &mut self.stats.queue_lat_us,
                now.duration_since(msg.received).as_micros() as u64,
            );
            match msg.value.get("t").and_then(|t| t.as_str()) {
                Some("move") => self.stats.moves += 1,
                Some("key") => self.stats.keys += 1,
                Some("ping") => self.stats.pings += 1,
                Some("drag") => {
                    self.stats.drags += 1;
                    if msg.value.get("phase").and_then(|p| p.as_str()) != Some("move") {
                        println!("spike: drag {}", msg.value);
                    }
                }
                Some("rtt") => {
                    self.stats.rtt_last = Some((
                        msg.value.get("n").and_then(|v| v.as_u64()).unwrap_or(0),
                        msg.value.get("p50").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        msg.value.get("p95").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    ));
                }
                Some("ready") => println!("spike: webview ready: {}", msg.value),
                _ => {}
            }
        }
    }

    fn frame(&mut self) {
        self.drain_ipc();
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let Some(window) = self.window.as_ref() else {
            return;
        };
        // The WSLg window manager ignores creation-time position hints and
        // cascades new windows, drifting them off-screen across runs; a
        // post-map move keeps automated screenshots meaningful.
        if !self.positioned {
            window.set_outer_position(winit::dpi::PhysicalPosition::new(60, 60));
            self.positioned = true;
        };
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            push_capped(
                &mut self.stats.frame_dt_us,
                now.duration_since(last).as_micros() as u64,
            );
        }
        self.last_frame = Some(now);

        let t = self.started.elapsed().as_secs_f64();
        fill_map(&mut self.map_rgba, MAP_SIDE, t);
        let size = window.inner_size();
        let side = size
            .width
            .saturating_sub(if self.config.overlay { PANEL_WIDTH } else { 0 })
            .min(size.height)
            .max(1);
        let frame = MultiViewFrame {
            clear: [0.02, 0.02, 0.04, 1.0],
            map: Some(MapFramePane {
                source: MapFrameSource::Cpu {
                    rgba: &self.map_rgba,
                    width: MAP_SIDE,
                    height: MAP_SIDE,
                },
                viewport: SurfaceViewport::new(0, 0, side, side),
                information: None,
            }),
            pov: None,
            focus: None,
        };
        let render_start = Instant::now();
        let _ = renderer.render_frame(frame);
        push_capped(
            &mut self.stats.render_us,
            render_start.elapsed().as_micros() as u64,
        );
        window.request_redraw();
    }

    fn summary(&self) {
        let s = &self.stats;
        let frames = s.frame_dt_us.len();
        let avg_fps = if frames > 0 {
            1_000_000.0 / (s.frame_dt_us.iter().sum::<u64>() as f64 / frames as f64)
        } else {
            0.0
        };
        println!("\n== SPIKE SUMMARY (overlay={}) ==", self.config.overlay);
        println!("frames               {frames} ({avg_fps:.1} fps avg)");
        println!(
            "frame dt ms          p50 {:.2}  p95 {:.2}",
            percentile_us(&s.frame_dt_us, 0.5),
            percentile_us(&s.frame_dt_us, 0.95)
        );
        println!(
            "render_frame ms      p50 {:.2}  p95 {:.2}",
            percentile_us(&s.render_us, 0.5),
            percentile_us(&s.render_us, 0.95)
        );
        println!(
            "ipc queue-lat ms     p50 {:.2}  p95 {:.2}  (n={})",
            percentile_us(&s.queue_lat_us, 0.5),
            percentile_us(&s.queue_lat_us, 0.95),
            s.queue_lat_us.len()
        );
        match s.rtt_last {
            Some((n, p50, p95)) => {
                println!("js rtt ms            p50 {p50:.2}  p95 {p95:.2}  (n={n})")
            }
            None => println!("js rtt ms            (no report received)"),
        }
        println!(
            "dom events           moves {}  keys {}  drags {}  pings {}",
            s.moves, s.keys, s.drags, s.pings
        );
        println!(
            "winit input events   cursor {}  key {}  button {}  (expected ~0 under overlay)",
            s.winit_cursor, s.winit_key, s.winit_button
        );
        println!("gtk pump max drain   {}", s.gtk_pump_max);
        println!("resizes              {}", s.resizes);
        if let Some(ms) = s.webview_create_ms {
            println!("webview create ms    {ms:.1}");
        }
        if let (Some(before), Some(after)) = (s.rss_before_kb, s.rss_after_kb) {
            println!(
                "self rss kb          before {before}  after {after}  (+{})",
                after.saturating_sub(before)
            );
        }
        println!("webkit procs rss kb  {}", webkit_processes_rss_kb());
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // Pinned position so automated WSLg screenshots capture the window.
        let attrs = Window::default_attributes()
            .with_title("wer overlay spike (M0)")
            .with_position(winit::dpi::PhysicalPosition::new(80, 80))
            .with_inner_size(winit::dpi::PhysicalSize::new(
                self.config.width,
                self.config.height,
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("spike: window creation failed: {err}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        let surface_window = Arc::clone(&window);
        let renderer = pollster::block_on(Renderer::new(
            Box::new(move || surface_window.clone().into()),
            size.width,
            size.height,
        ));
        match renderer {
            Ok(renderer) => self.renderer = Some(renderer),
            Err(err) => {
                eprintln!("spike: renderer creation failed: {err}");
                event_loop.exit();
                return;
            }
        }
        if self.config.overlay {
            self.create_webview(&window);
        } else {
            println!("spike: SPIKE_OVERLAY=0 — baseline run without webview");
        }
        window.request_redraw();
        self.window = Some(window);
        self.started = Instant::now();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.stats.resizes += 1;
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                if let Some(view) = self.webview.borrow().as_ref() {
                    let _ = view.set_bounds(wry::Rect {
                        position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                        size: wry::dpi::PhysicalSize::new(size.width, size.height).into(),
                    });
                }
            }
            // Counted to verify the "webview eats all input" assumption: with
            // the overlay covering the window these should stay at ~zero.
            WindowEvent::CursorMoved { .. } => self.stats.winit_cursor += 1,
            WindowEvent::KeyboardInput { .. } => self.stats.winit_key += 1,
            WindowEvent::MouseInput { .. } => self.stats.winit_button += 1,
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // GTK pump (implementation-plan.md §5): wry's WebKitGTK webview is
        // serviced by the GTK main loop, which winit does not drive. Bounded
        // so a flood of GTK work cannot starve the winit loop (M0 item 4).
        #[cfg(all(unix, not(target_os = "macos")))]
        if self.config.overlay {
            let mut drained: u32 = 0;
            while gtk::events_pending() && drained < 100 {
                gtk::main_iteration_do(false);
                drained += 1;
            }
            self.stats.gtk_pump_max = self.stats.gtk_pump_max.max(drained);
        }
        if self.config.seconds > 0
            && self.started.elapsed() >= Duration::from_secs(self.config.seconds)
        {
            event_loop.exit();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(8),
        ));
    }
}

fn main() {
    env_logger::init();
    let config = SpikeConfig::from_env();
    println!(
        "spike: overlay={} seconds={} window={}x{}",
        config.overlay, config.seconds, config.width, config.height
    );
    let event_loop = EventLoop::new().expect("spike: event loop");
    let mut app = App::new(config);
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("spike: event loop error: {err}");
    }
    app.summary();
}
