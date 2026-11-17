export async function createRuntime(options = {}) {
  if (isDesktopRuntimeAvailable()) {
    const { createDesktopRuntime } = await import('./desktop.js');
    return createDesktopRuntime(options);
  }

  const { createBrowserRuntime } = await import('./browser.js');
  return createBrowserRuntime(options);
}

function isDesktopRuntimeAvailable() {
  return globalThis.__TAURI__?.core?.invoke != null
    || globalThis.__TAURI__?.tauri?.invoke != null
    || globalThis.__TAURI__?.invoke != null;
}
