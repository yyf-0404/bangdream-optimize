import { cardTrainingStatusList, normalizeTrainingStatus } from '../../assets/index.js';
import { gameText } from '../preferences.js';
import { profilePreference } from '../preferences.js';

const servers = ['jp','en','tw','cn','kr'];
export function cardModel(core, player, cardId, configOverride, profileId) {
  const id = Number(cardId), record = core?.cards?.[id] ?? core?.cardsFix?.[id];
  const config = configOverride ?? player?.cardList?.[id];
  const saved = config ?? profilePreference(profileId, 'removedGrowth', {})[id] ?? {};
  const character = core?.characters?.[record?.characterId];
  const training = record ? cardTrainingStatusList(record) : [false,true];
  const episodes = [0,1].map(i => {
    if (!record) return 1;
    const entry = record?.stat?.episodes?.[i];
    return !entry ? 0 : ['performance','technique','visual'].every(k => !Number(entry[k])) ? 2 : 1;
  });
  const maxLevel = Math.max(1, Number(record?.levelLimit)||0, !record ? Number(saved.level)||0 : 0, ...Object.keys(record?.stat||{}).filter(k=>/^\d+$/.test(k)).map(Number));
  return {id, record, rawConfig: structuredClone(saved), unknown: !record, characterId: record?.characterId ?? 0,
    title: gameText(record?.prefix, `卡牌 ${id}`), name: gameText(character?.characterName, `角色 ${record?.characterId ?? '—'}`),
    band: Number(character?.bandId) || 0, rarity: Number(record?.rarity) || 0, attribute: record?.attribute || 'unknown',
    searchText: [id,...(record?.prefix||[]),...(character?.characterName||[]),...(character?.nickname||[])].join(' '),
    resource: record?.resourceSetName, releaseDates: Object.fromEntries(servers.map((s,i)=>[s,Number(record?.releasedAt?.[i])||null])),
    owned: config != null, maxLevel, level: Number(saved.level)||maxLevel, skill: Number(saved.skillLevel)||5,
    capabilities: [training,episodes], skillId: record?.skillId, skillRecord: core?.skills?.[record?.skillId] ?? core?.skillsFix?.[record?.skillId],
    growth: {trained:normalizeTrainingStatus(training,saved.training),illustTrained:normalizeTrainingStatus(training,saved.illustTrainingStatus),mastery:Number(saved.limitBreakRank)||0,episodes:episodes.map((v,i)=>v===2||v===1&&saved.episodes?.[i]!==false)},
  };
}
export function cardConfig(model) {
  if (model.unknown) return structuredClone(model.rawConfig);
  return {level:model.level,skillLevel:model.skill,training:model.growth.trained,illustTrainingStatus:model.growth.illustTrained,limitBreakRank:model.growth.mastery,episodes:[...model.growth.episodes]};
}
export function catalogModels(core, player, profileId) {
  return [...new Set([...Object.keys(core?.cards||{}),...Object.keys(player?.cardList||{})])].map(id => cardModel(core,player,id,undefined,profileId));
}
