import {TimingPlanDraft, planNoteRows, restoreFrameRanges, recomputeGlobalTimingPlans, referenceFrameRanges, recomputeTimingPlans} from '../../domain/timing-plan.js';
import {timingChartNoteTypes} from '../../domain/chart-note-types.js';
import {rawJudgementOffsets} from '../../domain/chart-reference.js';
import {drawTimingNote,drawTimingConnection,timingNoteBounds,TIMING_LANE_STEP} from './timing-note-glyph.js';
import {timingPlanStorageKey, loadTimingPlanRanges} from '../../data/timing-plan-store.js';
import {designIcon} from '../../design/fidelity.js';
import {readPreferences, savePreferences} from '../../shared/preferences.js';

const integer = n => Number(n).toLocaleString('zh-CN');
const signed = n => `${n > 0 ? '+' : ''}${integer(n)}`;
const judgementLabel = row => row.hit?.judgement?.toUpperCase() ?? '—';
const laneLabel = row => row.visualType==='directional' && row.width>1 && ['Left','Right'].includes(row.direction)
  ? `${row.lane}→${row.lane+(row.direction==='Left'?-1:1)*(row.width-1)}` : `${row.lane || '—'}`;
const time = seconds => {
  const centiseconds = Math.max(0, Math.round(seconds * 100));
  return `${Math.floor(centiseconds / 6000)}:${(centiseconds % 6000 / 100).toFixed(2).padStart(5, '0')}`;
};
let planId = 0;
const el = (tag, className = '', text) => {
  const node = document.createElement(tag); node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
function button(text, action, className = 'ui-button') {
  const node = el('button', className, text); node.type = 'button'; node.addEventListener('click', action); return node;
}
function arrowButton(label, action, backwards = false) {
  const node = button('', action, `ui-button ui-button--icon timing-arrow${backwards ? ' is-backwards' : ''}`);
  node.innerHTML = designIcon('chevron'); node.setAttribute('aria-label', label); node.title = label;
  return node;
}
function svgShape(tag, attrs, text, parent) {
  const node = document.createElementNS('http://www.w3.org/2000/svg', tag);
  for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, value);
  if (text !== undefined) node.textContent = text;
  parent?.append(node); return node;
}
function numberField(labelText, value, min = -72000, max = 72000, step = 1) {
  const label = el('label', 'timing-field ui-field'), input = el('input', 'ui-input');
  input.type = 'number'; input.inputMode = min < 0 ? 'text' : step < 1 ? 'decimal' : 'numeric';
  input.step = step; input.min = min; input.max = max; input.required = true; input.value = value;
  label.append(el('span', 'ui-field__label', labelText), input); return {label, input};
}
function fieldInteger(input) {
  if (!/^[+-]?\d+$/.test(input.value.trim()) || !Number.isSafeInteger(Number(input.value))) throw new Error('请输入整数帧');
  return Number(input.value);
}

