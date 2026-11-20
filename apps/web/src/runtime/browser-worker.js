const ASSET_VERSION = 'hit-timing-2';
const WASM_METHODS = new Map([
  ['calculate', 'calculateFromStaticData'],
  ['scoreRange', 'scoreRangeFromStaticData'],
  ['ptMaximize', 'ptMaximizeFromStaticData'],
  ['ptEvaluate', 'ptEvaluateFromStaticData'],
  ['timingPlan', 'optimizeHitTimingPlan'],
]);

let wasmPromise = null;

self.onmessage = async (event) => {
  const { id, type, payloadJson } = event.data ?? {};
  const method = WASM_METHODS.get(type);
  if (!method) return;

  try {
    const wasm = await loadWasm();
    if (typeof wasm[method] !== 'function') {
      throw new Error('计算核心需要更新');
    }
    const resultJson = wasm[method](payloadJson);
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
