import {cardGrowth} from './growth.js';

// Card-only power follows prepare_card in crates/core/src/model/preparation.rs,
// before character potential/tasks, area items and event bonuses are applied.
export function cardStatValues(card) {
  const fields = ['performance', 'technique', 'visual'];
  const level = Number(card.level), stat = card.record?.stat;
  const base = stat?.[level];
  if (!Number.isInteger(level) || level < 1 || !base
      || fields.some(key => base[key] == null || !Number.isFinite(Number(base[key])))) return null;
  const growth = cardGrowth(card);
  const additions = [growth.trained ? stat.training : null,
    ...growth.episodes.map((enabled, index) => enabled ? stat.episodes?.[index] : null)];
  const mastery = Number(card.rarity) * growth.mastery * 50;
  const values = Object.fromEntries(fields.map(key => [key,
    Number(base[key]) + mastery + additions.reduce((sum, value) => sum + Number(value?.[key] ?? 0), 0)]));
  if (Object.values(values).some(value => !Number.isFinite(value))) return null;
  return {...values, total: fields.reduce((sum, key) => sum + values[key], 0)};
}
