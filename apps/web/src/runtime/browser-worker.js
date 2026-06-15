const WASM_ASSET_VERSION = '20260615-current-core';

let wasmPromise = null;

self.onmessage = async (event) => {
  const { id, type, payloadJson } = event.data ?? {};
  if (type !== 'calculate') {
    return;
  }

  try {
    const wasm = await loadWasm();
    const resultJson = wasm.calculateFromStaticData(payloadJson);
    self.postMessage({ id, ok: true, resultJson });
  } catch (error) {
    self.postMessage({
      id,
      ok: false,
      error: error?.message ?? String(error),
    });
  }
};

function loadWasm() {
  wasmPromise ??= import(`../../pkg/bangdream_optimize_web_wasm.js?v=${WASM_ASSET_VERSION}`)
    .then(async (module) => {
      await module.default(new URL(
        `../../pkg/bangdream_optimize_web_wasm_bg.wasm?v=${WASM_ASSET_VERSION}`,
        import.meta.url,
      ));
      return module;
    });
  return wasmPromise;
}
