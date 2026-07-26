export async function submitFeedbackRequest({
  apiBaseUrl,
  payload,
  attachments = [],
  requireConfiguredBase = false,
}) {
  const base = normalizeApiBase(apiBaseUrl);
  if (requireConfiguredBase && !base) {
    throw new Error('桌面端反馈功能需要配置 feedbackApiBaseUrl');
  }
  const form = new FormData();
  form.append('payload', JSON.stringify(payload));
  for (const attachment of attachments) {
    form.append('attachment', attachment, attachment.name);
  }
  const response = await fetch(`${base}/api/feedback`, {
    method: 'POST',
    cache: 'no-cache',
    body: form,
  });
  const text = await response.text();
  let result;
  try {
    result = text ? JSON.parse(text) : null;
  } catch {
    result = null;
  }
  if (!response.ok) {
    const detail = result?.message || text.slice(0, 200) || `HTTP ${response.status}`;
    throw new Error(detail);
  }
  if (!result || result.status !== 'ok' || !result.data?.feedbackId) {
    throw new Error('反馈接口没有返回反馈编号');
  }
  return result.data;
}

function normalizeApiBase(baseUrl) {
  const normalized = String(baseUrl ?? '').trim().replace(/\/$/, '');
  if (normalized === 'undefined' || normalized === 'null') {
    return '';
  }
  return normalized;
}
