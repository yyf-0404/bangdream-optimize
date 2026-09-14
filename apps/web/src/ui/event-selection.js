export const eventTypeNames = {
  mission_live: '任务 Live · 协力',
  live_try: 'Live Try · EX',
  versus: '竞演 Live · 对邦',
  medley: '巡回演出 · 组曲',
  festival: '团队 Live · 5v5',
  challenge: '挑战 Live · CP',
};

export function mountEventSelection({root, target, elements, getPlayer, getProfileId, eventSnapshot, changeCustomType}) {
  const row = target.querySelector('.bo-event-row');
  const choice = document.createElement('div');
  choice.className = 'preview-event-choice';
  choice.innerHTML = '<span class="event-logo-slot"></span><select aria-label="当前活动" id="activity-event-select"></select>';
  row.querySelector('input').replaceWith(choice);
  const select = choice.querySelector('select');
  select.onchange = () => {
    if (select.value === '0') elements.customEvent.click();
    else {
      elements.eventSearch.value = select.value;
      elements.eventSearch.dispatchEvent(new Event('change', {bubbles: true}));
    }
  };

  const typeField = document.createElement('div');
  typeField.className = 'bo-field bo-event-type-display';
  typeField.innerHTML = '<span class="bo-field-label">活动类型</span><div class="bo-event-type-value"><span role="status"></span><button type="button" class="pb-link" hidden>更改类型</button></div>';
  row.querySelector('#bo-event-type').closest('label').replaceWith(typeField);
  const change = typeField.querySelector('button');
  change.onclick = () => openTypeChooser();

  // Reuse the legacy inputs and listeners: all/none, unavailable types and
  // future/unknown event handling remain owned by the reference view.
  const filters = elements.toggleEventTypeFilters.closest('fieldset');
  filters.classList.add('bo-event-filters');
  filters.querySelector('legend').textContent = '筛选类型';
  row.after(filters);

  let renderedPlayer;
  function renderChoices(player = renderedPlayer) {
    if (!player) return;
    const optionFor = (label, id) => {
      const type = eventSnapshot(id, player)?.eventType;
      const typeLabel = (eventTypeNames[type] || type || '').split(' · ').at(-1);
      const option = new Option(typeLabel ? `${label} · ${typeLabel}` : label, id);
      option.dataset.displayLabel = label;
      option.dataset.meta = typeLabel;
      return option;
    };
    const options = [optionFor('自定义活动 · 不使用活动图片', '0')];
    for (const entry of elements.eventOptions.options) {
      const id = entry.value.match(/^\d+/)?.[0];
      if (id && id !== '0') options.push(optionFor('#' + entry.value, id));
    }
    const current = String(player.currentEvent);
    if (!options.some(option => option.value === current)) {
      // A filter changes candidates, never the configured event. Retain its
      // trigger label without offering the filtered-out event in the popup.
      const selected = optionFor('#' + elements.eventSearch.value, current);
      selected.hidden = true;
      options.push(selected);
    }
    select.replaceChildren(...options);
    select.value = current;
  }
  // The old filter listeners rebuild the datalist, independently of form
  // rendering. Keep the actual combobox in sync with that single source.
  new MutationObserver(() => renderChoices()).observe(elements.eventOptions, {childList: true});

  function openTypeChooser() {
    const player = getPlayer();
    if (Number(player.currentEvent) !== 0) return;
    const profile = getProfileId();
    const current = eventSnapshot(0, player).eventType;
    const dialog = document.createElement('dialog');
    dialog.className = 'pb-dialog event-type-dialog';
    dialog.setAttribute('aria-labelledby', 'event-type-dialog-title');
    dialog.innerHTML = '<form method="dialog"><header class="pb-dialog-header"><div><p class="pb-eyebrow">自定义活动</p><h2 id="event-type-dialog-title">更改活动类型</h2></div><button type="button" class="pb-icon-button" aria-label="关闭活动类型选择">×</button></header><div class="event-type-dialog-body"><fieldset><legend>活动类型</legend><div class="event-type-options"></div></fieldset><p class="pb-help">演出设置与加成模型将随活动类型更新。</p></div><footer class="pb-dialog-footer"><button type="button" class="pb-link" data-cancel>取消</button><button type="submit" class="pb-primary">应用类型</button></footer></form>';
    const options = dialog.querySelector('.event-type-options');
    for (const [value, name] of Object.entries(eventTypeNames)) {
      const label = document.createElement('label');
      const radio = document.createElement('input');
      radio.type = 'radio';
      radio.name = 'custom-event-type';
      radio.value = value;
      radio.required = true;
      radio.checked = value === current;
      const text = document.createElement('span');
      text.textContent = name;
      label.append(radio, text);
      options.append(label);
    }
    dialog.querySelector('[data-cancel]').onclick = dialog.querySelector('.pb-icon-button').onclick = () => dialog.close();
    dialog.querySelector('form').onsubmit = e => {
      e.preventDefault();
      if (profile !== getProfileId() || Number(getPlayer().currentEvent) !== 0) {
        dialog.close();
        return;
      }
      const type = dialog.querySelector('input:checked')?.value;
      if (!type) return;
      if (type !== current) changeCustomType(type);
      dialog.close();
    };
    dialog.onclose = () => dialog.remove();
    root.append(dialog);
    dialog.showModal();
    dialog.querySelector('input:checked')?.focus();
  }

  return {render(player, event) {
    renderedPlayer = player;
    renderChoices(player);
    typeField.querySelector('[role=status]').textContent = eventTypeNames[event.eventType] || event.eventType;
    change.hidden = Number(player.currentEvent) !== 0;
  }};
}
