globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  ...(globalThis.BANGDREAM_OPTIMIZE_CONFIG ?? {}),
  // 仅用于提交反馈，不包含 SMTP 凭据。
  feedbackApiBaseUrl: 'https://your-feedback-api.example.com',
};
