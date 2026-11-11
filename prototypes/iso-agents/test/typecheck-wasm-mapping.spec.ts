import { describe, expect, it } from "vitest";
import {
  mapWasmDiagnostic,
  mapWasmDiagnostics,
  type WasmDiagnostic,
} from "../src/typecheck-wasm/mapping";
import { createMemoryFileSystem, syncFiles } from "../src/typecheck-wasm/memfs";

// Pure-mapping coverage for the tsc-rs-wasm checker (small-module
// discipline — mapping.ts and memfs.ts have zero runtime imports, so this
// spec never drags the 5.1 MB wasm module or the 8.8 MB typescript build
// into the vitest transport; the Phase 2 lesson). The real checker's
// in-workerd behavior is proven by scripts/test-typecheck-wasm-live.mjs
// (plan 06-02).

// The TS2322 shape the spike probe returned for `const n: number = 'one'`
// at /app/guest.ts (06-RESEARCH): startPosition is ZERO-based, category
// 1 = Error.
const TS2322: WasmDiagnostic = {
  fileName: "/app/guest.ts",
  startPosition: { line: 0, character: 6 },
  code: 2322,
  category: 1,
  text: "Type 'string' is not assignable to type 'number'.",
};

describe("wasm diagnostic mapping", () => {
  it("maps the probe's TS2322 to the flat 1-based repo shape at 1:7 /guest.ts", () => {
    expect(mapWasmDiagnostic(TS2322)).toEqual({
      code: 2322,
      message: "Type 'string' is not assignable to type 'number'.",
      line: 1,
      column: 7,
      file: "/guest.ts",
      category: "Error",
    });
  });

  it("maps slugify-style line-2 diagnostics to 2:9", () => {
    const mapped = mapWasmDiagnostic({
      fileName: "/app/guest.ts",
      startPosition: { line: 1, character: 8 },
      code: 2322,
      category: 1,
      text: "Type 'number' is not assignable to type 'string'.",
    });
    expect(mapped.line).toBe(2);
    expect(mapped.column).toBe(9);
    expect(mapped.file).toBe("/guest.ts");
  });

  it("maps config diagnostics (no startPosition/fileName) to the (program) -1/-1 shape", () => {
    expect(mapWasmDiagnostic({ code: 5103, category: 1, text: "Unknown option." })).toEqual({
      code: 5103,
      message: "Unknown option.",
      line: -1,
      column: -1,
      file: "(program)",
      category: "Error",
    });
  });

  it("maps every tsc category number to the ts.DiagnosticCategory name (unknown -> string)", () => {
    expect(mapWasmDiagnostic({ ...TS2322, category: 0 }).category).toBe("Warning");
    expect(mapWasmDiagnostic({ ...TS2322, category: 1 }).category).toBe("Error");
    expect(mapWasmDiagnostic({ ...TS2322, category: 2 }).category).toBe("Message");
    expect(mapWasmDiagnostic({ ...TS2322, category: 3 }).category).toBe("Suggestion");
    expect(mapWasmDiagnostic({ ...TS2322, category: 9 }).category).toBe("9");
  });

  it("sorts mapped diagnostics by file, then line, then column", () => {
    const sorted = mapWasmDiagnostics([
      { ...TS2322, startPosition: { line: 1, character: 0 } },
      { ...TS2322, fileName: "/app/a.ts", startPosition: { line: 0, character: 30 } },
      { ...TS2322, startPosition: { line: 0, character: 2 } },
      { code: 5103, category: 1, text: "config" },
    ]);
    expect(sorted.map((d) => `${d.file}:${d.line}:${d.column}`)).toEqual([
      "(program):-1:-1",
      "/a.ts:1:31",
      "/guest.ts:1:3",
      "/guest.ts:2:1",
    ]);
  });
});

