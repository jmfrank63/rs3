// Copies the pinned @jitl/quickjs-wasmfile-release-sync WASM artifact into
// src/sandbox/quickjs.wasm. Why a project-local copy: wrangler's default
// CompiledWasm rule does not reach node_modules/ ("No loader is configured
// for .wasm files" — 03-RESEARCH), so the variant's own artifact cannot be
// imported directly. The copy MUST track the package version — the
// PINNED_VERSION check below fails loudly on drift. Idempotent and
// byte-comparing: safe as a postinstall hook (a no-op when current).
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const pkgDir = join(root, "node_modules", "@jitl", "quickjs-wasmfile-release-sync");
const srcWasm = join(pkgDir, "dist", "emscripten-module.wasm");
const outWasm = join(root, "src", "sandbox", "quickjs.wasm");
const PINNED_VERSION = "0.32.0";

if (!existsSync(srcWasm)) {
  console.error(`FATAL: ${srcWasm} missing — run npm install first`);
  process.exit(1);
}
const installedVersion = JSON.parse(readFileSync(join(pkgDir, "package.json"), "utf8")).version;
if (installedVersion !== PINNED_VERSION) {
  console.error(
    `FATAL: @jitl/quickjs-wasmfile-release-sync@${installedVersion} is installed but ` +
      `src/sandbox/quickjs.wasm is pinned to ${PINNED_VERSION}. The copied .wasm must track ` +
      `the package version: bump PINNED_VERSION in scripts/copy-quickjs-wasm.mjs, re-run ` +
      `node scripts/copy-quickjs-wasm.mjs, and re-run the live sandbox harness.`,
  );
  process.exit(1);
}
mkdirSync(dirname(outWasm), { recursive: true });
const srcBytes = readFileSync(srcWasm);
const outBytes = existsSync(outWasm) ? readFileSync(outWasm) : null;
if (outBytes !== null && srcBytes.equals(outBytes)) {
  console.log(`src/sandbox/quickjs.wasm already up to date (${srcBytes.length} bytes)`);
} else {
  writeFileSync(outWasm, srcBytes);
  console.log(`copied ${srcWasm} -> ${outWasm} (${srcBytes.length} bytes)`);
}
