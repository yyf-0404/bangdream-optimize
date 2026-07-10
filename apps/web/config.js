// 计算只读取静态 game-data；生产 API 默认同源，8080 开发页的可选导入 API 指向 3100。
const localDevelopment = ['127.0.0.1', 'localhost'].includes(globalThis.location?.hostname)
  && globalThis.location?.port === '8080';
const defaultApiBaseUrl = localDevelopment
  ? `http://${globalThis.location.hostname}:3100`
  : '';
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
