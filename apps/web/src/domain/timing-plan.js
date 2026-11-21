import {isCompleteTimingResult, requireCompleteTimingResult, sameTimingChart, TIMING_RANGE_REVISION} from './hit-timing.js';
import {chartReferenceFrame, chartHitOffsets, MAX_HIT_FRAME} from './chart-reference.js';
export {chartReferenceFrame, MAX_HIT_FRAME};

export const PLAN_REVISION = 'bounded-hit-plan-120-v1';
export const RANGE_COORDINATES = 'chartPerfectFrameOffset120-v2';
export const GLOBAL_RANGE_REVISION = TIMING_RANGE_REVISION;

export function referenceFrameRanges(ranges, count) {
  const normalized=normalizeFrameRanges(ranges,count);
  if (normalized.some(r=>r.firstOffset>0 || r.lastOffset<0)) throw new Error('范围必须包含原始帧 0：最早偏移 ≤ 0，最晚偏移 ≥ 0');
  return normalized;
}

function checkedDraftSource(drafts) {
  const source = drafts[0]?.source;
  if (!source || drafts.length !== source.hitTiming.songs.length
    || drafts.some((draft, index) => draft.source !== source || draft.songIndex !== index)) {
    throw new Error('击打范围需要匹配当前选曲');
  }
  return source;
}

export async function recomputeGlobalTimingPlans(drafts, calculate, {isCurrent=()=>true}={}) {
  const source = checkedDraftSource(drafts);
  const generations=drafts.map(d=>d.generation);
  const current=()=>isCurrent() && drafts.every((d,i)=>d.generation===generations[i]);
  const request={revision:GLOBAL_RANGE_REVISION,songs:drafts.map((d,i)=>({
    songId:d.source.hitTiming.songs[i].songId,difficulty:d.source.hitTiming.songs[i].difficulty,
    reference:d.source.hitTiming.songs[i].input,
    ranges:referenceFrameRanges(d.ranges,d.original.input.notes.length),
  }))};
  if (!current()) return null;
  const response=await calculate(request);
  if (!current()) return null;
  requireCompleteTimingResult(response);
  const received=response.hitTiming.rangeRestrictions;
  if (response.eventId !== source.eventId || response.eventType !== source.eventType
    || received?.revision !== GLOBAL_RANGE_REVISION || received.songs?.length !== drafts.length) {
    throw new Error('重算结果与当前活动或击打范围不一致');
  }
  for (let i = 0; i < drafts.length; i++) {
    const expected=request.songs[i], actual=received.songs[i], song=response.hitTiming.songs[i];
    if (song.songId!==expected.songId || song.difficulty!==expected.difficulty
        || actual.songId!==expected.songId || actual.difficulty!==expected.difficulty
        || JSON.stringify(referenceFrameRanges(actual.ranges,song.input.notes.length))!==JSON.stringify(expected.ranges)) {
      throw new Error('重算结果与当前击打范围不一致');
    }
    if (!sameTimingChart(expected.reference,song.input)) throw new Error('重算谱面或按键编号已变化');
    for (const range of frameRangesToAbsolute(expected.ranges,song.input.notes,song.input.nativeTimeRouting?.targets)) {
      const frame=song.result.hitFrames[range.noteIndex];
      if (frame<range.firstFrame || frame>range.lastFrame) throw new Error(`按键 #${range.noteIndex+1} 超出限制`);
    }
  }
  if (!current()) return null;
  drafts.forEach((d,i)=>{
    d.original=response.hitTiming.songs[i];d.evaluatedKey=d.key;
    d.evaluatedResult=d.original.result;d.issue=null;
  });
  return response;
}

export function normalizeFrameRanges(ranges, noteCount) {
  if (!Array.isArray(ranges)) throw new Error('击打范围需要按键编号和最早、最晚偏移');
  const seen = new Set();
  return ranges.map(value => {
    const {noteIndex, firstOffset, lastOffset} = value ?? {};
    if (!Number.isSafeInteger(noteIndex) || noteIndex < 0 || noteIndex >= noteCount) throw new Error('按键编号超出本曲范围');
    if (seen.has(noteIndex)) throw new Error(`按键 #${noteIndex + 1} 的范围重复`);
    seen.add(noteIndex);
    if (![firstOffset, lastOffset].every(v => Number.isSafeInteger(v) && Math.abs(v) <= MAX_HIT_FRAME)) {
      throw new Error(`按键 #${noteIndex + 1} 的偏移必须为 −72000～+72000 内的整数帧`);
    }
    if (firstOffset > lastOffset) throw new Error(`按键 #${noteIndex + 1} 的最早偏移不能晚于最晚偏移`);
    return {noteIndex, firstOffset, lastOffset};
  }).sort((a, b) => a.noteIndex - b.noteIndex);
}

