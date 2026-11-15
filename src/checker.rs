//! The TypeScript checker: `ts_rust.wasm` (the Rust port of tsc, see `checker/PROVENANCE.md`) hosted
//! in wasmtime. One instance is held for the process so unchanged files are not re-parsed; any
//! error drops it and the next check starts from a fresh instance of the same compiled module.
//! The instance is also replaced after `CHECKER.RECYCLE_AFTER` runs, and a trapped check is retried
//! once on a fresh instance before it is reported as a `(program)` diagnostic.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use serde::{Deserialize, Serialize};
use wasmtime::{
    Cache, CacheConfig, Caller, Config, Engine, Extern, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc, Val,
};

const WASM: &[u8] = include_bytes!("../checker/ts_rust.wasm");
const GUEST_GLOBALS: &str = include_str!("../checker/guest-globals.d.ts");
const TSCONFIG: &str = include_str!("../checker/tsconfig.json");
const ROOT: &str = "/app";
/// Linear memory one instance may grow to; going past it traps the check.
const MEMORY_LIMIT: usize = 512 << 20;

/// Which stage of hosting the checker failed, so a broken module is reported, not hidden.
#[derive(Debug, thiserror::Error)]
pub enum CheckerError {
    #[error("checker compile: {0}")]
    Compile(String),
    #[error("checker instantiate: {0}")]
    Instantiate(String),
    #[error("checker trap: {0}")]
    Trap(String),
    #[error("checker bad output: {0}")]
    BadOutput(String),
    #[error("checker refused file: {0}")]
    BadFile(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: String,
    /// One-based; -1 for a diagnostic about the project rather than a file.
    pub line: i32,
    pub column: i32,
    pub code: u32,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOutput {
    pub diagnostics: Vec<Diagnostic>,
    /// The emitted JavaScript of every checked file, in path order.
    pub js: Option<String>,
}

/// The hash of the checker a coding goal's results depend on: the module, the guest globals and
/// the tsconfig, each length-prefixed so no two splits of the same bytes collide.
pub fn hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let mut hasher = blake3::Hasher::new();
        for part in [wasm(), GUEST_GLOBALS.as_bytes(), TSCONFIG.as_bytes()] {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
        hasher.finalize().to_hex().to_string()
    })
    .clone()
}

/// Modification stamps are never reused: the checker's parse cache is keyed on them.
static STAMP: AtomicU64 = AtomicU64::new(1);

// ---- the in-memory file system behind ts_host.fs (port of memfs.ts) ----

#[derive(Default)]
struct MemFs {
    files: BTreeMap<String, String>,
    stamps: BTreeMap<String, u64>,
}

impl MemFs {
    fn is_dir(&self, path: &str) -> bool {
        path == "/"
            || self
                .files
                .keys()
                .any(|k| k.starts_with(&format!("{}/", path.trim_end_matches('/'))))
    }

    fn put(&mut self, path: &str, text: String) {
        self.stamps
            .insert(path.to_string(), STAMP.fetch_add(1, Ordering::Relaxed));
        self.files.insert(path.to_string(), text);
    }

    fn remove(&mut self, path: &str) {
        self.files.remove(path);
        self.stamps.remove(path);
    }

    fn stat(&self, path: &str) -> Option<String> {
        if let Some(text) = self.files.get(path) {
            return Some(format!(
                "f {} {}",
                text.len(),
                self.stamps.get(path).copied().unwrap_or(0)
            ));
        }
        self.is_dir(path).then(|| "d 0 0".to_string())
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), String> {
        if self.is_dir(path) {
            return Err(format!("open {path}: is a directory"));
        }
        if let Some(parent) = path
            .match_indices('/')
            .map(|(i, _)| &path[..i])
            .find(|p| self.files.contains_key(*p))
        {
            return Err(format!("mkdir {parent}: not a directory"));
        }
        self.put(path, String::from_utf8_lossy(data).into_owned());
        Ok(())
    }

    /// Bring the files to exactly `wanted`; unchanged files keep their stamp, which is the
    /// checker's parse-cache key, and every write gets a fresh one. The JavaScript emitted for a
    /// source that is still wanted stays: the checker does not emit it again while it is unchanged.
    fn sync(&mut self, wanted: &BTreeMap<String, String>) {
        let emitted_for_wanted = |k: &str| {
            k.strip_suffix(".js")
                .is_some_and(|stem| wanted.contains_key(&format!("{stem}.ts")))
        };
        let stale: Vec<String> = self
            .files
            .keys()
            .filter(|k| !wanted.contains_key(*k) && !emitted_for_wanted(k))
            .cloned()
            .collect();
        stale.iter().for_each(|k| self.remove(k));
        for (path, text) in wanted {
            if self.files.get(path) != Some(text) {
                self.put(path, text.clone());
            }
        }
    }
}

