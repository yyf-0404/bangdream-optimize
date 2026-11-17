export const ATTRIBUTE_VALUES = Object.freeze(['cool', 'happy', 'pure', 'powerful']);
export const ATTRIBUTE_VALUES_WITH_ALL = Object.freeze([...ATTRIBUTE_VALUES, 'all']);

export function optionText(id, label) {
  return `${id} · ${label}`;
}

export function normalizedAttribute(value) {
  const attribute = String(value ?? '').toLowerCase();
  return ATTRIBUTE_VALUES_WITH_ALL.includes(attribute)
    ? attribute
    : undefined;
}

export function attributeLabel(attribute) {
  if (String(attribute).includes(',')) {
    return 'All';
  }
  return {
    cool: 'Cool',
    happy: 'Happy',
    pure: 'Pure',
    powerful: 'Powerful',
    all: 'All',
  }[attribute] ?? '未知属性';
}

export function selectedBandLabel(value) {
  if (value == null || value === '') {
    return '-';
  }
  if (String(value).includes(',')) {
    return '乐队 All';
  }
  const bandLabels = {
    PoppinParty: 'Poppin\'Party',
    Afterglow: 'Afterglow',
    HelloHappyWorld: 'Hello, Happy World!',
    PastelPalettes: 'Pastel*Palettes',
    Roselia: 'Roselia',
    RaiseASuilen: 'RAISE A SUILEN',
    Morfonica: 'Morfonica',
    MyGO: 'MyGO!!!!!',
    AveMujica: 'Ave Mujica',
    Everyone: '乐队 All',
  };
  return bandLabels[value] ?? String(value);
}

export function bandLabel(bandId) {
  return {
    1: 'Poppin\'Party',
    2: 'Afterglow',
    3: 'Hello, Happy World!',
    4: 'Pastel*Palettes',
    5: 'Roselia',
    18: 'RAISE A SUILEN',
    21: 'Morfonica',
    45: 'MyGO!!!!!',
    50: 'Ave Mujica',
    1000: '乐队 All',
  }[Number(bandId)] ?? `乐队 ${bandId}`;
}

export function magazineLabel(value) {
  return {
    performance: '演出',
    technique: '技巧',
    visual: '形象',
  }[value] ?? String(value ?? '-');
}

export function eventTypeLabel(value) {
  return {
    medley: '组曲LIVE',
    challenge: '挑战LIVE',
    versus: '竞演LIVE',
    live_try: 'LIVE试炼',
    festival: '团队LIVE FES',
    mission_live: '任务LIVE',
  }[value] ?? String(value ?? '-');
}

export function difficultyLabel(value) {
  return {
    0: 'Easy',
    1: 'Normal',
    2: 'Hard',
    3: 'Expert',
    4: 'Special',
  }[Number(value)] ?? `难度 ${value}`;
}

export function formatInteger(value) {
  const number = Number(value);
  return Number.isFinite(number) ? new Intl.NumberFormat('zh-CN').format(number) : '-';
}

export function fireCostForMultiplier(value) {
  return {
    1: 0,
    5: 1,
    10: 2,
    15: 3,
  }[Number(value)] ?? 0;
}

export function totalFireCost(plays = []) {
  return (Array.isArray(plays) ? plays : []).reduce((total, play) => {
    const count = Number(play?.count);
    const normalizedCount = Number.isSafeInteger(count) && count > 0 ? count : 0;
    return total + fireCostForMultiplier(play?.fireMultiplier) * normalizedCount;
  }, 0);
}

export function formatMs(value) {
  const number = Number(value);
  return Number.isFinite(number) ? `${number.toFixed(1)} ms` : '-';
}

export function formatDateTime(value, fallback = '时间未知') {
  const time = Number(value);
  if (!Number.isFinite(time)) {
    return fallback;
  }
  return new Intl.DateTimeFormat('zh-CN', {
    hour12: false,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(time));
}

export function formatNumberInput(value) {
  const number = Number(value);
  return Number.isFinite(number) ? String(number) : '0';
}

export function formatRatePercent(value) {
  return formatPercentNumber(Number(value) * 100);
}

export function formatRatePercentInput(value) {
  const percent = Number(value) * 100;
  return Number.isFinite(percent)
    ? Number(percent.toFixed(1))
    : 0;
}

export function formatPercentNumber(value) {
  const number = Number(value);
  return Number.isFinite(number) ? `${number.toFixed(1)}%` : '0.0%';
}

export function formatCompactPercentNumber(value, fractionDigits = 2) {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return '0%';
  }
  return `${number.toFixed(fractionDigits).replace(/\.?0+$/, '')}%`;
}

export function readOptionalInteger(value) {
  const trimmed = value.trim();
  if (!trimmed) {
    return undefined;
  }
  if (!/^\d+$/.test(trimmed)) {
    throw new Error(`活动 ID 无效：${value}`);
  }
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed)) {
    throw new Error(`活动 ID 无效：${value}`);
  }
  return parsed;
}

export function parseNonNegativeInteger(value, label) {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) {
    throw new Error(`${label} 无效：${value}`);
  }
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed) || parsed < 0) {
    throw new Error(`${label} 无效：${value}`);
  }
  return parsed;
}

export function parseEntityId(value, label) {
  const trimmed = value.trim();
  const match = trimmed.match(/^(\d+)(?:\b|\s|$|[·-])/);
  if (!match) {
    throw new Error(`${label} 无效：${value}`);
  }
  const parsed = Number(match[1]);
  if (!Number.isInteger(parsed) || parsed <= 0) {
    throw new Error(`${label} 无效：${value}`);
  }
  return parsed;
}

export function finiteNumberOrZero(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : 0;
}

export function positiveIntegerOrUndefined(value) {
  const number = Number(value);
  return Number.isInteger(number) && number > 0 ? number : undefined;
}

export function positiveIntegerOrDefault(value, fallback) {
  const number = Number(value);
  return Number.isInteger(number) && number > 0 ? number : fallback;
}

export function integerOrZero(value) {
  const number = Number(value);
  return Number.isInteger(number) ? number : 0;
}

export function booleanOrDefault(value, fallback) {
  return typeof value === 'boolean' ? value : fallback;
}

export function compactJoin(parts, separator = ' · ') {
  return parts.filter(hasText).join(separator);
}

export function hasText(value) {
  return value != null && String(value).trim() !== '';
}

export function numericStringSort(left, right) {
  return Number(left) - Number(right);
}
