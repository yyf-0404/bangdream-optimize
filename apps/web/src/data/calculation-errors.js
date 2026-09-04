const CONFIGURATION = 'configuration';
const DATA = 'data';
const UNSUPPORTED = 'unsupported';
const INTERNAL = 'internal';

export function explainCalculationError(error, {
  player,
  calculationRequest,
} = {}) {
  const message = String(error?.message ?? error ?? '未知错误');
  const minimumStat = Number(calculationRequest?.minimumPersonalStat);

  if (
    /目标总 PT 必须大于当前 PT/.test(message)
    || /target total PT \d+ is below current PT \d+/i.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'score-range-target-too-low',
      '目标 PT 设置过低',
      '目标总 PT 必须高于当前 PT，计算目标是两者的差值。',
      '提高目标总 PT，或修正当前 PT 后重新计算。',
    );
  }
  const cardCount = message.match(/at least five cards are required to build a team, got (\d+)/i);
  if (cardCount) {
    return issue(
      CONFIGURATION,
      'insufficient-cards',
      '可用卡牌不足',
      `组成一支队伍至少需要 5 张可用卡牌，当前计算只找到 ${cardCount[1]} 张。`,
      '重新导入或补全玩家档案，并检查卡牌等级、训练状态和角色加成配置。',
    );
  }
  if (/at least five distinct characters are required/i.test(message)) {
    return issue(
      CONFIGURATION,
      'insufficient-distinct-characters',
      '可用角色不足',
      '队伍中的 5 张卡必须属于不同角色，当前卡池无法满足该条件。',
      '补充其他角色的卡牌，或检查玩家档案是否漏掉已持有卡牌。',
    );
  }
  if (/team \d+ contains duplicate cards/i.test(message)) {
    return issue(
      CONFIGURATION,
      'duplicate-team-card',
      '队伍包含重复卡牌',
      '同一支指定队伍的五个卡位不能使用相同卡牌。',
      '重新选择队伍卡牌后计算。',
    );
  }
  if (/team \d+ contains duplicate characters/i.test(message)) {
    return issue(
      CONFIGURATION,
      'duplicate-team-character',
      '队伍包含重复角色',
      '同一支指定队伍必须由五个不同角色组成。',
      '替换重复角色的卡牌后计算。',
    );
  }
  if (/team \d+ captain \d+ must match third slot card \d+/i.test(message)) {
    return issue(
      CONFIGURATION,
      'fixed-team-captain-position',
      '队长位置无效',
      '指定队伍的队长必须是第三个卡位。',
      '重新选择第三个卡位，或重新导入主乐队。',
    );
  }
  if (/Medley teams must not reuse the same physical card/i.test(message)) {
    return issue(
      CONFIGURATION,
      'medley-card-conflict',
      '巡回演出队伍卡牌冲突',
      '三支队伍不能重复使用同一张物理卡牌。',
      '调整三支队伍；同一角色的不同卡牌仍可跨队使用。',
    );
  }
  if (/selected .* area-item group .* missing or contains a level-0 item/i.test(message)) {
    return issue(
      CONFIGURATION,
      'fixed-team-area-item-unavailable',
      '所选区域道具不可用',
      '所选乐队、属性或杂志道具不存在，或整组中含有 0 级组件。',
      '在玩家配置中提升整组道具等级，或选择另一组完整道具。',
    );
  }
  if (/card \d+ in team \d+ is missing from the player configuration/i.test(message)) {
    return issue(
      CONFIGURATION,
      'fixed-team-card-missing',
      '指定卡牌不在档案中',
      '队伍中存在未导入或已从玩家档案移除的卡牌。',
      '重新导入主乐队，或先在卡牌页补全这些卡牌。',
    );
  }
  if (
    /minimumPersonalStat .*non-negative/i.test(message)
    || /最低综合力需为非负整数/.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'invalid-minimum-stat',
      '最低综合力格式无效',
      '最低综合力必须是非负整数。',
      '修正最低综合力后重新计算。',
    );
  }
  if (
    Number.isFinite(minimumStat)
    && minimumStat > 0
    && calculationRequest?.liveVariant === 'cooperative'
    && /no valid PT-maximizing team was found/i.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'minimum-stat-unreachable',
      '最低综合力限制无法满足',
      `没有找到综合力达到 ${formatInteger(minimumStat)} 且角色互不重复的合法队伍。`,
      '降低自己的最低综合力，或补全卡牌、区域道具和角色加成配置。',
    );
  }
  if (/no valid area item combinations are available/i.test(message)) {
    return issue(
      CONFIGURATION,
      'no-area-item-combination',
      '区域道具配置不完整',
      '没有可枚举的完整区域道具组合。',
      '检查乐队、属性和海报/杂志道具是否已导入；任一组件为 0 级时对应整组不会使用。',
    );
  }
  if (
    /no build result found/i.test(message)
    || /no valid single-song result found/i.test(message)
    || /candidate list is empty/i.test(message)
    || /no valid medley plan found/i.test(message)
    || /no valid PT-maximizing team was found/i.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'no-valid-team',
      '无法组成合法队伍',
      '当前卡牌、角色互斥条件、道具和计算限制组合后没有可用方案。',
      player?.calculationMode === 'ptMaximize'
        ? '检查卡牌档案；协力模式还应尝试降低最低综合力。'
        : '检查卡牌档案和区域道具配置，组曲还需要足够的互斥卡牌组成三支队伍。',
    );
  }
  if (
    /requires exactly one song, got \d+/i.test(message)
    || /requires exactly three songs, got \d+/i.test(message)
    || /calculation requires \d+ songs, got \d+/i.test(message)
    || /requires matching songs and charts/i.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'invalid-song-count',
      '歌曲数量不符合演出模式',
      '单曲模式必须选择 1 首歌曲，组曲模式必须选择 3 首歌曲。',
      '返回活动配置，重新选择正确数量的歌曲。',
    );
  }
  if (
    /missionSupportPtBonus .*required/i.test(message)
    || /必须填写支援.*PT 加成/.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'missing-mission-support-bonus',
      '缺少支援乐队 PT 加成',
      '任务 Live 的单人计算需要支援乐队 PT 加成。',
      '填写支援 PT 加成后重新计算。',
    );
  }
  if (/cooperative teammate \d+ has invalid stat or leader skill parameters/i.test(message)) {
    return issue(
      CONFIGURATION,
      'invalid-teammate-parameters',
      '队友参数无效',
      '队友综合力、技能时长或分数加成存在缺失或非法值。',
      '检查每位队友的综合力、技能时长和分数加成。',
    );
  }
  if (
    /协力演出必须完整填写队友/.test(message)
    || /协力演出必须填写自己的最低综合力/.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'incomplete-cooperative-parameters',
      '协力参数填写不完整',
      '自己的最低综合力或队友综合力、技能时长、分数加成存在空值。',
      '补全当前演出模式显示的所有协力参数。',
    );
  }
  if (
    /cooperative leader player index .*between 0 and 4/i.test(message)
    || /team rank .*invalid/i.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'invalid-rank-or-leader',
      '排名或队长选择无效',
      '队长位置和队内排名必须在 1～5 的范围内。',
      '重新选择指定队长或排名。',
    );
  }
  if (/festival teammate \d+ has a negative expected score/i.test(message)) {
    return issue(
      CONFIGURATION,
      'invalid-teammate-score',
      '队友预计分数无效',
      '团队演出的队友预计分数不能为负数。',
      '修正队友预计分数后重新计算。',
    );
  }
  if (/团队演出必须填写队友预计分数/.test(message)) {
    return issue(
      CONFIGURATION,
      'incomplete-festival-scores',
      '队友预计分数填写不完整',
      '团队演出需要为队友填写预计分数。',
      '补全统一参数或四位队友的预计分数。',
    );
  }
  if (/total score \d+ is lower than personal score \d+/i.test(message)) {
    return issue(
      CONFIGURATION,
      'inconsistent-multiplayer-score',
      '多人分数参数互相矛盾',
      '全队总分不能低于自己的个人分数。',
      '提高队友预计分数，或检查个人分数和演出模式配置。',
    );
  }
  if (/fire multiplier .* is invalid/i.test(message)) {
    return issue(
      CONFIGURATION,
      'invalid-fire-multiplier',
      '消耗倍率无效',
      '当前演出模式不支持所提交的火或 CP 倍率。',
      '重新选择结果页提供的火或 CP 倍率。',
    );
  }
  if (/input for live variant .* is missing/i.test(message)) {
    return issue(
      CONFIGURATION,
      'missing-live-variant-input',
      '演出模式参数不完整',
      '计算请求缺少当前演出模式所需的排名、胜负、队友或挑战参数。',
      '返回活动配置，补全当前演出模式显示的参数后重新计算。',
    );
  }
  if (
    /does not support live variant/i.test(message)
    || /is not supported by maximize/i.test(message)
    || /is not supported by score range/i.test(message)
    || /cooperative calculation is not supported/i.test(message)
    || /is not supported by this event PT formula/i.test(message)
  ) {
    return issue(
      UNSUPPORTED,
      'unsupported-event-mode',
      '活动类型与演出模式不兼容',
      '当前活动类型不支持所选计算目标或演出模式。',
      '重新选择该活动支持的计算目标和演出模式。',
    );
  }
  if (
    /current event is not set/i.test(message)
    || /event songs for event \d+ are missing/i.test(message)
    || /未设置活动/.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'missing-event-configuration',
      '活动配置不完整',
      '没有设置活动，或该活动尚未保存候选歌曲。',
      '重新选择活动和歌曲后再计算。',
    );
  }
  if (
    /不能为空/.test(message)
    || /需为非负整数/.test(message)
    || /Auto 倍率必须为/.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'invalid-form-value',
      '计算参数格式无效',
      message,
      '修正对应输入框后重新计算。',
    );
  }
  if (
    /player card config for card \d+ is missing/i.test(message)
    || /character bonus for character \d+ is missing/i.test(message)
    || /skill level \d+ is invalid for card \d+/i.test(message)
    || /level \d+ is invalid for card \d+/i.test(message)
  ) {
    return issue(
      CONFIGURATION,
      'invalid-player-card-profile',
      '玩家卡牌档案不完整',
      '卡牌等级、技能等级或对应角色加成缺失或无效。',
      '重新导入玩家档案，或在卡牌与角色加成页面修正对应配置。',
    );
  }
  if (
    /area item definition \d+ is missing/i.test(message)
    || /area item \d+ does not map to a supported target key/i.test(message)
  ) {
    return issue(
      DATA,
      'invalid-area-item-data',
      '区域道具数据异常',
      '档案中的区域道具无法在当前游戏数据中解析。',
      '刷新游戏数据并重新导入档案；若仍出现，请附带诊断反馈。',
    );
  }
  if (
    /(?:event|song|chart|card) id .* is missing/i.test(message)
    || /(?:event|song|chart|card) \d+ is missing/i.test(message)
    || /(?:^|: )\w[\w.]* is missing$/i.test(message)
    || /(?:^|: )\w[\w.]* has invalid value:/i.test(message)
    || /chart count must be positive/i.test(message)
    || /chart is missing skill meta/i.test(message)
    || /score distribution is empty/i.test(message)
    || /duration .* is not supported by chart meta/i.test(message)
  ) {
    return issue(
      DATA,
      'missing-game-data',
      '游戏数据或谱面数据缺失',
      '所选活动、歌曲、谱面或卡牌的数据不完整。',
      '刷新本地数据；若目标服务器尚未发布该内容，请更换歌曲或活动。',
    );
  }

  return issue(
    INTERNAL,
    'unclassified-calculation-error',
    '计算过程中发生未分类错误',
    message,
    '保留当前配置，并通过结果页反馈按钮附带诊断数据。',
  );
}

export function scoreRangeEmptyExplanation(request = {}) {
  const currentPt = Number(request.currentPt);
  const targetTotalPt = Number(request.targetTotalPt);
  const delta = Number.isSafeInteger(currentPt)
    && Number.isSafeInteger(targetTotalPt)
    && targetTotalPt > currentPt
    ? targetTotalPt - currentPt
    : undefined;
  return {
    title: '没有精确命中目标 PT 的方案',
    detail: `${delta == null ? '目标 PT 增量' : `当前目标增量 ${formatInteger(delta)} PT`}可能低于单局可获得的最低 PT，或当前卡牌、道具和歌曲无法组合出该增量。`,
    suggestion: '提高目标总 PT，或调整候选歌曲、卡牌与区域道具配置。',
  };
}

function issue(category, code, title, detail, suggestion) {
  return { category, code, title, detail, suggestion };
}

function formatInteger(value) {
  return Math.round(Number(value)).toLocaleString('zh-CN');
}
