export function installCyclingSelect(trigger, select) {
  if (!trigger || !select) {
    return;
  }
  const cycle = (offset) => {
    if (trigger.disabled || select.disabled) {
      return;
    }
    const options = Array.from(select.options).filter((option) => !option.disabled);
    if (options.length === 0) {
      return;
    }
    select.value = nextCyclicValue(
      options.map((option) => option.value),
      select.value,
      offset,
    );
    select.dispatchEvent(new Event('change', { bubbles: true }));
  };
  trigger.addEventListener('click', () => cycle(1));
  trigger.addEventListener('contextmenu', (event) => {
    event.preventDefault();
    cycle(-1);
  });
}

export function nextCyclicValue(values, current, offset = 1) {
  const available = Array.from(values ?? []);
  if (available.length === 0) {
    return undefined;
  }
  const currentIndex = available.indexOf(current);
  if (currentIndex < 0) {
    return available[0];
  }
  const normalizedOffset = Number.isInteger(offset) ? offset : 1;
  const nextIndex = (
    currentIndex + normalizedOffset % available.length + available.length
  ) % available.length;
  return available[nextIndex];
}
