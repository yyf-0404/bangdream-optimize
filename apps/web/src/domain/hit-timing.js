import {SCORING_REVISION} from './scoring-revision.js';
import {chartReferenceFrame} from './chart-reference.js';

export const TIMING_REVISION = 'typed-time-windows-v13';
export const TIMING_PREPARATION_REVISION = 'visible-typed-time-midpoint-120-v6';
export const TIMING_RANGE_REVISION = 'global-relative-ranges-120-v2';

function sameValue(a,b) {
  if (Object.is(a,b)) return true;
  if (!a || !b || typeof a!=='object' || typeof b!=='object' || Array.isArray(a)!==Array.isArray(b)) return false;
  const keys=Object.keys(a);
  return keys.length===Object.keys(b).length && keys.every(k=>Object.hasOwn(b,k) && sameValue(a[k],b[k]));
}
export function sameTimingChart(a,b) {
  return !!a && !!b && ['notes','constraints','nativeTimeRouting','fps','playLevel','initialCombo','medleyCombo','feverFrames']
    .every(k=>sameValue(a[k],b[k]));
}

function validRangeRestrictions(timing) {
  const request=timing.rangeRestrictions;
  if (request==null) return true;
  if (request.revision !== TIMING_RANGE_REVISION || !Array.isArray(request.songs) || request.songs.length !== timing.songs.length) return false;
  return request.songs.every((range,i)=>{
    const song=timing.songs[i],seen=new Set();
    if (!range || range.songId!==song.songId || range.difficulty!==song.difficulty || !sameTimingChart(range.reference,song.input) || !Array.isArray(range.ranges)) return false;
    return range.ranges.every(r=>{
      if (!r || !Number.isSafeInteger(r.noteIndex) || r.noteIndex<0 || r.noteIndex>=song.input.notes.length || seen.has(r.noteIndex)
          || !Number.isSafeInteger(r.firstOffset) || !Number.isSafeInteger(r.lastOffset)
          || r.firstOffset>0 || r.firstOffset < -72000 || r.lastOffset<0 || r.lastOffset>72000) return false;
      seen.add(r.noteIndex);
      try {
        const offset=song.result.hitFrames[r.noteIndex]-chartReferenceFrame(song.input.notes[r.noteIndex],
          song.input.nativeTimeRouting?.targets?.[r.noteIndex]);
        return r.firstOffset<=offset && offset<=r.lastOffset;
      } catch { return false; }
    });
  });
}

export function timingOptimizationEnabled(player) {
  return player?.calculationMode === 'maximize' && player.hitTiming === true;
}

export function isCompleteTimingResult(result) {
  const timing=result?.hitTiming;
  return result?.scoringRevision === SCORING_REVISION
    && timing?.complete === true && timing.timingRevision === TIMING_REVISION
    && timing.preparationRevision === TIMING_PREPARATION_REVISION
    && timing.model === 'chartTimeVisibleNodes' && timing.fps === 120 && timing.offsetFrames === 0
    && timing.probabilitySemantics === 'perOrderOptimized'
    && hasCompleteTimingWitness(result);
}

function nonnegativeInteger(value, maximum = Number.MAX_SAFE_INTEGER) {
  return Number.isSafeInteger(value) && value >= 0 && value <= maximum;
}