/// What one `ts_host.fs` call answers (only read, stat and write are ever used by the module): not found (-1), an error string (-2-len) or bytes.
enum Reply {
    Missing,
    Failed(String),
    Bytes(Vec<u8>),
}

impl Reply {
    fn text(text: Option<String>) -> Self {
        text.map_or(Reply::Missing, |t| Reply::Bytes(t.into_bytes()))
    }

    fn done(result: Result<(), String>) -> Self {
        result.map_or_else(Reply::Failed, |()| Reply::Bytes(Vec::new()))
    }
}

fn fs_call(fs: &mut MemFs, op: u32, request: &[u8]) -> Reply {
    let text = String::from_utf8_lossy(request).into_owned();
    match op {
        0 => Reply::text(fs.files.get(&text).cloned()),
        1 => Reply::text(fs.stat(&text)),
        4 => {
            let nul = request
                .iter()
                .position(|b| *b == 0)
                .unwrap_or(request.len());
            let path = String::from_utf8_lossy(&request[..nul]).into_owned();
            Reply::done(fs.write(&path, request.get(nul + 1..).unwrap_or_default()))
        }
        _ => Reply::Missing,
    }
}

// ---- the host: imports of the module ----

struct Host {
    fs: MemFs,
    staged: Vec<u8>,
    limits: StoreLimits,
}

fn trap(message: &str) -> wasmtime::Error {
    wasmtime::format_err!("{message}")
}

fn memory(caller: &mut Caller<'_, Host>) -> wasmtime::Result<Memory> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| trap("module has no memory"))
}

fn read_bytes(caller: &mut Caller<'_, Host>, ptr: u32, len: u32) -> wasmtime::Result<Vec<u8>> {
    let memory = memory(caller)?;
    let (start, end) = (ptr as usize, ptr as usize + len as usize);
    memory
        .data(&*caller)
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| trap("read out of bounds"))
}

fn write_bytes(caller: &mut Caller<'_, Host>, ptr: u32, bytes: &[u8]) -> wasmtime::Result<()> {
    let memory = memory(caller)?;
    let start = ptr as usize;
    let target = memory
        .data_mut(&mut *caller)
        .get_mut(start..start + bytes.len());
    target
        .ok_or_else(|| trap("write out of bounds"))?
        .copy_from_slice(bytes);
    Ok(())
}

fn host_fs(mut caller: Caller<'_, Host>, op: u32, ptr: u32, len: u32) -> wasmtime::Result<i32> {
    let request = read_bytes(&mut caller, ptr, len)?;
    let (staged, result) = match fs_call(&mut caller.data_mut().fs, op, &request) {
        // -1 is the module's "not found"; it treats any unparseable answer the same way.
        Reply::Missing => (Vec::new(), u32::MAX as i32),
        Reply::Failed(message) => (message.clone().into_bytes(), -(message.len() as i32) - 2),
        Reply::Bytes(bytes) => {
            let len = bytes.len() as i32;
            (bytes, len)
        }
    };
    caller.data_mut().staged = staged;
    Ok(result)
}

fn host_fs_take(mut caller: Caller<'_, Host>, ptr: u32) -> wasmtime::Result<()> {
    let staged = std::mem::take(&mut caller.data_mut().staged);
    write_bytes(&mut caller, ptr, &staged)
}

// ---- the held instance ----

struct Held {
    store: Store<Host>,
    input: TypedFunc<u32, u32>,
    run: TypedFunc<(), i32>,
    output: TypedFunc<(), u32>,
    output_len: TypedFunc<(), u32>,
    memory: Memory,
    runs: u32,
}

static MODULE: OnceLock<Result<(Engine, Module), String>> = OnceLock::new();
static HELD: Mutex<Option<Held>> = Mutex::new(None);
static RECYCLE_AFTER: OnceLock<u32> = OnceLock::new();
static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();
static MODULE_BYTES: OnceLock<Vec<u8>> = OnceLock::new();
static INSTANCES: AtomicU64 = AtomicU64::new(0);
static CHECKS: AtomicU64 = AtomicU64::new(0);

