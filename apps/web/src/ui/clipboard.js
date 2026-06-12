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
