const COMMON_CARD_LEVELS = [1, 20, 30, 50, 60];

export function createCompactProfileCodec({ normalizedPlayer } = {}) {
  // Compact tuples: card [id, level, training, illust, ep1, ep2, limitBreak, skill],
  // character [id, potential P/T/V, task P/T/V], area item [id, level].
  function buildCompactProfilePayload(player) {
    const source = normalizedPlayer(player ?? {});
    return {
      v: 1,
      c: buildCompactCards(source.cardList),
      b: buildCompactCharacterBonuses(source.characterBouns),
      a: buildCompactAreaItems(source.areaItem),
    };
  }

  async function parseCompactProfileExport(text) {
    let payload;
    try {
      payload = JSON.parse(text);
    } catch (error) {
      throw new Error(`配置导入 JSON 解析失败：${error.message}`);
    }

    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
      throw new Error('配置导入内容必须是 JSON 对象');
    }
    if (payload.v !== 1 && payload.v !== 2) {
      throw new Error(`不支持的配置版本：${payload.v}`);
    }
    if (!['gz+b64', 'bit1+b64'].includes(payload.t) && payload.t != null) {
      throw new Error(`不支持的配置压缩格式：${payload.t}`);
    }
    if (payload.v === 2 && payload.t !== 'bit1+b64') {
      throw new Error(`不支持的配置压缩格式：${payload.t ?? '未指定'}`);
    }
    if (typeof payload.d !== 'string' || !payload.d) {
      throw new Error('配置缺少 base64 压缩内容');
    }
    if (payload.t === 'bit1+b64') {
      return decodeBitProfilePayload(base64ToArrayBuffer(payload.d));
    }
    if (typeof DecompressionStream !== 'function') {
      throw new Error('当前环境不支持 DecompressionStream，无法解析 Base64 配置');
    }

    const compressed = base64ToArrayBuffer(payload.d);
    const decompressed = await Promise.race([
      new Response(new Blob([compressed]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer(),
      timeoutReject('Base64 配置解压超时'),
    ]);
    const compact = parseCompactProfilePayloadText(new TextDecoder().decode(decompressed));
    if (!compact || compact.v !== 1) {
      throw new Error('Base64 配置解码失败：版本不符合');
    }
    return compact;
  }

  function compactProfileToPlayer(compactProfile, basePlayer = {}) {
    if (!compactProfile || typeof compactProfile !== 'object') {
      throw new Error('配置内容无效');
    }
    return normalizedPlayer({
      ...basePlayer,
      cardList: compactCardsToPlayer(compactProfile.c),
      characterBouns: compactCharacterBonusesToPlayer(compactProfile.b),
      areaItem: compactAreaItemsToPlayer(compactProfile.a),
    });
  }

  async function compressProfilePayload(payload) {
    const encoded = encodeBitProfilePayload(payload);
    return {
      version: 2,
      type: 'bit1+b64',
      typeDisplay: 'bitstream+base64',
      data: base64FromArrayBuffer(encoded),
    };
  }

  async function compressProfilePayloadAsGzip(payload) {
    const text = JSON.stringify(payload);
    if (typeof CompressionStream !== 'function') {
      throw new Error('当前环境不支持 CompressionStream，无法导出压缩配置');
    }

    const compressInputStream = createTextReadableStream(text);
    if (!compressInputStream) {
      throw new Error('无法创建压缩输入流');
    }
    const compressed = await Promise.race([
      new Response(compressInputStream.pipeThrough(new CompressionStream('gzip'))).arrayBuffer(),
      timeoutReject('gzip 压缩超时'),
    ]);

    return {
      version: 1,
      type: 'gz+b64',
      typeDisplay: 'gzip+base64',
      data: base64FromArrayBuffer(compressed),
    };
  }

  return {
    buildCompactProfilePayload,
    compactProfileToPlayer,
    compressProfilePayload,
    compressProfilePayloadAsGzip,
    parseCompactProfileExport,
  };

  function encodeBitProfilePayload(payload) {
    const writer = new BitWriter();
    writeAscii(writer, 'BDO1');
    writeCardsSection(writer, Array.isArray(payload?.c) ? payload.c : []);
    writeAreaItemsSection(writer, Array.isArray(payload?.a) ? payload.a : []);
    writeCharacterBonusesSection(writer, Array.isArray(payload?.b) ? payload.b : []);
    return writer.toArrayBuffer();
  }

  function decodeBitProfilePayload(buffer) {
    const reader = new BitReader(buffer);
    const magic = readAscii(reader, 4);
    if (magic !== 'BDO1') {
      throw new Error('Base64 配置解码失败：二进制格式不符合');
    }
    return {
      v: 1,
      c: readCardsSection(reader),
      a: readAreaItemsSection(reader),
      b: readCharacterBonusesSection(reader),
    };
  }

  function writeCardsSection(writer, cards) {
    const normalizedCards = cards
      .filter((entry) => Array.isArray(entry) && parsePositiveIntegerId(entry[0]))
      .map((entry) => ({
        id: parsePositiveIntegerId(entry[0]),
        level: parseNonNegativeIntegerOrZero(entry[1], 1),
        training: entry[2] === 1 ? 1 : 0,
        illust: entry[3] === 1 ? 1 : 0,
        ep1: entry[4] === 1 ? 1 : 0,
        ep2: entry[5] === 1 ? 1 : 0,
        limitBreak: clampInteger(entry[6], 0, 4),
        skill: clampInteger(entry[7], 1, 5),
      }))
      .sort((left, right) => left.id - right.id);
    const blocks = groupCardBlocks(normalizedCards);
    writer.writeVarint(blocks.length);
    let previousBlock = 0;
    for (const block of blocks) {
      writer.writeVarint(block.blockId - previousBlock);
      previousBlock = block.blockId;
      const sparseBits = estimateSparseLocalBits(block.cards.map((entry) => entry.localId));
      const useBitset = sparseBits >= 256;
      writer.writeBit(useBitset ? 1 : 0);
      if (useBitset) {
        const locals = new Set(block.cards.map((entry) => entry.localId));
        for (let localId = 0; localId < 256; localId += 1) {
          writer.writeBit(locals.has(localId) ? 1 : 0);
        }
      } else {
        writer.writeVarint(block.cards.length);
        let previousLocal = 0;
        for (const entry of block.cards) {
          writer.writeVarint(entry.localId - previousLocal);
          previousLocal = entry.localId;
        }
      }
      writeCardStateColumns(writer, block.cards);
    }
  }

  function readCardsSection(reader) {
    const cards = [];
    const blockCount = reader.readVarint();
    let previousBlock = 0;
    for (let blockIndex = 0; blockIndex < blockCount; blockIndex += 1) {
      const blockId = previousBlock + reader.readVarint();
      previousBlock = blockId;
      const mode = reader.readBit();
      const localIds = [];
      if (mode === 1) {
        for (let localId = 0; localId < 256; localId += 1) {
          if (reader.readBit() === 1) {
            localIds.push(localId);
          }
        }
      } else {
        const count = reader.readVarint();
        let previousLocal = 0;
        for (let index = 0; index < count; index += 1) {
          const localId = previousLocal + reader.readVarint();
          previousLocal = localId;
          localIds.push(localId);
        }
      }
      const states = readCardStateColumns(reader, localIds.length);
      for (let index = 0; index < localIds.length; index += 1) {
        const id = blockId * 256 + localIds[index];
        cards.push(cardStateTuple(id, states[index]));
      }
    }
    return cards;
  }

  function groupCardBlocks(cards) {
    const blockMap = new Map();
    for (const entry of cards) {
      const blockId = Math.floor(entry.id / 256);
      const localId = entry.id % 256;
      if (!blockMap.has(blockId)) {
        blockMap.set(blockId, []);
      }
      blockMap.get(blockId).push({ ...entry, localId });
    }
    return [...blockMap.entries()]
      .sort((left, right) => left[0] - right[0])
      .map(([blockId, blockCards]) => ({
        blockId,
        cards: blockCards.sort((left, right) => left.localId - right.localId),
      }));
  }

  function writeCardStateColumns(writer, cards) {
    for (const entry of cards) {
      writeCardFlags(writer, cardFlags(entry));
    }
    for (const entry of cards) {
      writeCardLevel(writer, entry.level);
    }
    for (const entry of cards) {
      writeMasterSkillCombined(writer, masterSkillCombined(entry.limitBreak, entry.skill));
    }
  }

  function readCardStateColumns(reader, count) {
    const flags = [];
    for (let index = 0; index < count; index += 1) {
      flags.push(readCardFlags(reader));
    }
    const levels = [];
    for (let index = 0; index < count; index += 1) {
      levels.push(readCardLevel(reader));
    }
    const masterSkills = [];
    for (let index = 0; index < count; index += 1) {
      masterSkills.push(readMasterSkillCombined(reader));
    }
    return flags.map((flagValue, index) => ({
      flags: flagValue,
      level: levels[index],
      masterSkill: masterSkills[index],
    }));
  }

  function cardFlags(entry) {
    return (
      (entry.training << 0)
      | (entry.illust << 1)
      | (entry.ep1 << 2)
      | (entry.ep2 << 3)
    );
  }

  function writeCardFlags(writer, flags) {
    const normalized = clampInteger(flags, 0, 15);
    if (normalized === 15) {
      writer.writeBit(0);
      return;
    }
    writer.writeBit(1);
    if (normalized === 12) {
      writer.writeBit(0);
      return;
    }
    writer.writeBit(1);
    if (normalized === 0) {
      writer.writeBit(0);
      return;
    }
    writer.writeBit(1);
    if (normalized === 13) {
      writer.writeBit(0);
      return;
    }
    writer.writeBit(1);
    writer.writeBits(normalized, 4);
  }

  function readCardFlags(reader) {
    if (reader.readBit() === 0) {
      return 15;
    }
    if (reader.readBit() === 0) {
      return 12;
    }
    if (reader.readBit() === 0) {
      return 0;
    }
    if (reader.readBit() === 0) {
      return 13;
    }
    return reader.readBits(4);
  }

  function cardStateTuple(id, state) {
    const flags = state.flags;
    const [limitBreak, skill] = splitMasterSkillCombined(state.masterSkill);
    return [
      id,
      state.level,
      flags & 1 ? 1 : 0,
      flags & 2 ? 1 : 0,
      flags & 4 ? 1 : 0,
      flags & 8 ? 1 : 0,
      limitBreak,
      skill,
    ];
  }

  function writeCardLevel(writer, level) {
    const commonIndex = COMMON_CARD_LEVELS.indexOf(level);
    if (commonIndex >= 0) {
      writer.writeBit(0);
      writer.writeBits(commonIndex, 3);
      return;
    }
    writer.writeBit(1);
    writer.writeVarint(Math.max(0, level));
  }

  function readCardLevel(reader) {
    if (reader.readBit() === 0) {
      return COMMON_CARD_LEVELS[reader.readBits(3)] ?? 60;
    }
    return reader.readVarint();
  }

  function masterSkillCombined(limitBreak, skill) {
    return clampInteger(limitBreak, 0, 4) * 5 + (clampInteger(skill, 1, 5) - 1);
  }

  function splitMasterSkillCombined(combined) {
    return [Math.floor(combined / 5), (combined % 5) + 1];
  }

  function writeMasterSkillCombined(writer, combined) {
    const [master, skillLevel] = splitMasterSkillCombined(clampInteger(combined, 0, 24));
    if (master === 0 && skillLevel === 1) {
      writer.writeBit(0);
      return;
    }
    writer.writeBit(1);
    if (master === 0 && skillLevel === 5) {
      writer.writeBit(0);
      return;
    }
    writer.writeBit(1);
    if (master === 4 && skillLevel === 5) {
      writer.writeBit(0);
      return;
    }
    writer.writeBit(1);
    writer.writeBits(combined, 5);
  }

  function readMasterSkillCombined(reader) {
    if (reader.readBit() === 0) {
      return 0;
    }
    if (reader.readBit() === 0) {
      return 4;
    }
    if (reader.readBit() === 0) {
      return 24;
    }
    return reader.readBits(5);
  }

  function writeAreaItemsSection(writer, items) {
    const blocks = groupSmallIdBlocks(items, ([id, item]) => ({
      id,
      level: clampInteger(item?.[1], 0, 31),
    }));
    writer.writeVarint(blocks.length);
    writeSmallIdBlocks(writer, blocks, (entry) => writer.writeBits(entry.level, 5));
  }

  function readAreaItemsSection(reader) {
    return readSmallIdBlocks(reader, () => [
      reader.currentId,
      reader.readBits(5),
    ]);
  }

  function writeCharacterBonusesSection(writer, bonuses) {
    const blocks = groupSmallIdBlocks(bonuses, ([id, bonus]) => ({
      id,
      values: [
        scaledRate(bonus?.[1]),
        scaledRate(bonus?.[2]),
        scaledRate(bonus?.[3]),
        scaledRate(bonus?.[4]),
        scaledRate(bonus?.[5]),
        scaledRate(bonus?.[6]),
      ],
    }));
    writer.writeVarint(blocks.length);
    writeSmallIdBlocks(writer, blocks, (entry) => {
      for (const value of entry.values) {
        writer.writeBits(value, 10);
      }
    });
  }

  function readCharacterBonusesSection(reader) {
    return readSmallIdBlocks(reader, () => {
      const values = [];
      for (let index = 0; index < 6; index += 1) {
        values.push(reader.readBits(10) / 1000);
      }
      return [reader.currentId, ...values];
    });
  }

  function groupSmallIdBlocks(entries, mapEntry) {
    const blockMap = new Map();
    for (const entry of entries) {
      if (!Array.isArray(entry)) {
        continue;
      }
      const id = parsePositiveIntegerId(entry[0]);
      if (!id) {
        continue;
      }
      const blockId = Math.floor(id / 64);
      const localId = id % 64;
      if (!blockMap.has(blockId)) {
        blockMap.set(blockId, []);
      }
      blockMap.get(blockId).push({ ...mapEntry([id, entry]), localId });
    }
    return [...blockMap.entries()]
      .sort((left, right) => left[0] - right[0])
      .map(([blockId, blockEntries]) => ({
        blockId,
        entries: blockEntries.sort((left, right) => left.localId - right.localId),
      }));
  }

  function writeSmallIdBlocks(writer, blocks, writeEntry) {
    let previousBlock = 0;
    for (const block of blocks) {
      writer.writeVarint(block.blockId - previousBlock);
      previousBlock = block.blockId;
      const locals = new Set(block.entries.map((entry) => entry.localId));
      for (let localId = 0; localId < 64; localId += 1) {
        writer.writeBit(locals.has(localId) ? 1 : 0);
      }
      for (const entry of block.entries) {
        writeEntry(entry);
      }
    }
  }

  function readSmallIdBlocks(reader, readEntry) {
    const list = [];
    const blockCount = reader.readVarint();
    let previousBlock = 0;
    for (let blockIndex = 0; blockIndex < blockCount; blockIndex += 1) {
      const blockId = previousBlock + reader.readVarint();
      previousBlock = blockId;
      const localIds = [];
      for (let localId = 0; localId < 64; localId += 1) {
        if (reader.readBit() === 1) {
          localIds.push(localId);
        }
      }
      for (const localId of localIds) {
        reader.currentId = blockId * 64 + localId;
        list.push(readEntry());
      }
    }
    delete reader.currentId;
    return list;
  }
}

function parseCompactProfilePayloadText(text) {
  try {
    const payload = JSON.parse(text);
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
      throw new Error('配置必须是 JSON 对象');
    }
    return {
      v: payload.v ?? 1,
      c: Array.isArray(payload.c) ? payload.c : [],
      b: Array.isArray(payload.b) ? payload.b : [],
      a: Array.isArray(payload.a) ? payload.a : [],
    };
  } catch (error) {
    throw new Error(`Base64 配置内容解析失败：${error.message}`);
  }
}