/// The checker module in use: the file `CHECKER.PATH` names, else the embedded one.
fn wasm() -> &'static [u8] {
    MODULE_BYTES.get().map_or(WASM, Vec::as_slice)
}

/// How many checks one instance serves before a fresh one replaces it (default 50), where the
/// compiled module is cached (default: wasmtime's per-user cache directory) and which module file
/// replaces the embedded one; the first call wins. An unreadable module file is an error.
pub fn configure(
    recycle_after: u32,
    cache: Option<PathBuf>,
    path: Option<PathBuf>,
) -> std::io::Result<()> {
    if let Some(path) = path {
        let _ = MODULE_BYTES.set(std::fs::read(path)?);
    }
    let _ = RECYCLE_AFTER.set(recycle_after);
    if let Some(dir) = cache {
        let _ = CACHE_DIR.set(dir);
    }
    Ok(())
}

/// Checker instances this process has created and checks it has run.
pub fn stats() -> (u64, u64) {
    (
        INSTANCES.load(Ordering::Relaxed),
        CHECKS.load(Ordering::Relaxed),
    )
}

/// An engine that keeps compiled code on disk. A cache that cannot be set up, or an entry that is
/// unreadable, only means the module is compiled again.
fn engine() -> Engine {
    let mut cache = CacheConfig::new();
    if let Some(dir) = CACHE_DIR.get() {
        cache.with_directory(dir);
    }
    let mut config = Config::new();
    if let Ok(cache) = Cache::new(cache) {
        config.cache(Some(cache));
    }
    Engine::new(&config).unwrap_or_default()
}

fn module() -> Result<&'static (Engine, Module), CheckerError> {
    MODULE
        .get_or_init(|| {
            let engine = engine();
            let module = Module::new(&engine, wasm()).map_err(|e| format!("{e:#}"))?;
            Ok((engine, module))
        })
        .as_ref()
        .map_err(|e| CheckerError::Compile(e.clone()))
}

const WASI_EBADF: i32 = 8;

fn linker(engine: &Engine, module: &Module) -> wasmtime::Result<Linker<Host>> {
    let mut linker = Linker::new(engine);
    linker.func_wrap("ts_host", "fs", host_fs)?;
    linker.func_wrap("ts_host", "fs_take", host_fs_take)?;
    for import in module
        .imports()
        .filter(|i| i.module() == "wasi_snapshot_preview1")
    {
        let Some(ty) = import.ty().func().cloned() else {
            continue;
        };
        // Every WASI call only has to succeed: clock, random bytes and environment are left as
        // zeroed memory, so they stay fixed and a check is deterministic. Two differ, as in the
        // iso-agents shim: `proc_exit` is the panic-hook exit, so it traps and execution never
        // continues, and `fd_prestat_get` is EBADF. No test can tell success from ENOSYS for
        // the calls this module never makes, so they are not told apart here.
        let name = import.name();
        let code = match name {
            "proc_exit" => None,
            "fd_prestat_get" => Some(WASI_EBADF),
            _ => Some(0),
        };
        linker.func_new("wasi_snapshot_preview1", name, ty, move |_, _, results| {
            let code = code.ok_or_else(|| trap("proc_exit"))?;
            if let Some(slot) = results.first_mut() {
                *slot = Val::I32(code);
            }
            Ok(())
        })?;
    }
    Ok(linker)
}

fn instantiate() -> Result<Held, CheckerError> {
    let (engine, module) = module()?;
    let fail = |e: wasmtime::Error| CheckerError::Instantiate(format!("{e:#}"));
    let linker = linker(engine, module).map_err(fail)?;
    let host = Host {
        fs: MemFs::default(),
        staged: Vec::new(),
        limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
    };
    let mut store = Store::new(engine, host);
    store.limiter(|host| &mut host.limits);
    let instance = linker.instantiate(&mut store, module).map_err(fail)?;
    INSTANCES.fetch_add(1, Ordering::Relaxed);
    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| CheckerError::Instantiate("no memory export".into()))?;
    Ok(Held {
        input: instance
            .get_typed_func(&mut store, "ts_input")
            .map_err(fail)?,
        run: instance
            .get_typed_func(&mut store, "ts_run")
            .map_err(fail)?,
        output: instance
            .get_typed_func(&mut store, "ts_output")
            .map_err(fail)?,
        output_len: instance
            .get_typed_func(&mut store, "ts_output_len")
            .map_err(fail)?,
        store,
        memory,
        runs: 0,
    })
}

