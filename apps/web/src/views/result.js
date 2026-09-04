import {
  attributeLabel,
  bandLabel,
  compactJoin,
  fireCostForMultiplier,
  formatInteger,
  formatMs,
  magazineLabel,
  selectedBandLabel,
  totalFireCost,
} from '../utils.js?v=3';
import {
  assetImage,
  attributeIconUrls,
  bandIconUrls,
} from '../assets/index.js?v=3';
import { cardPreviewItem } from '../ui/card-preview.js?v=3';
import { emptyMessage } from '../ui/dom.js?v=3';
import { renderDifficultyList } from './song.js?v=3';
import { scoreRangeEmptyExplanation } from '../data/calculation-errors.js?v=1';

const POINT_BONUS_EVENT_TYPES = new Set(['challenge', 'live_try', 'mission_live']);
const SINGLE_FIRE_PT_MULTIPLIERS = Object.freeze([
  { resource: 0, multiplier: 1 },
  { resource: 1, multiplier: 5 },
  { resource: 2, multiplier: 10 },
  { resource: 3, multiplier: 15 },
  { resource: 10, multiplier: 40 },
]);
const MEDLEY_FIRE_PT_MULTIPLIERS = Object.freeze([
  { resource: 0, perSongResource: 0, multiplier: 3 },
  { resource: 3, perSongResource: 1, multiplier: 15 },
  { resource: 6, perSongResource: 2, multiplier: 30 },
  { resource: 9, perSongResource: 3, multiplier: 45 },
]);
const CHALLENGE_CP_MULTIPLIERS = Object.freeze([
  { resource: 200, multiplier: 1 },
  { resource: 400, multiplier: 2 },
  { resource: 800, multiplier: 4 },
  { resource: 1600, multiplier: 8 },
]);

export function ptResultMultiplierOptions(liveVariant) {
  if (liveVariant === 'challenge_cp') {
    return CHALLENGE_CP_MULTIPLIERS;
  }
  return liveVariant === 'medley'
    ? MEDLEY_FIRE_PT_MULTIPLIERS
    : SINGLE_FIRE_PT_MULTIPLIERS;
}

export function renderMetrics(metricsElement, metrics) {
  metricsElement.textContent = '';
  metricsElement.hidden = !metrics;
  if (!metrics) {
    return;
  }

  const term = document.createElement('dt');
  term.textContent = '总耗时';
  const detail = document.createElement('dd');
  detail.textContent = formatMs(metrics.totalElapsedMs);
  metricsElement.append(term, detail);
}

export function renderResultSummary(resultElement, result, deps, { diagnostic } = {}) {
  resultElement.textContent = '';
  const failureDiagnostic = diagnostic?.error ? diagnostic : undefined;
  resultElement.hidden = !result && !failureDiagnostic;
  if (failureDiagnostic) {
    renderFailureDiagnostic(resultElement, failureDiagnostic);
    return;
  }
  if (!result) {
    return;
  }

  if (Array.isArray(result)) {
    renderScoreRangeSummary(resultElement, result, deps, diagnostic);
    return;
  }
  if (result.scoreMode && result.team?.evaluation?.averagePt) {
    renderPtEvaluateSummary(resultElement, result, deps);
    return;
  }
  if (result.scoreMode && result.medley?.averagePt) {
    renderPtEvaluateMedleySummary(resultElement, result, deps);
    return;
  }
  if (result.team?.evaluation?.averagePt) {
    renderPtMaximizeSummary(resultElement, result, deps);
    return;
  }
  if (result.medley?.averagePt) {
    renderPtMaximizeMedleySummary(resultElement, result, deps);
    return;
  }

  const songs = Array.isArray(result.songs) ? result.songs : [];
  const overview = document.createElement('section');
  overview.className = 'result-overview';
  overview.append(
    resultStat('总分', formatInteger(result.totalScore), 'strong'),
    resultStat('综合力', formatInteger(result.totalStat)),
    resultStat('活动', result.eventId == null ? '-' : `ID ${result.eventId}`),
  );
  resultElement.append(overview);

  const riskySongs = songs.filter((song) => song.skillQueueRisk === true);
  if (riskySongs.length > 0) {
    resultElement.append(renderSkillQueueRisk(riskySongs, deps));
  }
  if (result.items) {
    resultElement.append(renderSelectedItems(result.items, deps));
  }

  const songSection = document.createElement('section');
  songSection.className = 'result-section';
  const title = document.createElement('h3');
  title.textContent = '歌曲与队伍';
  const list = document.createElement('div');
  list.className = 'result-song-list';
  const maxScore = songs.reduce((max, song) => Math.max(max, Number(song.score) || 0), 0);
  for (const [index, song] of songs.entries()) {
    list.append(renderSongResult(song, index, maxScore, deps));
  }
  if (songs.length === 0) {
    list.append(emptyMessage('没有歌曲结果', 'result-empty'));
  }
  songSection.append(title, list);
  resultElement.append(songSection);
}

