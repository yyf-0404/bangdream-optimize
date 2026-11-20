import {SCORING_REVISION} from '../../src/domain/scoring-revision.js';
import {TIMING_REVISION, TIMING_PREPARATION_REVISION} from '../../src/domain/hit-timing.js';

// Coherent transport/action stub; native scoring is tested with actual engine
// results separately. The caller can choose a total without claiming a replay.
export function timingResult(totalScore = 300, eventType = 'medley') {
  const songCount = eventType === 'medley' ? 3 : 1;
  const songs = [];
  const witnesses = [];
  for (let i = 0; i < songCount; i++) {
    const score = Math.floor(totalScore / songCount) + (i < totalScore % songCount ? 1 : 0);
    songs.push({songId: 10 + i, difficulty: 3, score});
    witnesses.push({
      songId: 10 + i, difficulty: 3,
      input: {
        power: 12345.125, playLevel: 25, fps: 120,
        notes: [{time: 0.09999999999999999}], hitFrames: [12], hitOrder: [0],
        initialCombo: i, medleyCombo: eventType === 'medley',
      },
      result: {
        scoringRevision: SCORING_REVISION, timingRevision: TIMING_REVISION,
        fps: 120, scoringPower: 12345.125, total: score,
        hitFrames: [12], hitOrder: [0],
        trace: [{noteIndex: 0, frame: 12, combo: i + 1, raw: 'bad', judgement: 'perfect', offsetMs: 1e-14, score}],
      },
    });
  }
  return {
    scoringRevision: SCORING_REVISION, eventId: 0, eventType, totalScore, items: {band: 1}, songs,
    hitTiming: {
      complete: true, timingRevision: TIMING_REVISION, preparationRevision: TIMING_PREPARATION_REVISION,
      model: 'chartTimeVisibleNodes', fps: 120, offsetFrames: 0,
      probabilitySemantics: 'perOrderOptimized', songs: witnesses,
    },
  };
}
