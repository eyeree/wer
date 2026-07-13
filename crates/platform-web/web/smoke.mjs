import { access, readFile } from "node:fs/promises";
import { join } from "node:path";

const dist = process.argv[2] ?? "target/web-dist";
const required = [
  "index.html",
  "assets/app.css",
  "assets/app.js",
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

JSON.parse(await readFile(join(dist, "assets/manifest.json"), "utf8"));
console.log(`web smoke ok: ${dist}`);
