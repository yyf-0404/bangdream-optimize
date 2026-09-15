export function teamChoiceReason(card, teams, teamIndex, slotIndex, characterOf) {
  if (card?.custom && !card.enabled) return '此自定义卡牌已停用';
  if (!card?.owned) return '仅可使用持有卡牌';
  if (card.unknown || !card.characterId) return '卡牌资料暂缺';
  if (teams.some((team, index) => index !== teamIndex && team.includes(card.id))) return '已用于其他队伍';
  if ((teams[teamIndex] || []).some((id, index) => index !== slotIndex && id && characterOf(id) === card.characterId)) return '当前队伍其他卡位已有同一角色';
  return '';
}
export function placeTeamCard(team, slot, cardId) {
  const next = Array.from({length:5}, (_, index) => team[index] || 0);
  next[slot] = cardId;
  const following = next.findIndex((id, index) => !id && index > slot), first = next.findIndex(id => !id);
  return {team:next, slot:following >= 0 ? following : first >= 0 ? first : slot};
}
