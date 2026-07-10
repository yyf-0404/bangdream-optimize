import {
  attributeIconUrls,
  bandIconUrls,
} from '../assets/index.js?v=3';
import {
  attributeLabel,
  bandLabel,
  formatCompactPercentNumber,
  normalizedAttribute,
  numericStringSort,
  positiveIntegerOrUndefined,
} from '../utils.js?v=3';

export function createAreaItemHelpers({
  getAreaItemRecords,
  recordWithFix,
  serverScopedValue,
  maxAreaItemLevel,
}) {
  function areaItemGroups(player) {
    const groups = new Map();
    for (const areaItemId of mergedEntityIds(getAreaItemRecords(), player.areaItem)) {
      const areaItem = recordWithFix('areaItems', 'areaItemsFix', areaItemId);
      if (!areaItem) {
        continue;
      }
      const bucket = areaItemBucket(areaItemId, areaItem);
      if (!bucket) {
        continue;
      }

      const group = groups.get(bucket.key) ?? {
        key: bucket.key,
        label: bucket.label,
        bandId: bucket.bandId,
        attribute: bucket.attribute,
        isAll: bucket.isAll,
        category: bucket.category,
        categoryLabel: bucket.categoryLabel,
        categorySort: bucket.categorySort,
        rate: zeroRate(),
        areaItemIds: [],
      };
      group.areaItemIds.push(areaItemId);

      const level = positiveIntegerOrUndefined(player.areaItem[areaItemId]?.level) ?? 0;
      if (level > 0) {
        addRate(group.rate, areaItemRateAt(areaItem, level));
      }
      groups.set(bucket.key, group);
    }

    applyAreaItemShellCoffeeSummaryAdjustment(player, groups);

    return [...groups.values()]
      .map((group) => ({
        ...group,
        areaItemIds: group.areaItemIds.sort(numericStringSort),
      }))
      .sort((left, right) =>
        (left.categorySort ?? 99) - (right.categorySort ?? 99)
        || Number(Boolean(left.isAll)) - Number(Boolean(right.isAll))
        || left.key.localeCompare(right.key, undefined, { numeric: true }),
      );
  }

  function areaItemIconUrls(areaItemId) {
    const areaItem = recordWithFix('areaItems', 'areaItemsFix', areaItemId);
    const bucket = areaItem ? areaItemBucket(areaItemId, areaItem) : undefined;
    return areaItemGroupIconUrls(bucket);
  }

  function allAreaItemsAreMaxed(player) {
    const areaItemIds = Object.keys(getAreaItemRecords() ?? {});
    return areaItemIds.length > 0
      && areaItemIds.every((areaItemId) =>
        (positiveIntegerOrUndefined(player.areaItem?.[areaItemId]?.level) ?? 0)
        === maxAreaItemLevel(areaItemId),
      );
  }

  function applyAreaItemShellCoffeeSummaryAdjustment(player, summaries) {
    const shell = player.areaItem['59'];
    const coffee = player.areaItem['72'];
    if (!shell || !coffee) {
      return;
    }

    const shellLevel = positiveIntegerOrUndefined(shell.level) ?? 0;
    const coffeeLevel = positiveIntegerOrUndefined(coffee.level) ?? 0;
    if (shellLevel <= 0 || coffeeLevel <= 0) {
      return;
    }

    const adjustmentId = shellLevel < coffeeLevel ? '59' : '72';
    const adjustmentItem = recordWithFix('areaItems', 'areaItemsFix', adjustmentId);
    if (!adjustmentItem) {
      return;
    }

    const bucket = areaItemBucket(adjustmentId, adjustmentItem);
    const key = bucket?.key ?? 'attribute:cool,happy,powerful,pure';
    const summary = summaries.get(key) ?? {
      key,
      label: '属性 All',
      attribute: 'all',
      isAll: true,
      category: 'attribute',
      categoryLabel: '属性道具',
      categorySort: 20,
      rate: zeroRate(),
      areaItemIds: [],
    };
    const adjustmentLevel = adjustmentId === '59' ? shellLevel : coffeeLevel;
    subtractRate(summary.rate, areaItemRateAt(adjustmentItem, adjustmentLevel));
    summaries.set(key, summary);
  }

  function areaItemRateAt(areaItem, level) {
    return {
      performance: serverScopedRate(areaItem.performance?.[String(level)]),
      technique: serverScopedRate(areaItem.technique?.[String(level)]),
      visual: serverScopedRate(areaItem.visual?.[String(level)]),
    };
  }

  function serverScopedRate(value) {
    const number = Number(serverScopedValue(value));
    return Number.isFinite(number) ? number : 0;
  }

  return {
    allAreaItemsAreMaxed,
    areaItemGroups,
    areaItemIconUrls,
  };
}

