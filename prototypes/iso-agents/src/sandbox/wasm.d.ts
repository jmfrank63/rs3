// wrangler's default CompiledWasm rule turns a project-local .wasm import
// into a precompiled WebAssembly.Module binding (compiled at bundle time —
// zero runtime codegen, the only workerd-legal shape). No rule is added to
// wrangler.jsonc: CompiledWasm for **/*.wasm is a built-in default, an
// explicit entry only logs a fallback warning, and "type": "Wasm" fails
// wrangler config parsing outright (03-RESEARCH gotcha 12).
declare module "*.wasm" {
  const wasmModule: WebAssembly.Module;
  export default wasmModule;
}
