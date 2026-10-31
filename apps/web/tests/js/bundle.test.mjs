import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { bundle, contentHash, hashAsset, resolveModules } from "../../tools/bundle.mjs";

function workspace(files, generated = {}) {
  const root = mkdtempSync(join(tmpdir(), "gba-native-bundle-"));
  const source = join(root, "www");
  const dist = join(root, "dist");
  for (const [name, contents] of Object.entries(files)) {
    mkdirSync(join(source, name, ".."), { recursive: true });
    writeFileSync(join(source, name), contents);
  }
  const pkg = join(dist, "pkg");
  mkdirSync(pkg, { recursive: true });
  for (const [name, contents] of Object.entries(generated)) {
    writeFileSync(join(pkg, name), contents);
  }
  return { root, source, dist, pkg };
}

function hashed(dist, prefix) {
  const name = readdirSync(dist).find((entry) => entry.startsWith(prefix));
  assert.ok(name, `missing hashed ${prefix}* in ${dist}`);
  return name;
}

test("the bundle hashes every module and rewrites its import specifiers", () => {
  const { source, dist, pkg } = workspace({
    "index.html": '<link rel="stylesheet" href="./app.css" /><script type="module" src="./app.js"></script>',
    "app.js": 'import { tone } from "./audio.mjs";\nimport "./feature/extra.mjs";\n',
    "audio.mjs": "export const tone = 1;\n",
    "feature/extra.mjs": "export const extra = 2;\n",
  });
  const manifest = bundle({ source, dist, generated: pkg });

  const entryFile = hashed(dist, "app.");
  assert.match(entryFile, /^app\.[0-9a-f]{12}\.js$/);
  assert.equal(manifest.entry, entryFile);
  const entry = readFileSync(join(dist, entryFile), "utf8");
  assert.match(entry, /from "\.\/audio\.[0-9a-f]{12}\.mjs"/);
  assert.match(entry, /import "\.\/feature\/extra\.[0-9a-f]{12}\.mjs"/);

  const audioFile = hashed(dist, "audio.");
  assert.equal(readFileSync(join(dist, audioFile), "utf8"), "export const tone = 1;\n");
  const extraFile = hashed(join(dist, "feature"), "extra.");
  assert.equal(readFileSync(join(dist, "feature", extraFile), "utf8"), "export const extra = 2;\n");

  const html = readFileSync(join(dist, "index.html"), "utf8");
  assert.ok(html.includes(`src="./${entryFile}"`));
  const manifestFile = JSON.parse(readFileSync(join(dist, "bundle-manifest.json"), "utf8"));
  assert.deepEqual(manifestFile, manifest);
});

test("a generated wasm asset is hashed and patched in the glue module", () => {
  const { source, dist, pkg } = workspace(
    {
      "index.html": '<script type="module" src="./app.js"></script>',
      "app.js": 'import init from "./pkg/glue.js";\n',
    },
    {
      "glue.js": 'new URL("bindings_bg.wasm", import.meta.url);\n',
      "bindings_bg.wasm": "WASM",
    },
  );
  const manifest = bundle({ source, dist, generated: pkg });
  const glue = hashed(join(dist, "pkg"), "glue.");
  const glueText = readFileSync(join(dist, "pkg", glue), "utf8");
  const wasm = hashed(join(dist, "pkg"), "bindings_bg.");
  assert.ok(glueText.includes(wasm));
  assert.ok(manifest.assets.some((asset) => asset.path === `pkg/${wasm}`));
  assert.equal(
    manifest.assets.find((asset) => asset.path === `pkg/${wasm}`).sha256,
    createHash("sha256").update("WASM").digest("hex"),
  );
});

test("a missing import fails instead of shipping a broken page", () => {
  const { source } = workspace({ "app.js": 'import "./missing.mjs";\n' });
  assert.throws(() => resolveModules("app.js", source), /missing module/);
});

test("an import that leaves the source tree is rejected", () => {
  const { source } = workspace({ "app.js": 'import "../../outside.mjs";\n' });
  assert.throws(() => resolveModules("app.js", source), /outside/);
});

test("non-relative imports are ignored by the served-module closure", () => {
  const { source } = workspace({
    "app.js": 'import { thing } from "node:fs";\nimport "./local.mjs";\n',
    "local.mjs": "export const thing = 1;\n",
  });
  assert.deepEqual(
    resolveModules("app.js", source).map((module) => module.served),
    ["local.mjs", "app.js"],
  );
});

test("a CSS asset is hashed, rewritten in index.html and recorded", () => {
  const { dist } = workspace({
    "index.html": '<link rel="stylesheet" href="./app.css" />',
    "app.js": "",
  });
  mkdirSync(dist, { recursive: true });
  writeFileSync(join(dist, "app.css"), "body{color:red}\n");
  writeFileSync(join(dist, "index.html"), '<link rel="stylesheet" href="./app.css" />');
  writeFileSync(join(dist, "bundle-manifest.json"), JSON.stringify({ entry: "app.js", modules: [] }));
  const css = hashAsset(dist, "app.css");
  assert.match(css, /^app\.[0-9a-f]{12}\.css$/);
  assert.ok(readFileSync(join(dist, "index.html"), "utf8").includes(`./${css}`));
  const manifest = JSON.parse(readFileSync(join(dist, "bundle-manifest.json"), "utf8"));
  assert.equal(manifest.assets[0].path, css);
});
