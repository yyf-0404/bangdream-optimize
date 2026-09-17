import {normalizedCharacterBonus} from './player.js';
import {formatCompactPercentNumber} from '../utils.js';

export const statFields = [['performance', '演出'], ['technique', '技巧'], ['visual', '形象']];

// Equipment percentages are stored in percent units; character bonuses are ratios.
export function equipmentBonusText(rate) {
  if (!rate) return '加成数据缺失';
  if (statFields.every(([key]) => rate[key] === rate.performance)) {
    return `综合力 +${formatCompactPercentNumber(rate.performance)}`;
  }
  return statFields.filter(([key]) => Number(rate[key]) !== 0)
    .map(([key, name]) => `${name} +${formatCompactPercentNumber(rate[key])}`).join(' · ') || '综合力 +0%';
}

export function resultCharacterBonuses(result, player, cardCharacterId) {
  const teams = Array.isArray(result) ? result : [
    ...(result?.songs ?? []), result?.team, ...(result?.medley?.teams ?? []),
  ];
  const characters = new Set();
  for (const team of teams) for (const id of team?.teamCardIds ?? []) {
    const characterId = Number(player?.customCards?.[id]?.definition?.characterId ?? cardCharacterId?.(id));
    if (Number.isInteger(characterId) && characterId > 0) characters.add(characterId);
  }
  return [...characters].map(id => {
    const bonus = normalizedCharacterBonus(player?.characterBouns?.[id]);
    return {id, ...bonus, total: Object.fromEntries(statFields.map(([key]) =>
      [key, bonus.potential[key] + bonus.characterTask[key]]))};
  });
}
