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
    const dialog = document.createElement('dialog');
    dialog.className = 'app-confirm-dialog';

    const form = document.createElement('form');
    form.className = 'app-confirm-dialog-content';
    form.method = 'dialog';

    const heading = document.createElement('h3');
    heading.textContent = title;

    const body = document.createElement('div');
    body.className = 'app-confirm-dialog-body';
    for (const line of lines) {
      const paragraph = document.createElement('p');
      paragraph.textContent = line;
      body.append(paragraph);
    }

    const actions = document.createElement('div');
    actions.className = 'app-confirm-dialog-actions';

    const cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.textContent = cancelText;
    cancel.addEventListener('click', () => dialog.close('cancel'));

    const confirm = document.createElement('button');
    confirm.type = 'submit';
    confirm.className = danger ? 'primary danger-action' : 'primary';
    confirm.textContent = confirmText;

    actions.append(cancel, confirm);
    form.append(heading, body, actions);
    dialog.append(form);
    document.body.append(dialog);

    dialog.addEventListener('close', () => {
      const confirmed = dialog.returnValue !== 'cancel';
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
