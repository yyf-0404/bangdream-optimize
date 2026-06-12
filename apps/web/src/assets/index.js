export function cardIconUrls({ cardId, card, illustTrainingStatus = false }) {
  const resourceSetName = safeAssetName(card?.resourceSetName);
  const normalizedCardId = positiveIntegerOrUndefined(cardId);
  if (!resourceSetName || normalizedCardId == null) {
    return [];
  }

  const base = assetBaseUrl();
  const servers = cardAssetServers(card);
  const thumbFolder = cardThumbFolder(normalizedCardId);
  return servers.flatMap((server) =>
    cardIconSuffixes(card, illustTrainingStatus).map((suffix) =>
      `${base}/${server}/thumb/chara/${thumbFolder}/${resourceSetName}_${suffix}.png`,
    ),
  );
}

export function cardTrainingStatusList(card) {
  const training = card?.stat?.training;
  if (Number(card?.rarity) < 3 || training == null) {
    return [false];
  }
  if (
    finiteNumberOrZero(training.performance) === 0
    && finiteNumberOrZero(training.technique) === 0
    && finiteNumberOrZero(training.visual) === 0
  ) {
    return [true];
  }
  return [false, true];
}

export function normalizeTrainingStatus(statuses, value) {
  const preferred = booleanOrDefault(value, statuses.includes(true));
  if (statuses.includes(preferred)) {
    return preferred;
  }
  return statuses[statuses.length - 1] ?? false;
}

export function characterIconUrls(characterId) {
  const normalizedId = positiveIntegerOrUndefined(characterId);
  return normalizedId == null ? [] : [
    `${assetOriginUrl()}/res/icon/chara_icon_${normalizedId}.png`,
  ];
}

export function songCoverUrls({ songId, song }) {
  const jacketImage = safeAssetName(firstAssetValue(song?.jacketImage));
  if (!jacketImage) {
    return [];
  }

  const base = assetBaseUrl();
  const server = assetServer();
  const folder = `musicjacket${musicJacketBucket(songId)}`;
  const fallbackFolder = `musicjacket${legacyMusicJacketBucket(songId)}`;
  const normalizedJacket = jacketImage.toLowerCase();
  return [
    `${base}/${server}/musicjacket/${folder}_rip/assets-star-forassetbundle-startapp-musicjacket-${folder}-${normalizedJacket}-jacket.png`,
    `${base}/${server}/musicjacket/${fallbackFolder}_rip/assets-star-forassetbundle-startapp-musicjacket-${fallbackFolder}-${normalizedJacket}-jacket.png`,
  ];
}

export function bandIconUrls(bandId) {
  const id = positiveIntegerOrUndefined(bandId);
  if (id == null || id >= 1000) {
    return [];
  }
  return [
    `${assetOriginUrl()}/res/icon/band_${id}.svg`,
  ];
}

export function attributeIconUrls(attribute) {
  const normalized = normalizedAttribute(attribute);
  if (!normalized || normalized === 'all') {
    return [];
  }
  return [
    `${assetOriginUrl()}/res/icon/${normalized}.svg`,
  ];
}

export function starIconUrls(rarity) {
  const normalizedRarity = positiveIntegerOrUndefined(rarity);
  return normalizedRarity == null ? [] : [
    `${assetOriginUrl()}/res/icon/star_${normalizedRarity}.png`,
  ];
}

export function serverIconUrls(server) {
  const normalized = String(server ?? '').toLowerCase();
  return serverIndex(normalized) == null ? [] : [
    `${assetOriginUrl()}/res/icon/${normalized}.svg`,
  ];
}

export function assetImage(urls, className, alt = '') {
  const candidates = normalizeAssetUrls(urls);
  if (candidates.length === 0) {
    return null;
  }
  const image = document.createElement('img');
  image.className = className;
  image.alt = alt == null ? '' : String(alt);
  image.loading = 'lazy';
  image.decoding = 'async';
  image.dataset.assetIndex = '0';
  image.src = candidates[0];
  image.addEventListener('error', () => {
    const nextIndex = Number(image.dataset.assetIndex) + 1;
    if (nextIndex < candidates.length) {
      image.dataset.assetIndex = String(nextIndex);
      image.src = candidates[nextIndex];
      return;
    }
    image.hidden = true;
  });
  return image;
}