function compactCardsToPlayer(cards) {
  const cardList = {};
  if (!Array.isArray(cards)) {
    return cardList;
  }
  for (const entry of cards) {
    if (!Array.isArray(entry) || entry.length < 8) {
      continue;
    }
    const id = parsePositiveIntegerId(entry[0]);
    if (!id) {
      continue;
    }
    cardList[String(id)] = {
      level: parseNonNegativeIntegerOrZero(entry[1]),
      training: entry[2] === 1,
      illustTrainingStatus: entry[3] === 1,
      episodes: [
        entry[4] === 1,
        entry[5] === 1,
      ],
      limitBreakRank: parseNonNegativeIntegerOrZero(entry[6]),
      skillLevel: parseNonNegativeIntegerOrZero(entry[7], 1),
    };
  }
  return cardList;
}

function compactCharacterBonusesToPlayer(characterBonuses) {
  const bonuses = {};
  if (!Array.isArray(characterBonuses)) {
    return bonuses;
  }
  for (const entry of characterBonuses) {
    if (!Array.isArray(entry) || entry.length < 7) {
      continue;
    }
    const id = parsePositiveIntegerId(entry[0]);
    if (!id) {
      continue;
    }
    const potential = {
      performance: toNumber(entry[1]),
      technique: toNumber(entry[2]),
      visual: toNumber(entry[3]),
    };
    const characterTask = {
      performance: toNumber(entry[4]),
      technique: toNumber(entry[5]),
      visual: toNumber(entry[6]),
    };
    if (
      potential.performance === 0
      && potential.technique === 0
      && potential.visual === 0
      && characterTask.performance === 0
      && characterTask.technique === 0
      && characterTask.visual === 0
    ) {
      continue;
    }
    bonuses[String(id)] = { potential, characterTask };
  }
  return bonuses;
}

