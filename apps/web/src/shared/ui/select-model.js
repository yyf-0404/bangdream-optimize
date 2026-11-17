const normalize=value=>String(value??'').normalize('NFKC').trim().toLocaleLowerCase();

// Search displayed labels and stable IDs; retain source order within each rank.
export function searchOptions(options,query){
 const text=normalize(query),tokens=text.split(/\s+/).filter(Boolean);
 if(!tokens.length)return options.filter(option=>!option.hidden);
 return options.filter(option=>!option.hidden&&tokens.every(token=>normalize(`${option.label} ${option.value} ${option.group||''}`).includes(token)))
  .map((option,index)=>({option,index,rank:normalize(option.value)===text.replace(/^#/, '')?0:normalize(option.label).startsWith(text)?1:2}))
  .sort((a,b)=>a.rank-b.rank||a.index-b.index).map(row=>row.option);
}

export function moveOption(options,current,offset){
 const enabled=options.map((o,i)=>o.disabled?-1:i).filter(i=>i>=0);
 if(!enabled.length)return -1;
 if(offset===Infinity)return enabled.at(-1);
 if(offset===-Infinity)return enabled[0];
 const position=enabled.indexOf(current);
 if(position<0)return offset<0?enabled.at(-1):enabled[0];
 return enabled[Math.max(0,Math.min(enabled.length-1,position+offset))];
}

export function placeSelectPopup(anchor,viewport,{preferredWidth=240,desiredHeight=360,gap=6,margin=8,side}={}){
 const leftEdge=(viewport.left||0)+margin,topEdge=(viewport.top||0)+margin;
 const rightEdge=(viewport.left||0)+viewport.width-margin,bottomEdge=(viewport.top||0)+viewport.height-margin;
 const anchorTop=Math.min(bottomEdge,Math.max(topEdge,anchor.top)),anchorBottom=Math.min(bottomEdge,Math.max(topEdge,anchor.bottom));
 const below=Math.max(0,bottomEdge-anchorBottom-gap),above=Math.max(0,anchorTop-topEdge-gap);
 const upward=side?side==='above':below<Math.min(desiredHeight,200)&&above>below;
 const maxHeight=Math.min(desiredHeight,upward?above:below);
 const width=Math.min(Math.max(anchor.width,preferredWidth),Math.max(0,rightEdge-leftEdge));
 return {width,left:Math.min(Math.max(anchor.left,leftEdge),rightEdge-width),maxHeight,top:upward?anchorTop-gap-maxHeight:anchorBottom+gap,upward};
}
