const DOWNLOAD_EXTENSIONS = new Set(['.exe', '.msi', '.zip', '.7z']);

export function createDownloadActions({
  state,
  elements,
  setStatus,
  setError,
}) {
  let cachedEntries = null;

  function configureDownloadControls() {
    if (elements.openDesktopDownloads) {
      elements.openDesktopDownloads.hidden = state.runtime?.kind !== 'browser';
    }
  }

  async function handleOpenDesktopDownloads() {
    configureDownloadControls();
    if (!elements.desktopDownloadsDialog) {
      return;
    }
    renderDownloadStatus('加载中');
    renderDownloadEntries([]);
    openDialog(elements.desktopDownloadsDialog);
    try {
      const entries = cachedEntries ?? await loadDownloadEntries();
      cachedEntries = entries;
      renderDownloadEntries(entries);
      renderDownloadStatus(entries.length === 0 ? '没有可下载的桌面端文件' : '');
    } catch (error) {
      renderDownloadEntries([]);
      renderDownloadStatus('下载列表加载失败');
      setError(error);
    }
  }

  function handleCloseDesktopDownloadsDialog() {
    if (elements.desktopDownloadsDialog?.open) {
      elements.desktopDownloadsDialog.close();
    }
  }

  async function loadDownloadEntries() {
    const baseUrl = desktopDownloadsUrl();
    const response = await fetch(baseUrl, {
      cache: 'no-cache',
      headers: { Accept: 'application/json' },
    });
    if (!response.ok) {
      throw new Error(`下载列表加载失败：HTTP ${response.status}`);
    }
    const payload = await response.json();
    return normalizeDownloadEntries(payload, baseUrl)
      .filter((entry) => DOWNLOAD_EXTENSIONS.has(fileExtension(entry.name)))
      .sort((left, right) => {
        const timeOrder = (right.updatedAtMs ?? 0) - (left.updatedAtMs ?? 0);
        return timeOrder || right.name.localeCompare(left.name);
      });
  }

  function renderDownloadEntries(entries) {
    const container = elements.desktopDownloadsList;
    if (!container) {
      return;
    }
    container.textContent = '';
    for (const entry of entries) {
      const item = document.createElement('div');
      item.className = 'desktop-download-item';

      const link = document.createElement('a');
      link.className = 'desktop-download-button';
      link.href = entry.url;
      link.download = entry.name;
      link.textContent = versionLabel(entry.name);
      link.title = entry.name;
      link.addEventListener('click', () => {
        setStatus(`开始下载 ${entry.name}`);
      });

      const meta = document.createElement('span');
      meta.className = 'desktop-download-meta';
      meta.textContent = formatUpdatedAt(entry.updatedAtMs);

      item.append(link, meta);
      container.append(item);
    }
  }

  function renderDownloadStatus(message) {
    if (elements.desktopDownloadsStatus) {
      elements.desktopDownloadsStatus.textContent = message;
      elements.desktopDownloadsStatus.hidden = !message;
    }
  }

  return {
    configureDownloadControls,
    handleCloseDesktopDownloadsDialog,
    handleOpenDesktopDownloads,
  };
}

function normalizeDownloadEntries(payload, baseUrl) {
  if (!Array.isArray(payload)) {
    throw new Error('下载目录未启用 JSON autoindex');
  }
  return payload
    .map((entry) => {
      const name = String(entry?.name ?? '').trim();
      if (!name || name.includes('/')) {
        return null;
      }
      const type = String(entry?.type ?? 'file').toLowerCase();
      if (type && type !== 'file') {
        return null;
      }
      const updatedAtMs = Date.parse(entry?.mtime ?? entry?.modified ?? entry?.time ?? '');
      return {
        name,
        updatedAtMs: Number.isFinite(updatedAtMs) ? updatedAtMs : 0,
        url: downloadFileUrl(baseUrl, name),
      };
    })
    .filter(Boolean);
}

function desktopDownloadsUrl() {
  const configured = globalThis.BANGDREAM_OPTIMIZE_CONFIG?.desktopDownloadsUrl;
  const value = String(configured || '/downloads/').trim() || '/downloads/';
  return value.endsWith('/') ? value : `${value}/`;
}

function downloadFileUrl(baseUrl, name) {
  const documentBase = globalThis.location?.href ?? 'http://localhost/';
  const directoryUrl = new URL(baseUrl, documentBase);
  return new URL(encodeURIComponent(name), directoryUrl).toString();
}

function openDialog(dialog) {
  if (typeof dialog.showModal === 'function') {
    dialog.showModal();
  } else {
    dialog.setAttribute('open', '');
  }
}

function fileExtension(name) {
  const index = name.lastIndexOf('.');
  return index < 0 ? '' : name.slice(index).toLowerCase();
}

function versionLabel(name) {
  const normalizedName = String(name);
  const packageMatch = normalizedName.match(
    /^bangdream-optimize-desktop-v([^./\\]+(?:\.[^./\\-]+)*)-(windows-[A-Za-z0-9-]+)\.(exe|msi|zip|7z)$/i,
  );
  if (packageMatch) {
    return `v${packageMatch[1]} · ${packageMatch[2].toLowerCase()}`;
  }
  return name
    .replace(/\.(exe|msi|zip|7z)$/i, '')
    .replace(/^bangdream-optimize[-_]?/i, '')
    .replace(/[-_]+/g, ' ')
    .trim()
    || name;
}

function formatUpdatedAt(timestamp) {
  if (!timestamp) {
    return '更新时间未知';
  }
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(timestamp));
}
