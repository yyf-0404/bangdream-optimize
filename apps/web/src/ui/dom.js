export function inputCell({
  value,
  min,
  max,
  step,
  mode = 'integer',
  className,
  disabled = false,
  onChange,
}) {
  const cell = document.createElement('td');
  const input = document.createElement('input');
  input.type = 'number';
  input.inputMode = mode === 'float' ? 'decimal' : 'numeric';
  input.value = value;
  input.disabled = disabled;
  if (className) {
    input.className = className;
  }
  if (min != null) {
    input.min = min;
  }
  if (max != null) {
    input.max = max;
  }
  if (step != null) {
    input.step = step;
  }

  const message = document.createElement('div');
  message.className = 'input-validation-message';
  message.hidden = true;
  message.id = `input-validation-${Math.random().toString(36).slice(2, 10)}-${Date.now()}`;
  input.setAttribute('aria-describedby', message.id);

  function validate(value) {
    const trimmed = String(value).trim();
    if (!trimmed) {
      return { error: '不能为空', value: NaN };
    }

    const parsed = Number(value);
    const isValidNumber = mode === 'float'
      ? Number.isFinite(parsed)
      : Number.isInteger(parsed);
    if (!isValidNumber) {
      return {
        error: '需为数字',
        value: NaN,
      };
    }
    if (min != null && parsed < min) {
      return {
        error: `不能小于 ${min}`,
        value: NaN,
      };
    }
    if (max != null && parsed > max) {
      return {
        error: `不能大于 ${max}`,
        value: NaN,
      };
    }
    return { error: '', value: parsed };
  }

  function applyValidation(result) {
    input.classList.toggle('is-invalid', Boolean(result.error));
    input.setCustomValidity(result.error || '');
    message.textContent = result.error || '';
    message.hidden = !Boolean(result.error);
  }

  input.addEventListener('input', () => applyValidation(validate(input.value)));
  input.addEventListener('change', () => {
    const result = validate(input.value);
    applyValidation(result);
    if (result.error) {
      return;
    }
    onChange(result.value);
  });

  applyValidation(validate(input.value));
  cell.append(input, message);
  return cell;
}

export function unwrapCell(cell) {
  const wrap = document.createElement('div');
  wrap.append(...Array.from(cell.childNodes));
  return wrap;
}

export function emptyMessage(message, className, tagName = 'p') {
  const empty = document.createElement(tagName);
  empty.className = className;
  empty.textContent = message;
  return empty;
}