function compactAreaItemsToPlayer(items) {
  const areaItem = {};
  if (!Array.isArray(items)) {
    return areaItem;
  }
  for (const entry of items) {
    if (!Array.isArray(entry) || entry.length < 2) {
      continue;
    }
    const id = parsePositiveIntegerId(entry[0]);
    if (!id) {
      continue;
    }
    areaItem[String(id)] = {
      level: parseNonNegativeIntegerOrZero(entry[1]),
    };
  }
  return areaItem;
}

function buildCompactCards(cardList) {
  const list = [];
  if (!cardList || typeof cardList !== 'object') {
    return list;
  }
  for (const [cardId, config] of Object.entries(cardList)) {
    const id = parsePositiveIntegerId(cardId);
    if (!id || !config || typeof config !== 'object') {
      continue;
    }
    list.push([
      id,
      toInt(config.level, 0),
      config.training ? 1 : 0,
      config.illustTrainingStatus ? 1 : 0,
      arrayFlag(config.episodes?.[0]),
      arrayFlag(config.episodes?.[1]),
      toInt(config.limitBreakRank, 0),
      toInt(config.skillLevel, 1),
    ]);
  }
  list.sort((left, right) => left[0] - right[0]);
  return list;
}

function buildCompactCharacterBonuses(characterBouns) {
  const list = [];
  if (!characterBouns || typeof characterBouns !== 'object') {
    return list;
  }
  for (const [characterId, bonus] of Object.entries(characterBouns)) {
    const id = parsePositiveIntegerId(characterId);
    if (!id || !bonus || typeof bonus !== 'object') {
      continue;
    }
    const potential = bonus.potential ?? {};
    const task = bonus.characterTask ?? {};
    const values = [
      toNumber(potential.performance),
      toNumber(potential.technique),
      toNumber(potential.visual),
      toNumber(task.performance),
      toNumber(task.technique),
      toNumber(task.visual),
    ];
    if (values.every((value) => value === 0)) {
      continue;
    }
    list.push([id, ...values]);
  }
  list.sort((left, right) => left[0] - right[0]);
  return list;
}