function renderPtMaximizeSummary(resultElement, result, deps) {
  const team = result.team;
  const evaluation = team.evaluation;
  const average = evaluation.averagePt;
  const overview = document.createElement('section');
  overview.className = 'result-overview';
  const averagePtStat = resultStat('平均活动 PT', '-', 'strong');
  overview.append(
    averagePtStat,
    resultStat('平均分数', formatScoreDistributionAverage(evaluation.scoreDistribution)),
    resultStat('综合力', formatInteger(team.totalStat)),
  );
  let averageCpGainStat;
  if (evaluation.averageCpGain) {
    averageCpGainStat = resultStat('平均 CP 获取', '-');
    overview.append(averageCpGainStat);
  }
  let challengeCpCostStat;
  if (evaluation.challengeCpCost != null) {
    challengeCpCostStat = resultStat('CP 消耗', '-');
    overview.append(challengeCpCostStat);
  }
  const multiplierSelector = renderPtMultiplierSelector(result.liveVariant, ({ resource, multiplier }) => {
    setResultStatValue(averagePtStat, formatScaledAverageInteger(
      average.ptSum,
      average.sampleCount,
      multiplier,
    ));
    if (averageCpGainStat) {
      setResultStatValue(averageCpGainStat, formatScaledAverageFixed(
        evaluation.averageCpGain.ptSum,
        evaluation.averageCpGain.sampleCount,
        multiplier,
        4,
      ));
    }
    if (challengeCpCostStat) {
      setResultStatValue(challengeCpCostStat, `${formatInteger(resource)} CP`);
    }
  });
  resultElement.append(
    renderPtScenario(result),
    multiplierSelector,
    overview,
  );
  if (team.items) {
    resultElement.append(renderSelectedItems(team.items, deps));
  }
  resultElement.append(renderPtMaximizeSongSection(
    result.songs,
    [team],
    deps,
  ));
}

function renderPtMaximizeMedleySummary(resultElement, result, deps) {
  const medley = result.medley;
  const average = medley.averagePt;
  const overview = document.createElement('section');
  overview.className = 'result-overview';
  const averagePtStat = resultStat('平均活动 PT', '-', 'strong');
  overview.append(
    averagePtStat,
    resultStat('平均分数', formatAverageInteger(
      medley.totalScoreSum,
      medley.sampleCount,
    )),
  );
  const multiplierSelector = renderPtMultiplierSelector(result.liveVariant, ({ multiplier }) => {
    setResultStatValue(averagePtStat, formatScaledAverageInteger(
      average.ptSum,
      average.sampleCount,
      multiplier,
    ));
  });
  resultElement.append(
    renderPtScenario(result),
    multiplierSelector,
    overview,
  );

  const teams = Array.isArray(medley.teams) ? medley.teams : [];
  if (teams[0]?.items) {
    resultElement.append(renderSelectedItems(teams[0].items, deps));
  }
  resultElement.append(renderPtMaximizeSongSection(result.songs, teams, deps));
}

