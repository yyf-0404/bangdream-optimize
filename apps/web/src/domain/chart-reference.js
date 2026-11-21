export const MAX_HIT_FRAME = 72_000;

// Strongest grade first. The solver's green-node onset table is in reverse order.
const windows = {
  perfect: {units:3, slideLead:6, greenIndex:3},
  great: {units:6, slideLead:12, greenIndex:2},
  good: {units:7, slideLead:14, greenIndex:1},
  bad: {units:8, slideLead:16, greenIndex:0},
};

// The native clock rounds its f32 distance in 1/60-second units to even.
function judgementUnits(frame, time) {
  const units = Math.fround(Math.fround(Math.abs(frame / 120 - time)) / Math.fround(1 / 60));
  const floor = Math.floor(units), fraction = units - floor;
  return fraction === 0.5 ? floor + floor % 2 : Math.round(units);
}

function liveFrames(note, target, minimum) {
  const time = note?.time;
  if (!Number.isFinite(time) || time < 0 || time > 600) throw new Error('谱面参考帧无效');
  const window = note.window ?? {kind:'standard'};
  let first = 0, last = MAX_HIT_FRAME;
  if (window.kind === 'slideTime') {
    const headLead = windows[minimum].slideLead;
    const early = {head:headLead, middle:2, end:headLead, flickEnd:0, directionalEnd:0}[window.role];
    if (early === undefined) throw new Error('未知绿条节点类型');
    const arrival = Math.ceil(time * 120);
    first = Math.max(0, arrival - early);
    // Strict over-line starts Stop at floor(time*120)+1; its 14th update
    // expires after that frame's input. Match the core's nominal projection.
    last = window.last_frame ?? (window.role === 'flickEnd' ? Math.floor(time * 120) + 14
      : Math.floor(time * 120 + Math.fround(13 / 60) * 120));
  } else if (window.kind === 'nativeGreen') {
    first = window.first_frames?.[windows[minimum].greenIndex]; last = window.last_frame;
  } else if (window.kind === 'pinnedPerfect') {
    first = last = window.frame;
  } else if (window.kind === 'standard' || window.kind === 'widenedEndpoint') {
    const threshold = windows[minimum].units + Number(window.kind === 'widenedEndpoint');
    // Start just outside the widest accepted distance; test the fractional
    // boundary in the native clock rather than rounding nominal offsets.
    first = Math.max(0, Math.floor(time * 120) - 2 * threshold);
    last = Math.min(MAX_HIT_FRAME, Math.ceil(time * 120) + 2 * threshold);
    while (judgementUnits(first, time) >= threshold) first++;
    while (judgementUnits(last, time) >= threshold) last--;
  } else {
    throw new Error('未知判定窗，无法确定参考帧');
  }
  first = Math.max(first, target?.activeFromFrame ?? 0);
  last = Math.min(last, target?.activeUntilFrame ?? MAX_HIT_FRAME, MAX_HIT_FRAME);
  if (![first,last].every(Number.isSafeInteger) || first < 0 || first > last) {
    throw new Error('该键没有有效的 Perfect 参考帧');
  }
  return [first, last];
}

// Same coordinate as ReplayInput::chart_reference_frame, independent of the winner.
export function chartReferenceFrame(note, target) {
  const [first, last] = liveFrames(note, target, 'perfect');
  return Math.min(last, Math.max(first, Math.round(note.time * 120)));
}

// A global recalculation may change teams, skill order and trigger frames.
// The solver checks the active correction when it evaluates each candidate hit.
export function chartHitOffsets(note, target, reference) {
  const [first, last] = liveFrames(note, target, 'bad');
  return {firstOffset:first - reference, lastOffset:last - reference};
}

// Each live window includes all better grades. Show only the extra frames
// on either side so that each offset has exactly one raw judgement.
export function rawJudgementOffsets(note, target, reference) {
  const result = [];
  let better;
  for (const grade of Object.keys(windows)) {
    const [first, last] = liveFrames(note, target, grade);
    const ranges = [];
    if (!better) {
      ranges.push([first - reference, last - reference]);
    } else {
      if (first < better[0]) ranges.push([first - reference, better[0] - 1 - reference]);
      if (last > better[1]) ranges.push([better[1] + 1 - reference, last - reference]);
    }
    if (ranges.length) result.push({grade, ranges});
    better = [first, last];
  }
  return result;
}