// ---- one check ----

#[derive(Deserialize)]
struct RawPosition {
    line: i32,
    character: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDiagnostic {
    file_name: Option<String>,
    start_position: Option<RawPosition>,
    code: u32,
    text: String,
}

fn map_diagnostic(raw: RawDiagnostic) -> Diagnostic {
    let file = raw.file_name.map_or("(program)".to_string(), |f| {
        f.strip_prefix(ROOT).map_or(f.clone(), str::to_string)
    });
    let (line, column) = raw
        .start_position
        .map_or((-1, -1), |p| (p.line + 1, p.character + 1));
    Diagnostic {
        file,
        line,
        column,
        code: raw.code,
        message: raw.text,
    }
}

fn rooted(name: &str) -> Result<String, CheckerError> {
    let path = format!("/{}", name.trim_start_matches('/'));
    if !path.ends_with(".ts") || path == "/guest-globals.d.ts" {
        return Err(CheckerError::BadFile(path));
    }
    Ok(format!("{ROOT}{path}"))
}

/// The project: tsconfig listing every file, the trusted test globals, then the submitted files.
fn project(files: &[(String, String)]) -> Result<BTreeMap<String, String>, CheckerError> {
    let mut project = BTreeMap::new();
    for (name, text) in files {
        project.insert(rooted(name)?, text.clone());
    }
    project.insert(
        format!("{ROOT}/guest-globals.d.ts"),
        GUEST_GLOBALS.to_string(),
    );
    let mut config: serde_json::Value =
        serde_json::from_str(TSCONFIG).map_err(|e| CheckerError::BadOutput(e.to_string()))?;
    config["files"] = project.keys().cloned().collect::<Vec<_>>().into();
    project.insert(format!("{ROOT}/tsconfig.json"), config.to_string());
    Ok(project)
}

fn trapped(e: wasmtime::Error) -> CheckerError {
    CheckerError::Trap(format!("{e:#}"))
}

fn run(held: &mut Held, project: &BTreeMap<String, String>) -> Result<Vec<u8>, CheckerError> {
    held.store.data_mut().fs.sync(project);
    let request = ["/", "1", "-p", ROOT].join("\0");
    let ptr = held
        .input
        .call(&mut held.store, request.len() as u32)
        .map_err(trapped)?;
    held.memory
        .write(&mut held.store, ptr as usize, request.as_bytes())
        .map_err(|e| trapped(e.into()))?;
    held.run.call(&mut held.store, ()).map_err(trapped)?;
    let start = held.output.call(&mut held.store, ()).map_err(trapped)? as usize;
    let len = held.output_len.call(&mut held.store, ()).map_err(trapped)? as usize;
    let bytes = held.memory.data(&held.store).get(start..start + len);
    bytes
        .map(<[u8]>::to_vec)
        .ok_or_else(|| CheckerError::BadOutput("output out of bounds".into()))
}

fn emitted(fs: &MemFs) -> Option<String> {
    let js: Vec<&str> = fs
        .files
        .iter()
        .filter(|(k, _)| k.ends_with(".js"))
        .map(|(_, v)| v.as_str())
        .collect();
    (!js.is_empty()).then(|| js.join("\n"))
}

fn parse(output: &[u8]) -> Result<Vec<Diagnostic>, CheckerError> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawDiagnostic> =
        serde_json::from_slice(output).map_err(|e| CheckerError::BadOutput(e.to_string()))?;
    let mut diagnostics: Vec<Diagnostic> = raw.into_iter().map(map_diagnostic).collect();
    diagnostics.sort_by(|a, b| (&a.file, a.line, a.column).cmp(&(&b.file, b.line, b.column)));
    Ok(diagnostics)
}

/// Run the project on `held`, or on a fresh instance when there is none.
fn attempt(
    held: Option<Held>,
    project: &BTreeMap<String, String>,
) -> Result<(Held, Vec<u8>), CheckerError> {
    let mut held = match held {
        Some(held) => held,
        None => instantiate()?,
    };
    let output = run(&mut held, project)?;
    Ok((held, output))
}

