import { emptyMessage } from '../ui/dom.js?v=3';
import { iconButton } from '../ui/icons.js?v=3';
import {
  ATTRIBUTE_VALUES,
  compactJoin,
  eventTypeLabel,
  formatNumberInput,
} from '../utils.js?v=3';
import { nextCyclicValue } from '../ui/cycle-select.js?v=2';

export function createEventView({
  elements,
  customEventId,
  readPlayer,
  editableEventSnapshot,
  isSupportedEventType,
  eventLabel,
  eventDateRange,
  normalizedEventAttributeAndCharacterBonus,
  normalizedEventCharacterParameterBonus,
  normalizedEventAttributes,
  normalizedEventCharacters,
  normalizedEventMembers,
  attributeCell,
  percentCell,
  characterEntityCell,
  cardEntityCell,
  cardAttribute,
  attributeFallback,
  updateEventAttribute,
  deleteEventAttribute,
  updateEventCharacter,
  deleteEventCharacter,
  updateEventMember,
  deleteEventMember,
}) {
  function renderEventSummary(eventId) {
    if (eventId == null) {
      elements.eventSummary.textContent = '未设置活动';
      return;
    }

    const player = readPlayer();
    const event = editableEventSnapshot(eventId, player);
    if (!event) {
      elements.eventSummary.textContent = `自定义活动 · ID ${eventId}`;
      return;
    }
    if (!isSupportedEventType(event.eventType)) {
      elements.eventSummary.textContent = compactJoin([
        eventLabel(eventId, player),
        `ID ${eventId}`,
        `活动类型：${eventTypeLabel(event.eventType)}`,
        '当前计算目标不支持该活动类型',
      ]);
      return;
    }

    elements.eventSummary.textContent = compactJoin([
      eventLabel(eventId, player),
      eventParametersEditable(player) ? '自定义' : '预设',
      `ID ${eventId}`,
      `活动类型：${eventTypeLabel(event.eventType)}`,
      eventDateRange(event),
    ]);
  }

  function renderEventParameters(player) {
    const event = editableEventSnapshot(player.currentEvent, player);
    const editable = eventParametersEditable(player);
    if (!event) {
      renderEmptyEventParameters();
      return;
    }

    const combined = normalizedEventAttributeAndCharacterBonus(
      event.eventAttributeAndCharacterBonus,
    );
    elements.eventCombinedPercent.value = formatNumberInput(combined.parameterPercent);
    elements.eventCombinedPercent.disabled = !editable;

    const characterParam = normalizedEventCharacterParameterBonus(
      event.eventCharacterParameterBonus,
    );
    elements.eventCharacterParamPerformance.value = formatNumberInput(characterParam.performance);
    elements.eventCharacterParamTechnique.value = formatNumberInput(characterParam.technique);
    elements.eventCharacterParamVisual.value = formatNumberInput(characterParam.visual);
    elements.eventCharacterParamPerformance.disabled = !editable;
    elements.eventCharacterParamTechnique.disabled = !editable;
    elements.eventCharacterParamVisual.disabled = !editable;

    setEventAddControlsDisabled(!editable);

    renderEventAttributes(event, editable);
    renderEventCharacters(event, editable);
    renderEventMembers(event, editable);
  }

  function renderEmptyEventParameters() {
    elements.eventCombinedPercent.value = '0';
    elements.eventCombinedPercent.disabled = true;
    elements.eventCharacterParamPerformance.value = '0';
    elements.eventCharacterParamTechnique.value = '0';
    elements.eventCharacterParamVisual.value = '0';
    elements.eventCharacterParamPerformance.disabled = true;
    elements.eventCharacterParamTechnique.disabled = true;
    elements.eventCharacterParamVisual.disabled = true;
    setEventAddControlsDisabled(true);
    elements.eventAttributeRows.textContent = '';
    elements.eventCharacterRows.textContent = '';
    elements.eventMemberRows.textContent = '';
    elements.eventAttributeRows.append(emptyBonusMessage('未设置活动'));
    elements.eventCharacterRows.append(emptyBonusMessage('未设置活动'));
    elements.eventMemberRows.append(emptyBonusMessage('未设置活动'));
  }

  function eventParametersEditable(player) {
    return Number(player.currentEvent) === customEventId
      && player.eventOverrides?.[String(player.currentEvent)] != null;
  }

  function setEventAddControlsDisabled(disabled) {
    elements.eventParamTables.classList.toggle('is-readonly', disabled);
    for (const form of [
      elements.addEventAttribute.closest('.inline-form'),
      elements.addEventCharacter.closest('.inline-form'),
      elements.addEventMember.closest('.inline-form'),
    ]) {
      if (form) {
        form.hidden = disabled;
        if (disabled) {
          form.style.setProperty('display', 'none', 'important');
        } else {
          form.style.removeProperty('display');
        }
      }
    }

    for (const element of [
      elements.newEventAttribute,
      elements.newEventAttributePercent,
      elements.addEventAttribute,
      elements.newEventCharacterId,
      elements.newEventCharacterPercent,
      elements.addEventCharacter,
      elements.newEventMemberCardId,
      elements.newEventMemberPercent,
      elements.addEventMember,
    ]) {
      element.disabled = disabled;
    }
  }

  function renderEventAttributes(event, editable) {
    elements.eventAttributeRows.textContent = '';
    const attributes = normalizedEventAttributes(event.attributes);
    if (attributes.length === 0) {
      if (!editable) {
        elements.eventAttributeRows.append(emptyBonusMessage('没有属性加成'));
      }
      appendAddControl(elements.eventAttributeRows, elements.addEventAttribute, editable);
      return;
    }

    attributes.forEach((bonus, index) => {
      const item = document.createElement('div');
      item.className = 'event-attribute-bonus-item';

      const attribute = compactBonusVisual({
        className: 'event-attribute-bonus-attribute',
        disabled: !editable,
        label: '切换属性加成',
        onClick: () => updateEventAttribute(index, {
          attribute: nextCyclicValue(ATTRIBUTE_VALUES, bonus.attribute),
        }),
      });
      const attributeCellElement = attributeCell(bonus.attribute, () => {}, { disabled: true });
      const attributeIcon = attributeCellElement.querySelector('.attribute-icon, .attribute-swatch');
      if (attributeIcon) {
        attribute.append(attributeIcon);
      }

      const percent = bonusPercentControl(bonus.percent, (value) =>
        updateEventAttribute(index, { percent: value }),
        { disabled: !editable },
      );

      item.append(attribute, percent);
      if (editable) {
        item.append(deleteBonusButton('删除属性加成', () => deleteEventAttribute(index)));
      }
      elements.eventAttributeRows.append(item);
    });
    appendAddControl(elements.eventAttributeRows, elements.addEventAttribute, editable);
  }

  function renderEventCharacters(event, editable) {
    elements.eventCharacterRows.textContent = '';
    const characters = normalizedEventCharacters(event.characters);
    if (characters.length === 0) {
      if (!editable) {
        elements.eventCharacterRows.append(emptyBonusMessage('没有角色加成'));
      }
      appendAddControl(elements.eventCharacterRows, elements.addEventCharacter, editable);
      return;
    }

    characters.forEach((bonus, index) => {
      const item = document.createElement('div');
      item.className = 'event-character-bonus-item';

      const characterCell = characterEntityCell(bonus.characterId);
      const character = compactBonusVisual({
        className: 'event-character-bonus-character',
        disabled: !editable,
        label: '切换角色加成',
        onClick: () => openCharacterPicker(bonus.characterId, (characterId) =>
          updateEventCharacter(index, { characterId }),
        ),
      });
      character.append(...Array.from(characterCell.childNodes));

      const percent = bonusPercentControl(bonus.percent, (value) =>
        updateEventCharacter(index, { percent: value }),
        { disabled: !editable },
      );

      item.append(character, percent);
      if (editable) {
        item.append(deleteBonusButton('删除角色加成', () => deleteEventCharacter(index)));
      }
      elements.eventCharacterRows.append(item);
    });
    appendAddControl(elements.eventCharacterRows, elements.addEventCharacter, editable);
  }

  function emptyBonusMessage(message) {
    return emptyMessage(message, 'event-compact-bonus-empty');
  }

  function appendAddControl(container, button, editable) {
    const form = button?.closest('.inline-form');
    if (!form || !editable) {
      return;
    }
    form.hidden = false;
    form.style.removeProperty('display');
    container.append(form);
  }

  function compactBonusVisual({
    className,
    disabled,
    label,
    onClick,
  }) {
    const element = document.createElement(disabled ? 'div' : 'button');
    element.className = className;
    if (!disabled) {
      element.type = 'button';
      element.setAttribute('aria-label', label);
      element.addEventListener('click', onClick);
    }
    return element;
  }

  function bonusPercentControl(value, onChange, { disabled = false } = {}) {
    const percent = document.createElement('label');
    percent.className = 'event-compact-bonus-percent';
    const percentLabel = document.createElement('span');
    percentLabel.textContent = '加成';
    const percentInputCell = percentCell(value, onChange, { disabled });
    percent.append(percentLabel, ...Array.from(percentInputCell.childNodes));
    return percent;
  }

  function deleteBonusButton(label, onClick) {
    return iconButton({
      icon: 'trash',
      label,
      className: 'compact-button event-compact-bonus-delete',
      onClick,
    });
  }

  function openCharacterPicker(currentCharacterId, onSelect) {
    openEntityPicker({
      title: '选择角色',
      currentId: currentCharacterId,
      options: characterOptions(),
      onSelect,
    });
  }

  function openCardPicker(currentCardId, onSelect) {
    openEntityPicker({
      title: '选择卡牌',
      currentId: currentCardId,
      options: cardOptions(),
      onSelect,
    });
  }

  function openEntityPicker({
    title,
    currentId,
    options,
    onSelect,
  }) {
    if (options.length === 0) {
      return;
    }

    const dialog = document.createElement('dialog');
    dialog.className = 'event-character-picker-dialog';

    const content = document.createElement('form');
    content.method = 'dialog';
    content.className = 'event-character-picker-content';

    const titleElement = document.createElement('h3');
    titleElement.textContent = title;

    const select = document.createElement('select');
    for (const option of options) {
      const item = document.createElement('option');
      item.value = String(option.id);
      item.textContent = option.label;
      item.selected = option.id === Number(currentId);
      select.append(item);
    }

    const actions = document.createElement('div');
    actions.className = 'event-character-picker-actions';

    const cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.textContent = '取消';
    cancel.addEventListener('click', () => dialog.close());

    const confirm = document.createElement('button');
    confirm.type = 'submit';
    confirm.className = 'primary';
    confirm.textContent = '确定';

    actions.append(cancel, confirm);
    content.append(titleElement, select, actions);
    dialog.append(content);
    document.body.append(dialog);

    dialog.addEventListener('close', () => {
      dialog.remove();
    });
    content.addEventListener('submit', (event) => {
      event.preventDefault();
      const selectedId = Number.parseInt(select.value, 10);
      dialog.close();
      if (Number.isInteger(selectedId) && selectedId > 0) {
        onSelect(selectedId);
      }
    });

    dialog.showModal();
    select.focus();
  }

  function characterOptions() {
    return Array.from(elements.characterOptions?.options ?? [])
      .map((option) => {
        const id = Number.parseInt(String(option.value).match(/^\d+/)?.[0] ?? '', 10);
        return Number.isInteger(id) && id > 0
          ? { id, label: option.value || option.textContent || `角色 ${id}` }
          : undefined;
      })
      .filter(Boolean);
  }

  function cardOptions() {
    return Array.from(elements.cardOptions?.options ?? [])
      .map((option) => {
        const id = Number.parseInt(String(option.value).match(/^\d+/)?.[0] ?? '', 10);
        return Number.isInteger(id) && id > 0
          ? { id, label: option.value || option.textContent || `卡牌 ${id}` }
          : undefined;
      })
      .filter(Boolean);
  }

  function renderEventMembers(event, editable) {
    elements.eventMemberRows.textContent = '';
    const members = normalizedEventMembers(event.members);
    if (members.length === 0) {
      if (!editable) {
        elements.eventMemberRows.append(emptyBonusMessage('没有卡牌加成'));
      }
      appendAddControl(elements.eventMemberRows, elements.addEventMember, editable);
      return;
    }

    members.forEach((bonus, index) => {
      const item = document.createElement('div');
      item.className = 'event-member-bonus-item';

      const attribute = document.createElement('div');
      attribute.className = 'event-member-bonus-attribute';
      const attributeValue = cardAttribute(bonus.situationId);
      const attributeIcon = attributeFallback(attributeValue);
      if (attributeIcon) {
        attribute.append(attributeIcon);
      }

      const cardCell = cardEntityCell(bonus.situationId);
      const card = compactBonusVisual({
        className: 'event-member-bonus-card',
        disabled: !editable,
        label: '切换卡牌加成',
        onClick: () => openCardPicker(bonus.situationId, (situationId) =>
          updateEventMember(index, { situationId }),
        ),
      });
      card.append(attribute, ...Array.from(cardCell.childNodes));

      const meta = document.createElement('div');
      meta.className = 'event-member-bonus-meta';

      const id = document.createElement('span');
      id.className = 'event-member-bonus-id';
      id.textContent = `ID: ${bonus.situationId}`;

      meta.append(id);

      const percent = bonusPercentControl(bonus.percent, (value) =>
        updateEventMember(index, { percent: value }),
        { disabled: !editable },
      );

      item.append(card, meta, percent);
      if (editable) {
        item.append(deleteBonusButton('删除卡牌加成', () => deleteEventMember(index)));
      }
      elements.eventMemberRows.append(item);
    });
    appendAddControl(elements.eventMemberRows, elements.addEventMember, editable);
  }

  return {
    renderEventParameters,
    renderEventSummary,
  };
}