// Only the transport adapter uses absolute song frames. The immutable chart
// reference, never the current winner, anchors every edit and every recompute.
export function frameRangesToAbsolute(ranges, notes, targets = []) {
  return normalizeFrameRanges(ranges, notes.length).map(({noteIndex, firstOffset, lastOffset}) => {
    const base = chartReferenceFrame(notes[noteIndex], targets?.[noteIndex]);
    const firstFrame = Math.max(0, base + firstOffset), lastFrame = Math.min(MAX_HIT_FRAME, base + lastOffset);
    if (firstFrame > lastFrame) throw new Error(`按键 #${noteIndex + 1} 的范围与可计算帧域没有交集`);
    return {noteIndex, firstFrame, lastFrame};
  });
}

export function restoreFrameRanges(saved, notesBySong, targetsBySong = []) {
  if (!Array.isArray(saved?.ranges) || ![1, 3].includes(notesBySong.length)
    || saved.ranges.length !== notesBySong.length) throw new Error('保存范围不完整');
  if (saved.coordinates && saved.coordinates !== RANGE_COORDINATES) throw new Error('保存范围的坐标版本不兼容');
  return saved.ranges.map((ranges, songIndex) => {
    const notes = notesBySong[songIndex];
    if (!Array.isArray(ranges)) throw new Error('保存范围格式无效');
    // Records saved before relative input used unlabelled absolute frame ranges.
    const relative = saved.coordinates === RANGE_COORDINATES ? ranges : ranges.map(value => {
      const {noteIndex, firstFrame, lastFrame} = value ?? {};
      if (!Number.isSafeInteger(noteIndex) || noteIndex < 0 || noteIndex >= notes.length
          || ![firstFrame, lastFrame].every(f => Number.isSafeInteger(f) && f >= 0 && f <= MAX_HIT_FRAME)
          || firstFrame > lastFrame) throw new Error('旧版保存范围无效');
      const base = chartReferenceFrame(notes[noteIndex], targetsBySong[songIndex]?.[noteIndex]);
      return {noteIndex, firstOffset:firstFrame - base, lastOffset:lastFrame - base};
    });
    const normalized = normalizeFrameRanges(relative, notes.length);
    frameRangesToAbsolute(normalized, notes, targetsBySong[songIndex]);
    return normalized;
  });
}

