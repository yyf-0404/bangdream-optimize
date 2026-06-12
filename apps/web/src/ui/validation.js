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
