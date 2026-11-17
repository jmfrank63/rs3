# Provenance of the vendored checker

| File | Origin | Bytes | sha256 | blake3 |
|---|---|---|---|---|
| `ts_rust.wasm` | `prototypes/iso-agents/src/typecheck-wasm/ts_rust.wasm` (copied unchanged) | 5195794 | `e05783c80872375e5f5a578d8f535da44abfa6a9de4c1ac904ebefc43b242a71` | `1cdf060ffde1a1e25881f9b24c57fc48c65104df3e79ae967b0003496fed31db` |
| `guest-globals.d.ts` | `GUEST_GLOBALS_TEXT` in `prototypes/iso-agents/src/typecheck/vfs.ts`, byte for byte | 410 | `dcad9baa064f70eedd7b6653685b02e0938e643dacc9fd6bb65e4c99a060f832` | `e8c06c48f5bd6ba0694926de49ae8f5792adeed5f3ab9196cacf9cefdedd4afd` |
| `tsconfig.json` | The compiler options of `projectTsconfig` in `.../typecheck-wasm/checker.ts` (strict, es2022 target and lib); `files` is filled in per check | 82 | `e2a893489fd64ed9e7908615f4b81f13eaf38c9353cc035e979fd17b1a28a803` | `dc7c46730d402a9abf74e459fbfd6a6e94097c29f1fe1692e7cc0b8f6dfcf5b1` |

`ts_rust.wasm` is `ts_goport`, the Rust port of tsc, built to `wasm32-wasip1` from the multi-run fork
of pingdotgg/ts-rust (branch `multi-run-incr` @ `5ada636`, base pin `569df52`), per
`prototypes/iso-agents/docs/ts-rust-wasm.md`. That document records 5,166,826 bytes and a different
sha256 (`5a2d5914...`) for the fork build, so the committed file was rebuilt or replaced after the
document was written: the exact commit that produced these bytes is unknown. The hashes above are
authoritative for this copy.
