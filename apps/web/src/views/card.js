import {
  attributeIconUrls,
  assetImage,
  characterIconUrls,
  starIconUrls,
} from '../assets/index.js?v=2';
import { buttonIcon, iconButton } from '../ui/icons.js?v=2';
import { emptyMessage, unwrapCell } from '../ui/dom.js?v=2';
import {
  ATTRIBUTE_VALUES_WITH_ALL,
  attributeLabel,
  numericStringSort,
} from '../utils.js?v=2';

export function createCardView({
  rows,
  expandedGroups,
  groupCache,
  getCore,
  getPlayer,
  entityCell,
  inputCell,
  cardLabel,
  cardName,
  cardRarity,
  cardIconUrls,
  normalizedCardConfig,
  cardTrainingStatusList,
  maxCardLevel,
  cardEpisodeAlwaysRead,
  cardEpisodeAvailable,
  cardCharacterId,
  cardAttribute,
  characterLabel,
  updateCard,
  updateCardEpisode,
  deleteCard,
  clearCards,
}) {
  let cardIndexCache = null;

  function renderCards(player) {
    rows.textContent = '';
    const grouped = cardReferenceDataReady();
    const cardList = player.cardList ?? {};
    const cardIds = Object.keys(cardList);

    if (cardIds.length === 0) {
      rows.append(emptyCardMessage('还没有卡牌'));
      return;
    }

    const fragment = document.createDocumentFragment();
    fragment.append(cardListToolbar(cardIds.length));
    if (!grouped) {
      fragment.append(emptyCardMessage('卡牌数据加载中'));
      rows.append(fragment);
      return;
    }

    const indexKey = cardIndexKey(cardIds);
    const index = cardCharacterIndex(cardIds, indexKey);
    pruneExpandedCardGroups(index);
    const activeGroup = activeCharacterGroup(index.characterGroups);
    fragment.append(cardCharacterSwitcher(index.characterGroups, activeGroup?.key));
    if (activeGroup) {
      fragment.append(cardCharacterBrowser(activeGroup, index, cardList));
    }
    rows.append(fragment);
  }

  function expandCardGroupForCard(cardId) {
    if (cardReferenceDataReady()) {
      const group = cardGroup(cardId);
      setActiveCharacterGroupKey(group.characterKey);
      expandedGroups.add(group.attributeKey);
    }
  }

  function cardCharacterIndex(cardIds, key = cardIndexKey(cardIds)) {
    if (cardIndexCache?.key === key) {
      return cardIndexCache;
    }
    const cardsByCharacter = new Map();
    const characterGroups = new Map();
    for (const cardId of cardIds) {
      const meta = cardCharacterMeta(cardId);
      if (!characterGroups.has(meta.characterKey)) {
        characterGroups.set(meta.characterKey, {
          key: meta.characterKey,
          characterId: meta.characterId,
          label: meta.characterLabel,
          cardCount: 0,
        });
      }
      const characterGroup = characterGroups.get(meta.characterKey);
      characterGroup.cardCount += 1;
      if (!cardsByCharacter.has(meta.characterKey)) {
        cardsByCharacter.set(meta.characterKey, []);
      }
      cardsByCharacter.get(meta.characterKey).push(cardId);
    }
    const index = {
      key,
      cardsByCharacter,
      attributeGroupsByCharacter: new Map(),
      characterGroups: [...characterGroups.values()].sort(cardCharacterGroupSort),
    };
    cardIndexCache = index;
    return index;
  }

  function cardCharacterMeta(cardId) {
    const cacheKey = String(cardId);
    const cached = groupCache.get(cacheKey);
    if (cached) {
      return cached;
    }
    const characterId = cardCharacterId(cardId);
    return {
      characterId,
      characterKey: `character:${characterId ?? 0}`,
      characterLabel: characterId == null ? '未知角色' : characterLabel(characterId),
    };
  }

  function cardIndexKey(cardIds) {
    const core = getCore();
    return [
      Object.keys(core?.cards ?? {}).length,
      Object.keys(core?.characters ?? {}).length,
      cardIds.length,
      cardIds.join(','),
    ].join('|');
  }

  function attributeGroupsForCharacter(index, characterKey) {
    const cached = index.attributeGroupsByCharacter.get(characterKey);
    if (cached) {
      return cached;
    }
    const groups = cardAttributeGroups(index.cardsByCharacter.get(characterKey) ?? []);
    index.attributeGroupsByCharacter.set(characterKey, groups);
    return groups;
  }

  function cardAttributeGroups(cardIds) {
    const attributeGroups = new Map();
    for (const cardId of cardIds) {
      const meta = cardGroup(cardId);
      if (!attributeGroups.has(meta.attributeKey)) {
        attributeGroups.set(meta.attributeKey, {
          key: meta.attributeKey,
          characterId: meta.characterId,
          attribute: meta.attribute,
          label: meta.attributeLabel,
          cardIds: [],
        });
      }
      attributeGroups.get(meta.attributeKey).cardIds.push(cardId);
    }
    return [...attributeGroups.values()].sort(cardAttributeGroupSort);
  }

  function pruneExpandedCardGroups(index) {
    const validKeys = new Set();
    for (const group of index.characterGroups) {
      validKeys.add(group.key);
    }
    const expandedCharacters = index.characterGroups
      .filter((group) => expandedGroups.has(group.key));
    for (const group of expandedCharacters) {
      validKeys.add(attributeDefaultKey(group.key));
      for (const attributeGroup of attributeGroupsForCharacter(index, group.key)) {
        validKeys.add(attributeGroup.key);
        validKeys.add(rarityDefaultKey(attributeGroup.key));
        if (!expandedGroups.has(attributeGroup.key)) {
          continue;
        }
        for (const rarityGroup of cardRarityGroups(attributeGroup.cardIds, attributeGroup.key)) {
          validKeys.add(rarityGroup.key);
        }
      }
    }
    for (const key of expandedGroups) {
      if (!validKeys.has(key)) {
        expandedGroups.delete(key);
      }
    }
  }

  function cardDetailRow(cardId, rawConfig) {
    const config = normalizedCardConfig(cardId, rawConfig);
    const trainingStatusList = cardTrainingStatusList(cardId);
    const canSelectTrainingStatus = trainingStatusList.length > 1;
    const hasEpisode1 = cardEpisodeAvailable(cardId, 0);
    const hasEpisode2 = cardEpisodeAvailable(cardId, 1);
    const forceEpisode1 = cardEpisodeAlwaysRead(cardId, 0);
    const forceEpisode2 = cardEpisodeAlwaysRead(cardId, 1);
    const item = document.createElement('article');
    item.className = 'card-list-item';

    const entity = document.createElement('div');
    entity.className = 'card-list-entity';
    entity.append(cardEntityCell(cardId, config));

    const fields = document.createElement('div');
    fields.className = 'card-list-fields';
    const numberFields = document.createElement('div');
    numberFields.className = 'card-list-number-fields';
    numberFields.append(
      cardField('等级', inputControl({
        value: config.level,
        min: 1,
        max: maxCardLevel(cardId),
        onChange: (value) => updateCard(cardId, { level: value }),
      }), 'number'),
      cardField('技能', inputControl({
        value: config.skillLevel,
        min: 1,
        max: 5,
        onChange: (value) => updateCard(cardId, { skillLevel: value }),
      }), 'number'),
      cardField('突破', inputControl({
        value: config.limitBreakRank,
        min: 0,
        max: 4,
        onChange: (value) => updateCard(cardId, { limitBreakRank: value }),
      }), 'number'),
    );

    const checkFields = document.createElement('div');
    checkFields.className = 'card-list-check-fields';
    checkFields.append(
      cardField('训练', checkboxControl({
        label: '训练',
        checked: config.training,
        disabled: !canSelectTrainingStatus,
        onChange: (checked) => updateCard(cardId, { training: checked }),
      }), 'check'),
      cardField('图片', checkboxControl({
        label: '图片',
        checked: config.illustTrainingStatus,
        disabled: !canSelectTrainingStatus,
        onChange: (checked) => updateCard(cardId, { illustTrainingStatus: checked }),
      }), 'check'),
      cardField('剧情1', checkboxControl({
        label: '剧情1',
        checked: forceEpisode1 || (hasEpisode1 && config.episodes[0]),
        disabled: !hasEpisode1 || forceEpisode1,
        onChange: (checked) => updateCardEpisode(cardId, 0, checked),
      }), 'check'),
      cardField('剧情2', checkboxControl({
        label: '剧情2',
        checked: forceEpisode2 || (hasEpisode2 && config.episodes[1]),
        disabled: !hasEpisode2 || forceEpisode2,
        onChange: (checked) => updateCardEpisode(cardId, 1, checked),
      }), 'check'),
    );
    fields.append(numberFields, checkFields);

    const deleteButton = iconButton({
      icon: 'trash',
      label: '删除卡牌',
      title: '删除卡牌',
      className: 'compact-button card-list-delete',
      onClick: () => deleteCard(cardId),
    });

    item.append(entity, fields, deleteButton);
    return item;
  }

  function cardListToolbar(cardCount) {
    const toolbar = document.createElement('div');
    toolbar.className = 'card-list-toolbar';
    const summary = document.createElement('span');
    summary.className = 'card-list-summary';
    summary.textContent = `${cardCount} 张卡牌`;
    const clearButton = document.createElement('button');
    clearButton.type = 'button';
    clearButton.className = 'compact-button danger-action card-list-clear';
    clearButton.title = '清空卡牌列表';
    clearButton.append(buttonIcon('trash'));
    const label = document.createElement('span');
    label.textContent = '清空';
    clearButton.append(label);
    clearButton.addEventListener('click', clearCards);
    toolbar.append(summary, clearButton);
    return toolbar;
  }

  function cardReferenceDataReady() {
    const core = getCore();
    return Object.keys(core?.cards ?? {}).length > 0
      && Object.keys(core?.characters ?? {}).length > 0;
  }

  function cardGroup(cardId) {
    const cacheKey = String(cardId);
    const cached = groupCache.get(cacheKey);
    if (cached) {
      return cached;
    }
    const characterId = cardCharacterId(cardId);
    const attribute = cardAttribute(cardId);
    const characterKey = `character:${characterId ?? 0}`;
    const attributeKey = `attribute:${characterId ?? 0}:${attribute ?? ''}`;
    const group = {
      characterId,
      attribute,
      characterKey,
      attributeKey,
      characterLabel: characterId == null ? '未知角色' : characterLabel(characterId),
      attributeLabel: attributeLabel(attribute),
    };
    groupCache.set(cacheKey, group);
    return group;
  }

  function activeCharacterGroup(groups) {
    return groups.find((group) => expandedGroups.has(group.key));
  }

  function setActiveCharacterGroupKey(groupKey) {
    for (const key of [...expandedGroups]) {
      if (key.startsWith('character:') && key !== groupKey) {
        expandedGroups.delete(key);
      }
    }
    expandedGroups.add(groupKey);
  }

  function clearActiveCharacterGroupKey() {
    for (const key of [...expandedGroups]) {
      if (key.startsWith('character:')) {
        expandedGroups.delete(key);
      }
    }
  }

  function cardCharacterSwitcher(groups, activeKey) {
    const switcher = document.createElement('div');
    switcher.className = 'card-character-switcher';
    for (const group of groups) {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'card-character-switch';
      button.classList.toggle('is-active', group.key === activeKey);
      button.setAttribute('aria-pressed', group.key === activeKey ? 'true' : 'false');
      button.title = group.label;
      const icon = assetImage(characterIconUrls(group.characterId), 'entity-icon', group.label);
      const count = document.createElement('span');
      count.textContent = `${group.cardCount} 张`;
      if (icon) {
        button.append(icon);
      }
      button.append(count);
      button.addEventListener('click', () => {
        if (group.key === activeKey) {
          clearActiveCharacterGroupKey();
        } else {
          setActiveCharacterGroupKey(group.key);
        }
        renderCards(getPlayer());
      });
      switcher.append(button);
    }
    return switcher;
  }

  function cardCharacterBrowser(group, index, cardList) {
    const attributeGroups = attributeGroupsForCharacter(index, group.key);
    ensureDefaultExpandedAttributes(group, attributeGroups);
    const section = document.createElement('section');
    section.className = 'card-character-browser';
    for (const attributeGroup of attributeGroups) {
      section.append(cardAttributeGroupSection(attributeGroup, cardList));
    }
    return section;
  }

  function cardAttributeGroupSection(group, cardList) {
    const expanded = expandedGroups.has(group.key);
    const section = document.createElement('section');
    section.className = 'card-group-row card-attribute-group-row';
    section.classList.toggle('is-expanded', expanded);
    const content = document.createElement('div');
    content.className = 'card-group-content';
    if (expanded) {
      const rarityGroups = cardRarityGroups(group.cardIds, group.key);
      ensureDefaultExpandedRarities(group.key, rarityGroups);
      for (const rarityGroup of rarityGroups) {
        content.append(cardRarityGroupSection(rarityGroup, cardList));
      }
    }
    section.append(cardAttributeGroupToggle({
      expanded,
      icon: assetImage(attributeIconUrls(group.attribute), 'attribute-icon', group.attribute),
      text: `${group.cardIds.length} 张`,
      onClick: () => toggleCardGroup(group.key),
    }), content);
    return section;
  }

  function cardAttributeGroupToggle({ expanded, icon, text, onClick }) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'card-attribute-group-toggle';
    button.setAttribute('aria-expanded', expanded ? 'true' : 'false');
    if (icon) {
      button.append(icon);
    }
    const label = document.createElement('span');
    label.textContent = text;
    button.append(label);
    button.addEventListener('click', onClick);
    return button;
  }

  function cardRarityGroups(cardIds, parentKey = '') {
    const groups = new Map();
    for (const cardId of cardIds) {
      const rarity = cardRarity(cardId);
      if (!groups.has(rarity)) {
        groups.set(rarity, {
          key: `rarity:${parentKey}:${rarity}`,
          rarity,
          cardIds: [],
        });
      }
      groups.get(rarity).cardIds.push(cardId);
    }
    return [...groups.values()].sort((left, right) => right.rarity - left.rarity);
  }

  function cardRarityGroupSection(group, cardList) {
    const expanded = expandedGroups.has(group.key);
    const section = document.createElement('section');
    section.className = 'card-group-row card-rarity-group-row';
    section.classList.toggle('is-expanded', expanded);
    const label = document.createElement('button');
    label.type = 'button';
    label.className = 'card-rarity-group-label';
    label.setAttribute('aria-expanded', expanded ? 'true' : 'false');
    const rarity = Math.max(1, Math.min(5, Number(group.rarity) || 1));
    const icon = assetImage(starIconUrls(rarity), 'card-rarity-icon', `${rarity}星`);
    const text = document.createElement('span');
    text.textContent = `${group.cardIds.length} 张`;
    const chevron = document.createElement('span');
    chevron.className = 'card-rarity-chevron';
    chevron.append(buttonIcon('chevronDown'));
    if (icon) {
      label.append(icon);
    }
    label.append(text, chevron);
    label.addEventListener('click', () => toggleCardGroup(group.key));
    const content = document.createElement('div');
    content.className = 'card-rarity-group-content';
    if (expanded) {
      for (const cardId of group.cardIds.sort(numericStringSort)) {
        content.append(cardDetailRow(cardId, cardList[cardId]));
      }
    }
    section.append(label, content);
    return section;
  }

  function ensureDefaultExpandedAttributes(group, attributeGroups) {
    const defaultKey = attributeDefaultKey(group.key);
    if (expandedGroups.has(defaultKey)) {
      return;
    }
    for (const attributeGroup of attributeGroups) {
      expandedGroups.add(attributeGroup.key);
    }
    expandedGroups.add(defaultKey);
  }

  function ensureDefaultExpandedRarities(attributeKey, rarityGroups) {
    const defaultKey = rarityDefaultKey(attributeKey);
    if (expandedGroups.has(defaultKey)) {
      return;
    }
    for (const group of rarityGroups.slice(0, 2)) {
      expandedGroups.add(group.key);
    }
    expandedGroups.add(defaultKey);
  }

  function cardEntityCell(cardId, config) {
    const content = document.createElement('div');
    content.className = 'entity-content';
    const icon = assetImage(cardIconUrls(cardId, config), 'entity-icon', cardName(cardId));
    const name = document.createElement('span');
    name.className = 'entity-name';
    name.textContent = cardName(cardId);
    const meta = document.createElement('span');
    meta.className = 'entity-meta';
    meta.textContent = `ID: ${cardId}`;
    if (icon) {
      content.append(icon);
    }
    content.append(name, meta);
    return content;
  }

  function toggleCardGroup(groupKey) {
    if (expandedGroups.has(groupKey)) {
      expandedGroups.delete(groupKey);
    } else {
      expandedGroups.add(groupKey);
    }
    renderCards(getPlayer());
  }

  return {
    expandCardGroupForCard,
    renderCards,
  };

  function inputControl(options) {
    const cell = inputCell(options);
    return unwrapCell(cell);
  }

  function checkboxControl({ label, checked, disabled = false, onChange }) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'card-list-check-toggle';
    button.classList.toggle('is-active', checked);
    button.disabled = disabled;
    button.setAttribute('aria-pressed', checked ? 'true' : 'false');
    button.textContent = label;
    button.addEventListener('click', () => onChange(!checked));
    return button;
  }

  function cardField(label, control, variant = '') {
    const field = document.createElement(variant === 'check' ? 'div' : 'label');
    field.className = `card-list-field${variant ? ` is-${variant}` : ''}`;
    if (variant !== 'check') {
      const text = document.createElement('span');
      text.textContent = label;
      field.append(text);
    }
    field.append(control);
    return field;
  }

  function emptyCardMessage(message) {
    return emptyMessage(message, 'card-list-empty');
  }
}

function cardCharacterGroupSort(leftGroup, rightGroup) {
  const characterOrder = (leftGroup.characterId ?? Number.MAX_SAFE_INTEGER)
    - (rightGroup.characterId ?? Number.MAX_SAFE_INTEGER);
  if (characterOrder !== 0) {
    return characterOrder;
  }
  return leftGroup.key.localeCompare(rightGroup.key);
}

function cardAttributeGroupSort(leftGroup, rightGroup) {
  const attributeOrder = attributeSortIndex(leftGroup.attribute)
    - attributeSortIndex(rightGroup.attribute);
  if (attributeOrder !== 0) {
    return attributeOrder;
  }
  return leftGroup.key.localeCompare(rightGroup.key);
}

function attributeSortIndex(attribute) {
  return ATTRIBUTE_VALUES_WITH_ALL.indexOf(attribute) + 1 || 99;
}

function rarityDefaultKey(attributeKey) {
  return `rarity-default:${attributeKey}`;
}

function attributeDefaultKey(characterKey) {
  return `attribute-default:${characterKey}`;
}