function buildCompactAreaItems(areaItem) {
  const list = [];
  if (!areaItem || typeof areaItem !== 'object') {
    return list;
  }
  for (const [itemId, item] of Object.entries(areaItem)) {
    const id = parsePositiveIntegerId(itemId);
    if (!id || !item || typeof item !== 'object') {
      continue;
    }
    list.push([id, toInt(item.level, 0)]);
  }
  list.sort((left, right) => left[0] - right[0]);
  return list;
}

function base64ToArrayBuffer(base64) {
  const normalized = String(base64).trim().replace(/-/g, '+').replace(/_/g, '/');
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=');
  try {
    const binary = atob(padded);
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) {
      bytes[index] = binary.charCodeAt(index);
    }
    return bytes.buffer;
  } catch (error) {
    throw new Error(`Base64 解码失败：${error.message}`);
  }
}

function base64FromArrayBuffer(buffer) {
  const bytes = new Uint8Array(buffer);
  const parts = [];
  const chunkSize = 0x8000;
  for (let index = 0; index < bytes.length; index += chunkSize) {
    const chunk = bytes.subarray(index, Math.min(index + chunkSize, bytes.length));
    parts.push(String.fromCharCode(...chunk));
  }
  return btoa(parts.join(''));
}

function createTextReadableStream(text) {
  if (typeof TextEncoder !== 'function' || typeof ReadableStream === 'undefined') {
    return null;
  }

  const bytes = new TextEncoder().encode(text);
  return new ReadableStream({
    start(controller) {
      if (bytes.length > 0) {
        controller.enqueue(bytes);
      }
      controller.close();
    },
  });
}