describe("wasm memory filesystem", () => {
  const seed = (): Map<string, string> =>
    new Map([
      ["/app/index.ts", "export const a = 1;\n"],
      ["/app/util/helpers.ts", "export const b = 2;\n"],
    ]);

  it("reads seeded files and returns undefined for misses", () => {
    const fs = createMemoryFileSystem(seed());
    expect(fs.readFile("/app/index.ts")).toBe("export const a = 1;\n");
    expect(fs.readFile("/app/nope.ts")).toBeUndefined();
  });

  it("stats files with byte sizes and synthesizes directory stats", () => {
    const fs = createMemoryFileSystem(seed());
    expect(fs.stat("/app/index.ts")).toEqual({ isDirectory: false, size: 20, mtimeMs: 0 });
    expect(fs.stat("/app")).toEqual({ isDirectory: true, size: 0, mtimeMs: 0 });
    expect(fs.stat("/")).toEqual({ isDirectory: true, size: 0, mtimeMs: 0 });
    expect(fs.stat("/app/nope.ts")).toBeUndefined();
  });

  it("lists immediate directory entries with file/directory kinds (undefined for misses)", () => {
    const fs = createMemoryFileSystem(seed());
    expect(fs.readDirectory("/app")).toEqual([
      { name: "index.ts", kind: "file" },
      { name: "util", kind: "directory" },
    ]);
    expect(fs.readDirectory("/nope")).toBeUndefined();
  });

  it("realpaths existing files and directories only", () => {
    const fs = createMemoryFileSystem(seed());
    expect(fs.realpath("/app/index.ts")).toBe("/app/index.ts");
    expect(fs.realpath("/app/util")).toBe("/app/util");
    expect(fs.realpath("/nope")).toBeUndefined();
  });

  it("writes and appends text, rejecting paths under an existing file", () => {
    const fs = createMemoryFileSystem(seed());
    const bytes = (s: string) => new TextEncoder().encode(s);
    expect(fs.writeFile("/app/out.js", bytes("emit;\n"), false)).toBeUndefined();
    expect(fs.readFile("/app/out.js")).toBe("emit;\n");
    expect(fs.writeFile("/app/out.js", bytes("more;\n"), true)).toBeUndefined();
    expect(fs.readFile("/app/out.js")).toBe("emit;\nmore;\n");
    expect(fs.writeFile("/app/index.ts/nested.js", bytes("x"), false)).toBe(
      "mkdir /app/index.ts: not a directory",
    );
  });

  it("removes files and directory subtrees", () => {
    const fs = createMemoryFileSystem(seed());
    fs.remove("/app/util");
    expect(fs.readFile("/app/util/helpers.ts")).toBeUndefined();
    fs.remove("/app/index.ts");
    expect(fs.readFile("/app/index.ts")).toBeUndefined();
    expect(fs.readDirectory("/app")).toBeUndefined();
  });
});

describe("wasm memfs monotonic mtime stamps (FORK-02)", () => {
  // The fork's MultiRunHost parse cache keys on (path, mtime): a changed
  // file MUST stat a changed mtime or the stale parse is served (07-
  // RESEARCH negative control — exit 0/[] for a guest already parsed
  // clean). These specs pin the host side of that contract.
  it("stamps each write with a strictly increasing mtimeNs", () => {
    const fs = createMemoryFileSystem();
    const bytes = (s: string) => new TextEncoder().encode(s);
    fs.writeFile("/app/guest.ts", bytes("a"), false);
    const first = fs.stat("/app/guest.ts")!.mtimeNs!;
    fs.writeFile("/app/other.ts", bytes("b"), false);
    fs.writeFile("/app/guest.ts", bytes("a2"), false);
    const rewritten = fs.stat("/app/guest.ts")!.mtimeNs!;
    expect(first).toBe(1n);
    expect(rewritten).toBe(3n);
    expect(rewritten > first).toBe(true);
  });

  it("leaves constructor-seeded files unstamped (mtimeMs 0, no mtimeNs)", () => {
    const fs = createMemoryFileSystem(new Map([["/app/seed.ts", "export {};\n"]]));
    const stat = fs.stat("/app/seed.ts")!;
    expect(stat.mtimeMs).toBe(0);
    expect(stat.mtimeNs).toBeUndefined();
    expect(stat).toEqual({ isDirectory: false, size: 11, mtimeMs: 0 });
  });

  it("clears the stamp on remove and issues a fresh one on rewrite", () => {
    const fs = createMemoryFileSystem();
    const bytes = (s: string) => new TextEncoder().encode(s);
    fs.writeFile("/app/guest.ts", bytes("v1"), false);
    fs.remove("/app/guest.ts");
    expect(fs.stat("/app/guest.ts")).toBeUndefined();
    fs.writeFile("/app/guest.ts", bytes("v2"), false);
    expect(fs.stat("/app/guest.ts")!.mtimeNs).toBe(2n);
  });

  it("stamps are stable across reads (stat is pure)", () => {
    const fs = createMemoryFileSystem();
    fs.writeFile("/app/guest.ts", new TextEncoder().encode("x"), false);
    expect(fs.stat("/app/guest.ts")!.mtimeNs).toBe(fs.stat("/app/guest.ts")!.mtimeNs);
  });
});