function renderPtEvaluateSummary(resultElement, result, deps) {
  const team = result.team;
  const evaluation = team.evaluation;
  const averagePtStat = resultStat('平均活动 PT', '-', 'strong');
  const minPtStat = resultStat('最低活动 PT', '-');
  const maxPtStat = resultStat('最高活动 PT', '-');
  const overview = document.createElement('section');
  overview.className = 'result-overview';
  overview.append(
    averagePtStat,
    minPtStat,
    maxPtStat,
    resultStat('平均分数', formatScoreDistributionAverage(evaluation.scoreDistribution)),
    resultStat('最低分数', formatInteger(evaluation.scoreDistribution?.minScore)),
    resultStat('最高分数', formatInteger(evaluation.scoreDistribution?.maxScore)),
    resultStat('综合力', formatInteger(team.totalStat)),
  );
  if (Number(team.pointBonusBasisPoints) > 0) {
    overview.append(
      resultStat('活动加成', `${formatBasisPoints(team.pointBonusBasisPoints)}%`),
    );
  }
  let averageCpGainStat;
  if (evaluation.averageCpGain) {
    averageCpGainStat = resultStat('平均 CP 获取', '-');
    overview.append(averageCpGainStat);
  }
  let challengeCpCostStat;
  if (evaluation.challengeCpCost != null) {
    challengeCpCostStat = resultStat('CP 消耗', '-');
    overview.append(challengeCpCostStat);
  }
  const multiplierSelector = renderPtMultiplierSelector(result.liveVariant, ({ resource, multiplier }) => {
    setResultStatValue(averagePtStat, formatScaledAverageInteger(
      evaluation.averagePt.ptSum,
      evaluation.averagePt.sampleCount,
      multiplier,
    ));
    setResultStatValue(minPtStat, formatInteger(Number(evaluation.minPt) * multiplier));
    setResultStatValue(maxPtStat, formatInteger(Number(evaluation.maxPt) * multiplier));
    if (averageCpGainStat) {
      setResultStatValue(averageCpGainStat, formatScaledAverageFixed(
        evaluation.averageCpGain.ptSum,
        evaluation.averageCpGain.sampleCount,
        multiplier,
        4,
      ));
    }
    if (challengeCpCostStat) {
      setResultStatValue(challengeCpCostStat, `${formatInteger(resource)} CP`);
    }
  });
  resultElement.append(
    renderPtScenario(result),
    multiplierSelector,
    overview,
    renderScoreMode(result.scoreMode),
    renderSelectedItems(team.items, deps),
    renderPtMaximizeSongSection(result.songs, [team], deps, { detailedScore: true }),
  );
}

function renderPtEvaluateMedleySummary(resultElement, result, deps) {
  const medley = result.medley;
  const averagePtStat = resultStat('平均活动 PT', '-', 'strong');
  const minPtStat = resultStat('最低活动 PT', '-');
  const maxPtStat = resultStat('最高活动 PT', '-');
  const overview = document.createElement('section');
  overview.className = 'result-overview';
  overview.append(
    averagePtStat,
    minPtStat,
    maxPtStat,
    resultStat('平均分数', formatAverageInteger(medley.totalScoreSum, medley.sampleCount)),
    resultStat(
      '最低分数',
      formatInteger(medley.teams.reduce(
        (sum, team) => sum + Number(team.scoreDistribution?.minScore ?? 0),
        0,
      )),
    ),
    resultStat(
      '最高分数',
      formatInteger(medley.teams.reduce(
        (sum, team) => sum + Number(team.scoreDistribution?.maxScore ?? 0),
        0,
      )),
    ),
  );
  const multiplierSelector = renderPtMultiplierSelector(result.liveVariant, ({ multiplier }) => {
    setResultStatValue(averagePtStat, formatScaledAverageInteger(
      medley.averagePt.ptSum,
      medley.averagePt.sampleCount,
      multiplier,
    ));
    setResultStatValue(minPtStat, formatInteger(Number(medley.minPt) * multiplier));
    setResultStatValue(maxPtStat, formatInteger(Number(medley.maxPt) * multiplier));
  });
  resultElement.append(
    renderPtScenario(result),
    multiplierSelector,
    overview,
    renderScoreMode(result.scoreMode),
  );
  if (medley.teams[0]?.items) {
    resultElement.append(renderSelectedItems(medley.teams[0].items, deps));
  }
  resultElement.append(renderPtMaximizeSongSection(
    result.songs,
    medley.teams,
    deps,
    { detailedScore: true },
  ));
}