function timeoutReject(message) {
  return new Promise((_, reject) => {
    window.setTimeout(() => reject(new Error(message)), 1500);
  });
}

function parseNonNegativeIntegerOrZero(value, fallback = 0) {
  const number = Number(value);
  return Number.isInteger(number) && number >= 0 ? number : fallback;
}

function parsePositiveIntegerId(value) {
  const id = Number(value);
  return Number.isInteger(id) && id > 0 ? id : null;
}

function arrayFlag(value) {
  return value ? 1 : 0;
}

function toInt(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? Math.trunc(number) : fallback;
}

function toNumber(value) {
  const number = Number(value);
  return Number.isFinite(number) ? number : 0;
}

function clampInteger(value, min, max) {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return min;
  }
  return Math.max(min, Math.min(max, Math.trunc(number)));
}

function scaledRate(value) {
  return clampInteger(Math.round(toNumber(value) * 1000), 0, 1023);
}

function estimateSparseLocalBits(localIds) {
  let bits = estimateVarintBits(localIds.length);
  let previous = 0;
  for (const localId of localIds) {
    bits += estimateVarintBits(localId - previous);
    previous = localId;
  }
  return bits;
}

function estimateVarintBits(value) {
  let normalized = Math.max(0, Number(value) || 0);
  let bytes = 1;
  while (normalized >= 0x80) {
    normalized = Math.floor(normalized / 0x80);
    bytes += 1;
  }
  return bytes * 8;
}

