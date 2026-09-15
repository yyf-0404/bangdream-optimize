export async function copyTextToClipboard(text, { fallbackInput } = {}) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const input = fallbackInput ?? temporaryClipboardInput(text);

  try {
    input.focus();
    input.select();
    if (!document.execCommand('copy')) {
      throw new Error('浏览器不支持自动复制');
    }
  } finally {
    if (!fallbackInput) {
      input.remove();
    }
  }
}

export function copyImageToClipboard(imagePromise, runtime) {
  if (runtime?.copyImage) return imagePromise.then(blob => runtime.copyImage(blob));
  if (!globalThis.ClipboardItem || !navigator.clipboard?.write) {
    return Promise.reject(new Error('当前浏览器不支持复制图片'));
  }
  // Pass the promise during the click's user activation (required by Safari).
  return navigator.clipboard.write([new ClipboardItem({'image/png': imagePromise})]);
}

function temporaryClipboardInput(text) {
  const input = document.createElement('textarea');
  input.value = text;
  input.setAttribute('readonly', '');
  input.style.position = 'fixed';
  input.style.left = '-9999px';
  input.style.top = '0';
  document.body.append(input);
  return input;
}
