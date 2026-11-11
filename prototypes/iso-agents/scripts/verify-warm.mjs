#!/usr/bin/env node
// Verifies the warm typecheck claims against a RUNNING worker:
//   node scripts/verify-warm.mjs                     # changed-guest warm (default localhost:8787)
//   BASE=http://localhost:45069 node scripts/verify-warm.mjs
//   MODE=js node scripts/verify-warm.mjs             # label only
// Expect (TSCHECKER=tsc-rs-wasm held): changed-guest median ~5-15 ms after 2 warmups,
// identical-rerun median ~0.5-2 ms. (typescript@6.0.3 in-isolate: ~5-14 ms / ~5 ms.)

const BASE = process.env.BASE ?? "http://localhost:8787";
const LABEL = process.env.MODE === "js" ? "typescript@6.0.3 (js in-isolate)" : "ts-rust wasm (held)";
const N = 12;

async function typecheck(source) {
  const res = await fetch(`${BASE}/typecheck`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ files: { "/guest.ts": source }, runs: 1 }),
  });
  const json = await res.json();
  if (!json.ok) throw new Error(JSON.stringify(json.error));
  return json.data.results[0];
}

function guest(seed) {
  const lines = [];
  const names = [];
  for (let i = 0; i < 30; i++) {
    names.push(`v${i}_${seed}`);
    lines.push(`export const v${i}_${seed}: number = ${i} * ${seed + 1};`);
  }
  lines.push(`export const total${seed}: number = ${names.join("+")};`);
  return lines.join("\n");
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};

// warmup: first checks pay cold lib parse / JIT warmup — discard
await typecheck(guest(-1));
await typecheck(guest(-2));

const changed = [];
for (let i = 0; i < N; i++) changed.push((await typecheck(guest(i))).ms);

const src = guest(999);
const identical = [];
for (let i = 0; i < 5; i++) identical.push((await typecheck(src)).ms);

const fmt = (xs) => `min ${Math.min(...xs)} / median ${median(xs)} / max ${Math.max(...xs)} ms`;
console.log(`checker:        ${LABEL}`);
console.log(`changed guest   (${N} unique guests): ${fmt(changed)}`);
console.log(`identical rerun (5x same guest):   ${fmt(identical)}`);
