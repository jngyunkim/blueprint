import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";

const root = path.resolve(import.meta.dirname, "..");

test("updater installs and restarts inside one native command", () => {
  const source = readFileSync(path.join(root, "src", "main.ts"), "utf8");
  assert.match(source, /await invoke\("install_update_and_restart"\)/);
  assert.doesNotMatch(source, /downloadAndInstall/);
  assert.doesNotMatch(source, /plugin-process/);
  assert.match(source, /class="update-error"/);
  assert.match(source, /update-error-message/);
});