/// What a check that traps on a fresh instance too reports.
fn trapped_check() -> CheckOutput {
    CheckOutput {
        diagnostics: vec![Diagnostic {
            file: "(program)".into(),
            line: -1,
            column: -1,
            code: 0,
            message: "checker trapped".into(),
        }],
        js: None,
    }
}

/// Type-check `files` (path, text) as one project and emit their JavaScript. An error drops the
/// held instance, so the next call starts fresh; a trap is retried once on a fresh instance.
pub fn check(files: &[(String, String)]) -> Result<CheckOutput, CheckerError> {
    let project = project(files)?;
    let mut slot = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    let recycle_after = *RECYCLE_AFTER.get().unwrap_or(&50);
    let held = slot.take().filter(|held| held.runs < recycle_after);
    let (mut held, output) = match attempt(held, &project) {
        Err(CheckerError::Trap(_)) => match attempt(None, &project) {
            Err(CheckerError::Trap(_)) => return Ok(trapped_check()),
            other => other?,
        },
        other => other?,
    };
    held.runs += 1;
    CHECKS.fetch_add(1, Ordering::Relaxed);
    let diagnostics = parse(&output)?;
    let js = emitted(&held.store.data().fs);
    *slot = Some(held);
    Ok(CheckOutput { diagnostics, js })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checking_the_same_files_again_emits_the_same_js() {
        let files = one("/a.ts", "const x: number = 1;\n");
        let first = check(&files).unwrap();
        assert!(first.js.is_some());
        assert_eq!(check(&files).unwrap(), first);
    }

    #[test]
    fn the_js_of_a_removed_source_is_not_emitted_with_the_next_project() {
        check(&one("/old.ts", "const old1 = 1;\n")).unwrap();
        let js = check(&one("/new.ts", "const new1 = 2;\n"))
            .unwrap()
            .js
            .unwrap();
        assert!(js.contains("new1") && !js.contains("old1"), "{js}");
    }

    #[test]
    fn the_hash_is_a_stable_blake3_hex_digest() {
        assert_eq!(hash(), hash());
        assert_eq!(hash().len(), 64);
        assert!(hash().chars().all(|c| c.is_ascii_hexdigit()));
    }

    fn one(name: &str, text: &str) -> Vec<(String, String)> {
        vec![(name.to_string(), text.to_string())]
    }

    #[test]
    fn a_clean_file_has_no_diagnostics_and_emits_its_js() {
        let src = "export function add(a: number, b: number): number { return a + b; }\n";
        let out = check(&one("/guest.ts", src)).unwrap();
        assert_eq!(out.diagnostics, vec![]);
        assert_eq!(
            out.js.as_deref(),
            Some("export function add(a, b) { return a + b; }\n")
        );
    }

    #[test]
    fn a_type_error_is_reported_at_a_one_based_position() {
        let out = check(&one("/guest.ts", "const x: number = \"a\";\n")).unwrap();
        assert_eq!(out.diagnostics.len(), 1);
        let d = &out.diagnostics[0];
        assert_eq!(
            (d.file.as_str(), d.line, d.column, d.code),
            ("/guest.ts", 1, 7, 2322)
        );
        assert!(
            d.message.contains("not assignable to type 'number'"),
            "{}",
            d.message
        );
    }

    #[test]
    fn a_syntax_error_is_a_ts1xxx_diagnostic() {
        let out = check(&one("/guest.ts", "const = ;\n")).unwrap();
        assert!(
            out.diagnostics
                .iter()
                .any(|d| (1000..2000).contains(&d.code)),
            "{:?}",
            out.diagnostics
        );
    }

    #[test]
    fn a_fixed_file_checks_clean_after_a_broken_one() {
        // Same length on purpose: only a fresh modification stamp tells the checker it changed.
        let broken = check(&one("/guest.ts", "const x: number = \"a\";\n")).unwrap();
        assert!(!broken.diagnostics.is_empty());
        let fixed = check(&one("/guest.ts", "const x: number = 123;\n")).unwrap();
        assert_eq!(fixed.diagnostics, vec![]);
    }

    #[test]
    fn a_file_left_out_of_the_next_check_leaves_no_trace() {
        let both = [
            ("/a.ts", "export const a = 1;\n"),
            ("/b.ts", "export const bbb = 2;\n"),
        ];
        let files: Vec<_> = both
            .iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect();
        assert!(check(&files).unwrap().js.unwrap().contains("bbb"));
        let alone = check(&files[..1]).unwrap();
        assert_eq!(alone.js.as_deref(), Some("export const a = 1;\n"));
    }

    #[test]
    fn diagnostics_are_sorted_by_file_then_line() {
        let files = vec![
            (
                "/b.ts".to_string(),
                "const b: number = \"b\";\n".to_string(),
            ),
            (
                "/a.ts".to_string(),
                "const ok = 1;\nconst a: number = \"a\";\n".to_string(),
            ),
        ];
        let out = check(&files).unwrap();
        let at: Vec<_> = out
            .diagnostics
            .iter()
            .map(|d| (d.file.as_str(), d.line))
            .collect();
        assert_eq!(at, vec![("/a.ts", 2), ("/b.ts", 1)]);
    }

    #[test]
    fn a_non_typescript_or_reserved_path_is_refused() {
        assert!(check(&one("/notes.txt", "x")).is_err());
        assert!(check(&one("/tsconfig.json", "{}")).is_err());
        assert!(check(&one("/guest-globals.d.ts", "")).is_err());
    }

    #[test]
    fn the_test_globals_are_declared() {
        let src =
            "test(\"t\", () => { expect(1).toEqual(1); assert(true); console.log(\"hi\"); });\n";
        assert_eq!(check(&one("/guest.ts", src)).unwrap().diagnostics, vec![]);
    }

    #[test]
    fn a_crashing_check_is_a_program_diagnostic_and_the_next_check_still_works() {
        let src = format!("const x = {}1{};\n", "(".repeat(20_000), ")".repeat(20_000));
        let first = check(&one("/guest.ts", &src)).unwrap();
        assert_eq!(
            first.diagnostics,
            vec![Diagnostic {
                file: "(program)".into(),
                line: -1,
                column: -1,
                code: 0,
                message: "checker trapped".into(),
            }]
        );
        assert_eq!(first.js, None);
        assert_eq!(check(&one("/guest.ts", &src)).unwrap(), first);
        assert_eq!(
            check(&one("/guest.ts", "const y = 1;\n"))
                .unwrap()
                .diagnostics,
            vec![]
        );
    }

    #[test]
    fn diagnostics_are_the_same_after_the_checker_was_recycled() {
        let a = one("/a.ts", "export const a: number = \"x\";\n");
        let first = check(&a).unwrap();
        assert_eq!(first.diagnostics.len(), 1);
        for i in 0..60 {
            let b = one("/b.ts", &format!("export const b{i}: number = {i};\n"));
            assert_eq!(check(&b).unwrap().diagnostics, vec![]);
        }
        assert_eq!(check(&a).unwrap(), first);
    }

    #[test]
    fn files_import_each_other() {
        let files = vec![
            (
                "/lib.ts".to_string(),
                "export const n: number = 1;\n".to_string(),
            ),
            (
                "/main.ts".to_string(),
                "import { n } from \"./lib\";\nconst s: string = n;\n".to_string(),
            ),
        ];
        let out = check(&files).unwrap();
        let at: Vec<_> = out
            .diagnostics
            .iter()
            .map(|d| (d.file.as_str(), d.code))
            .collect();
        assert_eq!(at, vec![("/main.ts", 2322)]);
    }

    #[test]
    fn an_output_that_cannot_be_written_is_a_diagnostic() {
        let files = vec![
            ("/x.ts".to_string(), "export const a = 1;\n".to_string()),
            (
                "/x.js/y.ts".to_string(),
                "export const b = 1;\n".to_string(),
            ),
        ];
        let out = check(&files).unwrap();
        let project_wide: Vec<_> = out.diagnostics.iter().filter(|d| d.line == -1).collect();
        assert_eq!(project_wide.len(), 1, "{:?}", out.diagnostics);
        let d = project_wide[0];
        assert_eq!((d.file.as_str(), d.column), ("(program)", -1));
        assert!(d.message.contains("is a directory"), "{}", d.message);
    }

    #[test]
    fn an_import_of_a_missing_file_is_reported() {
        let out = check(&one(
            "/main.ts",
            "import { n } from \"./nope\";\nexport const m = n;\n",
        ))
        .unwrap();
        let at: Vec<_> = out
            .diagnostics
            .iter()
            .map(|d| (d.file.as_str(), d.line, d.code))
            .collect();
        assert_eq!(at, vec![("/main.ts", 1, 2307)]);
    }
}
