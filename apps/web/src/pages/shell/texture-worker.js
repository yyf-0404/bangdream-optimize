/* Lightweight, deterministic texture quilting for the external design preview.
   No model or library. Sampling is statistical, not semantic recognition. */
'use strict';
const W=2880,H=272,CORE=720,START=W-CORE,P=24,O=8,STEP=P-O;
const clamp=(v,a,b)=>Math.max(a,Math.min(b,v));
const smooth=v=>{v=clamp(v,0,1);return v*v*(3-2*v);};
function canvas(w,h){return new OffscreenCanvas(w,h);}
function blurred(input,radius){
 const out=canvas(input.width,input.height),ctx=out.getContext('2d');
 // Overscan avoids transparent fringes at the shared top and bottom edge.
 const pad=canvas(input.width+radius*6,input.height+radius*6),p=pad.getContext('2d'),r=radius*3;
 p.drawImage(input,r,r);p.drawImage(input,0,0,input.width,1,r,0,input.width,r);
 p.drawImage(input,0,input.height-1,input.width,1,r,r+input.height,input.width,r);
 p.drawImage(pad,r,0,1,pad.height,0,0,r,pad.height);
 p.drawImage(pad,r+input.width-1,0,1,pad.height,r+input.width,0,r,pad.height);
 ctx.filter=`blur(${radius}px)`;ctx.drawImage(pad,-r,-r);return out;
}
function pixels(c){return c.getContext('2d',{willReadFrequently:true}).getImageData(0,0,c.width,c.height).data;}
function pickBank(src,sw,sh){
 const candidates=[];
 for(let y=0;y<=sh-P;y+=12)for(let x=0;x<=sw-P;x+=12){
  if(x>sw*.18&&x<sw*.82-P&&y>sh*.2&&y<sh*.9-P)continue;
  let sum=0,sq=0,edge=0,alpha=0,n=0;
  for(let j=0;j<P;j+=3)for(let i=0;i<P;i+=3){
   const k=((y+j)*sw+x+i)*4;
   const v=(src[k]+src[k+1]+src[k+2])/3;
   sum+=v;sq+=v*v;alpha+=src[k+3];n++;
   if(i+3<P)edge+=Math.abs(v-(src[k+12]+src[k+13]+src[k+14])/3);
  }
  if(alpha/n<250)continue;
  const mean=sum/n,variance=Math.max(0,sq/n-mean*mean);
  // Penalize strong outlines and near-white/black empty borders.
  const score=variance*.13+edge/n*3+(mean>246||mean<9?120:0);
  candidates.push({x,y,score});
 }
 candidates.sort((a,b)=>a.score-b.score);
 const chosen=[];
 for(const item of candidates){
  if(chosen.some(q=>Math.abs(q.x-item.x)<P&&Math.abs(q.y-item.y)<P))continue;
  chosen.push(item);if(chosen.length===24)break;
 }
 if(!chosen.length)throw new Error('No opaque texture patches');
 const bank=chosen.map(({x,y})=>{
  const patch=new Uint8ClampedArray(P*P*4);
  for(let row=0;row<P;row++)patch.set(src.subarray(((y+row)*sw+x)*4,((y+row)*sw+x+P)*4),row*P*4);
  return patch;
 });
 return {bank,chosen};
}
function cut(cost,rows,cols){
 const scores=new Float32Array(cost),parents=new Int16Array(rows*cols),path=new Int16Array(rows);
 for(let y=1;y<rows;y++)for(let x=0;x<cols;x++){
  let best=x;
  for(let k=Math.max(0,x-1);k<=Math.min(cols-1,x+1);k++)if(scores[(y-1)*cols+k]<scores[(y-1)*cols+best])best=k;
  parents[y*cols+x]=best;scores[y*cols+x]+=scores[(y-1)*cols+best];
 }
 let best=0;for(let x=1;x<cols;x++)if(scores[(rows-1)*cols+x]<scores[(rows-1)*cols+best])best=x;
 path[rows-1]=best;for(let y=rows-1;y>0;y--)path[y-1]=parents[y*cols+path[y]];
 return path;
}
function synthesize(src,sw,sh,seed){
 const started=performance.now(),y0=Math.round((sh-296)*.42)+12;
 const coreCanvas=canvas(CORE,H),coreCtx=coreCanvas.getContext('2d');
 const core=new Uint8ClampedArray(CORE*H*4);
 for(let y=0;y<H;y++)core.set(src.subarray(((y+y0)*sw+12)*4,((y+y0)*sw+12+CORE)*4),y*CORE*4);
 coreCtx.putImageData(new ImageData(core,CORE,H),0,0);
 const {bank,chosen}=pickBank(src,sw,sh),out=new Uint8ClampedArray(W*H*4),uses=new Uint16Array(bank.length);
 for(let y=0;y<H;y++)out.set(core.subarray(y*CORE*4,(y+1)*CORE*4),(y*W+START)*4);
 let rng=seed>>>0,last=-1;
 const random=()=>{rng=(Math.imul(rng,1664525)+1013904223)>>>0;return rng/4294967296;};
 const error=(patch,x,y,px,py)=>{
  const a=(py*P+px)*4,b=((y+py)*W+x+px)*4;
  return ((patch[a]-out[b])**2+(patch[a+1]-out[b+1])**2+(patch[a+2]-out[b+2])**2)/3;
 };
 for(let x=START-STEP;x>=0;x-=STEP)for(let y=0;y<H;y+=STEP){
  const ph=Math.min(P,H-y),errs=[];
  for(let b=0;b<bank.length;b++){
   let total=0,n=0;
   for(let py=0;py<ph;py+=3)for(let px=0;px<P;px+=3)if(px>=STEP||(y>0&&py<O)){total+=error(bank[b],x,y,px,py);n++;}
   errs.push(total/Math.max(1,n)+uses[b]*1.8+(b===last?80:0));
  }
  const best=Math.min(...errs),options=[];
  errs.forEach((v,i)=>{if(v<=best*1.08+8)options.push(i);});
  const b=options[Math.floor(random()*options.length)],patch=bank[b];uses[b]++;last=b;
  const rightCost=new Float32Array(ph*O),topCost=new Float32Array(P*O);
  for(let py=0;py<ph;py++)for(let k=0;k<O;k++)rightCost[py*O+k]=error(patch,x,y,STEP+k,py);
  const right=cut(rightCost,ph,O);let top;
  if(y>0){for(let px=0;px<P;px++)for(let k=0;k<O;k++)topCost[px*O+k]=error(patch,x,y,px,k);top=cut(topCost,P,O);}
  for(let py=0;py<ph;py++)for(let px=0;px<P;px++){
   const a=(py*P+px)*4,d=((y+py)*W+x+px)*4;
   let mix=px<STEP?1:clamp((STEP+right[py]-px+1)/2,0,1);
   if(top&&py<O)mix=Math.min(mix,clamp((py-top[px]+1)/2,0,1));
   if(!out[d+3])mix=1;
   for(let c=0;c<3;c++)out[d+c]=patch[a+c]*mix+out[d+c]*(1-mix);
   out[d+3]=255;
  }
 }
 const quiltCanvas=canvas(W,H);quiltCanvas.getContext('2d').putImageData(new ImageData(out,W,H),0,0);
 const soft=pixels(blurred(quiltCanvas,12)),light=pixels(blurred(coreCanvas,32));
 const mean=[0,0,0];let count=0;
 for(const patch of bank)for(let k=0;k<patch.length;k+=16){for(let c=0;c<3;c++)mean[c]+=patch[k+c];count++;}
 for(let c=0;c<3;c++)mean[c]/=count;
 const rowLight=new Float32Array(H*3);
 for(let y=0;y<H;y++)for(let c=0;c<3;c++){
  for(let x=0;x<48;x++)rowLight[y*3+c]+=light[(y*CORE+x)*4+c]/48;
 }
 // Match low-frequency illumination separately; no horizontal stretching of detail.
 for(let y=0;y<H;y++)for(let x=0;x<W;x++){
  const k=(y*W+x)*4,d=.7*smooth((START-x)/1200);
  for(let c=0;c<3;c++)out[k+c]=rowLight[y*3+c]*(1-d)+mean[c]*d+.4*clamp(out[k+c]-soft[k+c],-32,32);
 }
 quiltCanvas.getContext('2d').putImageData(new ImageData(out,W,H),0,0);
 // Only blend the 256px seam. Three frequency bands avoid processing a full pyramid.
 const SX=START-64,SW=256,a=canvas(SW,H),b=canvas(SW,H),ac=a.getContext('2d');
 ac.save();ac.translate(64,0);ac.scale(-1,1);ac.drawImage(coreCanvas,0,0);ac.restore();ac.drawImage(coreCanvas,64,0);
 b.getContext('2d').drawImage(quiltCanvas,-SX,0);
 const aa=pixels(a),bb=pixels(b),a8=pixels(blurred(a,8)),b8=pixels(blurred(b,8)),a24=pixels(blurred(a,24)),b24=pixels(blurred(b,24));
 for(let y=0;y<H;y++)for(let x=0;x<SW;x++){
  const i=(y*SW+x)*4,k=(y*W+SX+x)*4,m0=smooth((x-64)/96),m1=smooth((x-32)/144),m2=smooth(x/224);
  for(let c=0;c<3;c++)out[k+c]=(aa[i+c]-a8[i+c])*m0+(bb[i+c]-b8[i+c])*(1-m0)+(a8[i+c]-a24[i+c])*m1+(b8[i+c]-b24[i+c])*(1-m1)+a24[i+c]*m2+b24[i+c]*(1-m2);
 }
 for(let y=0;y<H;y++)out.set(core.subarray((y*CORE+160)*4,(y+1)*CORE*4),(y*W+START+160)*4);
 quiltCanvas.getContext('2d').putImageData(new ImageData(out,W,H),0,0);
 return {canvas:quiltCanvas,meta:{milliseconds:Math.round(performance.now()-started),patches:bank.length,sampling:chosen.map(({x,y})=>[x,y,P,P]),width:W,height:H,protectedOffset:160}};
}
self.onmessage=async({data})=>{
 try{
  const result=synthesize(new Uint8ClampedArray(data.pixels),data.width,data.height,data.seed);
  const blob=await result.canvas.convertToBlob({type:'image/png'});
  self.postMessage({blob,meta:{...result.meta,bytes:blob.size}});
 }catch(error){self.postMessage({error:String(error.message||error)});}
};