// Verify the transported witness and its links to the displayed result. Native
// replay remains responsible for judgement, routing, skill clocks and scoring.
// This is a consistency gate, not a second Rust schema parser or replay engine.
function hasCompleteTimingWitness(result) {
  const timing = result?.hitTiming;
  const songCount = Array.isArray(result?.songs) ? result.songs.length : 0;
  if (timing?.complete !== true || ![1, 3].includes(songCount)
    || !Array.isArray(timing.songs) || timing.songs.length !== songCount
    || (result.eventType === 'medley' && songCount !== 3)
    || (['challenge', 'versus'].includes(result.eventType) && songCount !== 1)
    || typeof result.scoringRevision !== 'string' || !result.scoringRevision
    || typeof timing.timingRevision !== 'string' || !timing.timingRevision
    || !nonnegativeInteger(result.totalScore)
    || !nonnegativeInteger(timing.fps, 0xffff_ffff) || timing.fps === 0) return false;

  let total = 0;
  let combo = 0;
  const medley = songCount === 3;
  for (let songIndex = 0; songIndex < songCount; songIndex++) {
    const song = result.songs[songIndex];
    const witness = timing.songs[songIndex];
    const input = witness?.input;
    const replay = witness?.result;
    if (!song || !witness || !input || !replay
      || !nonnegativeInteger(song.songId, 0xffff_ffff) || !nonnegativeInteger(song.difficulty, 255)
      || witness.songId !== song.songId || witness.difficulty !== song.difficulty
      || replay.scoringRevision !== result.scoringRevision || replay.timingRevision !== timing.timingRevision
      || input.fps !== timing.fps || replay.fps !== timing.fps
      || !Number.isFinite(input.power) || input.power < 0 || replay.scoringPower !== input.power
      || input.medleyCombo !== medley || input.initialCombo !== combo
      || !Array.isArray(input.notes) || !input.notes.length || (medley && input.notes.length > 9_999 - combo)
      || !Array.isArray(input.hitFrames) || !Array.isArray(replay.hitFrames)
      || !Array.isArray(input.hitOrder) || !Array.isArray(replay.hitOrder) || !Array.isArray(replay.trace)
      || !nonnegativeInteger(replay.total) || song.score !== replay.total) return false;

    const count = input.notes.length;
    if ([input.hitFrames, replay.hitFrames, input.hitOrder, replay.hitOrder, replay.trace]
      .some(values => values.length !== count)) return false;
    const seen = new Uint8Array(count);
    let previousFrame = -1;
    let songTotal = 0;
    for (let rank = 0; rank < count; rank++) {
      const index = input.hitOrder[rank];
      if (!nonnegativeInteger(index, count - 1) || seen[index] || replay.hitOrder[rank] !== index) return false;
      seen[index] = 1;
      const frame = input.hitFrames[index];
      const note = input.notes[index];
      const hit = replay.trace[rank];
      if (!note || !Number.isFinite(note.time) || note.time < 0
        || !nonnegativeInteger(frame, Math.min(0xffff_ffff, 600 * timing.fps))
        || frame < previousFrame || replay.hitFrames[index] !== frame
        || hit?.noteIndex !== index || hit.frame !== frame || hit.combo !== combo + rank + 1
        || !['perfect', 'great'].includes(hit.judgement) || !nonnegativeInteger(hit.score, 0xffff_ffff)) return false;
      previousFrame = frame;
      songTotal += hit.score;
    }
    if (songTotal !== replay.total) return false;
    total += songTotal;
    combo += count;
  }
  return nonnegativeInteger(total) && total === result.totalScore && validRangeRestrictions(timing);
}

export function requireCompleteTimingResult(result) {
  if (!isCompleteTimingResult(result)) {
    throw new Error('击打优化结果缺少完整见证或使用不兼容的模型版本，请更新计算核心后重算');
  }
}

export function timingWitnessExport(result) {
  const timing = result?.hitTiming;
  // Preserve historical snapshots verbatim; exporting an old model does not
  // certify it for the current engine. Current results need the same gate as
  // calculation reception and cache reuse.
  if (timing?.complete !== true || !Array.isArray(timing.songs) || ![1, 3].includes(timing.songs.length)
    || (result.scoringRevision === SCORING_REVISION && !hasCompleteTimingWitness(result))) {
    throw new Error('没有完整的击打记录可导出');
  }
  return {
    schemaVersion: 1,
    scoringRevision: result.scoringRevision,
    eventId: result.eventId,
    totalScore: result.totalScore,
    items: result.items,
    teams: result.songs,
    hitTiming: timing,
  };
}

export function timingSongSummary(song) {
  const trace = Array.isArray(song?.result?.trace) ? song.result.trace.filter(Boolean) : [];
  const offsets = trace.map(note => note.offsetMs).filter(Number.isFinite);
  return {
    noteCount: trace.length,
    perfect: trace.filter(note => note.judgement === 'perfect').length,
    great: trace.filter(note => note.judgement === 'great').length,
    minOffsetMs: offsets.length ? Math.min(...offsets) : 0,
    maxOffsetMs: offsets.length ? Math.max(...offsets) : 0,
  };
}