function writeAscii(writer, text) {
  for (let index = 0; index < text.length; index += 1) {
    writer.writeBits(text.charCodeAt(index), 8);
  }
}

function readAscii(reader, length) {
  let text = '';
  for (let index = 0; index < length; index += 1) {
    text += String.fromCharCode(reader.readBits(8));
  }
  return text;
}

class BitWriter {
  constructor() {
    this.bytes = [];
    this.current = 0;
    this.offset = 0;
  }

  writeBit(bit) {
    if (bit) {
      this.current |= 1 << this.offset;
    }
    this.offset += 1;
    if (this.offset === 8) {
      this.flushByte();
    }
  }

  writeBits(value, bitCount) {
    let normalized = Number(value) >>> 0;
    for (let index = 0; index < bitCount; index += 1) {
      this.writeBit(normalized & 1);
      normalized >>>= 1;
    }
  }

  writeVarint(value) {
    let normalized = Math.max(0, Math.trunc(Number(value) || 0));
    do {
      let byte = normalized & 0x7f;
      normalized = Math.floor(normalized / 0x80);
      if (normalized > 0) {
        byte |= 0x80;
      }
      this.writeBits(byte, 8);
    } while (normalized > 0);
  }

  flushByte() {
    this.bytes.push(this.current);
    this.current = 0;
    this.offset = 0;
  }

  toArrayBuffer() {
    if (this.offset > 0) {
      this.flushByte();
    }
    return new Uint8Array(this.bytes).buffer;
  }
}

class BitReader {
  constructor(buffer) {
    this.bytes = new Uint8Array(buffer);
    this.byteIndex = 0;
    this.offset = 0;
  }

  readBit() {
    if (this.byteIndex >= this.bytes.length) {
      throw new Error('Base64 配置解码失败：二进制内容不完整');
    }
    const bit = (this.bytes[this.byteIndex] >> this.offset) & 1;
    this.offset += 1;
    if (this.offset === 8) {
      this.offset = 0;
      this.byteIndex += 1;
    }
    return bit;
  }

  readBits(bitCount) {
    let value = 0;
    for (let index = 0; index < bitCount; index += 1) {
      value |= this.readBit() << index;
    }
    return value;
  }

  readVarint() {
    let value = 0;
    let shift = 0;
    for (let index = 0; index < 5; index += 1) {
      const byte = this.readBits(8);
      value += (byte & 0x7f) * (2 ** shift);
      if ((byte & 0x80) === 0) {
        return value;
      }
      shift += 7;
    }
    throw new Error('Base64 配置解码失败：整数编码过长');
  }
}
