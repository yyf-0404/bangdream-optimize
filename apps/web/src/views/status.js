export function createStatusView({
  elements,
  renderResultSummary,
  renderMetrics,
}) {
  function formatErrorMessage(error) {
    return error?.message ?? String(error);
  }

  function appendLog(message) {
    const item = document.createElement('li');
    item.textContent = message;
    elements.log.append(item);
    elements.log.scrollTop = elements.log.scrollHeight;
  }

  function setStatus(message) {
    elements.status.textContent = message;
    elements.status.classList.remove('error');
  }

  function setError(error) {
    const message = formatErrorMessage(error);
    elements.status.textContent = `错误: ${message}`;
    elements.status.classList.add('error');
    appendLog(`错误: ${message}`);
    elements.result.textContent = '';
    renderResultSummary(null);
    renderMetrics(null);
  }

  function setGameDataError(error) {
    const message = formatErrorMessage(error);
    elements.status.textContent = `缺少游戏数据: ${message}`;
    elements.status.classList.add('error');
    renderResultSummary(null);
    appendLog(`error: ${message}`);
  }

  return {
    appendLog,
    setError,
    setGameDataError,
    setStatus,
  };
}
