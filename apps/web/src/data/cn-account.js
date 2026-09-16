// Credentials exist only for this request; never place them in profile or diagnostic state.
export async function importCnAccount({apiBaseUrl = '', credentials, signal, fetchImpl = fetch}) {
  const base = String(apiBaseUrl ?? '').trim().replace(/\/$/, '');
  const endpoint = new URL(`${base}/api/import/cn-account`, globalThis.location?.href ?? 'http://localhost/');
  if (endpoint.username || endpoint.password || (endpoint.protocol !== 'https:' && !(endpoint.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(endpoint.hostname)))) {
    throw new Error('账号密码导入需要 HTTPS 连接，请使用网站的 HTTPS 地址');
  }
  signal?.throwIfAborted();
  let response;
  try {
    response = await fetchImpl(endpoint.href, {
      method: 'POST', headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({account: credentials.account, password: credentials.password, channel: credentials.channel}),
      cache: 'no-store', credentials: 'omit', redirect: 'error',
      signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(240000)]) : AbortSignal.timeout(240000),
    });
  } catch (error) {
    signal?.throwIfAborted();
    throw new Error(error?.name === 'TimeoutError' ? '账号读取超时，请稍后重试' : '无法连接账号导入服务，请检查网络或后端服务');
  }
  if (!response.headers.get('content-type')?.includes('application/json')) {
    throw new Error('账号导入接口尚未就绪，请更新后端并配置 /api/import/cn-account 代理');
  }
  const result = await response.json();
  if (!response.ok || result?.status !== 'ok') throw new Error(typeof result?.message === 'string' ? result.message.slice(0, 300) : '账号读取失败，请稍后重试');
  signal?.throwIfAborted();
  return result.data;
}

export function mergeCnAccountImport(before, result) {
  const player = result?.player;
  if (!Number.isSafeInteger(result?.gameUid) || result.gameUid <= 0 || player?.playerId !== result.gameUid || !['android', 'ios'].includes(result.channel)) throw new Error('账号资料身份异常，已停止导入');
  for (const field of ['cardList', 'areaItem', 'characterBouns']) {
    const value = player[field];
    if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).some(id => !/^[1-9]\d*$/.test(id))) throw new Error('账号资料不完整，已停止导入');
    if (Object.values(value).some(record => !record || typeof record !== 'object' || Array.isArray(record))) throw new Error('账号资料不完整，已停止导入');
  }
  const merged = {...structuredClone(before), server: 'cn', playerId: result.gameUid};
  for (const field of ['cardList', 'areaItem', 'characterBouns']) {
    merged[field] ??= {};
    // Missing IDs mean no update; imported values (including zero) replace existing growth.
    for (const [id, record] of Object.entries(player[field])) {
      merged[field][id] = {...merged[field][id], ...structuredClone(record)};
    }
  }
  return merged;
}
