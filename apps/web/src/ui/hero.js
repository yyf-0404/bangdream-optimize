import { assetBaseUrl, cardArtUrls } from '../assets/index.js';
import { catalogModels } from './cards/model.js';
import { resolveCardCover } from './cards/rules.js';
import { profilePreference, gameText } from './preferences.js';
import { openCoverPicker } from './cover-picker.js';

const imageObjectURLs=new WeakMap();
function releaseImage(image){
  const url=image&&imageObjectURLs.get(image);
  if(url){URL.revokeObjectURL(url);imageObjectURLs.delete(image);}
}
async function loadImage(sources,readAsset){
  for(const src of sources){
    let objectURL,assetError;
    if(readAsset){
      try{
        const url=new URL(src,location.href);
        if(url.origin==='https://bestdori.com'){
          const blob=await readAsset(url.pathname.slice(1));objectURL=URL.createObjectURL(blob);
        }
      }catch(error){assetError=error.message||String(error);}
    }
    for(const cors of [true,false])try{
      const image=await new Promise((resolve,reject)=>{
        const img=new Image(),timer=setTimeout(()=>{img.src='';reject(new Error('Image timeout'));},10000);
        if(cors)img.crossOrigin='anonymous';img.decoding='async';
        img.onload=()=>{clearTimeout(timer);resolve(img);};img.onerror=()=>{clearTimeout(timer);reject(new Error('Image unavailable'));};
        img.src=objectURL||src;
      });
      image.dataset.source=src;
      image.dataset.pixelReadable=String(cors||!!objectURL);
      if(assetError)image.dataset.assetError=assetError;
      // The foreground is cloned for its shadow after loading. Keep the blob
      // alive until the whole event scene is replaced, including that clone.
      if(objectURL)imageObjectURLs.set(image,objectURL);
      return image;
    }catch{}
    if(objectURL)URL.revokeObjectURL(objectURL);
  }
  return null;
}
async function texture(image,seed,signal){
  const key=new URL(`../../assets/headers/quilt?recipe=1&source=${encodeURIComponent(image.dataset.source||image.src)}`,import.meta.url).href;
  let cache;try{cache=await caches.open('bangdream-optimize-hero-v1');const hit=await cache.match(key);if(hit)return hit.blob();}catch{}
 if(signal.aborted)return null;
  if(image.dataset.pixelReadable==='false')throw new Error(image.dataset.assetError||'Header asset is display-only; the same-origin image channel is unavailable');
  const canvas=document.createElement('canvas');const scale=Math.max(744/image.naturalWidth,296/image.naturalHeight);canvas.width=744;canvas.height=Math.round(image.naturalHeight*scale);
  const ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,(744-image.naturalWidth*scale)/2,0,image.naturalWidth*scale,canvas.height);
  const data=ctx.getImageData(0,0,canvas.width,canvas.height);
  const blob=await new Promise((resolve,reject)=>{
    const worker=new Worker(new URL('./texture-worker.js',import.meta.url));
    const finish=(value,error)=>{clearTimeout(timer);worker.terminate();signal.removeEventListener('abort',abort);error?reject(error):resolve(value);};
    const abort=()=>finish(null);const timer=setTimeout(()=>finish(null),15000);
    signal.addEventListener('abort',abort,{once:true});
    worker.onmessage=({data})=>finish(data.blob,data.error?new Error(data.error):null);worker.onerror=()=>finish(null);
    worker.postMessage({pixels:data.data.buffer,width:canvas.width,height:canvas.height,seed},[data.data.buffer]);
  });
  if(blob&&!signal.aborted&&cache)try{await cache.put(key,new Response(blob,{headers:{'Content-Type':'image/png','X-Asset-Bytes':String(blob.size)}}));const keys=await cache.keys();let bytes=0;for(let i=keys.length-1;i>=0;i--){const item=await cache.match(keys[i]);bytes+=Number(item.headers.get('X-Asset-Bytes')||0);if(keys.length-i>8||bytes>8*1024*1024)await cache.delete(keys[i]);}}catch{}
  return blob;
}
function fitTrim(image){
  const width=image.naturalWidth,height=image.naturalHeight;let bounds=[0,0,width,height];
  try{const canvas=document.createElement('canvas');canvas.width=width;canvas.height=height;const ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);const p=ctx.getImageData(0,0,width,height).data;let l=width,t=height,r=0,b=0;for(let y=0;y<height;y++)for(let x=0;x<width;x++)if(p[(y*width+x)*4+3]>4){l=Math.min(l,x);t=Math.min(t,y);r=Math.max(r,x+1);b=Math.max(b,y+1);}if(b>t)bounds=[l,t,r,b];}catch{}
 const h=bounds[3]-bounds[1];image.style.height=`${height/h*100}%`;image.style.top=`${-bounds[1]/h*100}%`;image.style.transform=`translateX(${(width-bounds[2])/width*100}%)`;
 image.dataset.contentBounds=bounds.join(',');image.dataset.sourceSize=[width,height].join(',');
}
export function createHeroes({getCore,getPlayer,getProfileId,readAsset}){
  const activity=document.querySelector('[data-hero=activity]'),cards=document.querySelector('[data-hero=cards]');
  let eventKey='',cardKey='',sequence=0,aborter,objectURL,activityImages=[];
  for(const [page,file] of Object.entries({player:'bg00034',result:'bg00026',archive:'bg00039'})){
    const host=document.querySelector(`[data-hero=${page}] .hero-visual`),scene=document.createElement('div');scene.className='hero-cover-scene';const image=document.createElement('img');image.alt='';image.src=new URL(`../../assets/headers/${file}.png`,import.meta.url).href;image.onerror=()=>scene.remove();scene.append(image);host.append(scene);
  }
  const coverButton=document.createElement('button');coverButton.type='button';coverButton.className='hero-cover-choice';coverButton.textContent='封面';coverButton.setAttribute('aria-label','设置卡牌封面');cards.append(coverButton);
  coverButton.onclick=()=>openCoverPicker({host:document.querySelector('#card-library-unified'),getCore,getPlayer,getProfileId,onApply:update,opener:coverButton});
  async function update(){
    const player=getPlayer(),core=getCore();if(!core)return;
    const selected=resolveCardCover(catalogModels(core,player,getProfileId()),player.server,profilePreference(getProfileId(),'cover',{}));
    const nextCardKey=`${getProfileId()}:${selected.card?.id}:${selected.variant}`;
    if(nextCardKey!==cardKey){cardKey=nextCardKey;const scene=document.createElement('div');scene.className='hero-cover-scene';const frame=document.createElement('div');frame.className='hero-cover-frame';cards.querySelector('.hero-visual').replaceChildren(scene,frame);if(selected.card){const image=await loadImage(cardArtUrls({card:selected.card.record,illustTrainingStatus:selected.variant!=='normal'}));if(nextCardKey===cardKey&&image){image.alt='';scene.append(image);}}}
    const event=player.eventPresets?.[player.currentEvent]||core.events?.[player.currentEvent];
    const nextEventKey=`${player.server}:${player.currentEvent}:${event?.assetBundleName}`;
    if(nextEventKey===eventKey)return;eventKey=nextEventKey;const token=++sequence;
    aborter?.abort();aborter=new AbortController();if(objectURL){URL.revokeObjectURL(objectURL);objectURL=null;}
    activityImages.forEach(releaseImage);activityImages=[];
    const visual=activity.querySelector('.hero-visual');visual.replaceChildren();activity.querySelector('.hero-event-logo')?.remove();
    const context=activity.querySelector('.hero-copy>p');context.hidden=false;context.textContent=player.currentEvent?`${{cn:'国服',jp:'日服',en:'国际服',tw:'台服',kr:'韩服'}[player.server]} · #${player.currentEvent} · ${gameText(event?.eventName,'')}`:'自定义活动';
    document.querySelector('.event-logo-slot')?.replaceChildren();
    if(!player.currentEvent||!event?.assetBundleName)return;
    const stage=document.createElement('div');stage.className='hero-event-stage';stage.dataset.textureState='loading';stage.innerHTML='<div class="hero-event-backdrop"></div><div class="hero-event-light"></div><div class="hero-event-mist"></div>';visual.append(stage);
    const resize=new ResizeObserver(()=>stage.style.setProperty('--scene-scale',String(visual.clientHeight/320)));resize.observe(visual);aborter.signal.addEventListener('abort',()=>resize.disconnect(),{once:true});
    const base=assetBaseUrl(),bundle=encodeURIComponent(event.assetBundleName),servers=[...new Set([player.server,'jp','cn','en','tw','kr'])];
    const layer=suffix=>servers.map(s=>`${base}/${s}/event/${bundle}/${suffix}`);
    const member=(event.members||[]).map(m=>core.cards?.[m.situationId]).filter(Boolean).sort((a,b)=>b.rarity-a.rarity)[0];
    const [back,front,logo]=await Promise.all([loadImage([...(member?cardArtUrls({card:member,illustTrainingStatus:false}):[]),...layer('topscreen_rip/bg_eventtop.png')],readAsset),loadImage(layer('topscreen_rip/trim_eventtop.png'),readAsset),loadImage(layer('images_rip/logo.png'))]);
    if(token!==sequence){[back,front,logo].forEach(releaseImage);return;}
    activityImages=[back,front,logo];
    if(front){fitTrim(front);front.alt='';front.className='hero-event-foreground';const shadow=front.cloneNode();shadow.className='hero-event-shadow';shadow.style.transform+=' translate(12px, 10px)';stage.append(shadow,front);}
    if(logo){logo.className='hero-event-logo';logo.alt=gameText(event.eventName,'活动')+' Logo';document.querySelector('.event-logo-slot')?.append(logo);}
    if(back){
      back.className='hero-event-background';back.alt='';stage.querySelector('.hero-event-backdrop').append(back);
      stage.dataset.textureState='generating';
      try{
        const blob=await texture(back,Number(player.currentEvent),aborter.signal);
        if(blob&&token===sequence){
          const quiltURL=URL.createObjectURL(blob),quilt=new Image();quilt.src=quiltURL;quilt.className='hero-event-quilt';quilt.alt='';
          try{await quilt.decode();}catch(error){URL.revokeObjectURL(quiltURL);throw error;}
          if(token!==sequence){URL.revokeObjectURL(quiltURL);return;}
          objectURL=quiltURL;stage.querySelector('.hero-event-backdrop').replaceChildren(quilt);stage.dataset.textureState='ready';
        }else if(token===sequence){stage.dataset.textureState='fallback';stage.dataset.textureError='Texture preparation timed out or worker unavailable';}
      }catch(error){stage.dataset.textureState='fallback';stage.dataset.textureError=error.message||String(error);}
    }else{stage.dataset.textureState='unavailable';}
  }
  return {update};
}
