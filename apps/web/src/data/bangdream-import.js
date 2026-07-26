export const BANGDREAM_ACCOUNT_LOGIN_HINT = '请使用自己的设备登陆一次该账号后重试';

export async function fetchBangDreamUserDataRequest({
  apiBaseUrl,
  userId,
  requireConfiguredBase = false,
}) {
  const base = normalizeApiBase(apiBaseUrl);
  if (requireConfiguredBase && !base) {
    throw new Error('桌面端游戏账号导入需要配置后端 apiBaseUrl');
  }
  const response = await fetch(`${base}/bangdream/user-data/import`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    cache: 'no-cache',
    body: JSON.stringify({ userId }),
  });
  const text = await response.text();
  let payload;
  try {
    payload = text ? JSON.parse(text) : null;
  } catch {
    payload = null;
  }
  if (!response.ok) {
    const detail = payload?.message || text.slice(0, 200) || `HTTP ${response.status}`;
    if (response.status === 405 || /\bHTTP\s+405\b/i.test(detail)) {
      throw new Error(BANGDREAM_ACCOUNT_LOGIN_HINT);
    }
    throw new Error(`游戏账号导入失败：${detail}`);
  }
  if (!payload || payload.status !== 'ok' || payload.data == null) {
    throw new Error('游戏账号导入没有返回配置数据');
  }
  return payload.data;
}

function normalizeApiBase(baseUrl) {
  const normalized = String(baseUrl ?? '').trim().replace(/\/$/, '');
  if (normalized === 'undefined' || normalized === 'null') {
    return '';
  }
  return normalized;
}
