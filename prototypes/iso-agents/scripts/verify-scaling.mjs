// Scaling A/B: warm changed-guest check vs guest size, per checker mode.
const BASE = process.env.BASE ?? "http://localhost:8787";
const N = 5;

async function check(source) {
  const res = await fetch(`${BASE}/typecheck`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ files: { "/guest.ts": source }, runs: 1 }),
  });
  const json = await res.json();
  if (!json.ok) throw new Error(JSON.stringify(json.error));
  return json.data.results[0].ms;
}

function guest(seed, size) {
  const names = [];
  for (let i = 0; i < size; i++) {
    names.push(`v${i}_${seed}`);
    // a real-ish mix: consts, typed params, string templates
    if (i % 3 === 0) names.push(`f${i}_${seed}(${i}, "${seed}")`);
  }
  const lines = [];
  for (let i = 0; i < size; i++) {
    lines.push(`export const v${i}_${seed}: number = ${i} * ${seed + 1};`);
  }
  lines.push(`export function f${seed}(a: number, b: string): number {`);
  lines.push(`  const parts: number[] = [${names.filter((n) => n.startsWith("v")).join(",")}];`);
  lines.push(`  let t: number = a;`);
  lines.push(`  for (const p of parts) t += p;`);
  lines.push(`  return b.length > 0 ? t : -t;`);
  lines.push(`}`);
  lines.push(`export const total${seed}: number = ${names.filter((n) => n.startsWith("v")).join("+")};`);
  return lines.join("\n");
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
};

await check(guest(-1, 30)); // warmups
await check(guest(-2, 30));

for (const size of [30, 300, 1000, 3000]) {
  const ms = [];
  for (let i = 0; i < N; i++) ms.push(await check(guest(i + size * 1000, size)));
  console.log(`size ${String(size).padStart(5)}: median ${median(ms)} ms  (min ${Math.min(...ms)}, max ${Math.max(...ms)})`);
}
