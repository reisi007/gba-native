#!/usr/bin/env node
// Build the served browser bundle from the actual ES-module import graph.
//
// The browser app is not bundled by Vite/Webpack: index.html loads app.js as a
// native ES module and wasm-bindgen emits pkg/. This tool is the bundle step:
//
//   1. walk the relative imports of the entry module (wasm-bindgen output is
//      read from the generated pkg/ root and served under pkg/),
//   2. rename every served artifact to include a content hash and rewrite the
//      import specifiers and the wasm reference in the generated glue,
//   3. copy index.html with hashed entry/CSS references, and
//   4. write bundle-manifest.json with the served path and SHA-256 of each file.
//
// A missing import, an import that leaves the source tree, or an import cycle
// is an error: the gate must fail instead of serving a broken module graph.

import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, extname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const IMPORT_PATTERN = /((?:from\s+|import\s*\(\s*|import\s+)["'])(\.{1,2}\/[^"']+)(["'])/g;

export function contentHash(bytes) {
  return createHash("sha256").update(bytes).digest("hex").slice(0, 12);
}

function hashedName(name, hash) {
  const extension = extname(name);
  return `${basename(name, extension)}.${hash}${extension}`;
}

function toPosix(path) {
  return path.split(sep).join("/");
}

function specifier(from, to) {
  const relativePath = toPosix(relative(dirname(from), to));
  return relativePath.startsWith(".") ? relativePath : `./${relativePath}`;
}

function resolveRequest(from, request, generatedRoot) {
  let absolute = resolve(dirname(from), request);
  if (!existsSync(absolute) && request.startsWith("./pkg/")) {
    absolute = join(generatedRoot, request.slice("./pkg/".length));
  }
  if (!existsSync(absolute)) {
    throw new Error(`missing module ${absolute} (imported by ${from})`);
  }
  return absolute;
}

export function resolveModules(entry, source, generated = source) {
  const sourceRoot = resolve(source);
  const generatedRoot = resolve(generated);
  const visited = new Set();
  const order = [];

  const served = (file) => {
    const absolute = resolve(file);
    if (absolute === sourceRoot || absolute.startsWith(sourceRoot + sep)) {
      return toPosix(relative(sourceRoot, absolute));
    }
    if (absolute === generatedRoot || absolute.startsWith(generatedRoot + sep)) {
      return toPosix(join("pkg", relative(generatedRoot, absolute)));
    }
    throw new Error(`module ${absolute} is outside ${sourceRoot} and ${generatedRoot}`);
  };
  const resolveImport = (from, request) => {
    const absolute = resolveRequest(from, request, generatedRoot);
    served(absolute);
    return absolute;
  };

  const visit = (file) => {
    const absolute = resolve(file);
    if (visited.has(absolute)) return;
    visited.add(absolute);
    const sourceText = readFileSync(absolute, "utf8");
    for (const match of sourceText.matchAll(IMPORT_PATTERN)) {
      visit(resolveImport(absolute, match[2]));
    }
    order.push(absolute);
  };
  const entryPath = existsSync(join(sourceRoot, entry))
    ? join(sourceRoot, entry)
    : join(generatedRoot, entry);
  visit(entryPath);
  return order.map((file) => ({ file, served: served(file) }));
}

export function bundle({ source, dist, generated, entry = "app.js" }) {
  const sourceRoot = resolve(source);
  const generatedRoot = resolve(generated ?? join(dist, "pkg"));
  const distRoot = resolve(dist);
  const modules = resolveModules(entry, sourceRoot, generatedRoot);

  // Generated wasm binaries are content-hashed first, because the glue module
  // references them by file name.
  const wasmNames = new Map();
  const assets = [];
  if (existsSync(generatedRoot)) {
    for (const name of readdirSync(generatedRoot)) {
      if (!name.endsWith(".wasm")) continue;
      const file = join(generatedRoot, name);
      const bytes = readFileSync(file);
      const hashed = hashedName(name, contentHash(bytes));
      writeFileSync(join(generatedRoot, hashed), bytes);
      rmSync(file);
      wasmNames.set(name, hashed);
      assets.push({
        path: toPosix(join("pkg", hashed)),
        sha256: createHash("sha256").update(bytes).digest("hex"),
      });
    }
  }

  const emitted = new Map();
  const files = [];
  const generatedToRemove = [];
  for (const module of modules) {
    let text = readFileSync(module.file, "utf8");
    text = text.replace(IMPORT_PATTERN, (match, prefix, request, suffix) => {
      const target = resolveRequest(module.file, request, generatedRoot);
      const dependency = emitted.get(target);
      if (!dependency) {
        throw new Error(`import cycle or unresolved import ${request} in ${module.file}`);
      }
      const from = join(distRoot, module.served);
      const to = join(distRoot, dependency);
      return `${prefix}${specifier(from, to)}${suffix}`;
    });
    for (const [original, hashed] of wasmNames) {
      text = text.split(original).join(hashed);
    }
    const bytes = Buffer.from(text);
    const servedHashed = join(dirname(module.served), hashedName(module.served, contentHash(bytes)));
    const target = join(distRoot, servedHashed);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, bytes);
    if (module.file.startsWith(generatedRoot + sep)) {
      generatedToRemove.push(module.file);
    }
    emitted.set(module.file, servedHashed);
    files.push({ path: servedHashed, sha256: createHash("sha256").update(bytes).digest("hex") });
  }

  for (const file of generatedToRemove) {
    if (existsSync(file)) rmSync(file);
  }

  const entryServed = emitted.get(resolve(join(sourceRoot, entry))) ??
    emitted.get(resolve(join(generatedRoot, entry)));
  if (!entryServed) throw new Error(`entry ${entry} was not emitted`);
  let html = readFileSync(join(sourceRoot, "index.html"), "utf8");
  html = html.replaceAll('src="./app.js"', `src="./${basename(entryServed)}"`);
  html = html.replaceAll("src='./app.js'", `src='./${basename(entryServed)}'`);
  writeFileSync(join(distRoot, "index.html"), html);

  const manifest = { entry: entryServed, modules: files, assets };
  writeFileSync(join(distRoot, "bundle-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

export function hashAsset(dist, asset) {
  const distRoot = resolve(dist);
  const file = join(distRoot, asset);
  const bytes = readFileSync(file);
  const hashed = hashedName(asset, contentHash(bytes));
  writeFileSync(join(distRoot, hashed), bytes);
  rmSync(file);
  const indexPath = join(distRoot, "index.html");
  if (existsSync(indexPath)) {
    const html = readFileSync(indexPath, "utf8").replaceAll(`./${asset}`, `./${hashed}`);
    writeFileSync(indexPath, html);
  }
  const manifestPath = join(distRoot, "bundle-manifest.json");
  if (existsSync(manifestPath)) {
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    manifest.assets = [...(manifest.assets ?? []), {
      path: hashed,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    }];
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  }
  return hashed;
}

function parse(argv) {
  const options = { entry: "app.js" };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const key = flag.replace(/^--/, "");
    if (!["source", "dist", "generated", "entry", "asset"].includes(key)) {
      throw new Error(`unknown argument: ${flag}`);
    }
    const value = argv[index + 1];
    if (!value) throw new Error(`${flag} needs a value`);
    options[key] = value;
    index += 1;
  }
  return options;
}

function main(argv) {
  const options = parse(argv);
  if (!options.dist) throw new Error("usage: bundle.mjs --dist DIR [--source DIR --generated DIR] [--asset FILE]");
  if (options.asset) {
    const hashed = hashAsset(options.dist, options.asset);
    console.log(`asset=${hashed}`);
    return;
  }
  if (!options.source) throw new Error("--source is required when building the bundle");
  const manifest = bundle(options);
  const assets = manifest.assets?.length ?? 0;
  console.log(`bundle modules=${manifest.modules.length} assets=${assets} entry=${manifest.entry}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`bundle: ${error.message}`);
    process.exit(1);
  }
}
