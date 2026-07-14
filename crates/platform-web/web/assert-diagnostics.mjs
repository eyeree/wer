import { execFileSync } from "node:child_process";

const [url] = process.argv.slice(2);
if (!url) throw new Error("usage: node assert-diagnostics.mjs <url>");

const session = `wer-alignment-diagnostics-${process.pid}`;
const tiers = ["low", "mid", "high"];
const modes = ["map", "pov", "split"];
const agent = (args, capture = false) =>
  execFileSync("agent-browser", ["--session", session, ...args], {
    cwd: process.cwd(),
    encoding: "utf8",
    stdio: capture ? ["ignore", "pipe", "pipe"] : ["ignore", "ignore", "inherit"],
  });
const evaluate = (expression) => {
  const response = JSON.parse(agent(["--json", "eval", expression], true));
  if (!response.success) throw new Error(`agent-browser evaluation failed: ${JSON.stringify(response)}`);
  return response.data?.result;
};
const assert = (condition, message) => {
  if (!condition) throw new Error(message);
};

const evidence = [];
try {
  agent(["set", "viewport", "900", "700", "1"]);
  for (const tier of tiers) {
    const tierUrl = new URL(url);
    tierUrl.searchParams.set("tier", tier);
    agent(["open", tierUrl.href]);
    agent([
      "wait",
      "--fn",
      "document.body.dataset.originFeatureHash !== undefined && window.__mapStatus?.presented === true && window.__panelStatus().sections > 0",
    ]);
    const run = evaluate(`(async () => {
      const app = window.__werApp;
      const frame = (dt = 0, time = 1) => JSON.parse(app.frame(dt, time));
      const timedFrame = (dt = 0, time = 1) => {
        const start = performance.now();
        const value = frame(dt, time);
        return { value, wall_ms: performance.now() - start };
      };
      const settle = (mode, limit = 24) => {
        let value;
        let stable = 0;
        for (let index = 0; index < limit; index += 1) {
          value = frame();
          const mapStable = mode !== "map" || (!value.map_dirty && value.map.upload_bytes === 0);
          const povStable = mode === "map" || value.pov.uploads === 0;
          stable = mapStable && povStable && !value.needs_frame ? stable + 1 : 0;
          if (stable >= 1) return { value, frames: index + 1, settled: true };
        }
        return { value, frames: limit, settled: false };
      };
      const stablePanel = () => {
        window.__refreshPanelForTest();
        window.__refreshPanelForTest();
        const before = {
          builds: app.panel_build_count(),
          dom: window.__panelStatus().domUpdates,
        };
        window.__refreshPanelForTest();
        window.__refreshPanelForTest();
        const after = {
          builds: app.panel_build_count(),
          dom: window.__panelStatus().domUpdates,
        };
        return {
          builds: after.builds - before.builds,
          dom_updates: after.dom - before.dom,
        };
      };

      let previous = frame();
      let capabilityOpened = false;
      const results = [];
      for (const mode of ${JSON.stringify(modes)}) {
        // Measure Map on the truthful CPU path first. Headless Chrome has no
        // WebGPU adapter, so open the typed capability gate only when POV is
        // needed; renderer attempt/present status must remain false and GPU
        // delta evidence is explicitly not applicable without a surface.
        if (mode !== "map" && !capabilityOpened) {
          app.renderer_available();
          previous = frame();
          capabilityOpened = true;
        }
        app.action("set-presentation", mode);
        const selected = timedFrame();
        const select_serial_delta = selected.value.update_serial - previous.update_serial;
        const settled = settle(mode);
        previous = settled.value;

        let hover = null;
        if (mode !== "map") {
          const pane = previous.layout.pov_pane;
          const point = [pane[0] + pane[2] / 2, pane[1] + pane[3] / 2];
          app.surface_focus(true);
          app.pointer_move(404, point[0], point[1], "pov");
          const first = frame(0, 2);
          app.pointer_move(404, point[0], point[1], "pov");
          const second = frame(0, 2);
          hover = {
            first_queries: first.pov.hover_queries,
            second_queries: second.pov.hover_queries,
            query_delta: second.pov.hover_queries - first.pov.hover_queries,
            changed_again: second.hover_changed,
          };
          previous = second;
        }

        const unchanged = timedFrame();
        const unchanged_serial_delta = unchanged.value.update_serial - previous.update_serial;
        previous = unchanged.value;
        const panel = stablePanel();
        const adapter = window.__viewerCharacterization().performance;
        results.push({
          mode,
          select_serial_delta,
          unchanged_serial_delta,
          settled: { frames: settled.frames, reached: settled.settled },
          presentation: {
            actual: unchanged.value.presentation.view.mode,
            focused: unchanged.value.presentation.view.focused,
            map_active: unchanged.value.map.active,
            pov_active: unchanged.value.pov.active,
          },
          renderer: unchanged.value.renderer_frame,
          map: {
            path: unchanged.value.map.path,
            dirty: unchanged.value.map_dirty,
            upload_bytes: unchanged.value.map.upload_bytes,
            delta_gate:
              mode === "split" && !unchanged.value.renderer_frame.attempted
                ? "not-applicable-no-surface"
                : mode === "map"
                  ? settled.settled
                    ? "gated-cpu-steady"
                    : "bounded-streaming-sample"
                  : "not-visible",
          },
          hover,
          panel,
          timing: {
            select_wall_ms: selected.wall_ms,
            unchanged_wall_ms: unchanged.wall_ms,
            adapter_update_ms: adapter.updateMs,
            adapter_compose_ms: adapter.composeMs,
            adapter_present_ms: adapter.presentMs,
            adapter_upload_kib: adapter.uploadKib,
          },
        });
      }
      return {
        tier: window.__viewerCharacterization().performance.tier,
        results,
      };
    })()`);

    assert(run.tier === tier, `${tier}: startup tier did not reach the shared world`);
    for (const result of run.results) {
      const expectedMap = result.mode !== "pov";
      const expectedPov = result.mode !== "map";
      assert(result.select_serial_delta === 1, `${tier}/${result.mode}: selection tick was not +1`);
      assert(result.unchanged_serial_delta === 1, `${tier}/${result.mode}: sample tick was not +1`);
      assert(result.presentation.actual === result.mode, `${tier}/${result.mode}: wrong mode`);
      assert(
        result.presentation.map_active === expectedMap && result.presentation.pov_active === expectedPov,
        `${tier}/${result.mode}: wrong active panes`,
      );
      assert(
        result.renderer.attempted === false && result.renderer.presented === false,
        `${tier}/${result.mode}: headless renderer status overclaimed a frame`,
      );
      assert(
        result.panel.builds === 0 && result.panel.dom_updates === 0,
        `${tier}/${result.mode}: unchanged panel rebuilt or mutated`,
      );
      if (result.mode === "map") {
        assert(
          result.map.path === "cpu" && result.map.upload_bytes === 0,
          `${tier}/map: bounded CPU sample reported a GPU upload path`,
        );
        if (result.settled.reached) {
          assert(
            result.map.dirty === false && result.map.delta_gate === "gated-cpu-steady",
            `${tier}/map: quiescent CPU sample redrew unexpectedly`,
          );
        } else {
          assert(
            result.map.delta_gate === "bounded-streaming-sample",
            `${tier}/map: active bounded sample overclaimed steady state`,
          );
        }
      }
      if (result.hover) {
        assert(
          result.hover.query_delta === 0 && result.hover.changed_again === false,
          `${tier}/${result.mode}: stationary POV hover missed its cache`,
        );
      }
      for (const [name, value] of Object.entries(result.timing)) {
        assert(value === null || (Number.isFinite(value) && value >= 0), `${tier}/${result.mode}: invalid ${name}`);
      }
    }
    evidence.push(run);
  }
} finally {
  try {
    agent(["close"]);
  } catch {
    // Preserve the first diagnostic or automation failure.
  }
}

console.log(JSON.stringify({
  schema: "native-web-alignment-local-diagnostics-v1",
  note: "Local wall-clock evidence only; no CI performance thresholds and no headless WebGPU pixels.",
  tiers: evidence,
}, null, 2));