function renderScoreMode(scoreMode) {
  const section = document.createElement('section');
  section.className = 'result-section result-diagnostic';
  const title = document.createElement('h3');
  title.textContent = '计分方式';
  const details = document.createElement('dl');
  details.className = 'result-diagnostic-grid';
  details.append(diagnosticItem(
    '演奏',
    scoreMode?.mode === 'auto'
      ? `自动演出 · ${scoreMode.baseMultiplier} 倍`
      : '手动演奏 · 全 Perfect',
  ));
  section.append(title, details);
  return section;
}

function renderPtMultiplierSelector(liveVariant, onChange) {
  const challenge = liveVariant === 'challenge_cp';
  const medley = liveVariant === 'medley';
  const section = document.createElement('section');
  section.className = 'result-section result-multiplier-section';
  const title = document.createElement('h3');
  title.textContent = medley ? '每曲倍率选择' : '倍率选择';
  const control = document.createElement('div');
  control.className = 'segmented-control result-multiplier-control';
  if (medley) {
    control.classList.add('result-multiplier-control-medley');
  }
  control.setAttribute('role', 'radiogroup');
  control.setAttribute(
    'aria-label',
    challenge ? '挑战演出 CP 倍率' : medley ? '组曲每曲火倍率' : '演出火倍率',
  );
  const options = ptResultMultiplierOptions(liveVariant);
  const radioName = `pt-result-multiplier-${challenge ? 'cp' : 'fire'}`;
  for (const [index, option] of options.entries()) {
    const label = document.createElement('label');
    const input = document.createElement('input');
    input.type = 'radio';
    input.name = radioName;
    input.value = String(option.multiplier);
    input.checked = index === 0;
    const text = document.createElement('span');
    text.textContent = challenge
      ? `${option.resource} CP / ${option.multiplier} 倍`
      : medley
        ? `每曲 ${option.perSongResource} 火 / ${option.multiplier} 倍`
        : `${option.resource} 火 / ${option.multiplier} 倍`;
    label.append(input, text);
    control.append(label);
  }
  const update = () => {
    const checked = control.querySelector('input:checked');
    const selected = options.find(
      (option) => option.multiplier === Number(checked?.value),
    ) ?? options[0];
    onChange(selected);
  };
  control.addEventListener('change', update);
  section.append(title, control);
  update();
  return section;
}

function setResultStatValue(stat, value) {
  const output = stat.querySelector('strong');
  if (output) {
    output.textContent = String(value ?? '-');
  }
}

function renderPtScenario(result) {
  const scenario = result.scenario ?? {};
  const section = document.createElement('section');
  section.className = 'result-section result-diagnostic result-scenario';
  const title = document.createElement('h3');
  title.textContent = '计算场景';
  const details = document.createElement('dl');
  details.className = 'result-diagnostic-grid result-scenario-grid';
  details.append(diagnosticItem('演出模式', ptLiveVariantLabel(result.liveVariant)));
  if (scenario.versus) {
    details.append(diagnosticItem(
      '排名',
      `第 ${Number(scenario.versus.teamRank) + 1} 名`,
    ));
  }
  if (scenario.festival) {
    details.append(
      diagnosticItem(
        '队内排名',
        `第 ${Number(scenario.festival.teamRank) + 1} 名`,
      ),
      diagnosticItem('队伍结果', scenario.festival.won ? '获胜' : '失败'),
    );
  }
  section.append(title, details);
  return section;
}

function ptLiveVariantLabel(value) {
  return {
    solo: '自由演出',
    cooperative: '协力演出',
    versus: '竞演演出',
    medley: '巡回演出',
    festival: '团队演出',
    challenge_cp: '挑战演出',
  }[value] ?? String(value ?? '-');
}

