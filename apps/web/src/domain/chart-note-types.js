// Presentation metadata only. Follow TimeDistanceChart's visible beat/event/node
// order, then verify the complete geometry before attaching it to witness rows.
// Judgment windows alone cannot distinguish Single, Flick and Directional.
export function timingChartNoteTypes(witness, raw) {
  const invalid = () => { throw new Error('原谱面与击打记录不一致，未套用键型'); };
  if (!Array.isArray(raw)) invalid();
  const tempo = raw.filter(e => e.type === 'BPM').map(e => ({beat:e.beat,bpm:e.bpm,time:0})).sort((a,b)=>a.beat-b.beat);
  if (!tempo.length || tempo[0].beat !== 0) invalid();
  for (let i=0;i<tempo.length;i++) {
    const s=tempo[i], p=tempo[i-1];
    if (!Number.isFinite(s.beat) || !Number.isFinite(s.bpm) || s.bpm<=0 || (p && s.beat<=p.beat)) invalid();
    if (p) s.time=p.time+(s.beat-p.beat)*60/p.bpm;
  }
  const points=[], edges=[];
  raw.forEach((event,eventIndex)=>{
    if (['BPM','System'].includes(event.type)) return;
    if (!['Single','Directional','Long','Slide'].includes(event.type)) invalid();
    const connected=['Long','Slide'].includes(event.type), nodes=connected?event.connections:[event];
    if (!Array.isArray(nodes) || (connected && nodes.length<2)) invalid();
    let previous=null;
    nodes.forEach((node,nodeIndex)=>{
      if (node.hidden === true) return;
      if (!Number.isFinite(node.beat) || node.beat<0 || !Number.isInteger(node.lane) || node.lane<0 || node.lane>6) invalid();
      if (node.direction!=null && !['Left','Right'].includes(node.direction)) invalid();
      if (node.flick!=null && typeof node.flick!=='boolean') invalid();
      const role=nodeIndex===0?'head':nodeIndex===nodes.length-1?'end':'middle';
      const directional=['Left','Right'].includes(node.direction), flick=node.flick===true;
      const connectionKind=connected?event.type.toLowerCase():null;
      const visualType=directional?'directional':flick?'flick':connected?(role==='middle'?'slide-middle':'hold'):'tap';
      const direction=directional?node.direction:null, width=directional?node.width??1:1;
      if (directional && (!Number.isInteger(width) || width<1 || width>7)) invalid();
      const gesture=directional?`${direction==='Left'?'左':'右'}滑${width>1?` ×${width}`:''}`:flick?'滑键':'';
      const tail=directional?`定向尾滑 · ${gesture}`:flick?'尾滑':'尾';
      const kind=event.type==='Slide'?(role==='middle'?'绿条中间':role==='head'?'绿条头':`绿条${tail}`)
        :event.type==='Long'?(role==='head'?'长按头':`长按${tail}`):gesture||'单键';
      const windowKind=event.type==='Slide'?'slideTime':event.type==='Long' && role==='end'?'widenedEndpoint':'standard';
      const windowRole=event.type==='Slide'?(role==='end' && directional?'directionalEnd':role==='end' && flick?'flickEnd':role):null;
      const point={eventIndex,nodeIndex,beat:node.beat,lane:node.lane,skill:node.skill===true,windowKind,windowRole,
        display:{visualType,kind,connectionKind,direction,width}};
      points.push(point);if(previous)edges.push([previous,point]);previous=connected?point:null;
    });
  });
  points.sort((a,b)=>a.beat-b.beat || a.eventIndex-b.eventIndex || a.nodeIndex-b.nodeIndex);
  const input=witness?.input;
  if (points.length!==input?.notes?.length || points.length!==input.nativeTimeRouting?.targets?.length) invalid();
  let segment=0,trigger=0;
  points.forEach((point,index)=>{
    point.index=index;
    while(segment+1<tempo.length && tempo[segment+1].beat<=point.beat)segment++;
    const s=tempo[segment],seconds=s.time+(point.beat-s.beat)*60/s.bpm,note=input.notes[index],window=note.window??{kind:'standard'};
    if (Math.abs(note.time-seconds)>1e-9 || input.nativeTimeRouting.targets[index].button!==point.lane
      || (note.trigger??null)!==(point.skill?trigger++:null) || window.kind!==point.windowKind
      || (window.role??null)!==point.windowRole) invalid();
  });
  const edgeKey=edges=>edges.map(([a,b])=>`${a}:${b}`).sort().join(',');
  if(edgeKey(edges.map(([a,b])=>[a.index,b.index]))!==edgeKey((input.constraints??[]).map(e=>[e.before,e.after])))invalid();
  return points.map(p=>p.display);
}
