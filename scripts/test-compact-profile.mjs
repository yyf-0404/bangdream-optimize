import assert from 'node:assert/strict';

import { createCompactProfileCodec } from '../apps/web/src/data/compact-profile.js';

function normalizedPlayer(player = {}) {
  return {
    server: player.server ?? 'cn',
    cardList: player.cardList ?? {},
    characterBouns: player.characterBouns ?? {},
    areaItem: player.areaItem ?? {},
  };
}

const codec = createCompactProfileCodec({ normalizedPlayer });

function roundTrip(player) {
  return codec.compactProfileToPlayer(codec.buildCompactProfilePayload(player), {});
}

function assertRejectsMessage(promise, pattern) {
  return assert.rejects(promise, (error) => {
    assert.match(error.message, pattern);
    return true;
  });
}

assert.deepEqual(codec.buildCompactProfilePayload({}), {
  v: 1,
  c: [],
  b: [],
  a: [],
});

{
  const restored = roundTrip({
    cardList: {
      101: {
        level: 60,
        training: true,
        illustTrainingStatus: false,
        episodes: [true, false],
        limitBreakRank: 2,
        skillLevel: 5,
      },
    },
  });
  assert.deepEqual(restored.cardList['101'], {
    level: 60,
    training: true,
    illustTrainingStatus: false,
    episodes: [true, false],
    limitBreakRank: 2,
    skillLevel: 5,
  });
}

{
  const restored = roundTrip({
    characterBouns: {
      1: {
        potential: { performance: 0.1, technique: 0.2, visual: 0.3 },
        characterTask: { performance: 0.4, technique: 0.5, visual: 0.6 },
      },
    },
  });
  assert.deepEqual(restored.characterBouns['1'], {
    potential: { performance: 0.1, technique: 0.2, visual: 0.3 },
    characterTask: { performance: 0.4, technique: 0.5, visual: 0.6 },
  });
}

{
  const restored = roundTrip({
    areaItem: {
      11: { level: 6 },
    },
  });
  assert.deepEqual(restored.areaItem['11'], { level: 6 });
}

{
  const source = {
    cardList: {
      101: {
        level: 60,
        training: true,
        illustTrainingStatus: false,
        episodes: [true, false],
        limitBreakRank: 4,
        skillLevel: 5,
      },
      102: {
        level: 1,
        training: false,
        illustTrainingStatus: false,
        episodes: [false, true],
        limitBreakRank: 0,
        skillLevel: 1,
      },
      90123: {
        level: 37,
        training: true,
        illustTrainingStatus: true,
        episodes: [true, true],
        limitBreakRank: 2,
        skillLevel: 3,
      },
    },
    characterBouns: {
      1: {
        potential: { performance: 0.05, technique: 0.05, visual: 0.05 },
        characterTask: { performance: 0.042, technique: 0.039, visual: 0.041 },
      },
      40: {
        potential: { performance: 0.1, technique: 0.2, visual: 0.3 },
        characterTask: { performance: 0.4, technique: 0.5, visual: 0.6 },
      },
    },
    areaItem: {
      1: { level: 8 },
      103: { level: 7 },
    },
  };
  const payload = codec.buildCompactProfilePayload(source);
  const compressed = await codec.compressProfilePayload(payload);
  assert.equal(compressed.version, 2);
  assert.equal(compressed.type, 'bit1+b64');
  const compact = await codec.parseCompactProfileExport(JSON.stringify({
    v: compressed.version,
    t: compressed.type,
    d: compressed.data,
  }));
  const restored = codec.compactProfileToPlayer(compact, {});
  assert.deepEqual(restored.cardList, normalizedPlayer(source).cardList);
  assert.deepEqual(restored.areaItem, normalizedPlayer(source).areaItem);
  assert.deepEqual(restored.characterBouns, normalizedPlayer(source).characterBouns);
}

await assertRejectsMessage(
  codec.parseCompactProfileExport('{'),
  /配置导入 JSON 解析失败/,
);

await assertRejectsMessage(
  codec.parseCompactProfileExport(JSON.stringify({ v: 3, t: 'gz+b64', d: 'abc' })),
  /不支持的配置版本：3/,
);

await assertRejectsMessage(
  codec.parseCompactProfileExport(JSON.stringify({ v: 1, t: 'zip', d: 'abc' })),
  /不支持的配置压缩格式：zip/,
);

await assertRejectsMessage(
  codec.parseCompactProfileExport(JSON.stringify({ v: 1, t: 'gz+b64' })),
  /配置缺少 base64 压缩内容/,
);