function renderPtMaximizeSongSection(songs, teams, deps, { detailedScore = false } = {}) {
  const section = document.createElement('section');
  section.className = 'result-section';
  const title = document.createElement('h3');
  title.textContent = '歌曲与队伍';
  section.append(title);
  const list = document.createElement('div');
  list.className = 'result-song-list';
  const songResults = teams.flatMap((team, index) => {
    const song = songs?.[index];
    if (!song) {
      return [];
    }
    const scoreDistribution = team.scoreDistribution ?? team.evaluation?.scoreDistribution;
    return [{
      ...song,
      score: roundedAverage(
        scoreDistribution?.scoreSum,
        scoreDistribution?.sampleCount,
      ),
      stat: team.totalStat,
      teamCardIds: team.teamCardIds,
      captainCardId: team.captainCardId,
      scoreDistribution,
      detailedScore,
    }];
  });
  const maxScore = songResults.reduce(
    (max, song) => Math.max(max, Number(song.score) || 0),
    0,
  );
  for (const [index, song] of songResults.entries()) {
    list.append(renderSongResult(song, index, maxScore, deps, {
      skillTitle: '队伍',
      showSkillOrder: false,
    }));
  }
  if (songResults.length === 0) {
    list.append(emptyMessage('没有歌曲结果', 'result-empty'));
  }
  section.append(list);
  return section;
}

function renderSkillQueueRisk(songs, deps) {
  const warning = document.createElement('section');
  warning.className = 'result-risk';
  warning.setAttribute('role', 'alert');
  const title = document.createElement('strong');
  title.textContent = '技能重叠提示';
  const detail = document.createElement('p');
  const labels = songs.map((song) => compactJoin([
    deps.songLabel(song.songId),
    `ID ${song.songId}`,
    `难度 ${song.difficulty}`,
  ], ' · '));
  detail.textContent = `以下谱面存在技能窗口重叠：${labels.join('；')}。计算使用精确的独立技能窗口，并允许重叠增量直接相加；所有谱面统一使用 5×6 增量矩阵和 32 状态 DP。`;
  warning.append(title, detail);
  return warning;
}

function renderFailureDiagnostic(resultElement, diagnostic) {
  const error = diagnostic.error ?? {};
  const overview = document.createElement('section');
  overview.className = 'result-overview';
  overview.append(
    resultStat('状态', '计算失败', 'danger'),
    resultStat('原因', error.title ?? '未分类错误', 'danger'),
    resultStat('问题类型', calculationErrorCategoryLabel(error.category)),
    resultStat('活动', diagnostic.eventId == null ? '-' : `ID ${diagnostic.eventId}`),
  );

  const section = document.createElement('section');
  section.className = 'result-section result-diagnostic';
  const title = document.createElement('h3');
  title.textContent = '结果诊断';
  const details = document.createElement('dl');
  details.className = 'result-diagnostic-grid';
  details.append(
    diagnosticItem('问题说明', error.detail ?? error.message ?? '未知错误'),
    diagnosticItem('处理建议', error.suggestion ?? '请附带诊断数据提交反馈。'),
    diagnosticItem('原始错误', error.message ?? '未知错误'),
    diagnosticItem('运行阶段', diagnostic.phase ?? 'calculation'),
    diagnosticItem('运行时', diagnostic.runtime ?? 'unknown'),
    diagnosticItem('执行位置', error.executionContext ?? '-'),
  );
  section.append(title, details);

  if (error.stack) {
    const stackDetails = document.createElement('details');
    stackDetails.className = 'result-diagnostic-stack';
    const summary = document.createElement('summary');
    summary.textContent = '调用栈';
    const stack = document.createElement('pre');
    stack.textContent = error.stack;
    stackDetails.append(summary, stack);
    section.append(stackDetails);
  }

  resultElement.append(overview, section);
}

function diagnosticItem(label, value) {
  const wrapper = document.createElement('div');
  const term = document.createElement('dt');
  term.textContent = label;
  const detail = document.createElement('dd');
  detail.textContent = String(value ?? '-');
  wrapper.append(term, detail);
  return wrapper;
}

function calculationErrorCategoryLabel(category) {
  return {
    configuration: '配置问题',
    data: '数据问题',
    unsupported: '模式不支持',
    internal: '内部错误',
  }[category] ?? '未分类';
}

