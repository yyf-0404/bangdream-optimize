globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  ...(globalThis.BANGDREAM_OPTIMIZE_CONFIG ?? {}),
  // 仅用于国服账号导入；桌面游戏数据默认直接读取 Bestdori 原始 API。
  bangDreamImportApiBaseUrl: 'https://your-import-api.example.com',
};
