// 默认走同源；如需跨端口调试，请在 page 中配置 BANGDREAM_OPTIMIZE_CONFIG.apiBaseUrl。
const defaultApiBaseUrl = '';
const currentConfig = globalThis.BANGDREAM_OPTIMIZE_CONFIG;

globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  gameDataBaseUrl: '/game-data',
  desktopDownloadsUrl: '/downloads/',
  apiBaseUrl: defaultApiBaseUrl,
  assetOriginUrl: 'https://bestdori.com',
  assetBaseUrl: 'https://bestdori.com/assets',
  assetServer: 'jp',
  ...(currentConfig && typeof currentConfig === 'object' ? currentConfig : {}),
};