export function parseFrameRanges(text, noteCount) {
  const ranges = [];
  for (const [lineIndex, line] of String(text).split(/\r?\n/).entries()) {
    if (!line.trim()) continue;
    const match = line.trim().match(/^#?(\d+)[\s,，]+([+-]?\d+)[\s,，]+([+-]?\d+)$/);
    if (!match) throw new Error(`第 ${lineIndex + 1} 行格式应为：按键编号 最早偏移 最晚偏移`);
    ranges.push({noteIndex: Number(match[1]) - 1, firstOffset: Number(match[2]), lastOffset: Number(match[3])});
  }
  return normalizeFrameRanges(ranges, noteCount);
}

export function formatFrameRanges(ranges) {
  return ranges.map(r => `${r.noteIndex + 1} ${r.firstOffset > 0 ? '+' : ''}${r.firstOffset} ${r.lastOffset > 0 ? '+' : ''}${r.lastOffset}`).join('\n');
}

export function makeTimingPlanRequest(result, songIndex, ranges) {
  requireCompleteTimingResult(result);
  const song = result.hitTiming.songs[songIndex];
  if (!song) throw new Error('请选择一首有效歌曲');
  return {
    scoringRevision: result.scoringRevision,
    timingRevision: result.hitTiming.timingRevision,
    preparationRevision: result.hitTiming.preparationRevision,
    input: song.input,
    ranges: frameRangesToAbsolute(ranges, song.input.notes, song.input.nativeTimeRouting?.targets),
  };
}

function checkedEvaluation(source, songIndex, ranges, response) {
  if (response?.planRevision !== PLAN_REVISION) throw new Error('击打调整核心版本不兼容，请更新后重算');
  if (response.status !== 'complete') throw new Error(response.message || '击打范围重算未完成');
  const replayed = response.result;
  if (!replayed) throw new Error('重算结果缺少完整击打记录');
  const input = source.hitTiming.songs[songIndex].input;
  for (const range of frameRangesToAbsolute(ranges, input.notes, input.nativeTimeRouting?.targets)) {
    const frame = replayed.hitFrames?.[range.noteIndex];
    if (!Number.isSafeInteger(frame) || frame < range.firstFrame || frame > range.lastFrame) {
      throw new Error(`按键 #${range.noteIndex + 1} 的重算结果超出指定范围`);
    }
  }
  // Reuse the complete transport consistency gate, without publishing this
  // temporary object as an unrestricted full-pool maximum.
  const witness = source.hitTiming.songs[songIndex];
  const songs = source.songs.map((s, i) => i === songIndex ? {...s, score: replayed.total} : s);
  const timingSongs = source.hitTiming.songs.map((s, i) => i === songIndex ? {
    ...s, input: {...witness.input, hitFrames: replayed.hitFrames, hitOrder: replayed.hitOrder}, result: replayed,
  } : s);
  const candidate = {...source, songs, totalScore: songs.reduce((n, s) => n + s.score, 0),
    hitTiming: {...source.hitTiming, songs: timingSongs}};
  if (!isCompleteTimingResult(candidate)) throw new Error('重算结果的分数、帧序或模型记录不完整');
  return replayed;
}

export class TimingPlanDraft {
  constructor(source, songIndex, {includeReference=false}={}) {
    this.source = source;
    this.songIndex = songIndex;
    this.original = source.hitTiming.songs[songIndex];
    this.includeReference=includeReference;
    this.ranges = [];
    this.history = [];
    this.future = [];
    this.generation = 0;
    this.evaluatedKey = '';
    this.evaluatedResult = null;
    this.issue = null;
  }
  get key() { return JSON.stringify(this.ranges); }
  get result() {
    if (this.evaluatedKey === this.key) return this.evaluatedResult;
    return this.ranges.length || this.issue ? null : this.original.result;
  }
  setRanges(ranges, {record = true} = {}) {
    const {notes, nativeTimeRouting} = this.original.input;
    const targets = nativeTimeRouting?.targets;
    const next = (this.includeReference ? referenceFrameRanges : normalizeFrameRanges)(ranges, notes.length);
    if (this.includeReference) {
      // Older editor limits could extend beyond the live window. Keep their
      // effective intersection when opening a result or restoring saved edits.
      for (const range of next) {
        const note = notes[range.noteIndex], target = targets?.[range.noteIndex];
        const bounds = chartHitOffsets(note, target, chartReferenceFrame(note, target));
        range.firstOffset = Math.max(range.firstOffset, bounds.firstOffset);
        range.lastOffset = Math.min(range.lastOffset, bounds.lastOffset);
      }
    } else {
      frameRangesToAbsolute(next, notes, targets);
    }
    if (JSON.stringify(next) === this.key) return false;
    if (record) {
      this.history.push(this.ranges);
      if (this.history.length > 30) this.history.shift();
      this.future = [];
    }
    this.ranges = next;
    this.generation++;
    this.issue = null;
    return true;
  }
  setRange(noteIndex, firstOffset, lastOffset) {
    return this.setRanges([...this.ranges.filter(r => r.noteIndex !== noteIndex), {noteIndex, firstOffset, lastOffset}]);
  }
  removeRange(noteIndex) { return this.setRanges(this.ranges.filter(r => r.noteIndex !== noteIndex)); }
  undo() {
    if (!this.history.length) return false;
    this.future.push(this.ranges);
    return this.setRanges(this.history.pop(), {record: false});
  }
  redo() {
    if (!this.future.length) return false;
    this.history.push(this.ranges);
    return this.setRanges(this.future.pop(), {record: false});
  }
  accept(response, generation) {
    if (generation !== this.generation) return false;
    if (response?.status === 'infeasible' && response.planRevision === PLAN_REVISION) {
      this.evaluatedKey = '';
      this.evaluatedResult = null;
      this.issue = {message: response.message, noteIndices: response.noteIndices ?? []};
      return true;
    }
    this.evaluatedResult = checkedEvaluation(this.source, this.songIndex, this.ranges, response);
    this.evaluatedKey = this.key;
    this.issue = null;
    return true;
  }
  request() { return makeTimingPlanRequest(this.source, this.songIndex, this.ranges); }
}

// Publish every song from the same snapshot after all responses validate.
export async function recomputeTimingPlans(drafts, optimize, {onProgress = () => {}, isCurrent = () => true} = {}) {
  checkedDraftSource(drafts);
  const generations = drafts.map(d => d.generation);
  const requests = drafts.map(d => d.request());
  const responses = [];
  const current = () => isCurrent() && drafts.every((d, i) => d.generation === generations[i]);
  for (let i = 0; i < drafts.length; i++) {
    if (!current()) return false;
    onProgress(i);
    const response = await optimize(requests[i]);
    if (!current()) return false;
    // Validate on a temporary draft so a corrupt third response cannot commit
    // either of the first two songs. Infeasible is a valid completed outcome.
    const check = new TimingPlanDraft(drafts[i].source, i);
    check.setRanges(drafts[i].ranges);
    check.accept(response, check.generation);
    responses.push(response);
  }
  if (!current()) return false;
  responses.forEach((response, i) => drafts[i].accept(response, generations[i]));
  return true;
}

export function timingPlanExport(source, drafts) {
  requireCompleteTimingResult(source);
  if (checkedDraftSource(drafts) !== source || drafts.some(draft => !draft.result)) {
    throw new Error('请先完成已修改范围的重算，再导出方案');
  }
  const songs = drafts.map(draft => ({
    songId: draft.original.songId, difficulty: draft.original.difficulty,
    relativeRanges: draft.ranges,
    ranges: frameRangesToAbsolute(draft.ranges, draft.original.input.notes, draft.original.input.nativeTimeRouting?.targets),
    input: {...draft.original.input, hitFrames: draft.result.hitFrames, hitOrder: draft.result.hitOrder},
    result: draft.result,
  }));
  return {
    schemaVersion: 2, kind: 'hitTimingRangePlan', planRevision: PLAN_REVISION,
    rangeCoordinates: {relativeRanges:RANGE_COORDINATES, ranges:'absoluteSongFrames120'},
    scoringRevision: source.scoringRevision, eventId: source.eventId,
    scope: 'fixedTeamsAndSkillOrders', sourceTotalScore: source.totalScore,
    totalScore: songs.reduce((sum, song) => sum + song.result.total, 0), items: source.items,
    teams: source.songs.map((song, index) => ({
      songId: song.songId, difficulty: song.difficulty, teamCardIds: song.teamCardIds,
      captainCardId: song.captainCardId,
      skillOrderCardIds: song.teamOrder?.skillOrderCardIds,
      score: songs[index].result.total,
    })),
    hitTiming: {
      timingRevision: source.hitTiming.timingRevision, preparationRevision: source.hitTiming.preparationRevision,
      model: source.hitTiming.model, fps: source.hitTiming.fps, offsetFrames: source.hitTiming.offsetFrames,
      complete: true, probabilitySemantics: 'fixedChosenOrder', songs,
    },
  };
}

export function planNoteRows(witness, evaluation = witness.result, noteTypes = []) {
  const trace = new Map((evaluation?.trace ?? []).map(t => [t.noteIndex, t]));
  const before = new Set((witness.input.constraints ?? []).map(e => e.before));
  const after = new Set((witness.input.constraints ?? []).map(e => e.after));
  // Long and Slide both have ordering edges. The declared window, rather than
  // the existence of an edge, identifies their different node types.
  const longHeads = new Set((witness.input.constraints ?? [])
    .filter(e => witness.input.notes[e.after]?.window?.kind === 'widenedEndpoint')
    .map(e => e.before));
  const slideKinds = {head:'绿条头', middle:'绿条中间', end:'绿条尾', flickEnd:'绿条尾滑', directionalEnd:'绿条定向尾滑'};
  const connectionKind = (note, index) => note.window?.kind === 'slideTime' ? 'slide'
    : note.window?.kind === 'widenedEndpoint' || longHeads.has(index) ? 'long'
    : before.has(index) || after.has(index) ? 'slide' : null;
  return witness.input.notes.map((note, index) => {
    const target = witness.input.nativeTimeRouting?.targets?.[index];
    const chartFrame = chartReferenceFrame(note, target);
    return {
      index, number: index + 1, time: note.time, chartFrame,
      defaultRange: chartHitOffsets(note, target, chartFrame),
      lane: (target?.button ?? -1) + 1,
      kind: note.window?.kind === 'slideTime' ? slideKinds[note.window.role]
        : note.window?.kind === 'widenedEndpoint' ? '长按尾' : longHeads.has(index) ? '长按头'
        : before.has(index) || after.has(index) ? (after.has(index) ? (before.has(index) ? '绿条中间' : '绿条尾') : '绿条头') : '单键',
      connectionKind: connectionKind(note, index),
      visualType: note.window?.kind === 'slideTime'
        ? ({middle:'slide-middle',flickEnd:'flick',directionalEnd:'directional'}[note.window.role] ?? 'hold')
        : note.window?.kind === 'widenedEndpoint' || longHeads.has(index) ? 'hold' : 'unknown',
      ...noteTypes[index],
      trigger: note.trigger, originalFrame: witness.input.hitFrames[index],
      frame: evaluation?.hitFrames?.[index], hit: trace.get(index),
      window: target,
    };
  });
}
