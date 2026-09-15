import {customCardsMarkup} from './template.js';
import {customSkillDurationError,stepCustomSkillDuration,customCardEntry,customCardDraft,customCardLabel,nextCustomCardId,CUSTOM_CARD_ID_BASE} from '../../models/custom-cards.js';
import {cardArtUrls,assetOriginUrl} from '../../assets/index.js';
import {gameText} from '../preferences.js';

export function createCustomCardsView({host,getCore,getPlayer,getProfileId,writePlayer,onChange}) {
const portal=document.createElement('div');portal.className='custom-card-ui';document.body.append(portal);
const dialogStart=customCardsMarkup.indexOf('<dialog');
host.classList.add('custom-card-ui');host.innerHTML=customCardsMarkup.slice(0,dialogStart);portal.innerHTML=customCardsMarkup.slice(dialogStart);
const selector=s=>s.replace(/#([a-z][\w-]*)/g,'#cc-$1'),$=s=>host.querySelector(selector(s))||portal.querySelector(selector(s)),all=s=>[...host.querySelectorAll(selector(s)),...portal.querySelectorAll(selector(s))];
const escape=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const paths={edit:'M4 20h4L20 8l-4-4L4 16zM14 6l4 4',plus:'M12 4v16M4 12h16',search:'M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0',close:'m6 6 12 12M18 6 6 18',identity:'M4 5h16v14H4zM8 9h1m-1 5h1m4-5h4m-4 5h4',power:'M4 20V9m8 11V4m8 16V12',skill:'m13 3-8 11h7l-1 7 8-11h-7z',image:'M3 4h18v16H3zM3 16l6-6 5 5 3-3 4 4M16 8h.1',growth:'M4 20h16M6 16l4-5 4 2 5-9m-5 0h5v5',copy:'M8 8h12v13H8zM5 16H3V3h12v2',delete:'M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7',chevron:'m9 6 6 6-6 6',cards:'M7 5h13v16H7zM4 17H3V3h12'};
const icon=n=>`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${paths[n]||paths.cards}"/></svg>`;
function hydrate(root=host){root.querySelectorAll('[data-icon]').forEach(n=>n.outerHTML=icon(n.dataset.icon));}
const bands = {1:"Poppin'Party", 2:'Afterglow', 3:'Hello, Happy World!', 4:'Pastel＊Palettes', 5:'Roselia', 18:'RAISE A SUILEN', 21:'Morfonica', 45:'MyGO!!!!!',50:'Ave Mujica'};
const attrs=['Powerful','Cool','Happy','Pure'], labels=['演出','技巧','形象'];
const asset=name=>assetOriginUrl()+'/res/icon/'+name;
const attrImage=a=>`<img src="${asset(a.toLowerCase()+'.svg')}" alt="${escape(a)}">`;
const rarityImage=r=>`<img class="rarity" src="${asset('star_'+r+'.png')}" alt="${r} 星">`;
const number=n=>Number.isFinite(n)?Math.round(n).toLocaleString('zh-CN'):'—';
const idText=customCardLabel;
let data={characters:[],examples:[]},cards=[],nextSequence=CUSTOM_CARD_ID_BASE+1,profileId;
const fallback=new URL('../../../assets/brand.svg',import.meta.url).href;
const artwork=c=>c.image||fallback;
const character=id=>data.characters.find(c=>c.id===Number(id))||{id:0,name:'未知角色',band:0};
function seed(){return [{enabled:true,name:'',character:data.characters[0]?.id,attribute:'Powerful',rarity:5,stats:[11000,11000,11000],mastery:0,skillType:'score',duration:7,score:130,unifiedScore:150,lowerScore:100,conditionAttribute:'none',conditionBand:0,image:'',notes:'',advanced:null}];}
let filter='all',draft=null,original='',editing=false,openedBy=null,confirmAction=null;
const editor=$('#editor'),form=$('#card-form'),f=name=>form.elements.namedItem(name);
$('#attribute-filter').innerHTML=['all',...attrs].map(a=>`<button type="button" data-filter="${a}" aria-pressed="${a==='all'}" aria-label="${a==='all'?'全部属性':a}">${a==='all'?'全部':attrImage(a)}</button>`).join('');
$('#attribute-options').innerHTML=attrs.map(a=>`<label><input type="radio" name="attribute" value="${a}" required><span>${attrImage(a)}${a}</span></label>`).join('');
$('#rarity-options').innerHTML=[5,4,3,2,1].map(r=>`<label><input type="radio" name="rarity" value="${r}"><span>${rarityImage(r)}${r}</span></label>`).join('');
f('conditionBand').innerHTML='<option value="0">不限制</option>'+Object.entries(bands).map(([id,name])=>`<option value="${id}">${escape(name)}</option>`).join('');
$('#stats-inputs').innerHTML=labels.map((label,i)=>`<label>${label}<input type="number" name="stat${i}" min="0" max="999999" step="1" required aria-label="基础${label}"></label>`).join('');
hydrate();hydrate(portal);

function baseStats(c){if(!c.advanced)return c.stats;const a=c.advanced,row=a.levels.find(r=>r.level===a.current);return (row?.stats||[NaN,NaN,NaN]).map((value,i)=>value+a.bonuses.reduce((sum,b)=>sum+(b.enabled?b.stats[i]:0),0));}
function power(c){const values=baseStats(c).map(v=>v+c.rarity*c.mastery*50);return {values,total:values.reduce((a,b)=>a+b,0)};}
const skillNames={score:'分数提升',perfect:'PERFECT 条件 · P',great:'GREAT 以下降档 · G',unified:'队伍条件加分',rateup:'PERFECT 递增'};
function skill(c){const duration=Number.isFinite(c.duration)?c.duration:'—',value=c.skillType==='unified'?(c.conditionAttribute==='none'&&!Number(c.conditionBand)?`${c.unifiedScore}%`:`${c.score} → ${c.unifiedScore}%`):c.skillType==='rateup'?`${c.score} + 0.5×P`:c.score+'%';let description=`${duration} 秒内，得分提升 ${c.score}%。`;
 if(c.skillType==='perfect')description=`${duration} 秒内，PERFECT 时得分提升 ${c.score}%。`;
 if(c.skillType==='great')description=`${duration} 秒内得分提升 ${c.score}%，出现 GREAT 以下判定后降为 ${c.lowerScore}%。`;
 if(c.skillType==='unified'){const condition=[c.conditionAttribute!=='none'?c.conditionAttribute:'',Number(c.conditionBand)>0?bands[c.conditionBand]:''].filter(Boolean).join(' 与 ');description=condition?`${duration} 秒内得分提升 ${c.score}%；全队满足 ${condition} 时提升至 ${c.unifiedScore}%。`:`${duration} 秒内得分提升 ${c.unifiedScore}%；属性与乐队均不限制，直接使用触发后加成。`;}
 if(c.skillType==='rateup')description=`${duration} 秒内，得分从 ${c.score}% 起，每个 PERFECT 再提升 0.5%，上限 ${c.score+50}%。`;
 return {value,description,duration,name:skillNames[c.skillType]};
}
function toast(message){const node=$('#toast');node.textContent=message;node.hidden=false;clearTimeout(toast.timer);toast.timer=setTimeout(()=>node.hidden=true,3300);}
function persist(next){
 try {
  if(profileId!==getProfileId())throw new Error('档案已切换，请重新打开编辑器');
  const player=structuredClone(getPlayer()),customCards={};
  for(const card of next)customCards[card.id]=customCardEntry(card,getCore(),{id:card.id,uid:card.uid});
  player.customCards=customCards;
  player.nextCustomCardId=Math.max(nextSequence,CUSTOM_CARD_ID_BASE+1,...next.map(c=>c.id+1));
  for(const card of Object.values(customCards))if(card.enabled)player.characterBouns[card.definition.characterId]??={potential:{performance:0,technique:0,visual:0},characterTask:{performance:0,technique:0,visual:0}};
  writePlayer(player);cards=Object.values(customCards).map(customCardDraft);nextSequence=player.nextCustomCardId;render();onChange?.();return true;
 }catch(error){const message=error.message||'保存失败，请重试。';$('#form-status').textContent=message;toast(message);return false;}
}
function render(){const term=$('#search').value.trim().toLowerCase();let shown=cards.filter(c=>(filter==='all'||c.attribute===filter)&&`${c.name} ${character(c.character).name} ${idText(c.id)}`.toLowerCase().includes(term));const by=$('#sort').value;shown.sort((a,b)=>by==='power'?power(b).total-power(a).total:by==='rarity'?b.rarity-a.rarity||b.updated-a.updated:b.updated-a.updated);
 $('#list-count').textContent=`${shown.length} 张自定义卡牌 · 已启用 ${shown.filter(c=>c.enabled).length}${shown.length!==cards.length?' / 共 '+cards.length+' 张':''}`;
 $('#custom-list').innerHTML=shown.map(c=>{const p=power(c),s=skill(c);return `<article class="custom-row" data-card="${c.id}" data-enabled="${c.enabled}"><div class="row-identity"><div class="row-art-wrap"><img class="row-art" src="${escape(artwork(c))}" alt="${escape(c.name)}">${c.mastery>0?`<span class="row-mastery" role="img" aria-label="突破等级 ${c.mastery}" title="突破等级 ${c.mastery}"><img src="${new URL('../cards/game-icons/limit-break.png',import.meta.url).href}" alt=""><b>${c.mastery}</b></span>`:''}</div><div><div class="row-meta"><span class="row-id">${idText(c.id)}${c.demo?'<small class="sample-tag">示例</small>':''}</span><div class="row-tags">${attrImage(c.attribute)}${rarityImage(c.rarity)}</div></div><h3>${escape(c.name)}</h3><p>${escape(character(c.character).name)}</p></div></div><div class="row-power"><dl class="row-stats">${labels.map((l,i)=>`<div><dt>${l}</dt><dd>${number(p.values[i])}</dd></div>`).join('')}</dl><div class="row-total">合计<b>${number(p.total)}</b></div></div><div class="row-skill"><strong>${escape(s.value)}</strong><p>${escape(s.name)}</p><div class="duration">${s.duration}s</div></div><div class="row-actions"><button type="button" class="enable-toggle" data-action="toggle" role="switch" aria-checked="${c.enabled}" aria-label="启用 ${escape(c.name)}" title="${c.enabled?'停用后保留配置，不参与计算':'启用后加入计算候选'}"><span class="switch-track" aria-hidden="true"></span><span>${c.enabled?'已启用':'未启用'}</span></button><button type="button" class="edit" data-action="edit" aria-label="编辑 ${escape(c.name)}">${icon('edit')}编辑</button><button type="button" data-action="copy" aria-label="复制 ${escape(c.name)}" title="复制卡牌">${icon('copy')}</button><button type="button" class="danger" data-action="delete" aria-label="删除 ${escape(c.name)}" title="删除卡牌">${icon('delete')}</button></div></article>`;}).join('');
 $('#empty').hidden=shown.length>0;$('#empty h3').textContent=cards.length?'没有符合条件的卡牌':'还没有自定义卡牌';$('#empty p').textContent=cards.length?'尝试修改搜索内容或选择其他属性。':'从三维与技能开始，建立一张用于试算的卡牌。';$('#empty-create').hidden=cards.length>0;
}
function confirm(title,copy,accept,action){$('#confirm-title').textContent=title;$('#confirm-copy').textContent=copy;$('#confirm-accept').textContent=accept;confirmAction=action;$('#confirm-dialog').showModal();$('#confirm-cancel').focus();}
$('#confirm-cancel').onclick=()=>$('#confirm-dialog').close();$('#confirm-accept').onclick=()=>{$('#confirm-dialog').close();confirmAction?.();};
function closeEditor(){editor.close();draft=null;openedBy?.focus();}
function requestClose(){if(JSON.stringify(draft)!==original)confirm('放弃未保存的修改？','这次修改尚未保存到自定义卡牌列表。','放弃修改',closeEditor);else closeEditor();}
function nextId(){return nextSequence;}
function openEditor(card){editing=!!card;openedBy=document.activeElement;draft=card?structuredClone(card):{...seed()[0],id:nextId(),demo:false,name:'',stats:[11000,11000,11000],notes:'',advanced:null};
 $('#editor-title').textContent=editing?'编辑自定义卡牌':'创建自定义卡牌';$('#editor-eyebrow').textContent=idText(draft.id)+' / CUSTOM CARD';$('#form-status').textContent='保存到当前档案的自定义列表';$('#image-error').textContent='';$('#image-file').value='';
 for(const name of ['name','character','mastery','skillType','duration','score','unifiedScore','lowerScore','conditionAttribute','conditionBand','notes'])f(name).value=draft[name];
 for(const a of all('input[name=attribute]'))a.checked=a.value===draft.attribute;
 for(const a of all('input[name=rarity]'))a.checked=Number(a.value)===draft.rarity;
 durationTouched=false;syncSkillConstraints();
 f('enabled').checked=draft.enabled;f('advancedEnabled').checked=!!draft.advanced;all('.extra-section').forEach(x=>x.open=false);renderAdvanced();syncConditional();paintPreview();original=JSON.stringify(draft);editor.showModal();$('.editor-scroll').scrollTop=0;f('name').focus({preventScroll:true});
}
form.addEventListener('invalid',e=>{let parent=e.target.closest('details');while(parent){parent.open=true;parent=parent.parentElement.closest('details');}},true);
let durationTouched=false;
function validateDuration(show = durationTouched) {
 const input=f('duration'),message=customSkillDurationError(input.value,draft.skillType);
 const error=$('#duration-error'),wasHidden=error.hidden;
 input.setCustomValidity(message);
 input.setAttribute('aria-invalid',String(!!message&&show));
 error.textContent=show?message:'';
 error.hidden=!message||!show;
 if(wasHidden&&!error.hidden)error.scrollIntoView({block:'nearest'});
 return !message;
}
function syncSkillConstraints(){
 const rateup=draft.skillType==='rateup';
 if(rateup)draft.score=100;
 f('score').value=draft.score;f('score').readOnly=rateup;
 f('score').title=rateup?'递增技能固定从 100% 开始，最高 150%':'';
 f('duration').min=rateup?'5':'3';f('duration').max=rateup?'7':'8';
 $('#duration-help').textContent=rateup?'上下切换 5、5.5、6、6.5、7 秒，也可直接输入。':'上下切换相邻的可用时长，也可直接输入。';
 validateDuration();
}
f('duration').addEventListener('blur',()=>{if(draft){durationTouched=true;validateDuration(true);}});
f('duration').addEventListener('invalid',()=>{if(draft){durationTouched=true;validateDuration(true);}});
function stepDuration(direction){
 if(!draft)return;
 const input=f('duration');
 input.value=stepCustomSkillDuration(input.value,draft.skillType,direction);
 durationTouched=true;
 input.dispatchEvent(new Event('input',{bubbles:true}));
 input.focus({preventScroll:true});
}
f('duration').addEventListener('keydown',event=>{
 if(event.key!=='ArrowUp'&&event.key!=='ArrowDown')return;
 event.preventDefault();stepDuration(event.key==='ArrowUp'?1:-1);
});
all('[data-duration-step]').forEach(button=>{
 button.addEventListener('pointerdown',event=>event.preventDefault());
 button.addEventListener('click',()=>stepDuration(Number(button.dataset.durationStep)));
});
function syncConditional(){const unified=draft.skillType==='unified',great=draft.skillType==='great';$('#unified-fields').hidden=!unified;$('#great-fields').hidden=!great;$('#rateup-note').hidden=draft.skillType!=='rateup';$('#score-label').textContent=unified?'基础加成':great?'未降档加成':'加成';
 $('#condition-help').textContent=draft.conditionAttribute==='none'&&!Number(draft.conditionBand)?'属性与乐队均不限制：无需队伍条件，直接使用触发后加成。':'不限制表示不检查该项；设置两项时需要同时满足。';
 for(const root of [$('#unified-fields'),$('#great-fields')])root.querySelectorAll('input,select').forEach(i=>i.disabled=root.hidden||!!i.closest('label[hidden]'));
}
function renderAdvanced(){const a=draft.advanced;$('#advanced-fields').hidden=!a;f('cardLevel').disabled=!a;all('[name^=stat]').forEach((n,i)=>{n.readOnly=!!a;n.value=a?(a.levels.find(x=>x.level===a.current)?.stats[i]??''):draft.stats[i];});
 $('#stat-basis').textContent=a?'分项养成 · 当前等级基础值':'满级 · 特训后 · 剧情全开';$('#stat-help').textContent=a?'下方填写各等级基础值；特训、剧情和突破分别累加。':'填写 0 突破的三维，突破加值由系统另行计算。';
 if(!a){$('#level-rows').replaceChildren();$('#growth-bonuses').replaceChildren();return;}
 f('cardLevel').innerHTML=a.levels.map(r=>`<option value="${r.level}">${r.level}</option>`).join('');f('cardLevel').value=a.current;
 $('#level-rows').innerHTML=a.levels.map((r,i)=>`<div class="level-row"><input type="number" min="1" max="100" step="1" required value="${r.level}" data-level="${i}" aria-label="第 ${i+1} 行等级">${r.stats.map((v,j)=>`<input type="number" min="0" max="999999" step="1" required value="${v}" data-level-stat="${i},${j}" aria-label="${r.level} 级基础${labels[j]}">`).join('')}<button type="button" data-remove-level="${i}" ${a.levels.length===1?'disabled':''} aria-label="移除 ${r.level} 级数据">${icon('close')}</button></div>`).join('');
 $('#growth-bonuses').innerHTML=a.bonuses.map((b,i)=>`<div class="growth-row"><label class="growth-toggle"><input type="checkbox" data-bonus-enabled="${i}" ${b.enabled?'checked':''}>${['特训','剧情 1','剧情 2'][i]}</label><div class="stats-inputs">${b.stats.map((v,j)=>`<label>${labels[j]}<input type="number" min="0" max="99999" step="1" required value="${v}" data-bonus-stat="${i},${j}" aria-label="${['特训','剧情 1','剧情 2'][i]}${labels[j]}加值"></label>`).join('')}</div></div>`).join('');
}
function collect(){draft.enabled=f('enabled').checked;for(const name of ['name','attribute','skillType','conditionAttribute','notes'])draft[name]=f(name).value;
 for(const name of ['character','rarity','mastery','duration','score','unifiedScore','lowerScore','conditionBand'])draft[name]=f(name).value===''?NaN:Number(f(name).value);
 if(!draft.advanced)draft.stats=labels.map((_,i)=>f('stat'+i).value===''?NaN:Number(f('stat'+i).value));
}
function paintPreview(){$('#enabled-label').textContent=draft.enabled?'已启用':'未启用';$('#preview-enabled').hidden=draft.enabled;const p=power(draft),s=skill(draft),c=character(draft.character),valid=p.values.every(Number.isFinite);$('#preview-art').src=artwork(draft);$('#preview-art').alt=draft.name||'自定义卡牌原画';$('#preview-id').textContent=idText(draft.id);$('#preview-name').textContent=draft.name.trim()||c.name+' · 自定义';$('#preview-character').textContent=c.name;
 $('#preview-tags').innerHTML=attrImage(draft.attribute)+`<span>${draft.attribute}</span>`+rarityImage(draft.rarity);
 $('#band-display').innerHTML=`<img src="${asset('band_'+c.band+'.svg')}" alt=""><span>${escape(bands[c.band])}</span>`;
 $('#preview-total').textContent=valid?number(p.total):'—';const increment=draft.rarity*draft.mastery*50;$('#preview-break').textContent=`三维合计 ${number(baseStats(draft).reduce((a,b)=>a+b,0))} + 突破 ${number(increment*3)}`;$('#break-addition').textContent=`每项 +${number(increment)} · 合计 +${number(increment*3)}`;$('#preview-skill').textContent=s.value;$('#preview-description').textContent=s.description;$('#skill-description-inline').textContent=s.description;
 all('[data-art]').forEach(b=>b.setAttribute('aria-pressed',String(draft.image===data.examples[Number(b.dataset.art)].image)));
 if(draft.advanced)all('[name^=stat]').forEach((n,i)=>n.value=draft.advanced.levels.find(x=>x.level===draft.advanced.current)?.stats[i]??'');
}
form.addEventListener('input',e=>{const n=e.target;if(!draft||n.type==='file')return;
 if(n.name==='advancedEnabled'){if(n.checked)draft.advanced={current:60,levels:[{level:60,stats:[...draft.stats]}],bonuses:[0,1,2].map(()=>({enabled:true,stats:[0,0,0]}))};else{draft.stats=baseStats(draft);draft.advanced=null;}renderAdvanced();}
 if(n.name==='cardLevel'){draft.advanced.current=Number(n.value);}
 if(n.dataset.level!==undefined){const index=Number(n.dataset.level),row=draft.advanced.levels[index],previous=row.level;row.level=Number(n.value);if(draft.advanced.current===previous)draft.advanced.current=row.level;all('[data-level]').forEach(input=>input.setCustomValidity(draft.advanced.levels.filter(x=>x.level===Number(input.value)).length>1?'等级不能重复':''));f('cardLevel').innerHTML=draft.advanced.levels.map(r=>`<option value="${r.level}">${r.level}</option>`).join('');f('cardLevel').value=draft.advanced.current;}
 if(n.dataset.levelStat){const [i,j]=n.dataset.levelStat.split(',').map(Number);draft.advanced.levels[i].stats[j]=n.value===''?NaN:Number(n.value);}
 if(n.dataset.bonusEnabled!==undefined)draft.advanced.bonuses[Number(n.dataset.bonusEnabled)].enabled=n.checked;
 if(n.dataset.bonusStat){const [i,j]=n.dataset.bonusStat.split(',').map(Number);draft.advanced.bonuses[i].stats[j]=n.value===''?NaN:Number(n.value);}
 collect();if(n.name==='skillType'){durationTouched=true;syncSkillConstraints();}if(n.name==='duration')validateDuration();syncConditional();paintPreview();
});
$('#add-level').onclick=()=>{const a=draft.advanced;const level=Array.from({length:100},(_,i)=>i+1).find(v=>!a.levels.some(r=>r.level===v));if(!level)return; a.levels.push({level,stats:[0,0,0]});renderAdvanced();};
$('#level-rows').onclick=e=>{const b=e.target.closest('[data-remove-level]');if(!b||draft.advanced.levels.length===1)return;draft.advanced.levels.splice(Number(b.dataset.removeLevel),1);if(!draft.advanced.levels.some(x=>x.level===draft.advanced.current))draft.advanced.current=draft.advanced.levels[0].level;renderAdvanced();paintPreview();};
$('#art-gallery').onclick=e=>{const b=e.target.closest('[data-art]');if(!b)return;draft.image=data.examples[Number(b.dataset.art)].image;paintPreview();};
$('#image-file').onchange=async e=>{const file=e.target.files[0];if(!file)return;if(!['image/png','image/jpeg','image/webp'].includes(file.type)||file.size>2*1024*1024){$('#image-error').textContent='请选择 2 MB 以内的 PNG、JPEG 或 WebP 图片。';return;}const active=draft;try{const src=await new Promise((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>resolve(reader.result);reader.onerror=reject;reader.readAsDataURL(file);});if(active!==draft)return;const image=new Image();image.src=src;await image.decode();if(active!==draft)return;draft.image=src;$('#image-error').textContent='';paintPreview();}catch{$('#image-error').textContent='无法读取这张图片，请更换文件。';}};
form.onsubmit=e=>{e.preventDefault();durationTouched=true;validateDuration(true);if(!form.reportValidity())return;collect();draft.name=draft.name.trim()||character(draft.character).name+' · 自定义';draft.updated=Date.now();draft.demo=false;if(persist([...cards.filter(c=>c.id!==draft.id),structuredClone(draft)])){closeEditor();toast(editing?'卡牌修改已保存':'自定义卡牌已创建');}};
all('[data-close]').forEach(b=>b.onclick=requestClose);editor.addEventListener('cancel',e=>{e.preventDefault();requestClose();});
$('#create').onclick=$('#empty-create').onclick=()=>openEditor();$('#search').oninput=render;$('#sort').onchange=render;
$('#attribute-filter').onclick=e=>{const b=e.target.closest('[data-filter]');if(!b)return;filter=b.dataset.filter;all('[data-filter]').forEach(x=>x.setAttribute('aria-pressed',String(x===b)));render();};
$('#custom-list').onclick=e=>{const button=e.target.closest('[data-action]'),row=e.target.closest('[data-card]');if(!button||!row)return;const c=cards.find(c=>c.id===Number(row.dataset.card));if(button.dataset.action==='toggle'){if(persist(cards.map(x=>x.id===c.id?{...x,enabled:!x.enabled}:x))){$(`[data-card="${c.id}"] [data-action=toggle]`)?.focus();toast(c.enabled?'已停用，卡牌配置仍保留':'已启用此自定义卡牌');}}if(button.dataset.action==='edit')openEditor(c);if(button.dataset.action==='copy'){if(persist([...cards,{...structuredClone(c),id:nextId(),uid:undefined,name:c.name+' · 副本',updated:Date.now(),demo:false}]))toast('已复制，可继续编辑新卡牌');}if(button.dataset.action==='delete')confirm('删除自定义卡牌？',`「${c.name}」将从当前档案移除。${getPlayer().ptEvaluate?.teams?.flat().includes(c.id)?'指定队伍仍在使用它，删除后需要重新选择卡位。':'已有计算结果会保留当时的卡牌资料。'}`,'删除卡牌',()=>{if(persist(cards.filter(x=>x.id!==c.id)))toast('自定义卡牌已删除');});};

function refresh(){
 const current=getProfileId();
 if(profileId!==current){if(editor.open)closeEditor();$('#confirm-dialog').close();filter='all';$('#search').value='';$('#sort').value='updated';all('[data-filter]').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.filter==='all')));}
 profileId=current;const core=getCore(),player=getPlayer();
 data.characters=Object.entries(core?.characters||{}).filter(([,c])=>bands[c.bandId]).map(([id,c])=>({id:Number(id),band:Number(c.bandId),name:gameText(c.characterName,'角色 '+id)}));
 data.examples=Object.keys(player.cardList||{}).map(id=>({id,record:core?.cards?.[id]})).filter(c=>c.record?.resourceSetName).slice(-6).reverse().map(c=>({name:gameText(core.characters[c.record.characterId]?.characterName,'卡牌 '+c.id),image:cardArtUrls({card:c.record})[0]?.replace(/^.*?\/assets\//,'https://bestdori.com/assets/')})).filter(c=>c.image);
 f('character').innerHTML=Object.entries(bands).map(([id,band])=>`<optgroup label="${escape(band)}">${data.characters.filter(c=>c.band===Number(id)).map(c=>`<option value="${c.id}">${escape(c.name)}</option>`).join('')}</optgroup>`).join('');
$('#art-gallery').innerHTML=data.examples.map((c,i)=>`<button type="button" data-art="${i}" aria-pressed="false" aria-label="使用${escape(c.name)}的参考原画"><img src="${c.image}" alt="${escape(c.name)}"></button>`).join('');

 cards=Object.values(player.customCards||{}).map(customCardDraft);nextSequence=nextCustomCardId(player);render();
}
refresh();return {refresh,openEditor,destroy(){editor.close();$('#confirm-dialog').close();portal.remove();host.remove();}};
}