function renderScoreRangeSummary(resultElement, results, deps, diagnostic) {
  const first = results[0];
  if (!first) {
    const explanation = scoreRangeEmptyExplanation(diagnostic?.calculationRequest);
    const section = document.createElement('section');
    section.className = 'result-section result-diagnostic';
    const title = document.createElement('h3');
    title.textContent = explanation.title;
    const details = document.createElement('dl');
    details.className = 'result-diagnostic-grid';
    details.append(
      diagnosticItem('可能原因', explanation.detail),
      diagnosticItem('处理建议', explanation.suggestion),
    );
    section.append(title, details);
    resultElement.append(section);
    return;
  }

  const overview = document.createElement('section');
  overview.className = 'result-overview score-range-overview';
  const reportedFireCost = Number(first.totalFireCost);
  const fireCost = Number.isSafeInteger(reportedFireCost) && reportedFireCost >= 0
    ? reportedFireCost
    : totalFireCost(first.plays);
  const overviewStats = [
    resultStat('目标增量', `${formatInteger(first.targetDeltaPt)} PT`, 'strong'),
    resultStat('演奏次数', `${formatInteger(first.playCount)} 局`),
    resultStat('总火耗', `${formatInteger(fireCost)} 火`),
    resultStat('综合力', formatInteger(first.totalStat)),
  ];
  if (POINT_BONUS_EVENT_TYPES.has(String(first.eventType))) {
    overview.classList.add('has-point-bonus');
    overviewStats.push(
      resultStat('活动加成', `${formatBasisPoints(first.pointBonusBasisPoints)}%`),
    );
  }
  overview.append(...overviewStats);
  resultElement.append(overview, renderScoreRangeContent(first, deps));
}

function renderScoreRangeContent(result, deps) {
  const content = document.createElement('div');
  content.className = 'score-range-result-content';

  if (result.items) {
    content.append(renderSelectedItems(result.items, deps));
  }
  content.append(renderScoreRangeTeam(result.teamCardIds, deps));

  const playSection = document.createElement('section');
  playSection.className = 'result-section score-range-play-section';
  const title = document.createElement('h3');
  title.textContent = '演奏安排';
  const plays = document.createElement('div');
  plays.className = 'score-range-play-list';
  for (const play of result.plays ?? []) {
    plays.append(renderScoreRangePlay(play, deps));
  }
  if (!plays.childElementCount) {
    plays.append(emptyMessage('没有演奏明细', 'result-empty'));
  }
  playSection.append(title, plays);
  content.append(playSection);
  return content;
}

function renderScoreRangeTeam(cardIds, deps) {
  const section = document.createElement('section');
  section.className = 'result-skill-order';
  const title = document.createElement('div');
  title.className = 'result-item result-skill-order-title';
  const label = document.createElement('span');
  label.textContent = '队伍';
  title.append(label);
  const preview = document.createElement('div');
  preview.className = 'result-skill-preview';
  for (const [index, cardId] of (cardIds ?? []).entries()) {
    preview.append(resultSkillCard(cardId, {
      isCaptain: false,
      orderIndex: index,
    }, deps));
  }
  if (!preview.childElementCount) {
    preview.append(emptyMessage('没有队伍卡片', 'card-preview-empty'));
  }
  section.append(title, preview);
  return section;
}

function renderScoreRangePlay(play, deps) {
  const row = document.createElement('article');
  row.className = 'result-song score-range-play';
  const header = document.createElement('div');
  header.className = 'result-song-header';
  const cover = assetImage(
    deps.songCoverUrls(play.songId),
    'song-cover',
    deps.songLabel(play.songId),
  );
  const content = document.createElement('div');
  content.className = 'result-song-content';
  const title = document.createElement('div');
  title.className = 'result-song-title';
  const name = document.createElement('strong');
  name.textContent = deps.songLabel(play.songId);
  const meta = document.createElement('span');
  meta.className = 'result-song-meta';
  meta.textContent = compactJoin([
    `ID ${play.songId}`,
    `${fireCostForMultiplier(play.fireMultiplier)} 火`,
    `${play.count} 局`,
  ], ' · ');
  title.append(name, meta);
  const difficulties = renderDifficultyList(
    deps.getSongRecord?.(play.songId),
    play.difficulty,
  );
  difficulties.classList.add('result-song-difficulty-list');
  content.append(title, difficulties);
  const pt = document.createElement('div');
  pt.className = 'result-song-score';
  pt.textContent = `${formatInteger(play.pt)} PT`;
  if (cover) {
    header.append(cover);
  } else {
    header.classList.add('no-cover');
  }
  header.append(content, pt);

  const details = document.createElement('div');
  details.className = 'result-song-details';
  details.append(resultItem('单局得分', formatInteger(play.score)));
  row.append(header, details);
  return row;
}