export function createHitTimingPlan(source, {songLabel = id => `歌曲 ${id}`, getRuntime, getProfileId, getCore, player, onResult} = {}) {
  const drafts = source.hitTiming.songs.map((_, index) => new TimingPlanDraft(source, index, { includeReference: true }));
  const multiple = drafts.length > 1;
  const applyLabel = multiple ? '应用并重算三曲' : '应用并重算';
  source.hitTiming.rangeRestrictions?.songs?.forEach((song,i)=>{
    drafts[i].setRanges(song.ranges,{record:false});drafts[i].evaluatedKey=drafts[i].key;drafts[i].evaluatedResult=drafts[i].original.result;
  });
  const calculationPlayer=player?structuredClone(player):null;
  let displayFps = readPreferences().timingDisplayFps === 60 ? 60 : 120;
  const displayFrames = frames => frames * displayFps / 120;
  const signedFrames = frames => signed(displayFrames(frames));
  function readFrameOffset(input) {
    const frames = input.valueAsNumber / displayFrames(1);
    if (!Number.isSafeInteger(frames)) throw new Error(displayFps === 60 ? '请输入 0.5 帧的整数倍' : '请输入整数帧');
    return frames;
  }
  let currentResult=source;
  let planReady=true,failure='';
  let appliedKeys=drafts.map(d=>d.key),appliedPlanValid=true;
  const pendingSongs=()=>drafts.filter((d,i)=>d.key!==appliedKeys[i]).length;
  const views = drafts.map(d => ({selected:0, scroll:Math.max(0, d.original.input.notes[0].time - 0.75) * 240}));
  const noteTypes = drafts.map(() => []), typeErrors = drafts.map(() => '');
  let songIndex = 0, busy = false, initializing = true, rows = [], keyPromise;
  let chartEnd = 120, zoomPixels = 240, noteMarkers = [], pendingScroll = null;
  const noteSkin = 'original';
  const id = `timing-plan-${++planId}`;
  const current = () => drafts[songIndex], view = () => views[songIndex];
  const profileId = getProfileId?.() ?? '';
  const storageKey = () => keyPromise ??= timingPlanStorageKey(source, profileId);
  const root = el('section', 'timing-plan'); root.setAttribute('aria-label', '击打方案');
  root.dataset.noteSkin=noteSkin;
  const heading = el('header', 'timing-heading'), title = el('div', 'timing-heading-title'), total = el('div', 'timing-total');
  const titleText = el('h2', 'ui-section__title timing-title', '击打方案'); titleText.insertAdjacentHTML('afterbegin', designIcon('activity'));
  const frameUnits = el('div', 'ui-segmented');
  frameUnits.setAttribute('role', 'radiogroup'); frameUnits.setAttribute('aria-label', '帧数显示单位');
  for (const fps of [60, 120]) {
    const label = el('label'), radio = el('input');
    radio.type = 'radio'; radio.name = `${id}-fps`; radio.value = fps; radio.checked = fps === displayFps;
    radio.addEventListener('change', () => {
      // Preserve unsubmitted offsets in the new unit, including blank fields.
      const values = [...rangeForm.querySelectorAll('input')].map(input => input.valueAsNumber * fps / displayFps);
      displayFps = fps;
      savePreferences({timingDisplayFps: fps});
      refresh();
      rangeForm.querySelectorAll('input').forEach((input, i) => {
        input.value = Number.isFinite(values[i]) ? values[i] : '';
      });
    });
    label.append(radio, document.createTextNode(`${fps} 帧/秒`)); frameUnits.append(label);
  }
  title.append(titleText, frameUnits); heading.append(title, total);
  const tabs = el('div', 'timing-tabs'); tabs.setAttribute('role', 'tablist'); tabs.setAttribute('aria-label', '歌曲谱面');
  const panel = el('div', 'timing-panel'); panel.id = `${id}-panel`;
  panel.setAttribute('role', multiple ? 'tabpanel' : 'region');
  if (!multiple) panel.setAttribute('aria-label', songLabel(drafts[0].original.songId));
  const tabViews = multiple ? drafts.map((draft, index) => {
    const tab = button('', () => selectSong(index), 'timing-tab');
    tab.id = `${id}-song-${index}`;
    tab.setAttribute('role', 'tab');
    tab.setAttribute('aria-controls', panel.id);
    tab.title = songLabel(draft.original.songId);
    const score = el('b'), state = el('small', 'timing-tab-state', '待应用');
    const meta = el('span', 'timing-tab-meta');
    meta.append(score, state);
    tab.append(el('span', 'timing-tab-title', tab.title), meta);
    tab.addEventListener('keydown', event => {
      if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? drafts.length - 1
        : (index + (event.key === 'ArrowRight' ? 1 : drafts.length - 1)) % drafts.length;
      selectSong(next);
      tabViews[next].tab.focus({preventScroll:true});
    });
    tabs.append(tab);
    return {tab, score, state};
  }) : [];
  const toolbar = el('div', 'timing-toolbar');
  const skillLabel = el('label', 'timing-inline-field'), skills = el('select', 'ui-select');
  skillLabel.append(el('span', '', '技能区间'), skills); skills.setAttribute('aria-label', '跳转技能区间');
  skills.addEventListener('change', () => {
    if (skills.value === '') return;
    const event = displayed().skillEvents?.find(e => e.kind === 'start' && e.index === Number(skills.value));
    if (!event) return;
    const trigger = rows.find(row => row.trigger === event.index);
    if (trigger) selectNote(trigger.index, {reveal:false});
    skills.value = String(event.index);
    scrollToFrame(event.firstInputFrame);
  });
  const changes = el('div', 'timing-change-nav');
  const previous = arrowButton('上一个偏移按键', () => jumpChanged(-1), true);
  const next = arrowButton('下一个偏移按键', () => jumpChanged(1));
  const shiftedLabel = el('label', 'timing-inline-field timing-shift-picker');
  const shifted = el('select', 'ui-select');
  shifted.setAttribute('aria-label', '偏移按键');
  shifted.dataset.searchable = 'false';
  const currentKey = el('option');
  currentKey.value = ''; currentKey.hidden = true;
  shiftedLabel.append(el('span', '', '偏移按键'), shifted);
  shifted.addEventListener('change', () => {
    if (shifted.value !== '') selectNote(Number(shifted.value));
  });
  changes.append(previous, shiftedLabel, next);
  const locateForm = el('form', 'timing-locate'), locate = numberField('#', 1, 1);
  const go = el('button', 'ui-button ui-button--text ui-button--brand', '定位'); go.type = 'submit'; locateForm.noValidate = true;
  const locateError = el('p', 'timing-field-error ui-field__error');
  locateError.id = `${id}-locate-error`; locateError.setAttribute('role', 'alert');
  locate.input.setAttribute('aria-label', '按键编号');
  locate.input.setAttribute('aria-describedby', locateError.id);
  locateForm.append(locate.label, go);
  locateForm.addEventListener('submit', event => {
    event.preventDefault();
    try {
      const n = fieldInteger(locate.input);
      if (n < 1 || n > rows.length) throw new Error(`按键编号应为 1–${rows.length}`);
      selectNote(n - 1);
    } catch (error) { locateError.textContent = error.message; locate.input.setAttribute('aria-invalid', 'true'); locate.input.focus(); }
  });
  const zoomLabel = el('label', 'timing-zoom'), zoom = el('input');
  const zoomValue = el('span', 'timing-zoom-value', '100%');
  zoom.type = 'range'; zoom.min = '50'; zoom.max = '200'; zoom.step = '5'; zoom.value = '100';
  zoom.setAttribute('aria-label', '谱面缩放');
  zoom.setAttribute('aria-valuetext', '100%');
  zoomLabel.append(el('span', '', '缩放'), zoom, zoomValue);
  let zoomFrame;
  zoom.addEventListener('input', () => {
    zoomValue.textContent = zoom.value + '%';
    zoom.setAttribute('aria-valuetext', zoomValue.textContent);
    if (zoomFrame !== undefined) return;
    zoomFrame = requestAnimationFrame(() => {
      zoomFrame = undefined;
      if (!root.isConnected) return;
      const nextPixels = 240 * Number(zoom.value) / 100;
      const ratio = nextPixels / zoomPixels;
      const center = chart.clientWidth / 2;
      for (const savedView of views) {
        const anchor = savedView === view() ? center : 0;
        savedView.scroll = Math.max(0, 30 + (savedView.scroll + anchor - 30) * ratio - anchor);
      }
      zoomPixels = nextPixels;
      renderChart();
    });
  });
  const navigation = el('div', 'timing-navigation');
  navigation.append(skillLabel, changes, zoomLabel);
  toolbar.append(navigation);
  const chartHeader = el('header', 'timing-chart-header');
  const chartTitle = el('h3', 'ui-subtitle timing-chart-title'), chartState = el('span', 'timing-chart-state');
  chartHeader.append(chartTitle, chartState);
  const overview = el('div', 'timing-overview'), overviewHeading = el('div', 'timing-overview-heading');
  const viewportLabel = el('span'), overviewTrack = el('div', 'timing-overview-track'), overviewArt = el('div', 'timing-overview-art');
  const scrubber = el('input', 'timing-scrubber'); scrubber.type = 'range'; scrubber.min = '0'; scrubber.max = '0'; scrubber.step = '1';
  scrubber.setAttribute('aria-label', '全曲浏览位置'); scrubber.title = '拖动定位；方向键微调，Home / End 跳到曲首或曲尾';
  scrubber.addEventListener('input', () => { chart.scrollLeft = Number(scrubber.value); updateViewport(); });
  let overviewWindowWidth = 0, overviewDrag;
  function dragOverview(clientX) {
    const {left, width} = scrubber.getBoundingClientRect();
    const travel = width - overviewWindowWidth;
    if (travel <= 0) return;
    const position = Math.max(0, Math.min(1, (clientX - left - overviewDrag.offset) / travel));
    chart.scrollLeft = position * Number(scrubber.max);
    updateViewport();
  }
  scrubber.addEventListener('pointerdown', event => {
    if (!event.isPrimary || event.button !== 0 || scrubber.disabled || overviewDrag) return;
    const {left, width} = scrubber.getBoundingClientRect();
    const maximum = Number(scrubber.max);
    if (!maximum || width <= overviewWindowWidth) return;
    event.preventDefault();
    const windowLeft = (width - overviewWindowWidth) * Number(scrubber.value) / maximum;
    const withinWindow = event.clientX - left - windowLeft;
    const offset = withinWindow >= 0 && withinWindow <= overviewWindowWidth ? withinWindow : overviewWindowWidth / 2;
    overviewDrag = {pointerId: event.pointerId, offset};
    scrubber.focus({preventScroll: true});
    scrubber.setPointerCapture(event.pointerId);
    dragOverview(event.clientX);
  });
  scrubber.addEventListener('pointermove', event => {
    if (overviewDrag?.pointerId === event.pointerId) dragOverview(event.clientX);
  });
  scrubber.addEventListener('lostpointercapture', event => {
    if (overviewDrag?.pointerId === event.pointerId) overviewDrag = undefined;
  });
  overviewHeading.append(el('span', '', '全曲概览'), viewportLabel);
  overviewTrack.append(overviewArt, scrubber); overview.append(overviewHeading, overviewTrack);
  const chartFrame = el('div', 'timing-chart-frame'), lanes = el('div', 'timing-lanes'); lanes.setAttribute('aria-hidden', 'true');
  lanes.append(el('span', '', '轨')); for (let i = 1; i <= 7; i++) lanes.append(el('span', '', i));
  const chart = el('div', 'timing-chart'); chart.setAttribute('aria-label', '完整谱面，可鼠标拖动或使用上方全曲概览浏览'); chart.tabIndex = 0;
  chart.addEventListener('scroll', updateViewport, {passive:true});
  let chartDrag, suppressChartClick = false;
  function stopChartDrag() {
    chartDrag = undefined;
    delete chart.dataset.dragging;
  }
  chart.addEventListener('pointerdown', event => {
    if (chartDrag) return;
    suppressChartClick = false;
    if (event.pointerType !== 'mouse' || event.button !== 0 || chart.dataset.canPan !== 'true'
      || !chart.firstElementChild?.contains(event.target)) return;
    chartDrag = {pointerId: event.pointerId, startX: event.clientX, scrollLeft: chart.scrollLeft, moved: false};
    // Keep a simple click on the original note, even when the mouse is captured.
    event.target.setPointerCapture(event.pointerId);
  });
  function panChart(event) {
    if (chartDrag?.pointerId !== event.pointerId) return;
    const distance = event.clientX - chartDrag.startX;
    if (!chartDrag.moved) {
      if (Math.abs(distance) < 4) return;
      chartDrag.moved = true;
      suppressChartClick = true;
      chart.dataset.dragging = 'true';
    }
    chart.scrollLeft = chartDrag.scrollLeft - distance;
    updateViewport();
  }
  chart.addEventListener('pointermove', panChart);
  chart.addEventListener('pointerup', event => {
    if (chartDrag?.pointerId !== event.pointerId) return;
    panChart(event);
    stopChartDrag();
  });
  for (const type of ['pointercancel', 'lostpointercapture']) {
    chart.addEventListener(type, event => {
      if (chartDrag?.pointerId === event.pointerId) stopChartDrag();
    });
  }
  chart.addEventListener('dragstart', event => {
    if (chartDrag) event.preventDefault();
  });
  chart.addEventListener('click', event => {
    if (!suppressChartClick || !event.detail) return;
    suppressChartClick = false;
    event.stopPropagation();
  }, true);
  chart.addEventListener('keydown', event => {
    if (event.target === chart && ['Home','End'].includes(event.key)) {
      event.preventDefault(); chart.scrollLeft = event.key === 'Home' ? 0 : chart.scrollWidth;
    }
  });
  chartFrame.append(lanes, chart);
  const legend = el('div', 'timing-legend');
  for (const [visualType,text] of [['tap','单键'],['flick','滑键'],['hold','长按 / 绿条'],['slide-middle','中间点'],['directional','定向滑键']]) {
    const row={visualType,direction:visualType==='directional'?'Left':null};
    const bounds=timingNoteBounds(row);
    const item=el('span','timing-key-legend'), icon=svgShape('svg',{viewBox:`${bounds.x} ${bounds.y} ${bounds.width} ${bounds.height}`,width:24,height:22,'aria-hidden':'true'});
    drawTimingNote(icon,row,noteSkin);item.append(icon,document.createTextNode(text));legend.append(item);
  }
  for (const [cls,text] of [['timing-moved','偏移'],['timing-original','原位置'],['timing-skill','技能区间'],['timing-range-mark','限制范围']])legend.append(el('span',cls,text));
  const typeState=el('span','timing-type-state');legend.append(typeState);
  const chartBlock = el('div', 'timing-chart-block');
  chartBlock.append(chartHeader, toolbar, overview, chartFrame, legend);
  const editor = el('section', 'timing-editor'), editorHeading = el('header', 'timing-editor-heading');
  const editorTitle = el('h3', 'ui-subtitle', '按键编辑'); editorTitle.id = `${id}-editor`;
  editor.setAttribute('aria-labelledby', editorTitle.id);
  editorHeading.append(editorTitle, locateForm);
  const noteDetails = el('div', 'timing-note-details'), noteSummary = el('div', 'timing-note-summary');
  const noteMeta = el('span'), activeSkill = el('span', 'timing-active-skill');
  const comparison = el('dl', 'timing-comparison');
  noteSummary.append(noteMeta, activeSkill);
  noteDetails.append(noteSummary, comparison);
  const rangeEditor = el('div', 'timing-range-editor');
  const rangeHeading = el('div', 'timing-range-heading');
  const rangeState = el('span', 'timing-range-state', '已加限制');
  const removeRange = button('移除限制', () => change(() => current().removeRange(view().selected)), 'ui-button ui-button--text');
  rangeHeading.append(el('h4', 'ui-subtitle', '可击打范围'));
  const rangeForm = el('form', 'timing-range-form'); rangeForm.noValidate = true;
  rangeEditor.append(rangeHeading, rangeForm);
  editor.append(editorHeading, locateError, noteDetails, rangeEditor);

  const limitList = el('section', 'timing-list'), listHeading = el('header', 'timing-list-heading');
  const listTitle = el('h3', 'ui-subtitle', '已设限制'); listTitle.id = `${id}-limits`;
  limitList.setAttribute('aria-labelledby', listTitle.id);
  const limitCount = el('span', 'timing-list-count'), limitBox = el('div', 'timing-list-box');
  listHeading.append(listTitle, limitCount);
  const undo = button('撤销', () => change(() => current().undo()), 'ui-button ui-button--text'); undo.title = '撤销本曲上次限制修改';
  const redo = button('重做', () => change(() => current().redo()), 'ui-button ui-button--text'); redo.title = '重做本曲限制修改';
  const reset = button('清空本曲限制', () => change(() => current().setRanges([])), 'ui-button ui-button--text ui-button--danger');
  const history = el('div', 'timing-history'); history.append(undo, redo);
  const listActions = el('div', 'timing-list-actions'); listActions.append(history, reset);
  limitList.append(listHeading, limitBox, listActions);

  const status = el('p', 'ui-text ui-text--secondary timing-status');
  status.id = `${id}-status`; status.setAttribute('role', 'status'); status.setAttribute('aria-live', 'polite'); status.setAttribute('aria-atomic', 'true');
  const applyChanges = button(applyLabel, () => void recompute(), 'ui-button ui-button--primary');
  const applyState = el('span', 'timing-apply-state'), applyActions = el('div', 'timing-apply-actions');
  applyState.setAttribute('role', 'status');
  applyActions.append(applyState, applyChanges);
  const applyFooter = el('div', 'timing-apply-footer'); applyFooter.append(applyActions, status);
  const records = el('details', 'timing-records'); records.append(el('summary', '', '全部按键记录'));
  const tableBox = el('div', 'timing-table-box');
  records.append(tableBox);
  records.addEventListener('toggle', () => { if (records.open) renderDetails(); });
  const editLayout = el('div', 'timing-edit-layout');
  editLayout.append(editor, limitList, applyFooter);
  panel.append(chartBlock, editLayout, records); root.append(heading);
  if (multiple) root.append(tabs);
  root.append(panel);
  root.getTimingPlanResult=()=>{
    if (!planReady || busy || initializing || drafts.some(d=>!d.result || d.issue)) throw new Error('请先应用列表中的限制并完成重算，再导出');
    return currentResult;
  };

  function displayed() { const d = current(); return d.result ?? d.evaluatedResult ?? d.original.result; }
  function selectSong(index) {
    if (index === songIndex) return;
    songIndex = index;
    refresh();
  }
  function message(text, error = false) { status.textContent = text; status.dataset.kind = error ? 'error' : 'info'; }
  function changedIndices() { return rows.filter(r => r.frame !== r.chartFrame).map(r => r.index); }
  function scrollToFrame(frame) { chart.scrollLeft = Math.max(0, 30 + frame / 120 * zoomPixels - chart.clientWidth / 3); updateViewport(); }
  function selectNote(index, {focus = false, reveal = true} = {}) {
    if (!rows[index]) return;
    const previous = view().selected; view().selected = index;
    updateMarker(previous); updateMarker(index); renderEditor(); updateSelectionControls(); syncListSelection(true);
    if (reveal) scrollToFrame(Math.min(rows[index].chartFrame, rows[index].frame));
    if (focus) noteMarkers[index]?.focus({preventScroll:true});
  }
  function updateSelectionControls() {
    const row = rows[view().selected];
    locate.input.value = row.number; locate.input.max = rows.length; locate.input.removeAttribute('aria-invalid');
    locateError.textContent = '';
    skills.value = String(row.hit?.activeSkill ?? row.trigger ?? '');
    shifted.value = row.frame === row.chartFrame ? '' : String(row.index);
  }
  function updateMarker(index) {
    const marker = noteMarkers[index], row = rows[index]; if (!marker || !row) return;
    const selected = index === view().selected;
    marker.classList.toggle('is-selected', selected); marker.setAttribute('aria-pressed', selected); marker.setAttribute('tabindex', selected ? '0' : '-1');
    marker.querySelector('text')?.remove();
    if (selected) svgShape('text', {x:0, y:timingNoteBounds(row).y-5, 'text-anchor':'middle'}, `#${row.number}${row.frame === row.chartFrame ? '' : ` ${signedFrames(row.frame - row.chartFrame)}`}`, marker);
    if(selected)marker.parentNode?.append(marker);
  }
  function jumpChanged(direction) {
    const indices = changedIndices(); if (!indices.length) return;
    const selected = view().selected;
    selectNote(direction > 0 ? (indices.find(i => i > selected) ?? indices[0]) : (indices.findLast(i => i < selected) ?? indices.at(-1)));
  }
  function locateFromList(index) {
    selectNote(index, {focus:true}); chartFrame.scrollIntoView({block:'nearest'});
  }
  function change(action) {
    if (busy || initializing || action()===false) return false;
    // Editing the list never submits a calculation. Undoing all pending edits
    // restores the last published result without treating it as a new solve.
    planReady=appliedPlanValid && pendingSongs()===0;failure='';refresh();
    return true;
  }
  function refresh() {
    const draft = current(), working = busy || initializing; rows = planNoteRows(draft.original, displayed(), noteTypes[songIndex]);
    const complete = planReady && !working && drafts.every(d => d.result);
    const sum = currentResult.totalScore;
    const timingScore = el('div', 'timing-total-value');
    timingScore.append(el('strong', '', integer(sum)));
    if (sum !== source.totalScore) timingScore.append(el('small', '', `较初算 ${signed(sum - source.totalScore)}`));
    const scoreLabel = multiple ? '三曲总分' : '方案得分';
    total.replaceChildren(el('span', '', complete ? scoreLabel : `上次${scoreLabel}`), timingScore);
    if (multiple) {
      tabViews.forEach(({tab, score, state}, i) => {
        tab.setAttribute('aria-selected', String(i === songIndex));
        tab.tabIndex = i === songIndex ? 0 : -1;
        score.textContent = integer(currentResult.songs[i].score);
        state.hidden = drafts[i].key === appliedKeys[i];
      });
      panel.setAttribute('aria-labelledby', `${id}-song-${songIndex}`);
    }
    const events = displayed().skillEvents ?? []; skills.replaceChildren(el('option', '', '选择区间…')); skills.firstChild.value = '';
    for (const start of events.filter(e => e.kind === 'start')) {
      const end = events.find(e => e.kind === 'end' && e.index === start.index);
      const option = el('option', '', `${start.index + 1} · ${time(start.firstInputFrame / 120)}–${time((end?.lastInputFrame ?? start.firstInputFrame) / 120)}`);
      option.value = start.index; skills.append(option);
    }
    skills.disabled = skills.options.length === 1;
    const indices = changedIndices();
    currentKey.textContent = indices.length ? `${integer(indices.length)} 个偏移按键` : '无偏移按键';
    shifted.replaceChildren(currentKey, ...indices.map(index => {
      const row = rows[index], offset = row.frame - row.chartFrame;
      const option = el('option', '', `#${row.number} · ${time(row.time)} · ${signedFrames(offset)} 帧`);
      option.value = String(index);
      return option;
    }));
    shifted.disabled = previous.disabled = next.disabled = !indices.length;
    updateSelectionControls();
    undo.disabled = working || !draft.history.length; redo.disabled = working || !draft.future.length; reset.disabled = working || !draft.ranges.length;
    applyChanges.disabled = working || complete || !calculationPlayer;
    applyChanges.textContent = busy ? '重算中…' : applyLabel;
    const pending = pendingSongs();
    applyActions.dataset.pending = String(!working && pending > 0);
    applyState.textContent = initializing ? '正在准备方案' : busy ? '正在重算'
      : !calculationPlayer ? '请重新计算以载入原始档案'
      : pending ? (multiple ? `${pending} 首歌曲的限制待应用` : '限制待应用')
      : complete ? '修改限制后可重算' : '需重新计算';
    root.dataset.exportReady = String(complete && drafts.every(d => !d.issue));
    root.dispatchEvent(new Event('timing-plan-readiness', { bubbles: true }));
    chartTitle.textContent = songLabel(draft.original.songId);
    chartState.textContent = complete ? '' : busy ? '上次方案 · 重算中' : '上次方案';
    typeState.textContent=typeErrors[songIndex]?'键型资料未匹配 · 基础显示':'';
    typeState.title=typeErrors[songIndex];
    root.setAttribute('aria-busy', String(working));
    if (initializing) message('正在准备击打方案…');
    else if (busy) message('正在重新搜索最优编队、技能顺序和道具…');
    else if (failure) message(failure,true);
    else if (draft.issue) message(draft.issue.message, true);
    else if (drafts.some(d => d.issue)) message('其他歌曲的限制无解，请切换曲目调整。', true);
    else message('');
    renderChart(); renderOverview(); renderEditor(); renderDetails(); renderLists();
  }
  function renderEditor() {
    const draft = current(), row = rows[view().selected], working = busy || initializing; if (!row) return;
    const range = draft.ranges.find(r => r.noteIndex === row.index), offset = row.frame - row.chartFrame;
    noteMeta.textContent = `${row.kind} · ${laneLabel(row)} 轨`;
    activeSkill.textContent = row.hit?.activeSkill != null ? `技能 ${row.hit.activeSkill + 1}` : '';
    const timing = [['原始参考', time(row.chartFrame / 120)]];
    if (offset) timing.push(['方案击打', time(row.frame / 120)]);
    const offsetLabel = offset ? `${offset < 0 ? '提前' : '延后'} ${displayFrames(Math.abs(offset))} 帧` : '原位击打';
    timing.push(['击打偏移', offsetLabel, offset ? 'is-shifted' : ''], ['判定', judgementLabel(row)]);
    comparison.replaceChildren();
    for (const [label, value, valueClass] of timing) {
      const item = el('div');
      item.append(el('dt', '', label), el('dd', valueClass, value));
      comparison.append(item);
    }
    const {firstOffset, lastOffset} = range ?? row.defaultRange;
    const {firstOffset: lower, lastOffset: upper} = row.defaultRange;
    const first = numberField('最早偏移（帧）', displayFrames(firstOffset), displayFrames(lower), 0, displayFrames(1));
    const last = numberField('最晚偏移（帧）', displayFrames(lastOffset), 0, displayFrames(upper), displayFrames(1));
    first.input.disabled = last.input.disabled = working;
    if (range) {
      removeRange.disabled = working;
      rangeHeading.append(rangeState, removeRange);
    } else {
      rangeState.remove(); removeRange.remove();
    }
    const apply = el('button', 'ui-button ui-button--text ui-button--brand timing-range-save', range ? '更新限制' : '添加限制'); apply.type = 'submit'; apply.disabled = working;
    const fieldError = el('p', 'timing-field-error ui-field__error'); fieldError.id = `${id}-range-error`; fieldError.setAttribute('role', 'alert');
    const hint = el('dl', 'ui-field__hint timing-judgement-ranges');
    hint.id = `${id}-range-hint`;
    hint.setAttribute('aria-label', '原始判定范围');
    const note = draft.original.input.notes[row.index];
    for (const {grade, ranges} of rawJudgementOffsets(note, row.window, row.chartFrame)) {
      const offsets = ranges.map(([start, end]) => start === end ? signedFrames(start) : `${signedFrames(start)}～${signedFrames(end)}`);
      const item = el('div');
      item.append(el('dt', '', grade.toUpperCase()), el('dd', '', `${offsets.join(' / ')} 帧`));
      hint.append(item);
    }
    first.input.setAttribute('aria-describedby', `${hint.id} ${fieldError.id}`); last.input.setAttribute('aria-describedby', `${hint.id} ${fieldError.id}`);
    rangeForm.replaceChildren(first.label, el('span', 'timing-range-separator', '至'), last.label, apply, fieldError, hint);
    rangeForm.onsubmit = event => {
      event.preventDefault();
      let target = first.input;
      try {
        first.input.removeAttribute('aria-invalid'); last.input.removeAttribute('aria-invalid'); fieldError.textContent = '';
        const earliest = readFrameOffset(first.input);
        if (earliest < lower || earliest > 0) throw new Error(`最早偏移须在 ${signedFrames(lower)} 到 0 帧之间`);
        target = last.input; const latest = readFrameOffset(last.input);
        if (latest < 0 || latest > upper) throw new Error(`最晚偏移须在 0 到 ${signedFrames(upper)} 帧之间`);
        if (change(() => draft.setRange(row.index, earliest, latest))) {
          editor.querySelector('.timing-range-save')?.focus({preventScroll:true});
        }
      } catch (error) { fieldError.textContent = error.message; target.setAttribute('aria-invalid', 'true'); target.focus(); }
    };
  }
  function renderChart() {
    const draft = current(), evaluation = displayed(), px = zoomPixels;
    chartEnd = rows.reduce((last, row) => Math.max(last, row.chartFrame, row.frame), 0) + 120;
    const lastFrame = chartEnd, width = Math.max(640, lastFrame / 120 * px + 60);
    const svg = svgShape('svg', {width, height:320, viewBox:`0 0 ${width} 320`}); noteMarkers = [];
    svg.style.width = `${width}px`; // Override the shared icon sizing without scaling the whole chart down.
    svg.setAttribute('role', 'group'); svg.setAttribute('aria-label', `完整七轨谱面 · ${rows.length} 个按键`);
    const shape = (tag, attrs, text, parent = svg) => svgShape(tag, attrs, text, parent);
    const x = f => 30 + f / 120 * px, y = lane => 48 + (Math.max(1, lane) - 1) * TIMING_LANE_STEP, events = evaluation.skillEvents ?? [];
    for (const start of events.filter(e => e.kind === 'start')) {
      const end = events.find(e => e.kind === 'end' && e.index === start.index), right = Math.min(lastFrame, end?.lastInputFrame ?? lastFrame);
      if (right < start.firstInputFrame) continue;
      shape('rect', {x:x(start.firstInputFrame), y:5, width:Math.max(1, x(right) - x(start.firstInputFrame)), height:286, class:'plan-skill-area'});
      for (const frame of [start.firstInputFrame, right]) shape('line', {x1:x(frame), x2:x(frame), y1:5, y2:291, class:'plan-skill-boundary'});
      shape('text', {x:x(start.firstInputFrame) + 7, y:22, class:'plan-skill-label', 'data-start':x(start.firstInputFrame), 'data-end':x(right)}, `技能 ${start.index + 1}`);
    }
    for (let lane = 1; lane <= 7; lane++) shape('line', {x1:0, x2:width, y1:y(lane), y2:y(lane), class:'plan-lane'});
    for (let second = 0; second <= lastFrame / 120; second++) {
      shape('line', {x1:x(second * 120), x2:x(second * 120), y1:32, y2:291, class:'plan-grid'});
      shape('text', {x:x(second * 120), y:309, class:'plan-axis', 'text-anchor':'middle'}, time(second));
    }
    const connections=shape('g',{'aria-hidden':'true'}),guides=shape('g',{'aria-hidden':'true'}),notes=shape('g',{class:'plan-note-layer'});
    for (const edge of draft.original.input.constraints ?? []) {
      const a = rows[edge.before], b = rows[edge.after]; if (!a || !b) continue;
      const attrs={x1:x(a.frame),x2:x(b.frame),y1:y(a.lane),y2:y(b.lane)};
      drawTimingConnection(connections,attrs,noteSkin,a.connectionKind==='long');
    }
    const ranges = new Map(draft.ranges.map(r => [r.noteIndex, r]));
    for (const row of rows) {
      const range = ranges.get(row.index), moved = row.frame !== row.chartFrame;
      if (range) {
        const left = x(Math.max(0, row.chartFrame + range.firstOffset)), right = x(Math.min(lastFrame, row.chartFrame + range.lastOffset)), top = y(row.lane) + 19;
        shape('path', {d:`M${left} ${top - 2}v4m0 -2H${right}m0 -2v4`, class:'plan-range'},undefined,guides);
      }
      if (moved) {
        shape('rect', {x:x(row.chartFrame)-4, y:y(row.lane)-7, width:8,height:14,class:'plan-original'},undefined,guides);
        shape('path', {d:`M${x(row.chartFrame)} ${y(row.lane)+8}v7H${x(row.frame)}`,class:'plan-displacement'},undefined,guides);
      }
      const marker = shape('g', {class:`plan-note${moved ? ' is-moved' : ''}${row.index === view().selected ? ' is-selected' : ''}`,
        transform:`translate(${x(row.frame)} ${y(row.lane)})`, role:'button', tabindex:row.index === view().selected ? '0' : '-1',
        'aria-label':`按键 #${row.number}，${row.kind}，${laneLabel(row)} 轨，${moved ? `${signedFrames(row.frame - row.chartFrame)} 帧` : '原位'}`,
        'aria-pressed':row.index === view().selected, 'data-note-index':row.index, 'data-chart-frame':row.chartFrame, 'data-hit-frame':row.frame,
        'data-note-type':row.visualType,'data-note-direction':row.direction??'','data-note-width':row.width??1},undefined,notes);
      const bounds=timingNoteBounds(row);
      shape('rect', {...bounds, class:'plan-hit-target'}, undefined, marker);
      const cursor = bounds.x - 3;
      shape('path', {d:`M${cursor} -8v16m0 -8h5`,class:'plan-selection','aria-hidden':'true'},undefined,marker);
      drawTimingNote(marker,row,noteSkin);
      if(moved)shape('path',{d:'M0 13L2 15L0 17L-2 15Z',class:'plan-shift-mark'},undefined,marker);
      shape('title', {}, `按键 #${row.number} · ${row.kind} · ${laneLabel(row)} 轨 · ${signedFrames(row.frame-row.chartFrame)} 帧`, marker);
      noteMarkers.push(marker); updateMarker(row.index);
      marker.addEventListener('click', () => selectNote(row.index, {reveal:false}));
      marker.addEventListener('keydown', event => {
        if (['ArrowLeft','ArrowRight','Home','End'].includes(event.key)) {
          event.preventDefault(); selectNote(event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, row.index + (event.key === 'ArrowRight' ? 1 : -1))), {focus:true});
        } else if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); selectNote(row.index, {focus:true, reveal:false}); }
      });
    }
    updateMarker(view().selected);
    stopChartDrag();
    pendingScroll = view().scroll; chart.replaceChildren(svg); updateViewport();
  }
  function renderOverview() {
    const svg = svgShape('svg', {viewBox:'0 0 1000 28', preserveAspectRatio:'none', 'aria-hidden':'true'}), events = displayed().skillEvents ?? [];
    const x = frame => frame / chartEnd * 1000;
    for (const start of events.filter(event => event.kind === 'start')) {
      const end = events.find(event => event.kind === 'end' && event.index === start.index);
      svgShape('rect', {x:x(start.firstInputFrame), y:3, width:Math.max(1, x(end?.lastInputFrame ?? chartEnd) - x(start.firstInputFrame)), height:22, class:'overview-skill'}, undefined, svg);
    }
    for (const row of rows) {
      if (row.frame !== row.chartFrame) svgShape('line', {x1:x(row.frame), x2:x(row.frame), y1:10, y2:22, class:'overview-shift'}, undefined, svg);
    }
    for (const range of current().ranges) {
      const frame = rows[range.noteIndex].chartFrame;
      svgShape('circle', {cx:x(frame), cy:6, r:2, class:'overview-limit'}, undefined, svg);
    }
    overviewArt.replaceChildren(svg); updateViewport();
  }
  function updateViewport() {
    if (!chart.clientWidth) return;
    if (pendingScroll !== null) { chart.scrollLeft = pendingScroll; pendingScroll = null; }
    view().scroll = chart.scrollLeft; updateSkillLabels();
    const maximum = Math.max(0, chart.scrollWidth - chart.clientWidth);
    chart.dataset.canPan = String(maximum > 0);
    scrubber.max = maximum; scrubber.value = chart.scrollLeft; scrubber.disabled = !maximum;
    const first = Math.max(0, (chart.scrollLeft - 30) / zoomPixels), last = Math.min(chartEnd / 120, (chart.scrollLeft + chart.clientWidth - 30) / zoomPixels);
    viewportLabel.textContent = `${time(first)}–${time(Math.max(first, last))} / ${time(chartEnd / 120)}`;
    scrubber.setAttribute('aria-valuetext', `可见 ${time(first)} 至 ${time(Math.max(first, last))}，全曲 ${time(chartEnd / 120)}`);
    const fraction = chart.scrollWidth ? Math.min(1, chart.clientWidth / chart.scrollWidth) : 1;
    const width = overviewTrack.clientWidth;
    overviewWindowWidth = Math.min(width, Math.max(16, width * fraction));
    overviewTrack.style.setProperty('--viewport-width', `${overviewWindowWidth}px`);
  }
  function updateSkillLabels() {
    // Keep the active band's title legible when its start scrolls out of view.
    for (const label of chart.querySelectorAll('.plan-skill-label')) {
      const left = Math.max(Number(label.dataset.start) + 7, chart.scrollLeft + 7);
      label.setAttribute('x', left); label.style.visibility = left + 45 <= Number(label.dataset.end) ? 'visible' : 'hidden';
    }
  }
  function renderDetails() {
    if (!records.open) return;
    const ranges = new Map(current().ranges.map(r => [r.noteIndex, r]));
    const table = el('table'), head = el('thead'), tr = el('tr');
    for (const name of ['按键','谱面时间','偏移 / 帧','范围 / 帧','判定','得分']) { const th = el('th', '', name); th.scope = 'col'; tr.append(th); }
    head.append(tr); table.append(head); const body = el('tbody');
    for (const row of rows) {
      const line = el('tr'), first = el('td'), range = ranges.get(row.index) ?? row.defaultRange;
      line.dataset.noteIndex = row.index;
      first.append(button(`#${row.number}`, () => locateFromList(row.index), 'ui-button ui-button--text')); line.append(first);
      for (const text of [time(row.time), signedFrames(row.frame - row.chartFrame), `${signedFrames(range.firstOffset)}～${signedFrames(range.lastOffset)}`, judgementLabel(row), row.hit ? integer(row.hit.score) : '—']) line.append(el('td', '', text));
      body.append(line);
    }
    table.append(body); tableBox.replaceChildren(table); syncListSelection();
  }
  function renderLists() {
    const ranges=new Map(current().ranges.map(r=>[r.noteIndex,r]));
    const selected = rows.filter(row => ranges.has(row.index));
    limitCount.textContent=`${integer(selected.length)} 个`;
    const scroll=limitBox.scrollTop;
    if (!selected.length) {
      limitBox.replaceChildren(el('p', 'ui-text ui-text--secondary timing-empty', '在按键编辑中设置可击打范围。'));
      return;
    }
    const table=el('table'),head=el('thead'),labels=el('tr'),body=el('tbody');
    table.setAttribute('aria-label', '本曲已设限制');
    for (const title of ['按键','范围 / 帧','操作']) {
      const th=el('th','',title);th.scope='col';labels.append(th);
    }
    head.append(labels);table.append(head,body);
    for (const row of selected) {
      const tr=el('tr'),key=el('td');tr.dataset.noteIndex=row.index;
      const locate=button(`#${row.number}`,()=>locateFromList(row.index),'ui-button ui-button--text');locate.setAttribute('aria-label',`定位按键 #${row.number}`);
      key.append(locate, el('span','timing-list-lane',`${laneLabel(row)} 轨`));tr.append(key);
      const range=ranges.get(row.index),action=el('td'),remove=button('移除',()=>change(()=>current().removeRange(row.index)),'ui-button ui-button--text');
      remove.disabled=busy||initializing;remove.setAttribute('aria-label',`移除按键 #${row.number} 的限制`);
      action.append(remove);tr.append(el('td','timing-range-value',range.firstOffset === 0 && range.lastOffset === 0 ? '仅原始帧' : `${signedFrames(range.firstOffset)} ～ ${signedFrames(range.lastOffset)}`),action);
      body.append(tr);
    }
    limitBox.replaceChildren(table);limitBox.scrollTop=scroll;
    syncListSelection();
  }
  function syncListSelection(reveal = false) {
    for (const box of [limitBox, tableBox]) {
      for (const line of box.querySelectorAll('tr[data-note-index]')) {
        const selected=Number(line.dataset.noteIndex)===view().selected;
        line.dataset.selected=String(selected);
        const key=line.querySelector('button');
        if(selected)key?.setAttribute('aria-current','true');else key?.removeAttribute('aria-current');
        if (selected && reveal && box.clientHeight) {
          const bounds = line.getBoundingClientRect(), viewport = box.getBoundingClientRect(), header = box.querySelector('thead')?.getBoundingClientRect().height ?? 0;
          if (bounds.top < viewport.top + header) box.scrollTop -= viewport.top + header - bounds.top;
          else if (bounds.bottom > viewport.bottom) box.scrollTop += bounds.bottom - viewport.bottom;
        }
      }
    }
  }
  async function recompute() {
    if (busy || initializing) return; busy = true; planReady=false;failure='';refresh();
    try {
      const runtime = getRuntime?.(); if (typeof runtime?.calculate !== 'function' || !calculationPlayer) throw new Error('缺少原始档案，请从完整计算结果打开击打方案');
      const applied = await recomputeGlobalTimingPlans(drafts, request => runtime.calculate({
        player:structuredClone(calculationPlayer),server:calculationPlayer.server,eventId:source.eventId,core:getCore?.(),
        options:{hitTiming:true,hitTimingRanges:request},
      }), {
        isCurrent:() => root.isConnected && (getProfileId?.()??'')===profileId,
      });
      if (applied) {currentResult=applied;appliedKeys=drafts.map(d=>d.key);appliedPlanValid=true;planReady=true;onResult?.(applied,root);}
      busy = false;
      if (root.isConnected) { refresh(); message(applied ? '已按限制重算最优编队、技能顺序和道具。' : '范围已改变，请重新应用。'); }
    } catch (error) { busy = false;planReady=false;failure=error.message??String(error); if (root.isConnected) refresh(); }
  }
  root.addEventListener('keydown', event => {
    if ((event.ctrlKey || event.metaKey) && !event.target.matches('input,textarea,select') && event.key.toLowerCase() === 'z') {
      event.preventDefault(); change(() => event.shiftKey ? current().redo() : current().undo());
    }
  });
  refresh(); const initialGenerations = drafts.map(d => d.generation),initialKeys=drafts.map(d=>d.key);
  const resizeObserver = new ResizeObserver(() => {
    if (root.isConnected) updateViewport(); else resizeObserver.disconnect();
  });
  resizeObserver.observe(chart);
  root.dataset.noteTypes='loading';
  void Promise.allSettled(drafts.map(async (draft,i)=>{
    const {songId,difficulty}=draft.original;
    const cached=getCore?.()?.charts?.find(c=>c.songId===songId && c.difficulty===difficulty);
    const raw=cached?.data??(await getRuntime?.()?.syncChart?.(songId,difficulty))?.data;
    noteTypes[i]=timingChartNoteTypes(draft.original,raw);
  })).then(results=>{
    results.forEach((result,i)=>{if(result.status==='rejected')typeErrors[i]=result.reason?.message??String(result.reason);});
    root.dataset.noteTypes=typeErrors.some(Boolean)?'partial':'ready';
    if(root.isConnected) {
      // Metadata may arrive while a range is being typed. Update the drawing
      // and label without replacing the user's input fields or their focus.
      rows=planNoteRows(current().original,displayed(),noteTypes[songIndex]);
      typeState.textContent=typeErrors[songIndex]?'键型资料未匹配 · 基础显示':'';
      typeState.title=typeErrors[songIndex];
      const row = rows[view().selected];
      noteMeta.textContent = `${row.kind} · ${laneLabel(row)} 轨`;
      const focused=chart.contains(document.activeElement);
      renderChart();renderDetails();
      if(focused)noteMarkers[view().selected]?.focus({preventScroll:true});
    }
  });
  void (async () => {
    try {
      const saved = await loadTimingPlanRanges(await storageKey());
      if (!root.isConnected || drafts.some((d, i) => d.generation !== initialGenerations[i])) return;
      if (saved) {
        const ranges=restoreFrameRanges(saved,drafts.map(d=>d.original.input.notes),drafts.map(d=>d.original.input.nativeTimeRouting?.targets)).map((r,i)=>referenceFrameRanges(r,drafts[i].original.input.notes.length));
        ranges.forEach((r,i)=>drafts[i].setRanges(r,{record:false}));
      }
    } catch (error) { failure=`保存范围未载入：${error.message}`; }
    if (root.isConnected) {
      if (drafts.some((d,i)=>d.key!==initialKeys[i])) {initializing=false;planReady=false;refresh();}
      else {
        try {
          const runtime=getRuntime?.();
          if (typeof runtime?.optimizeHitTimingPlan==='function') {
            const applied=await recomputeTimingPlans(drafts,r=>runtime.optimizeHitTimingPlan(r),{
              isCurrent:()=>root.isConnected && (getProfileId?.()??'')===profileId,
            });
            if (applied) {
              if (drafts.some((d,i)=>d.result.total!==source.hitTiming.songs[i].result.total)) throw new Error('原始最高分与当前计算核心不一致，请重新计算');
              currentResult={...source,hitTiming:{...source.hitTiming,songs:drafts.map(d=>({
                ...d.original,input:{...d.original.input,hitFrames:d.result.hitFrames,hitOrder:d.result.hitOrder},result:d.result,
              }))}};
              onResult?.(currentResult,root);
            }
          }
        } catch(error) {failure=error.message??String(error);planReady=false;appliedPlanValid=false;}
        initializing=false;if(root.isConnected)refresh();
      }
    }
  })();
  return root;
}
