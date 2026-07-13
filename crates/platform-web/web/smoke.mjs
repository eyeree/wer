import { access, readFile } from "node:fs/promises";
import { join } from "node:path";

const dist = process.argv[2] ?? "target/web-dist";
const required = [
  "index.html",
  "help/index.html",
  "docs/world-model.html",
  "assets/app.css",
  "assets/app.js",
  "assets/commands.js",
  "assets/manifest.json",
  "generated/platform_web.js",
  "generated/platform_web_bg.wasm",
];

for (const path of required) {
  await access(join(dist, path));
}

const html = await readFile(join(dist, "index.html"), "utf8");
for (const url of ["./assets/app.css", "./assets/app.js", "./docs/world-model.html", "./help/"]) {
  if (!html.includes(url)) {
    throw new Error(`index.html does not contain relative URL ${url}`);
  }
}

const app = await readFile(join(dist, "assets/app.js"), "utf8");
if (/https?:\/\//.test(app)) {
  throw new Error("app.js contains an external network URL");
}
if (!app.includes("origin_feature_hash")) {
  throw new Error("app.js does not call the origin feature hash parity export");
}

const docs = await readFile(join(dist, "docs/world-model.html"), "utf8");
for (const heading of ["World Model", "Possibility", "Terrain"]) {
  if (!docs.includes(heading)) {
    throw new Error(`generated world-model docs missing expected text ${heading}`);
  }
}

const commands = await readFile(join(dist, "assets/commands.js"), "utf8");
const help = await readFile(join(dist, "help/index.html"), "utf8");
for (const match of commands.matchAll(/id: "([^"]+)"/g)) {
  if (!help.includes(`data-help-command="${match[1]}"`)) {
    throw new Error(`help page missing command ${match[1]}`);
  }
}

JSON.parse(await readFile(join(dist, "assets/manifest.json"), "utf8"));
console.log(`web smoke ok: ${dist}`);