function cardIconSuffixes(card, illustTrainingStatus) {
  const statuses = cardTrainingStatusList(card);
  const preferredStatus = normalizeTrainingStatus(statuses, illustTrainingStatus);
  return [
    preferredStatus,
    ...statuses.filter((status) => status !== preferredStatus),
  ].map((status) => status ? 'after_training' : 'normal');
}

function normalizeAssetUrls(urls) {
  return (Array.isArray(urls) ? urls : [urls]).filter(hasText);
}

function musicJacketBucket(songId) {
  const id = positiveIntegerOrUndefined(songId) ?? 0;
  if (id === 13 || id === 40) {
    return '30';
  }
  return String(Math.ceil(id / 10) * 10);
}

function legacyMusicJacketBucket(songId) {
  const id = positiveIntegerOrUndefined(songId) ?? 0;
  if (id <= 10) {
    return '001';
  }
  return padNumber(Math.ceil(id / 10) * 10, 3);
}

function cardThumbFolder(cardId) {
  const rip = cardId < 9999
    ? padNumber(Math.floor(cardId / 50), 3)
    : '200';
  return `card00${rip}_rip`;
}

function cardAssetServers(card) {
  const preferred = assetServer();
  const releasedAt = Array.isArray(card?.releasedAt) ? card.releasedAt : undefined;
  if (!releasedAt) {
    return [preferred];
  }

  const releasedServers = uniqueServers([preferred, ...assetServerPriority()])
    .filter((server) => isReleasedOnServer(releasedAt, server));
  return releasedServers.length > 0 ? releasedServers : [preferred];
}

function assetBaseUrl() {
  const configured = globalThis.BANGDREAM_OPTIMIZE_CONFIG?.assetBaseUrl;
  return String(configured || 'https://bestdori.com/assets').replace(/\/$/, '');
}

function assetOriginUrl() {
  const configured = globalThis.BANGDREAM_OPTIMIZE_CONFIG?.assetOriginUrl;
  if (configured) {
    return String(configured).replace(/\/$/, '');
  }
  return assetBaseUrl().replace(/\/assets$/, '');
}

function assetServer() {
  return String(globalThis.BANGDREAM_OPTIMIZE_CONFIG?.assetServer || 'jp').toLowerCase();
}

function assetServerPriority() {
  const configured = globalThis.BANGDREAM_OPTIMIZE_CONFIG?.assetServerPriority;
  if (Array.isArray(configured)) {
    return configured;
  }
  return ['cn', 'jp', 'tw', 'en', 'kr'];
}

function isReleasedOnServer(releasedAt, server) {
  const index = serverIndex(server);
  return index != null && releasedAt[index] != null;
}

function serverIndex(server) {
  switch (String(server ?? '').toLowerCase()) {
    case 'jp':
      return 0;
    case 'en':
      return 1;
    case 'tw':
      return 2;
    case 'cn':
      return 3;
    case 'kr':
      return 4;
    default:
      return undefined;
  }
}

function uniqueServers(servers) {
  const result = [];
  const seen = new Set();
  for (const server of servers) {
    const normalized = String(server ?? '').toLowerCase();
    if (serverIndex(normalized) == null || seen.has(normalized)) {
      continue;
    }
    seen.add(normalized);
    result.push(normalized);
  }
  return result;
}

function safeAssetName(value) {
  const text = String(value ?? '').trim();
  return /^[a-zA-Z0-9_-]+$/.test(text) ? text : '';
}

function firstAssetValue(value) {
  if (Array.isArray(value)) {
    return value.find(hasText);
  }
  return value;
}

function normalizedAttribute(value) {
  const attribute = String(value ?? '').toLowerCase();
  return ['cool', 'happy', 'pure', 'powerful', 'all'].includes(attribute)
    ? attribute
    : undefined;
}

function positiveIntegerOrUndefined(value) {
  const number = Number(value);
  return Number.isInteger(number) && number > 0 ? number : undefined;
}

function finiteNumberOrZero(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : 0;
}

function booleanOrDefault(value, fallback) {
  return typeof value === 'boolean' ? value : fallback;
}

function hasText(value) {
  return value != null && String(value).trim() !== '';
}

function padNumber(value, width) {
  return String(value).padStart(width, '0');
}