function formatBasisPoints(value) {
  const number = Number(value);
  return Number.isFinite(number) ? (number / 100).toFixed(2).replace(/\.00$/, '') : '0';
}

function resultStat(label, value, tone) {
  const item = document.createElement('div');
  item.className = compactJoin(['result-stat', tone && `result-stat-${tone}`]);
  const labelNode = document.createElement('span');
  labelNode.textContent = label;
  const valueNode = document.createElement('strong');
  valueNode.textContent = String(value ?? '-');
  item.append(labelNode, valueNode);
  return item;
}

function renderSelectedItems(items, deps) {
  const bandId = deps.selectedBandId(items.band);
  const section = document.createElement('section');
  section.className = 'result-section result-items-section';
  const title = document.createElement('h3');
  title.textContent = '道具选择';
  const itemsGrid = document.createElement('div');
  itemsGrid.className = 'result-items';
  itemsGrid.append(
    resultItem('乐队道具', bandId == null ? selectedBandLabel(items.band) : bandLabel(bandId), {
      imageUrls: bandIconUrls(bandId),
    }),
    resultItem('属性道具', attributeLabel(items.attribute), {
      imageUrls: attributeIconUrls(items.attribute),
    }),
    resultItem('杂志道具', magazineLabel(items.magazine)),
  );
  section.append(title, itemsGrid);
  return section;
}

function resultItem(label, value, { imageUrls } = {}) {
  const item = document.createElement('div');
  const icon = assetImage(imageUrls, 'result-item-icon', value);
  item.className = compactJoin(['result-item', icon && 'has-icon'], ' ');
  const labelNode = document.createElement('span');
  labelNode.textContent = label;
  const valueNode = document.createElement('strong');
  valueNode.textContent = String(value ?? '-');
  if (icon) {
    item.append(icon);
  }
  item.append(labelNode, valueNode);
  return item;
}

function renderSongResult(song, index, maxScore, deps, {
  skillTitle = '最优技能顺序',
  showSkillOrder = true,
} = {}) {
  const card = document.createElement('article');
  card.className = 'result-song';

  const header = document.createElement('div');
  header.className = 'result-song-header';
  const cover = assetImage(deps.songCoverUrls(song.songId), 'song-cover', deps.songLabel(song.songId));
  const content = document.createElement('div');
  content.className = 'result-song-content';
  const title = document.createElement('div');
  title.className = 'result-song-title';
  const songName = document.createElement('strong');
  songName.textContent = deps.songLabel(song.songId);
  const meta = document.createElement('span');
  meta.className = 'result-song-meta';
  meta.textContent = compactJoin([
    `#${index + 1}`,
    `ID ${song.songId}`,
  ], ' · ');
  const difficultyList = renderDifficultyList(deps.getSongRecord?.(song.songId), song.difficulty);
  difficultyList.classList.add('result-song-difficulty-list');
  title.append(songName, meta);

  const score = document.createElement('div');
  score.className = 'result-song-score';
  score.textContent = formatInteger(song.score);
  content.append(title, difficultyList);
  if (cover) {
    header.append(cover);
  } else {
    header.classList.add('no-cover');
  }
  header.append(content, score);

  const bar = document.createElement('div');
  bar.className = 'result-score-bar';
  const fill = document.createElement('span');
  const ratio = maxScore > 0 ? Math.max(0, Math.min(1, Number(song.score) / maxScore)) : 0;
  fill.style.width = `${Math.max(4, ratio * 100)}%`;
  bar.append(fill);

  const details = document.createElement('div');
  details.className = 'result-song-details';
  details.classList.toggle('has-detailed-score', Boolean(
    song.detailedScore && song.scoreDistribution,
  ));
  details.append(
    resultItem('综合力', formatInteger(song.stat)),
  );
  if (song.detailedScore && song.scoreDistribution) {
    details.append(
      resultItem('最低分数', formatInteger(song.scoreDistribution.minScore)),
      resultItem('平均分数', formatScoreDistributionAverage(song.scoreDistribution)),
      resultItem('最高分数', formatInteger(song.scoreDistribution.maxScore)),
    );
  }

  card.append(header, bar, details, renderSkillOrder(song, deps, {
    sectionTitle: skillTitle,
    showOrder: showSkillOrder,
  }));
  return card;
}