export function areaItemGroupIconUrls(group) {
  return [
    ...bandIconUrls(group?.bandId),
    ...attributeIconUrls(group?.attribute),
  ];
}

export function formatAreaItemRate(rate) {
  if (
    rate.performance === rate.technique
    && rate.technique === rate.visual
  ) {
    return formatCompactPercentNumber(rate.performance);
  }
  const parts = [
    ['演出', rate.performance],
    ['技巧', rate.technique],
    ['形象', rate.visual],
  ].filter(([, value]) => value !== 0);
  if (parts.length === 0) {
    return '0%';
  }
  return parts
    .map(([label, value]) => `${label} ${formatCompactPercentNumber(value)}`)
    .join(' / ');
}

function areaItemBucket(areaItemId, areaItem) {
  const id = Number(areaItemId);
  const targetBandIds = normalizedIntegerArray(areaItem.targetBandIds);
  const targetAttributes = normalizedAttributeArray(areaItem.targetAttributes);

  if (targetBandIds.length === 1) {
    const bandId = String(targetBandIds[0]);
    return {
      key: `band:${bandId}`,
      label: bandLabel(bandId),
      bandId: Number(bandId),
      category: 'band',
      categoryLabel: '乐队道具',
      categorySort: 10,
    };
  }

  if (targetAttributes.length === 1) {
    const attribute = targetAttributes[0];
    return {
      key: `attribute:${attribute}`,
      label: `属性 ${attributeLabel(attribute)}`,
      attribute,
      category: 'attribute',
      categoryLabel: '属性道具',
      categorySort: 20,
    };
  }

  if (id >= 80) {
    const magazine = {
      80: ['performance', '杂志 演出'],
      81: ['technique', '杂志 技巧'],
      82: ['visual', '杂志 形象'],
    }[id];
    return magazine
      ? {
        key: `magazine:${magazine[0]}`,
        label: magazine[1],
        category: 'magazine',
        categoryLabel: '杂志道具',
        categorySort: 30,
      }
      : undefined;
  }

  if (id >= 73) {
    const bandKey = targetBandKey(targetBandIds);
    return {
      key: `band:${bandKey}`,
      label: bandLabel(1000),
      bandId: 1000,
      isAll: true,
      category: 'band',
      categoryLabel: '乐队道具',
      categorySort: 10,
    };
  }

  const attributeKey = targetAttributeKey(targetAttributes);
  return {
    key: `attribute:${attributeKey}`,
    label: '属性 All',
    attribute: 'all',
    isAll: true,
    category: 'attribute',
    categoryLabel: '属性道具',
    categorySort: 20,
  };
}

function zeroRate() {
  return {
    performance: 0,
    technique: 0,
    visual: 0,
  };
}

function addRate(target, source) {
  target.performance += source.performance;
  target.technique += source.technique;
  target.visual += source.visual;
}

function subtractRate(target, source) {
  target.performance -= source.performance;
  target.technique -= source.technique;
  target.visual -= source.visual;
}

function normalizedIntegerArray(value) {
  return Array.isArray(value)
    ? value
      .map(Number)
      .filter((number) => Number.isInteger(number) && number > 0)
    : [];
}

function normalizedAttributeArray(value) {
  return Array.isArray(value)
    ? value.map(normalizedAttribute).filter((attribute) => attribute != null)
    : [];
}

function targetBandKey(targetBandIds) {
  const ids = [...new Set(targetBandIds)].sort((left, right) => left - right);
  return ids.length > 0 ? ids.join(',') : '1000';
}

function targetAttributeKey(targetAttributes) {
  const attributes = [...new Set(targetAttributes)].sort();
  return attributes.length > 0 ? attributes.join(',') : '~all';
}

function mergedEntityIds(records = {}, selected = {}) {
  return Object.keys({
    ...(records ?? {}),
    ...(selected ?? {}),
  }).sort(numericStringSort);
}
