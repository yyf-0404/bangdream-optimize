const ASSET_VERSION = '5';

let wasmPromise = null;

self.onmessage = async (event) => {
  const { id, type, payloadJson } = event.data ?? {};
  if (type !== 'calculate' && type !== 'scoreRange' && type !== 'ptMaximize') {
    return;
  }

  try {
    const wasm = await loadWasm();
    const resultJson = type === 'scoreRange'
      ? wasm.scoreRangeFromStaticData(payloadJson)
      : type === 'ptMaximize'
        ? wasm.ptMaximizeFromStaticData(payloadJson)
        : wasm.calculateFromStaticData(payloadJson);
    self.postMessage({ id, ok: true, resultJson });
  } catch (error) {
    self.postMessage({
      id,
      ok: false,
      error: serializeError(error),
    });
  }
};

function serializeError(error) {
  return {
    name: typeof error?.name === 'string' && error.name ? error.name : 'Error',
    message: error?.message ?? String(error),
    stack: typeof error?.stack === 'string' ? error.stack : undefined,
    executionContext: 'browser-worker',
  };
}

function loadWasm() {
  wasmPromise ??= import(`../../pkg/bangdream_optimize_web_wasm.js?v=${ASSET_VERSION}`)
    .then(async (module) => {
      await module.default(new URL(
        `../../pkg/bangdream_optimize_web_wasm_bg.wasm?v=${ASSET_VERSION}`,
        import.meta.url,
      ));
      return module;
    });
  return wasmPromise;
}
