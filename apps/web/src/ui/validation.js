const FIELD_VALIDATION_CLASS = 'input-validation-message';

function resolveInput(input) {
  return input instanceof HTMLInputElement || input instanceof HTMLSelectElement ? input : null;
}

function getMessageElement(input) {
  if (!input) {
    return null;
  }
  if (input.dataset?.bangdreamValidationMessageId) {
    return document.getElementById(input.dataset.bangdreamValidationMessageId);
  }

  const expectedId = input.id ? `${input.id}-validation` : '';
  const found = expectedId ? document.getElementById(expectedId) : null;
  if (found && found.classList.contains(FIELD_VALIDATION_CLASS)) {
    input.dataset.bangdreamValidationMessageId = found.id;
    return found;
  }

  let sibling = input.nextElementSibling;
  if (sibling && sibling.classList.contains(FIELD_VALIDATION_CLASS)) {
    input.dataset.bangdreamValidationMessageId = sibling.id;
    return sibling;
  }

  sibling = input.parentElement
    ? Array.from(input.parentElement.querySelectorAll(`.${FIELD_VALIDATION_CLASS}`))[0]
    : null;
  if (sibling) {
    input.dataset.bangdreamValidationMessageId = sibling.id;
    return sibling;
  }

  const message = document.createElement('div');
  message.className = FIELD_VALIDATION_CLASS;
  message.id = `${input.id || `validation-${Date.now()}`}-validation`;
  message.hidden = true;
  input.insertAdjacentElement('afterend', message);
  input.dataset.bangdreamValidationMessageId = message.id;
  return message;
}

function normalizeErrorMessage(message) {
  if (message == null) {
    return '';
  }
  if (message instanceof Error) {
    return message.message ?? '';
  }
  return String(message);
}

export function setFieldValidationMessage(input, message) {
  const field = resolveInput(input);
  if (!field) {
    return false;
  }
  const text = normalizeErrorMessage(message).trim();
  const node = getMessageElement(field);
  if (!node) {
    return false;
  }
  if (!text) {
    field.setCustomValidity('');
    field.classList.remove('is-invalid');
    field.removeAttribute('aria-invalid');
    field.removeAttribute('aria-describedby');
    node.textContent = '';
    node.hidden = true;
    return false;
  }
  field.setCustomValidity(text);
  field.classList.add('is-invalid');
  field.setAttribute('aria-invalid', 'true');
  field.setAttribute('aria-describedby', node.id);
  node.textContent = text;
  node.hidden = false;
  return true;
}

export function clearFieldValidationMessage(input) {
  return setFieldValidationMessage(input, '');
}

let clearRevealedError = () => {};
export function clearValidationReveal() { clearRevealedError(); }

// A control can be replaced while activating its page. Resolve it again after
// navigation, and focus the visible combobox instead of its hidden native select.
export function revealValidationError(error, {activatePage, field} = {}) {
  if (!globalThis.document) return false;
  const destination = error?.validationTarget;
  let selector = destination?.selector;
  if (field?.dataset?.setting) selector = `[data-setting="${field.dataset.setting}"]`;
  else if (field?.id) selector = `#${field.id}`;
  if (!selector && !field) return false;
  const page = destination?.page || field?.closest('[data-page-panel]')?.dataset.pagePanel;
  const draftValue = field?.value;
  clearValidationReveal();
  if (page) activatePage?.(page);
  let target = (selector && document.querySelector(selector)) || field;
  if (!target) return false;
  if (field && target !== field && draftValue !== undefined) target.value = draftValue;
  const section = target.closest('.bo-section,.activity-selector-card,.page-section,[data-page-panel]');
  for (let parent = target.parentElement; parent; parent = parent.parentElement) {
    if (parent.tagName === 'DETAILS') parent.open = true;
  }
  const proxy = target.closest('.bo-select')?.querySelector('.bo-select-trigger');
  if (proxy) target = proxy;
  if (!target.getClientRects().length) target = section;
  if (!target) return false;
  const container = target.closest('.calc-field,.bo-field,.field') || target;
  const message = document.createElement('p');
  message.className = 'validation-callout';
  message.id = 'active-validation-message';
  message.setAttribute('role', 'alert');
  message.textContent = error?.message || field?.validationMessage || '请检查此处配置';
  if (container.matches('input,select,button')) container.after(message);
  else container.append(message);
  target.classList.add('validation-focus');
  const previous = target.getAttribute('aria-describedby');
  target.setAttribute('aria-describedby', [previous, message.id].filter(Boolean).join(' '));
  const addedTabIndex = !target.hasAttribute('tabindex') && !target.matches('input,select,button,a');
  if (addedTabIndex) target.tabIndex = -1;
  const scope = section || container;
  const clear = () => {
    message.remove();
    target.classList.remove('validation-focus');
    if (previous) target.setAttribute('aria-describedby', previous);
    else target.removeAttribute('aria-describedby');
    if (addedTabIndex) target.removeAttribute('tabindex');
    scope.removeEventListener('input', clear);
    scope.removeEventListener('change', clear);
    clearRevealedError = () => {};
  };
  scope.addEventListener('input', clear);
  scope.addEventListener('change', clear);
  clearRevealedError = clear;
  requestAnimationFrame(() => {
    target.scrollIntoView({block: 'center', behavior: 'auto'});
    target.focus({preventScroll: true});
  });
  return true;
}
