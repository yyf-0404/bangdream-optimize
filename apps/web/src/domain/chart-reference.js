export const MAX_HIT_FRAME = 72_000;

// Same coordinate as ReplayInput::chart_reference_frame: nearest live Perfect
// frame to the immutable chart time. This never follows the optimized winner.
export function chartReferenceFrame(note, target) {
  const time = note?.time;
  if (!Number.isFinite(time) || time < 0 || time > 600) throw new Error('谱面参考帧无效');
  const window = note.window ?? {kind:'standard'};
  let first = 0, last = MAX_HIT_FRAME;
  if (window.kind === 'slideTime') {
    const early = {head:6, middle:2, end:6, flickEnd:0, directionalEnd:0}[window.role];
    if (early === undefined) throw new Error('未知绿条节点类型');
    const arrival = Math.ceil(time * 120);
    first = Math.max(0, arrival - early);
    // Strict over-line starts Stop at floor(time*120)+1; its 14th update
    // expires after that frame's input. Match the core's nominal projection.
    last = window.last_frame ?? (window.role === 'flickEnd' ? Math.floor(time * 120) + 14
      : Math.floor(time * 120 + Math.fround(13 / 60) * 120));
  } else if (window.kind === 'nativeGreen') {
    first = window.first_frames?.[3]; last = window.last_frame;
  } else if (window.kind === 'pinnedPerfect') {
    first = last = window.frame;
  } else if (!['standard', 'widenedEndpoint'].includes(window.kind)) {
    throw new Error('未知判定窗，无法确定参考帧');
  }
  first = Math.max(first, target?.activeFromFrame ?? 0);
  last = Math.min(last, target?.activeUntilFrame ?? MAX_HIT_FRAME, MAX_HIT_FRAME);
  if (![first,last].every(Number.isSafeInteger) || first < 0 || first > last) {
    throw new Error('该键没有有效的 Perfect 参考帧');
  }
  const frame = Math.min(last, Math.max(first, Math.round(time * 120)));
  if (['standard', 'widenedEndpoint'].includes(window.kind)) {
    // A lifetime can clip even the nearest ordinary frame. Retain the native
    // f32 quotient and ties-to-even check rather than accepting a non-P frame.
    const units = Math.fround(Math.fround(Math.abs(frame / 120 - time)) / Math.fround(1 / 60));
    const floor = Math.floor(units), fraction = units - floor;
    const rounded = fraction < 0.5 ? floor : fraction > 0.5 ? floor + 1 : floor + floor % 2;
    if (rounded >= (window.kind === 'widenedEndpoint' ? 4 : 3)) {
      throw new Error('该键没有有效的 Perfect 参考帧');
    }
  }
  return frame;
}
