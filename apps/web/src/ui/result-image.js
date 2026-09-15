import {copyImageToClipboard} from './clipboard.js';

// Snapshot before yielding: later profile changes must not alter the saved result.
export async function renderResultImage(page, runtime) {
  if (!page || page.querySelector('#result-summary')?.hidden) throw new Error('请先完成一次计算');
  const content = page.querySelector('.result-content');
  const panel = content?.querySelector('.result-panel');
  if (!panel) throw new Error('结果内容尚未就绪，请重试');
  // Crop to the result body, with a small symmetric gutter instead of the SPA inset.
  const gutter = 24;
  const width = Math.min(1200, Math.ceil(panel.getBoundingClientRect().width)) + gutter * 2;
  const host = document.createElement('div');
  host.style.cssText = `position:fixed;left:-24000px;top:0;width:${width}px;pointer-events:none;z-index:-1`;
  host.setAttribute('aria-hidden', 'true');
  // Retain the page's style scope, but exclude its hero, tools and history drawer.
  const snapshot = page.cloneNode(false);
  snapshot.hidden = false;
  snapshot.classList.add('result-image-snapshot');
  snapshot.style.cssText = `display:block;width:${width}px;max-width:none;margin:0;padding:0;overflow:visible;height:auto;min-height:0`;
  const resultContent = content.cloneNode(true);
  resultContent.style.cssText = `box-sizing:border-box;width:100%;max-width:none;margin:0;padding:${gutter}px`;
  snapshot.append(resultContent);
  snapshot.querySelectorAll('dialog,.heading-actions,.log-panel,[hidden],.result-song-actions,.card-art-switch').forEach(n => n.remove());
  for (const image of snapshot.querySelectorAll('img')) image.loading = 'eager';
  host.append(snapshot);
  document.body.append(host);
  try {
    const assets = new Map();
    for (const image of snapshot.querySelectorAll('img')) {
      const url = new URL(image.src, location.href);
      if (url.hostname !== 'bestdori.com') continue;
      if (!assets.has(url.href)) assets.set(url.href, []);
      assets.get(url.href).push(image);
    }
    const pending = [...assets.entries()];
    await Promise.all(Array.from({length: Math.min(5, pending.length)}, async () => {
      while (pending.length) {
        const [url, images] = pending.shift();
        const blob = await runtime.loadResultAsset(new URL(url).pathname.slice(1));
        const dataUrl = await new Promise((resolve, reject) => {
          const reader = new FileReader();reader.onload=()=>resolve(reader.result);reader.onerror=reject;reader.readAsDataURL(blob);
        });
        for (const image of images) {image.srcset='';image.src=dataUrl;}
      }
    }));
    await import('../vendor/html-to-image.js');
    await document.fonts?.ready;
    const height = Math.ceil(snapshot.scrollHeight);
    const blob = await globalThis.htmlToImage.toBlob(snapshot, {
      backgroundColor: '#fff', width, height,
      pixelRatio: Math.min(2, 15000 / Math.max(width, height)),
      skipFonts: true,
      style: {margin: '0'},
    });
    if (!blob?.size) throw new Error('结果图片生成失败，请重试');
    return blob;
  } catch (error) {
    throw new Error(error?.message || '结果图片生成失败：图片素材无法读取，请重试');
  } finally { host.remove(); }
}

export function offerResultImage(blob, runtime, {copied = false} = {}) {
  const dialog = document.createElement('dialog');
  dialog.className = 'accepted-result-dialog result-image-dialog';
  dialog.setAttribute('aria-label', '保存结果图片');
  dialog.innerHTML = '<header class="dialog-top"><h2>保存结果图片</h2><button type="button" class="icon-button" aria-label="关闭保存结果">×</button></header><div class="dialog-content"><p role="status">剪贴板写入未成功。可以重试复制，或下载图片。</p><img alt="本次计算结果图片"></div><footer class="dialog-bottom"><a class="text-button" download="bangdream-result.png">下载图片</a><button type="button" class="primary">复制图片</button></footer>';
  if (copied) dialog.querySelector('[role=status]').textContent = '结果图片已复制，可直接粘贴。';
  const url = URL.createObjectURL(blob);
  dialog.querySelector('img').src = dialog.querySelector('a').href = url;
  dialog.querySelector('header button').onclick = () => dialog.close();
  dialog.querySelector('footer button').onclick = async () => {
    try { await copyImageToClipboard(Promise.resolve(blob), runtime); dialog.querySelector('[role=status]').textContent = '结果图片已复制，可直接粘贴。'; }
    catch { dialog.querySelector('[role=status]').textContent = '请允许此页面访问剪贴板，或使用下载图片。'; }
  };
  dialog.onclose = () => {dialog.remove(); URL.revokeObjectURL(url);};
  document.body.append(dialog);
  dialog.showModal();
}
