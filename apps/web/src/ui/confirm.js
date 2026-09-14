import {designFragment} from './approved/templates.js';

export function confirmDialog({
  title = '确认操作',
  lines = [],
  confirmText = '确定',
  cancelText = '取消',
  danger = false,
} = {}) {
  if (typeof HTMLDialogElement !== 'function') {
    return Promise.resolve(window.confirm([title, ...lines].join('\n')));
  }

  return new Promise((resolve) => {
    const dialog = designFragment('confirm-dialog');
    const heading = dialog.querySelector('#confirm-title');
    heading.textContent = title;

    const body = dialog.querySelector('#confirm-body');
    for (const line of lines) {
      const paragraph = document.createElement('p');
      paragraph.textContent = line;
      body.append(paragraph);
    }

    const cancel = dialog.querySelector('#confirm-cancel');
    cancel.type = 'button';
    cancel.textContent = cancelText;
    cancel.addEventListener('click', () => dialog.close('cancel'));

    const confirm = dialog.querySelector('#confirm-apply');
    confirm.type = 'button';
    confirm.className = danger ? 'primary danger-fill' : 'primary';
    confirm.textContent = confirmText;
    confirm.onclick=()=>dialog.close('confirm');
    const close=dialog.querySelector('#close-confirm');close.type='button';close.textContent='×';close.onclick=()=>dialog.close('cancel');
    (document.querySelector('#archive-dialog-host')||document.body).append(dialog);

    dialog.addEventListener('close', () => {
      const confirmed = dialog.returnValue === 'confirm';
      dialog.remove();
      resolve(confirmed);
    }, { once: true });

    try {
      dialog.showModal();
    } catch {
      dialog.remove();
      resolve(window.confirm([title, ...lines].join('\n')));
    }
  });
}
