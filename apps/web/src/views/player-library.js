import {designFragment} from '../ui/approved/templates.js';
import {hydrateDesignIcons} from '../ui/design-controls.js';
import {assetBaseUrl,assetImage,characterIconUrls,bandIconUrls,attributeIconUrls} from '../assets/index.js';
import {gameText,profilePreference,saveProfilePreference} from '../ui/preferences.js';
import {bandOrder} from '../ui/cards/rules.js';
import {bandLabel} from '../utils.js';
import {designIcon} from '../ui/fidelity.js';
import {confirmDialog} from '../ui/confirm.js';
const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};
const btn=(text,fn,cls='text-button')=>{const n=el('button',cls,text);n.type='button';n.onclick=fn;return n;};
const pct=value=>(Math.round(Number(value||0)*1000)/10).toLocaleString()+'%';
const tones={1:'#b03d77',2:'#ae4056',3:'#8b651e',4:'#277764',5:'#53578e',18:'#23757a',21:'#24758e',45:'#397693',all:'#786783'};
const attributeTones={powerful:'#b43e63',cool:'#4c69b4',happy:'#906323',pure:'#357258'};
function material(node,color='#786783',art){node.style.setProperty('--material-ink',color);node.style.setProperty('--material-rgb',[1,3,5].map(i=>parseInt(color.slice(i,i+2),16)).join(' '));if(art)node.style.setProperty('--material-mark',`url("${art}")`);}
export function itemArtUrls(id,server='jp'){
 const number=Number(id);if(!Number.isInteger(number)||number<=0)return [];
 return [...new Set([server,'jp','cn','en'])].map(s=>`${assetBaseUrl()}/${s}/thumb/areaitem/group00000_rip/areaItemRes${String(number).padStart(5,'0')}.png`);
}
export function createPlayerView({elements,readPlayer,writePlayer,getProfileId,getCore,recordWithFix,serverScopedValue,areaItemGroups,areaItemGroupIconUrls,areaItemLabel,maxAreaItemLevel,formatAreaItemRate,normalizedCharacterBonus,maxCharacterBonusForPlayer,updateAreaItem}){
 let expanded=new Set(),profile,characterSearch='',ownedOnly=false,characterBand='all';
 const page=document.querySelector('#player-library');
 const areaSection=elements.areaItemRows.closest('.panel'),characterSection=elements.characterBonusRows.closest('.panel');
 areaSection.id='area-section';areaSection.className='page-section';characterSection.id='character-section';characterSection.className='page-section character-section';
 const nav=el('nav','section-nav');nav.setAttribute('aria-label','页内分区');nav.innerHTML='<a href="#area-section">'+designIcon('activity')+'区域道具 <small data-items></small></a><a href="#character-section">'+designIcon('player')+'角色加成 <small data-characters></small></a>';
 page.querySelector('.page-hero').after(nav);
 for(const [section,number,title,kind]of[[areaSection,'01','区域道具','area'],[characterSection,'02','角色加成','character']]){
  const source=designFragment(kind==='area'?'player-area':'player-characters');
  const header=source.querySelector('.section-heading');header.querySelector('.section-title .muted').dataset.progress='';
  header.querySelector('h2').textContent=title;section.querySelector('.panel-header').replaceWith(header);
  const buttons=header.querySelectorAll('button');buttons[0].type=buttons[1].type='button';buttons[0].onclick=()=>bulk(kind,true);buttons[1].onclick=()=>bulk(kind,false);

 }
 elements.setAreaItems.hidden=elements.setCharacterBonuses.hidden=elements.toggleCharacterBonuses.hidden=true;
 function prefs(){if(profile!==getProfileId()){profile=getProfileId();expanded=new Set(profilePreference(profile,'itemGroups',['band:1']));}}
 function saveExpanded(){saveProfilePreference(profile,'itemGroups',[...expanded]);renderAreaItems(readPlayer());}
 async function bulk(kind,max,ids){
  const origin=getProfileId(),player=readPlayer();ids??=kind==='area'?areaItemGroups(player).flatMap(g=>g.areaItemIds):Object.keys(getCore().characters);
  if(!await confirmDialog({title:max?'设为满级':'清零加成',lines:[`将${ids.length} ${kind==='area'?'件道具':'位角色'}${max?'设为当前服最高等级':'设为 0'}。`],confirmText:'应用修改',danger:!max})||origin!==getProfileId())return;
  const latest=readPlayer(),limits=maxCharacterBonusForPlayer(latest);
  for(const id of ids)if(kind==='area')latest.areaItem[id]={...latest.areaItem[id],level:max?maxAreaItemLevel(id):0};else latest.characterBouns[id]={potential:Object.fromEntries(['performance','technique','visual'].map(k=>[k,max?limits.potential:0])),characterTask:Object.fromEntries(['performance','technique','visual'].map(k=>[k,max?limits.characterTask:0]))};
  writePlayer(latest);kind==='area'?renderAreaItems(latest):renderCharacterBonuses(latest);
 }
 function renderAreaItems(player){
  prefs();const root=elements.areaItemRows;root.className='accepted-item-library';const template=designFragment('player-area');template.querySelector('.section-heading').remove();root.replaceChildren(...template.children);hydrateDesignIcons(root);
  const groups=areaItemGroups(player),all=groups.flatMap(g=>g.areaItemIds),bands=groups.filter(g=>g.category==='band').sort((a,b)=>(a.isAll?99:bandOrder.indexOf(Number(a.bandId)))-(b.isAll?99:bandOrder.indexOf(Number(b.bandId))));
  nav.querySelector('[data-items]').textContent=all.length;areaSection.querySelector('[data-progress]').textContent=`${all.filter(id=>player.areaItem[id]?.level===maxAreaItemLevel(id)).length} / ${all.length} 件已满级`;
  const toggle=root.querySelector('#expand-bands');elements.toggleAreaItems.className='text-button';elements.toggleAreaItems.textContent=expanded.size===bands.length?'折叠全部':'展开全部';toggle.replaceWith(elements.toggleAreaItems);
  const bandHost=root.querySelector('#band-items');
  for(const group of bands){
   const section=el('div',`band-row ${expanded.has(group.key)?'is-expanded':''}`);material(section,tones[group.bandId]||tones.all,bandIconUrls(group.bandId)[0]);
   const header=btn('',()=>{expanded.has(group.key)?expanded.delete(group.key):expanded.add(group.key);saveExpanded();},'band-summary');header.setAttribute('aria-expanded',String(expanded.has(group.key)));
   const identity=el('span','band-identity'),image=assetImage(areaItemGroupIconUrls(group),'band-logo','');identity.append(image||el('span','common-logo','CiRCLE'));
   const name=el('span','band-name',group.isAll?'全乐队通用':group.label);name.append(el('small','',`${group.areaItemIds.filter(id=>player.areaItem[id]?.level===maxAreaItemLevel(id)).length} / ${group.areaItemIds.length} 件已满级`));identity.append(name);
   const mini=el('span','mini-items');mini.setAttribute('aria-hidden','true');for(const id of group.areaItemIds){const level=player.areaItem[id]?.level||0,n=el('span',`mini-item ${level===maxAreaItemLevel(id)?'is-max':''}`);n.append(assetImage(itemArtUrls(id,player.server),'','')||el('span'),el('small','',level?'Lv.'+level:'—'));mini.append(n);}
   const rate=el('span','group-rate');rate.append(el('small','','计算加成'),el('strong','',formatAreaItemRate(group.rate)));const chevron=el('span','chevron');chevron.innerHTML='<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><path d="m6 9 6 6 6-6"/></svg>';header.append(identity,mini,rate,chevron);section.append(header);
   if(expanded.has(group.key)){const detail=el('div','band-detail'),toolbar=el('div','shelf-toolbar');toolbar.append(el('span','','等级 0 表示未配置'),btn('本组全部满级',()=>bulk('area',true,group.areaItemIds)));const shelf=el('div','item-shelf');for(const id of group.areaItemIds)shelf.append(item(player,id));detail.append(toolbar,shelf);section.append(detail);}bandHost.append(section);
  }
  const attributes=root.querySelector('#attribute-items');
  for(const group of groups.filter(g=>g.category==='attribute').sort((a,b)=>(a.isAll?99:Object.keys(attributeTones).indexOf(a.attribute))-(b.isAll?99:Object.keys(attributeTones).indexOf(b.attribute)))){const section=el('div','attribute-group'+(group.isAll?' attribute-common':'')),head=el('div','attribute-heading');material(section,attributeTones[group.attribute]);section.style.setProperty('--group-color',attributeTones[group.attribute]||'#8b779d');
   if(group.isAll){const icons=el('span','all-attributes');for(const attr of Object.keys(attributeTones))icons.append(assetImage(attributeIconUrls(attr),'',''));head.append(icons,el('h4','','通用'));}else{head.append(assetImage(areaItemGroupIconUrls(group),'','')||el('span'),el('h4','',group.attribute.charAt(0).toUpperCase()+group.attribute.slice(1)),el('strong','',formatAreaItemRate(group.rate)));}
   const shelf=el('div','attribute-shelf');for(const id of group.areaItemIds)shelf.append(item(player,id));section.append(head,shelf);attributes.append(section);
  }
  const magazines=root.querySelector('#magazine-items');for(const group of groups.filter(g=>g.category==='magazine'))for(const id of group.areaItemIds)magazines.append(item(player,id));
 }
 function item(player,id){
  const level=player.areaItem[id]?.level||0,max=maxAreaItemLevel(id),card=el('div','item'),art=el('div','item-art');art.append(assetImage(itemArtUrls(id,player.server),'',areaItemLabel(id))||el('span'));card.append(art);
  const name=el('div','item-name',areaItemLabel(id));name.title=areaItemLabel(id);const record=recordWithFix('areaItems','areaItemsFix',id),rate=Object.fromEntries(['performance','technique','visual'].map(key=>[key,level?Number(serverScopedValue(record?.[key]?.[String(level)]))||0:0]));card.append(name,el('p','item-rate','+'+formatAreaItemRate(rate)));
  const control=el('div','level-control'),input=el('input');input.type='number';input.min=0;input.max=max;input.step=1;input.value=level;input.setAttribute('aria-label',areaItemLabel(id)+'等级');
  const change=value=>{input.value=Math.max(0,Math.min(max,value));updateAreaItem(id,{level:Number(input.value)});};
  const down=btn('−',()=>change(Number(input.value)-1),''),up=btn('+',()=>change(Number(input.value)+1),'');down.disabled=level===0;up.disabled=level===max;down.setAttribute('aria-label','降低'+areaItemLabel(id)+'等级');up.setAttribute('aria-label','提高'+areaItemLabel(id)+'等级');control.append(down,input,up);input.onchange=()=>{if(input.reportValidity())change(Number(input.value));};
  card.append(control,el('span','level-caption'+(level===max?' is-max':''),level===0?'未配置':level===max?'已满级':'上限 '+max));return card;
 }
 function renderCharacterBonuses(player){
  prefs();const root=elements.characterBonusRows;root.className='accepted-character-library';const template=designFragment('player-characters');template.querySelector('.section-heading').remove();root.replaceChildren(...template.children);hydrateDesignIcons(root);
  const search=root.querySelector('#character-search');search.value=characterSearch;
  let timer;search.oninput=()=>{clearTimeout(timer);characterSearch=search.value;timer=setTimeout(()=>{const position=search.selectionStart;renderCharacterBonuses(readPlayer());const next=root.querySelector('input[type=search]');next.focus();next.setSelectionRange(position,position);},150);};
  const check=root.querySelector('#owned-only');check.checked=ownedOnly;check.onchange=()=>{ownedOnly=check.checked;renderCharacterBonuses(readPlayer());};
  const records=getCore()?.characters||{},ownedIds=new Set([...Object.keys(player.cardList).map(id=>Number(getCore().cards[id]?.characterId)),...Object.values(player.customCards||{}).filter(c=>c.enabled).map(c=>c.definition.characterId)]),ids=Object.keys(records).filter(id=>(characterBand==='all'||Number(records[id].bandId)===Number(characterBand))&&(!ownedOnly||ownedIds.has(Number(id)))&&(!characterSearch||[...(records[id].characterName||[]),...(records[id].nickname||[]),bandLabel(records[id].bandId),id].join(' ').toLowerCase().includes(characterSearch.toLowerCase())));
  nav.querySelector('[data-characters]').textContent=Object.keys(records).length;characterSection.querySelector('[data-progress]').textContent=`${ids.length} / ${Object.keys(records).length} 位角色`;
  const filter=root.querySelector('#character-bands');filter.setAttribute('role','group');filter.setAttribute('aria-label','角色乐队筛选');for(const band of ['all',...bandOrder.filter(band=>Object.values(records).some(c=>Number(c.bandId)===band))]){const b=btn(band==='all'?'全部':'',()=>{characterBand=String(band);renderCharacterBonuses(readPlayer());},'');if(band!=='all')b.append(assetImage(bandIconUrls(band),'',bandLabel(band))||el('span'));b.setAttribute('aria-pressed',String(String(band)===characterBand));filter.append(b);}
  for(const band of [...new Set([...bandOrder,...ids.map(id=>Number(records[id].bandId))])]){
   const members=ids.filter(id=>Number(records[id].bandId)===band);if(!members.length)continue;
   const row=el('section','character-band');material(row,tones[band]);const label=el('div','character-band-label');label.append(assetImage(bandIconUrls(band),'',bandLabel(band))||el('span'),el('small','',members.length+' 位成员'));row.append(label);const host=el('div','members');
   for(const id of members){const bonus=normalizedCharacterBonus(player.characterBouns[id]),card=btn('',()=>editCharacter(id),'member');card.dataset.owned=String(ownedIds.has(Number(id)));const name=gameText(records[id].characterName,`角色 ${id}`);card.setAttribute('aria-label','编辑'+name+'的潜能和任务加成');card.append(assetImage(characterIconUrls(id),'',name)||el('span'),el('span','member-name',name));
    const matrix=el('span','member-bonus-matrix'),fields=[['performance','演出'],['technique','技巧'],['visual','形象']],description=[];
    matrix.append(el('span'));for(const [,title]of fields)matrix.append(el('span','member-bonus-heading',title));
    for(const [key,title]of[['potential','潜能'],['characterTask','任务']]){
     matrix.append(el('span','member-bonus-heading',title));
     for(const [field,label]of fields){const value=el('b','member-bonus-value',pct(bonus[key][field]));value.dataset.bonus=key+'.'+field;value.title=title+' · '+label;matrix.append(value);description.push(title+label+' '+pct(bonus[key][field]));}
    }
    card.setAttribute('aria-label','编辑'+name+'的潜能和任务加成，'+description.join('，'));card.append(matrix);host.append(card);
   }row.append(host);root.querySelector('#character-groups').append(row);
  }
  root.querySelector('#character-empty').hidden=ids.length>0;
  root.querySelector('#reset-character-filter').type='button';root.querySelector('#reset-character-filter').onclick=()=>{characterSearch='';characterBand='all';ownedOnly=false;renderCharacterBonuses(readPlayer());};

 }
 function editCharacter(id){
  const origin=getProfileId(),player=readPlayer(),draft=structuredClone(normalizedCharacterBonus(player.characterBouns[id])),record=getCore().characters[id],name=gameText(record?.characterName,`角色 ${id}`),dialog=el('dialog','editor');dialog.id='character-editor';dialog.setAttribute('aria-label','角色加成：'+name);material(dialog,tones[record.bandId],bandIconUrls(record.bandId)[0]);
  dialog.innerHTML='<form method="dialog"><div class="dialog-top"><span>角色加成</span><button type="button" class="icon-button" aria-label="关闭角色加成">×</button></div><div class="editor-identity"><div><span class="eyebrow"></span><h2></h2><p>分别设置三项能力的加成比例</p></div></div><div class="editor-matrix"><div class="matrix-head"><span></span><span>演出</span><span>技巧</span><span>形象</span></div></div><div class="editor-total"><span>合计</span><div></div></div><div class="editor-quick"></div><div class="dialog-bottom"><span>保存到当前档案</span><button type="submit" value="save" class="primary">应用修改</button></div></form>';
  const identity=dialog.querySelector('.editor-identity');identity.prepend(assetImage(characterIconUrls(id),'',name)||el('span'));identity.querySelector('h2').textContent=name;identity.querySelector('.eyebrow').textContent=bandLabel(record.bandId);dialog.querySelector('[aria-label=关闭角色加成]').onclick=()=>dialog.close('cancel');
  const matrix=dialog.querySelector('.editor-matrix');
  const totals=()=>{const host=dialog.querySelector('.editor-total>div');host.replaceChildren();for(const [field,text]of[['performance','演出'],['technique','技巧'],['visual','形象']]){const values=[...matrix.querySelectorAll(`[data-field=${field}]`)].map(n=>Number(n.value));const span=el('span','',text);span.append(el('strong','',pct((values[0]+values[1])/100)));host.append(span);}};
  for(const [key,title]of[['potential','潜能'],['characterTask','任务']]){const row=el('div','matrix-row');row.append(el('b','',title));for(const [field,text]of[['performance','演出'],['technique','技巧'],['visual','形象']]){const label=el('label'),input=el('input');input.type='number';input.min=0;input.step=.1;input.value=(draft[key][field]*100).toFixed(1);input.required=true;input.dataset.group=key;input.dataset.field=field;input.setAttribute('aria-label',title+text+'百分比');input.oninput=totals;label.append(input,el('span','','%'));row.append(label);}matrix.append(row);}
  const max=maxCharacterBonusForPlayer(player),quick=dialog.querySelector('.editor-quick');quick.append(btn('设为当前服满级',()=>{for(const input of matrix.querySelectorAll('input'))input.value=(max[input.dataset.group]*100).toFixed(1);totals();}),btn('清零',()=>{for(const input of matrix.querySelectorAll('input'))input.value=0;totals();}),el('span','',`当前服快捷值：潜能 ${pct(max.potential)} · 任务 ${pct(max.characterTask)}`));totals();
  const trigger=document.activeElement;dialog.onclose=()=>{if(dialog.returnValue==='save'&&origin===getProfileId()){for(const input of matrix.querySelectorAll('input'))draft[input.dataset.group][input.dataset.field]=Number(input.value)/100;const latest=readPlayer();latest.characterBouns[id]=draft;writePlayer(latest);renderCharacterBonuses(latest);}dialog.remove();trigger?.focus();};page.append(dialog);dialog.showModal();
 }
 return {renderAreaItems,renderCharacterBonuses,handleToggleAreaItems(){prefs();const bands=areaItemGroups(readPlayer()).filter(g=>g.category==='band');expanded=expanded.size===bands.length?new Set():new Set(bands.map(g=>g.key));saveExpanded();},handleToggleCharacterBonuses(){renderCharacterBonuses(readPlayer());}};
}
