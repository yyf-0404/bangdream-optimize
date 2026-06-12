import { emptyMessage, unwrapCell } from '../ui/dom.js?v=1';
import {
  formatRatePercent,
  formatRatePercentInput,
} from '../utils.js?v=1';

export function createPlayerView({
  elements,
  expandedAreaItemGroups,
  isCharacterBonusesCollapsed,
  setCharacterBonusesCollapsed,
  readPlayer,
  areaItemGroups,
  areaItemGroupIconUrls,
  areaItemIconUrls,
  areaItemLabel,
  maxAreaItemLevel,
  formatAreaItemRate,
  hasAreaItemResources,
  hasCharacterResources,
  allAreaItemsAreMaxed,
  allCharacterBonusesAreMaxed,
  characterIdsForPlayer,
  selectedCardCharacterIds,
  normalizedCharacterBonus,
  entityCell,
  characterEntityCell,
  inputCell,
  updateAreaItem,
  updateCharacterBonus,
}) {
  function renderAreaItems(player) {
    elements.areaItemRows.textContent = '';

    const groups = areaItemGroups(player);
    if (groups.length === 0) {
      elements.areaItemRows.append(emptyListMessage('还没有区域道具资源'));
      setAreaItemLevelToggle(player);
      return;
    }

    setAreaItemBulkToggle(groups);
    setAreaItemLevelToggle(player);

    let currentCategory;
    for (const group of groups) {
      if (group.category !== currentCategory) {
        elements.areaItemRows.append(areaItemCategoryTitle(group));
        currentCategory = group.category;
      }
      elements.areaItemRows.append(areaItemGroupSummaryRow(group));
      if (areaItemGroupIsCollapsible(group) && !expandedAreaItemGroups.has(group.key)) {
        continue;
      }
      for (const areaItemId of group.areaItemIds) {
        elements.areaItemRows.append(areaItemConfigRow(player, areaItemId));
      }
    }
  }

  function renderCharacterBonuses(player) {
    const collapsed = isCharacterBonusesCollapsed();
    elements.characterBonusRows.textContent = '';
    setCharacterBonusLevelToggle(player);
    setSectionCollapsed(elements.toggleCharacterBonuses, collapsed);

    if (collapsed) {
      renderCharacterBonusSummary(player);
      return;
    }

    const characterIds = characterIdsForPlayer(player);

    if (characterIds.length === 0) {
      elements.characterBonusRows.append(emptyListMessage('还没有角色资源'));
      return;
    }

    characterIds.forEach((characterId, index) => {
      if (index % 5 === 0) {
        elements.characterBonusRows.append(characterBonusRowLabel());
      }
      const bonus = normalizedCharacterBonus(player.characterBouns[characterId]);
      elements.characterBonusRows.append(characterBonusEditorItem(characterId, bonus));
    });
  }

  function handleToggleAreaItems() {
    const groups = areaItemGroups(readPlayer()).filter(areaItemGroupIsCollapsible);
    if (anyAreaItemGroupExpanded(groups)) {
      for (const group of groups) {
        expandedAreaItemGroups.delete(group.key);
      }
    } else {
      for (const group of groups) {
        expandedAreaItemGroups.add(group.key);
      }
    }
    renderAreaItems(readPlayer());
  }

  function handleToggleCharacterBonuses() {
    setCharacterBonusesCollapsed(!isCharacterBonusesCollapsed());
    renderCharacterBonuses(readPlayer());
  }

  function areaItemGroupSummaryRow(group) {
    const expanded = expandedAreaItemGroups.has(group.key);
    const item = document.createElement('button');
    item.type = 'button';
    item.className = 'player-list-item summary-row area-item-group-item area-item-group-toggle';
    item.setAttribute('aria-expanded', expanded ? 'true' : 'false');
    item.addEventListener('click', () => toggleAreaItemGroup(group.key));
    item.append(
      playerEntityBlock(entityCell(group.key, group.label, {
        imageUrls: areaItemGroupIconUrls(group),
      }), 'player-list-entity'),
      playerMetric('计算加成', formatAreaItemRate(group.rate)),
    );
    return item;
  }

  function areaItemCategoryTitle(group) {
    const title = document.createElement('div');
    title.className = 'area-item-category-title';
    title.textContent = group.categoryLabel ?? '其他道具';
    return title;
  }

  function areaItemConfigRow(player, areaItemId) {
    const config = player.areaItem[areaItemId] ?? {};
    const item = document.createElement('div');
    item.className = 'player-list-item area-item-detail-row';
    item.append(
      playerEntityBlock(entityCell(areaItemId, areaItemLabel(areaItemId), {
        imageUrls: areaItemIconUrls(areaItemId),
      }), 'player-list-entity'),
      playerField('等级', inputControl({
        value: config.level ?? 0,
        min: 0,
        max: maxAreaItemLevel(areaItemId),
        onChange: (value) => updateAreaItem(areaItemId, { level: value }),
      })),
    );
    return item;
  }

  function renderCharacterBonusSummary(player) {
    const characterIds = selectedCardCharacterIds(player);
    if (characterIds.length === 0) {
      elements.characterBonusRows.append(emptyListMessage('还没有选择卡牌'));
      return;
    }

    characterIds.forEach((characterId, index) => {
      if (index % 5 === 0) {
        elements.characterBonusRows.append(characterBonusRowLabel({ collapsed: true }));
      }
      const bonus = normalizedCharacterBonus(player.characterBouns[characterId]);
      const average = (
        bonus.potential.performance
        + bonus.potential.technique
        + bonus.potential.visual
        + bonus.characterTask.performance
        + bonus.characterTask.technique
        + bonus.characterTask.visual
      ) / 3;
      const item = document.createElement('div');
      item.className = 'player-list-item summary-row character-bonus-item character-bonus-summary-item';
      item.append(
        playerEntityBlock(characterEntityCell(characterId), 'player-list-entity'),
        playerMetric('平均加成', formatRatePercent(average)),
      );
      elements.characterBonusRows.append(item);
    });
  }

  function characterBonusEditorItem(characterId, bonus) {
    const item = document.createElement('div');
    item.className = 'player-list-item character-bonus-item';
    item.append(
      playerEntityBlock(characterEntityCell(characterId), 'player-list-entity'),
      characterBonusRateRow([
        ['演出', percentRateControl(bonus.potential.performance, (value) =>
          updateCharacterBonus(characterId, 'potential', 'performance', value))],
        ['技巧', percentRateControl(bonus.potential.technique, (value) =>
          updateCharacterBonus(characterId, 'potential', 'technique', value))],
        ['视觉', percentRateControl(bonus.potential.visual, (value) =>
          updateCharacterBonus(characterId, 'potential', 'visual', value))],
      ]),
      characterBonusRateRow([
        ['演出', percentRateControl(bonus.characterTask.performance, (value) =>
          updateCharacterBonus(characterId, 'characterTask', 'performance', value))],
        ['技巧', percentRateControl(bonus.characterTask.technique, (value) =>
          updateCharacterBonus(characterId, 'characterTask', 'technique', value))],
        ['视觉', percentRateControl(bonus.characterTask.visual, (value) =>
          updateCharacterBonus(characterId, 'characterTask', 'visual', value))],
      ]),
    );
    return item;
  }

  function characterBonusRowLabel({ collapsed = false } = {}) {
    const item = document.createElement('div');
    item.className = `character-bonus-row-label${collapsed ? ' is-collapsed' : ''}`;
    if (!collapsed) {
      const unit = document.createElement('span');
      unit.className = 'character-bonus-row-unit';
      unit.textContent = '(%)';
      const potential = document.createElement('span');
      potential.textContent = '潜能';
      const task = document.createElement('span');
      task.textContent = '任务';
      item.append(unit, potential, task);
    }
    return item;
  }

  function characterBonusRateRow(fields) {
    const row = document.createElement('div');
    row.className = 'character-bonus-rate-row';
    for (const [fieldLabel, control] of fields) {
      const field = document.createElement('label');
      field.className = 'character-bonus-rate-field';
      const text = document.createElement('span');
      text.textContent = fieldLabel;
      field.append(text, control);
      row.append(field);
    }
    return row;
  }

  function setAreaItemBulkToggle(groups) {
    const collapsibleGroups = groups.filter(areaItemGroupIsCollapsible);
    const anyExpanded = anyAreaItemGroupExpanded(collapsibleGroups);
    elements.toggleAreaItems.textContent = anyExpanded ? '全部折叠' : '全部展开';
    elements.toggleAreaItems.setAttribute('aria-expanded', anyExpanded ? 'true' : 'false');
    elements.toggleAreaItems.disabled = collapsibleGroups.length === 0;
  }

  function setAreaItemLevelToggle(player) {
    const maxed = allAreaItemsAreMaxed(player);
    elements.setAreaItems.textContent = maxed ? '全部清零' : '全部满级';
    elements.setAreaItems.disabled = !hasAreaItemResources();
  }

  function setCharacterBonusLevelToggle(player) {
    const maxed = allCharacterBonusesAreMaxed(player);
    elements.setCharacterBonuses.textContent = maxed ? '全部清零' : '全部满级';
    elements.setCharacterBonuses.disabled = !hasCharacterResources();
  }

  function setSectionCollapsed(button, collapsed) {
    button.textContent = collapsed ? '展开' : '折叠';
    button.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
  }

  function areaItemGroupIsCollapsible(group) {
    return group.areaItemIds.length > 0;
  }

  function anyAreaItemGroupExpanded(groups) {
    return groups.some((group) => expandedAreaItemGroups.has(group.key));
  }

  function toggleAreaItemGroup(groupKey) {
    if (expandedAreaItemGroups.has(groupKey)) {
      expandedAreaItemGroups.delete(groupKey);
    } else {
      expandedAreaItemGroups.add(groupKey);
    }
    renderAreaItems(readPlayer());
  }

  function inputControl(options) {
    return unwrapCell(inputCell(options));
  }

  function percentRateControl(value, onChange) {
    return inputControl({
      value: formatRatePercentInput(value),
      min: 0,
      step: 0.1,
      mode: 'float',
      className: 'stat-rate-input',
      onChange: (percent) => onChange(percent / 100),
    });
  }

  function playerEntityBlock(cell, className) {
    const block = unwrapCell(cell);
    block.className = className;
    return block;
  }

  function playerField(label, control) {
    const field = document.createElement('label');
    field.className = 'player-list-field';
    const text = document.createElement('span');
    text.textContent = label;
    field.append(text, control);
    return field;
  }

  function playerMetric(label, value) {
    const metric = document.createElement('div');
    metric.className = 'player-list-metric';
    const labelElement = document.createElement('span');
    labelElement.textContent = label;
    const valueElement = document.createElement('strong');
    valueElement.textContent = value;
    metric.append(labelElement, valueElement);
    return metric;
  }

  function emptyListMessage(message) {
    return emptyMessage(message, 'player-list-empty');
  }

  return {
    handleToggleAreaItems,
    handleToggleCharacterBonuses,
    renderAreaItems,
    renderCharacterBonuses,
  };
}