function renderSkillOrder(song, deps, {
  sectionTitle = '最优技能顺序',
  showOrder = true,
} = {}) {
  const section = document.createElement('section');
  section.className = 'result-skill-order';
  const title = document.createElement('div');
  title.className = 'result-item result-skill-order-title';
  const titleText = document.createElement('span');
  titleText.textContent = sectionTitle;
  title.append(titleText);
  const preview = document.createElement('div');
  preview.className = 'result-skill-preview';
  const cardIds = skillOrderCardIds(song);
  if (cardIds.length === 0) {
    preview.append(emptyMessage('没有队伍卡片', 'card-preview-empty'));
  } else {
    for (const [index, cardId] of cardIds.entries()) {
      preview.append(resultSkillCard(cardId, {
        isCaptain: cardId === song.captainCardId,
        orderIndex: showOrder ? index : undefined,
      }, deps));
    }
  }
  section.append(title, preview);
  return section;
}

function skillOrderCardIds(song) {
  const explicitOrder = song.skillOrderCardIds ?? song.skillOrderCardIdsBySong ?? song.skillOrder;
  const cardIds = Array.isArray(explicitOrder) ? explicitOrder : song.teamCardIds;
  return Array.isArray(cardIds) ? cardIds : [];
}

function resultSkillCard(cardId, { isCaptain, orderIndex }, deps) {
  let orderBadge;
  if (Number.isInteger(orderIndex)) {
    orderBadge = document.createElement('span');
    orderBadge.className = 'result-skill-order-badge';
    orderBadge.textContent = String(orderIndex + 1);
  }
  const playerCard = deps.cardConfig(cardId);
  const nameText = deps.cardName?.(cardId) ?? deps.cardLabel(cardId);
  return cardPreviewItem({
    id: cardId,
    name: nameText,
    rarity: deps.cardRarity?.(cardId),
    attribute: deps.cardAttribute(cardId),
    imageUrls: deps.cardIconUrls(cardId, playerCard),
    className: compactJoin(['result-skill-card', isCaptain && 'captain']),
    title: deps.cardLabel(cardId),
    leading: orderBadge,
  });
}

function formatScoreDistributionAverage(distribution) {
  return formatAverageInteger(distribution?.scoreSum, distribution?.sampleCount);
}

function formatAverageInteger(sum, count) {
  const average = roundedAverage(sum, count);
  return average == null ? '-' : formatInteger(average);
}

export function formatScaledAverageInteger(sum, count, multiplier) {
  const average = roundedAverage(Number(sum) * Number(multiplier), count);
  return average == null ? '-' : formatInteger(average);
}

export function formatScaledAverageFixed(sum, count, multiplier, digits) {
  const numerator = Number(sum) * Number(multiplier);
  const denominator = Number(count);
  if (
    !Number.isFinite(numerator)
    || !Number.isFinite(denominator)
    || denominator <= 0
  ) {
    return '-';
  }
  return (numerator / denominator).toFixed(digits);
}

function roundedAverage(sum, count) {
  const numerator = Number(sum);
  const denominator = Number(count);
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || denominator <= 0) {
    return undefined;
  }
  return Math.round(numerator / denominator);
}
