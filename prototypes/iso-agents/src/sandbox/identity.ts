// Boot identity (RUN-02; mirrors the typechecker identity flip): the variant
// id healthz reports after the first sandbox execution. "unloaded" before —
// the QuickJS WASM module loads lazily on first execute (~5ms), so /healthz
// stays fast and flips only after the sandbox is genuinely used.
export const SANDBOX_IDENTITY = "quickjs-wasmfile-release-sync@0.32.0";

let loadedOnce = false;

// Called by the runtime once the QuickJS WASM module has actually loaded.
export function markSandboxLoaded(): void {
  loadedOnce = true;
}

export function getSandboxIdentity(): string {
  return loadedOnce ? SANDBOX_IDENTITY : "unloaded";
}
