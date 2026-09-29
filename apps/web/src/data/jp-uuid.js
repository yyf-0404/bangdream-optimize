// The UUID is used for this request only; it is never saved in the profile.
export async function importJpUuid({apiBaseUrl = '', request, signal, fetchImpl = fetch}) {
  const base = String(apiBaseUrl ?? '').trim().replace(/\/$/, '');
  const endpoint = new URL(`${base}/api/import/jp-uuid`, globalThis.location?.href ?? 'http://localhost/');
  if (endpoint.username || endpoint.password || (endpoint.protocol !== 'https:' && !(endpoint.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(endpoint.hostname)))) {
    throw new Error('日服 UUID 导入需要 HTTPS 连接，请使用网站的 HTTPS 地址');
  }
  signal?.throwIfAborted();
  let response;
  try {
    response = await fetchImpl(endpoint.href, {
      method: 'POST', headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({playerId: request.playerId, uuid: request.uuid}),
      cache: 'no-store', credentials: 'omit', redirect: 'error',
      signal: signal ? AbortSignal.any([signal, AbortSignal.timeout(240000)]) : AbortSignal.timeout(240000),
    });
  } catch (error) {
    signal?.throwIfAborted();
    throw new Error(error?.name === 'TimeoutError' ? '日服资料读取超时，请稍后重试' : '无法连接日服导入服务，请检查网络或后端服务');
  }
  if (!response.headers.get('content-type')?.includes('application/json')) {
    throw new Error('日服导入接口尚未就绪，请更新后端并配置 /api/import/jp-uuid 代理');
  }
  const result = await response.json();
  if (!response.ok || result?.status !== 'ok') throw new Error(typeof result?.message === 'string' ? result.message.slice(0, 300) : '日服资料读取失败，请稍后重试');
  signal?.throwIfAborted();
  return result.data;
}