describe("held-fs bridge sync ordering (v5 H3)", () => {
  const bytes = (s: string) => new TextEncoder().encode(s);

  // The reviewer's repro shape: a previous check's FILE at a path that the
  // next check needs as a PARENT DIRECTORY. Write-first ordering failed the
  // write with "mkdir ...: not a directory" on every retry; remove-first
  // clears the conflict before the write.
  it("removes obsolete paths BEFORE writing replacements (file -> child of that name)", () => {
    const fs = createMemoryFileSystem(new Map([["/app/dir.ts", "previous check's file"]]));
    const order: string[] = [];
    const realRemove = fs.remove.bind(fs);
    const realWrite = fs.writeFile.bind(fs);
    fs.remove = (path) => {
      order.push(`remove ${path}`);
      return realRemove(path);
    };
    fs.writeFile = (path, data, append) => {
      order.push(`write ${path}`);
      return realWrite(path, data, append);
    };
    syncFiles(fs, new Map([["/app/dir.ts/guest.ts", "export {};\n"]]));
    expect(order).toEqual(["remove /app/dir.ts", "write /app/dir.ts/guest.ts"]);
    expect(fs.readFile("/app/dir.ts/guest.ts")).toBe("export {};\n");
    expect(fs.readFile("/app/dir.ts")).toBeUndefined();
  });

  it("brings the fs exactly to the target set in both directions", () => {
    const fs = createMemoryFileSystem(new Map([["/app/guest.ts", "v1"]]));
    syncFiles(fs, new Map([["/app/other.ts", "v2"]]));
    expect([...fs.files.keys()].sort()).toEqual(["/app/other.ts"]);
    syncFiles(fs, new Map([["/app/guest.ts", "v1"]]));
    expect([...fs.files.keys()]).toEqual(["/app/guest.ts"]);
  });

  it("skips unchanged files so their mtime stamps (parse-cache keys) survive", () => {
    const fs = createMemoryFileSystem();
    fs.writeFile("/app/guest.ts", bytes("same"), false);
    const stamped = fs.stat("/app/guest.ts")!.mtimeNs!;
    const order: string[] = [];
    const realWrite = fs.writeFile.bind(fs);
    fs.writeFile = (path, data, append) => {
      order.push(path);
      return realWrite(path, data, append);
    };
    syncFiles(fs, new Map([["/app/guest.ts", "same"], ["/app/new.ts", "x"]]));
    expect(order).toEqual(["/app/new.ts"]);
    expect(fs.stat("/app/guest.ts")!.mtimeNs).toBe(stamped);
  });

  it("throws on a write error, naming the path", () => {
    // A self-contradictory target (file + child under it) still fails the
    // write honestly — after remove-first the conflict is the target's own.
    const fs = createMemoryFileSystem();
    expect(() =>
      syncFiles(fs, new Map([["/app/a.ts", "x"], ["/app/a.ts/nested.ts", "y"]])),
    ).toThrow(/seeding \/app\/a\.ts\/nested\.ts failed: .*not a directory/);
  });
});
