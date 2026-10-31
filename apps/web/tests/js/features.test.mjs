import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { FEATURES } from "../../www/features.mjs";

function canonicalIds() {
  const path = fileURLToPath(
    new URL("../../../../crates/core/src/frontend/features.rs", import.meta.url),
  );
  const source = readFileSync(path, "utf8");
  const block = source.match(/pub const FEATURE_IDS: &\[&str\] = &\[([\s\S]*?)\];/);
  assert.ok(block, "the shared feature contract is missing");
  return [...block[1].matchAll(/"([a-z-]+)"/g)].map((match) => match[1]);
}

test("the browser declares exactly the shared feature contract", () => {
  assert.deepEqual(FEATURES, canonicalIds());
});

test("every declared feature id is unique and kebab-case", () => {
  assert.equal(new Set(FEATURES).size, FEATURES.length);
  for (const id of FEATURES) {
    assert.match(id, /^[a-z]+(-[a-z]+)*$/);
  }
});
