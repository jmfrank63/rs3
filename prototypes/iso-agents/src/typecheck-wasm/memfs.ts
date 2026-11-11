// In-memory filesystem behind ts_rust.wasm's ts_host.fs bridge (WASM-01): a
// TypeScript port of pingdotgg/ts-rust@569df52 npm/wasm/core.js's
// memoryFileSystem. PURE module — zero imports — so vitest specs exercise it
// directly instead of dragging the 5.1 MB wasm module into the worker
// transport (Phase 2 lesson). Two lifetimes: a fresh fs per runTsc call
// (dies with the instance), and the HELD fs (FORK-02) that lives as long as
// the held instance — where every writeFile/remove stamps a fresh monotonic
// mtimeNs, the fork's parse-cache key (constant mtimes serve STALE parses —
// 07-RESEARCH negative control).
export interface MemFsStat {
  isDirectory: boolean;
  isFile?: boolean;
  size: number;
  mtimeMs?: number;
  mtimeNs?: bigint;
}

export interface MemFsEntry {
  name: string;
  kind: "file" | "directory" | "symlink" | "other";
}

export interface MemoryFileSystem {
  files: Map<string, string>;
  readFile(path: string): string | undefined;
  stat(path: string): MemFsStat | undefined;
  readDirectory(path: string): MemFsEntry[] | undefined;
  realpath(path: string): string | undefined;
  // Error string on failure, undefined on success (the shim's writeResult
  // maps both onto the fs-bridge protocol).
  writeFile(path: string, data: Uint8Array, append: boolean): string | undefined;
  remove(path: string): string | undefined;
  // Optional: the check filesystem never implements it; absence is a
  // success no-op (core.js contract — a read-only check never chtimes).
  chtimes?(path: string, atimeNs?: bigint, mtimeNs?: bigint): string | undefined;
}

const ENCODER = new TextEncoder();
const FILE_DECODER = new TextDecoder("utf-8", { ignoreBOM: true, fatal: false });

export function createMemoryFileSystem(files: Map<string, string> = new Map()): MemoryFileSystem {
  const map = files instanceof Map ? files : new Map(Object.entries(files) as [string, string][]);
  // Monotonic per-write stamp source + per-path stamps. BigInt steps, never
  // reset for the fs's lifetime. Constructor-seeded files carry NO stamp —
  // they stat mtimeMs: 0 exactly as before (the fresh path never re-parses
  // within one instance, so it has nothing to go stale).
  let tick = 0n;
  const stamps = new Map<string, bigint>();
  const dirPrefix = (path: string): string => (path.endsWith("/") ? path : `${path}/`);
  const isDir = (path: string): boolean => {
    if (path === "/") return true;
    const prefix = dirPrefix(path);
    for (const key of map.keys()) {
      if (key.startsWith(prefix)) return true;
    }
    return false;
  };
  return {
    files: map,
    readFile: (path) => map.get(path),
    stat(path) {
      const text = map.get(path);
      if (text !== undefined) {
        const stamp = stamps.get(path);
        return {
          isDirectory: false,
          size: ENCODER.encode(text).length,
          mtimeMs: 0,
          ...(stamp !== undefined ? { mtimeNs: stamp } : {}),
        };
      }
      return isDir(path) ? { isDirectory: true, size: 0, mtimeMs: 0 } : undefined;
    },
    readDirectory(path) {
      const prefix = dirPrefix(path);
      const entries = new Map<string, MemFsEntry["kind"]>();
      for (const key of map.keys()) {
        if (!key.startsWith(prefix)) continue;
        const rest = key.slice(prefix.length);
        const slash = rest.indexOf("/");
        const name = slash < 0 ? rest : rest.slice(0, slash);
        if (!entries.has(name)) entries.set(name, slash < 0 ? "file" : "directory");
      }
      return entries.size || isDir(path)
        ? [...entries].map(([name, kind]) => ({ name, kind }))
        : undefined;
    },
    realpath: (path) => (map.has(path) || isDir(path) ? path : undefined),
    writeFile(path, data, append) {
      if (isDir(path)) return `open ${path}: is a directory`;
      for (let slash = path.indexOf("/"); slash >= 0; slash = path.indexOf("/", slash + 1)) {
        const parent = path.slice(0, slash);
        if (map.has(parent)) return `mkdir ${parent}: not a directory`;
      }
      const text = FILE_DECODER.decode(data);
      map.set(path, append ? (map.get(path) ?? "") + text : text);
      stamps.set(path, ++tick);
      return undefined;
    },
    remove(path) {
      const prefix = dirPrefix(path);
      for (const key of [...map.keys()]) {
        if (key === path || key.startsWith(prefix)) {
          map.delete(key);
          stamps.delete(key);
        }
      }
      return undefined;
    },
  };
}

// Held-fs bridge sync (v5 H3): bring `fs` EXACTLY to `files`. Obsolete paths
// are removed BEFORE replacements are written — the old write-first order
// left the previous check's conflicts in place during the writes, so a file
// that became a parent directory ("dir.ts" -> "dir.ts/guest.ts") failed with
// "mkdir ...: not a directory" on EVERY subsequent check, and the throw
// escaped before the instance could be recycled. Unchanged paths are
// skipped so their monotonic mtimeNs stamps (the fork's parse-cache key)
// survive untouched. Throws on the first write error (the shim's held-run
// catch turns that into a full held-state drop).
export function syncFiles(fs: MemoryFileSystem, files: Map<string, string>): void {
  for (const path of [...fs.files.keys()]) {
    if (!files.has(path)) {
      fs.remove(path);
    }
  }
  for (const [path, content] of files) {
    if (fs.readFile(path) !== content) {
      const err = fs.writeFile(path, ENCODER.encode(content), false);
      if (err !== undefined) {
        throw new Error(`seeding ${path} failed: ${err}`);
      }
    }
  }
}
