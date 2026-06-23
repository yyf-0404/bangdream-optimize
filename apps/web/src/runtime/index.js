export async function createRuntime(options = {}) {
  if (isDesktopRuntimeAvailable()) {
    const { createDesktopRuntime } = await import('./desktop.js?v=2');
    return createDesktopRuntime(options);
  }

  const { createBrowserRuntime } = await import('./browser.js?v=2');
  return createBrowserRuntime(options);
}

function isDesktopRuntimeAvailable() {
  return globalThis.__TAURI__?.core?.invoke != null
    || globalThis.__TAURI__?.tauri?.invoke != null
    || globalThis.__TAURI__?.invoke != null;
}
