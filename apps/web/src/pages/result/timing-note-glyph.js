// Both renderers use the same note anchor. Rotate the game's vertical chart
// clockwise: lane Left -> up, lane Right -> down, and future time -> right.
const skinUrl=name=>{const url=new URL(`./note-skins/default/${name}.png`,import.meta.url);url.search=new URL(import.meta.url).search;return url.href;};
const textures=Object.fromEntries(['tap','flick','flick_arrow','slide','sep','left','right','left_arrow','right_arrow','tap_skill','long'].map(name=>[name,skinUrl(name)]));
// SVG source coordinates: x runs across lanes before the clockwise rotation.
export const TIMING_LANE_STEP=34;
// CN client: lane spacing 2.2 world units, sprites 100 pixels/unit. The 308 px
// body rect includes transparent margins; it is NOT one lane wide. Keep the
// client's lane-axis scale and the icon animation's first pose (x = +/-1.6).
// Only the time-axis thickness is widened for this static editor.
const notePixelScale=TIMING_LANE_STEP/220,noteDepth=20;
const noteSpan=308*notePixelScale,directionDepth=noteDepth,arrowSpan=138*notePixelScale;
const flickArrowSpan=171*notePixelScale,flickArrowDepth=138*noteDepth/120;
const flickArrowOffset=70*noteDepth/120; // FlickNoteIcon animation first pose: y = +0.7.
function directionalLayout(row) {
  if(row.visualType!=='directional' || !['Left','Right'].includes(row.direction))return null;
  const sign=row.direction==='Left'?-1:1,count=Math.max(1,Math.min(7,row.width??1));
  const centers=Array.from({length:count},(_,i)=>i*TIMING_LANE_STEP*sign);
  const tip=centers.at(-1)+sign*160*notePixelScale;
  const arrowDepth=(row.direction==='Left'?171:170)*directionDepth/120;
  return {sign,centers,tip,arrowDepth};
}
export function timingNoteBounds(row) {
  const layout=directionalLayout(row);
  if(!layout) {
    const right=row.visualType==='flick'?Math.max(18,flickArrowOffset+flickArrowDepth/2+3):18;
    return {x:-18,y:-noteSpan/2-3,width:18+right,height:noteSpan+6};
  }
  const low=Math.min(-18,...layout.centers.map(x=>x-noteSpan/2-3),layout.tip-arrowSpan/2-3);
  const high=Math.max(18,...layout.centers.map(x=>x+noteSpan/2+3),layout.tip+arrowSpan/2+3);
  return {x:-18,y:low,width:36,height:high-low};
}
function shape(parent, tag, attrs) {
  const node=document.createElementNS('http://www.w3.org/2000/svg',tag);
  for(const [key,value] of Object.entries(attrs))node.setAttribute(key,value);
  parent.append(node);return node;
}

function drawVector(art,row) {
  const type=row.visualType??'unknown',direction=row.direction;
  if(type==='directional') {
    const layout=directionalLayout(row);
    if(layout) {
      const {sign,centers,tip,arrowDepth}=layout;
      for(const [index,x] of centers.entries()) {
        const part=shape(art,'g',{transform:`translate(${x} 0)`,class:'glyph-direction-body','data-direction-part':index});
        // Match the visible portion of the client tile, without its margins.
        const halfSpan=106*notePixelScale;
        shape(part,'path',{d:`M${-halfSpan*sign} -10 L${halfSpan*sign} 0 L${-halfSpan*sign} 10 Z`,class:'glyph-body'});
        shape(part,'path',{d:`M${-6*sign} -5 L${5*sign} 0 L${-6*sign} 5`,class:'glyph-arrow'});
      }
      const end=shape(art,'g',{transform:`translate(${tip} 0)`,class:'glyph-direction-tip'});
      shape(end,'path',{d:`M${-arrowSpan/2*sign} ${-arrowDepth/2} L${arrowSpan/2*sign} 0 L${-arrowSpan/2*sign} ${arrowDepth/2}`,class:'glyph-direction-repeat'});
    } else {
      // Historical witnesses know this is directional but not which direction.
      shape(art,'path',{d:'M-9 0L0-7L9 0L0 7Z',class:'glyph-body'});
    }
  } else if(type==='slide-middle') {
    const body=shape(art,'g',{transform:`scale(${noteSpan/26} ${noteDepth/14})`});
    shape(body,'rect',{x:-8,y:-2.5,width:16,height:5,rx:2.5,class:'glyph-body'});
    shape(body,'path',{d:'M-5 0H5',class:'glyph-shine'});
  } else {
    const body=shape(art,'g',{transform:`scale(${noteSpan/26} ${noteDepth/14})`});
    shape(body,'ellipse',{cx:0,cy:0,rx:9,ry:6,class:'glyph-body'});
    shape(body,'ellipse',{cx:0,cy:-1,rx:5.5,ry:3.5,class:'glyph-center'});
    shape(body,'rect',{x:-11,y:-2,width:22,height:4,rx:2,class:'glyph-band'});
    if(type==='flick')shape(body,'path',{d:'M-5-6L0-10L5-6M-5-11L0-15L5-11',class:'glyph-flick-arrows'});
  }
}

