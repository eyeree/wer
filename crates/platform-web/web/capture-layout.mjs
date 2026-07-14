import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const [url, output] = process.argv.slice(2);
if (!url || !output) {
  throw new Error("usage: node capture-layout.mjs <url> <output.json>");
}

const session = `wer-layout-${process.pid}`;
const cases = [
  { width: 1280, height: 720, dpr: 1 },
  { width: 900, height: 700, dpr: 1 },
  { width: 700, height: 700, dpr: 1 },
];

const agent = (args, options = {}) =>
  execFileSync("agent-browser", ["--session", session, ...args], {
    cwd: process.cwd(),
    encoding: "utf8",
    stdio: options.capture ? ["ignore", "pipe", "pipe"] : ["ignore", "ignore", "inherit"],
  });

const measurements = [];
try {
  for (const viewport of cases) {
    agent(["set", "viewport", String(viewport.width), String(viewport.height), String(viewport.dpr)]);
    agent(["open", url]);
    agent(["wait", "--fn", "document.body.dataset.originFeatureHash !== undefined"]);
    const raw = agent(["--json", "eval", "window.__viewerCharacterization()"], {
      capture: true,
    });
    const response = JSON.parse(raw);
    if (!response.success || !response.data?.result) {
      throw new Error(`agent-browser evaluation failed: ${raw}`);
    }
    measurements.push({
      name: `${viewport.width}x${viewport.height}@${viewport.dpr}`,
      requested: viewport,
      measured: response.data.result,
    });
  }
} finally {
  try {
    agent(["close"]);
  } catch {
    // Preserve the original capture failure; a stale named session is harmless.
  }
}

const fixture = {
  schema: "native-web-alignment-layout-characterization-v1",
  purpose:
    "Milestone 0 pre-alignment geometry evidence; known overflow/stretch defects are intentionally recorded, not accepted behavior.",
  gpuPixelsCaptured: false,
  cases: measurements,
};
const path = resolve(output);
await mkdir(dirname(path), { recursive: true });
await writeFile(path, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(`recorded browser layout characterization: ${path}`);