function drawOriginal(art,row) {
  const type=row.visualType??'unknown',direction=row.direction;
  const image=(name,x,y,width,height)=>shape(art,'image',{href:textures[name],x,y,width,height,preserveAspectRatio:'none','data-texture':name,class:'plan-note-texture'});
  const layout=directionalLayout(row);
  if(layout) {
    const name=direction.toLowerCase();
    // Adjacent lane bodies, with ONE icon at the gesture's leading end. The
    // client also has a BackLine renderer; this static view omits that effect
    // rather than stretching its texture into an independently sized link.
    for(const [index,x] of layout.centers.entries()) {
      const part=image(name,x-noteSpan/2,-directionDepth/2,noteSpan,directionDepth);
      part.classList.add('glyph-direction-body');part.dataset.directionPart=index;
    }
    image(`${name}_arrow`,layout.tip-arrowSpan/2,-layout.arrowDepth/2,arrowSpan,layout.arrowDepth).classList.add('glyph-direction-tip');
    return true;
  }
  let name=({tap:'tap',flick:'flick',hold:'slide','slide-middle':'sep'})[type];
  if(!name)return false;
  if(row.trigger!=null && ['tap','hold'].includes(type))name='tap_skill';
  // Export restored the client's Sprite rect and central pivot. Widen the time
  // dimension independently, keeping the strike line at zero. The ordinary
  // flick has a separate client arrow sprite; its body keeps the same anchor.
  image(name,-noteSpan/2,-noteDepth/2,noteSpan,noteDepth);
  if(type==='flick') {
    image('flick_arrow',-flickArrowSpan/2,-flickArrowOffset-flickArrowDepth/2,flickArrowSpan,flickArrowDepth);
  }
  return true;
}

export function drawTimingNote(parent,row,skin='vector') {
  const type=row.visualType??'unknown',direction=row.direction;
  const glyph=shape(parent,'g',{class:`plan-glyph glyph-${type}${direction?` direction-${direction.toLowerCase()}`:''}`,'aria-hidden':'true','data-skin':skin});
  const art=shape(glyph,'g',{class:'plan-glyph-art',transform:'rotate(90)'});
  const original=skin==='original' && drawOriginal(art,row);
  if(!original)drawVector(art,row);
  if(row.trigger!=null && !(original && ['tap','hold'].includes(type)))shape(glyph,'path',{d:'M-12-13L-10-9L-6-7L-10-5L-12-1L-14-5L-18-7L-14-9Z',class:'plan-trigger'});
  return glyph;
}

export function drawTimingConnection(parent,attrs,skin='vector',long=false) {
  const {x1,y1,x2,y2}=attrs,dx=x2-x1,dy=y2-y1;
  if(dx===0) {
    // Same-frame ordered nodes have no filled time span. Keep their connection
    // visible without inventing a duration or dividing by a zero time delta.
    if(dy)shape(parent,'line',{...attrs,class:'plan-chain-instant'});
    return;
  }
  const span=skin==='original'?20:long?10:12;
  if(skin==='original') {
    // Map each texture row across lanes; shear along time rather than rotating
    // a rectangle. Both end faces remain vertical and meet the node strike bars.
    shape(parent,'image',{href:textures.long,x:0,y:0,width:1,height:1,preserveAspectRatio:'none',
      transform:`matrix(0 ${span} ${dx} ${dy} ${x1} ${y1-span/2})`,class:'plan-chain-texture'});
  } else {
    shape(parent,'path',{d:`M${x1} ${y1-span/2}L${x2} ${y2-span/2}V${y2+span/2}L${x1} ${y1+span/2}Z`,class:`plan-chain${long?' is-long':''}`});
    shape(parent,'line',{...attrs,class:'plan-chain-shine'});
  }
}
