import {mountCatalogTemplate} from './template.js';
import { language } from '../preferences.js';
import { defaultFilters, filterCards, groupCards, attributeNames, normalizeCardSort } from './rules.js';
import { createFilterControls } from './filters.js';
import { cardBriefMarkup, bindCardBrief } from './presentation.js';
import { assetImage, starIconUrls, attributeIconUrls, characterIconUrls } from '../../assets/index.js';
import { profilePreference, saveProfilePreference } from '../preferences.js';
import { icon } from '../shell.js';

export function toggleSelectionScope(selected, ids) {
  const next = new Set(selected), all = ids.length > 0 && ids.every(id=>next.has(id));
  for (const id of ids) all ? next.delete(id) : next.add(id);
  return next;
}
const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};
const button=(text,fn)=>{const n=el('button','',text);n.type='button';n.onclick=fn;return n;};
const options={group:[['rarity','稀有度'],['attribute','属性'],['character','角色'],['none','不分组']],sort:[['owned-release','持有状态'],['release','发布时间'],['rarity','稀有度'],['id','卡牌 ID']]};
const orderDescription={
 'owned-release':{desc:'已持有优先，发布时间新到旧',asc:'未持有优先，发布时间旧到新'},
 release:{desc:'发布时间新到旧',asc:'发布时间旧到新'},
 rarity:{desc:'稀有度高到低，同稀有度发布时间新到旧',asc:'稀有度低到高，同稀有度发布时间旧到新'},
 id:{desc:'卡牌 ID 大到小',asc:'卡牌 ID 小到大'},
};

// Inventory and team picker share filtering, grouping, sorting and card presentation.
export function createCardCatalog({root,getCards,getCharacters,getProfileId,getServer,onOpen,onIllustrationChange,onBulk,onPick,selectionMode='inventory',candidateFilter=()=>true,onReset,selectedIds=()=>[],disabledReason=()=>''}) {
  let profile,renderedLanguage,characters=[],filters,search='',group='rarity',sort='owned-release',sortDirection='desc',selected=new Set(),bulk=false,controls,observer,timer;
  let matched=[],groups=[];
  root.classList.add('card-catalog','shared-card-library');
  mountCatalogTemplate(root);
  const q=s=>root.querySelector(s);
  for(const [key,entries] of Object.entries(options))q(`[data-view=${key}]`).replaceChildren(...entries.map(([value,text])=>{const o=el('option','',text);o.value=value;return o;}));
  q('[data-show-results]').onclick=()=>q('.catalog-results').scrollIntoView({block:'start',behavior:'smooth'});
  q('[data-bulk-toggle]').hidden=!onBulk;
  q('[data-bulk-toggle]').onclick=()=>{bulk=!bulk;if(!bulk)selected.clear();q('[data-bulk-toggle]').lastChild.textContent=bulk?'完成管理':'批量管理';q('[data-bulk-toggle]').setAttribute('aria-pressed',String(bulk));root.dataset.bulk=String(bulk);paintResults();};
  q('[data-back-filter]').onclick=()=>q('.catalog-filters').scrollIntoView({block:'start',behavior:'smooth'});
  q('input[type=search]').oninput=e=>{search=e.target.value;clearTimeout(timer);timer=setTimeout(refresh,120);};
  for(const key of ['group','sort'])q(`[data-view=${key}]`).onchange=e=>{if(key==='group')group=e.target.value;else sort=e.target.value;refresh();};
  q('[data-sort-direction]').onclick=()=>{sortDirection=sortDirection==='desc'?'asc':'desc';refresh();};
  const viewKey=selectionMode==='team'?'pickerView':selectionMode==='bonus'?'bonusCardView':'cardView';
  function reset(){filters=defaultFilters(characters,getServer());if(selectionMode==='team')filters.server=null;search='';q('input[type=search]').value='';onReset?.();refresh();}
  function save(){saveProfilePreference(profile,viewKey,{filters:Object.fromEntries(Object.entries(filters).map(([k,v])=>[k,v===null?null:[...v]])),search,group,sort,sortDirection});}
  function init(){
    const changedProfile=profile!==getProfileId();profile=getProfileId();renderedLanguage=language();characters=getCharacters();
    filters=defaultFilters(characters,getServer());search='';group='rarity';sort='owned-release';sortDirection='desc';
    const saved=profilePreference(profile,viewKey,null);
    if(saved){for(const [k,v] of Object.entries(saved.filters||{}))if(k in filters)filters[k]=v===null?null:new Set(v);search=saved.search||'';group=saved.group||'rarity';({sort,sortDirection}=normalizeCardSort(saved.sort,saved.sortDirection));}
    if(selectionMode==='team'){filters.server=null;filters.ownership=new Set(['owned']);}
    if(changedProfile)selected=new Set();
    for(const n of root.querySelectorAll('[data-host],[data-actions]'))n.replaceChildren();
    // Picker candidates may include any held card; release restriction is opt-in.
    const hosts=Object.fromEntries([...root.querySelectorAll('[data-host]')].map(n=>[n.dataset.host,n]));
    const locked=selectionMode==='team'?['server','ownership']:[];
    for(const key of locked){hosts[key].closest('.filter-row').hidden=true;delete hosts[key];}
    const actionHosts=Object.fromEntries([...root.querySelectorAll('[data-actions]')].filter(n=>!locked.includes(n.dataset.actions)).map(n=>[n.dataset.actions,n]));
    controls=createFilterControls(q('.catalog-filters'),{characters,getFilters:()=>filters,onChange:refresh,onReset:reset,hosts,actionHosts});
    q('input[type=search]').value=search;
  }
  function refresh(){
    if(profile!==getProfileId()||renderedLanguage!==language()||!filters)init();
    controls.update();q('[data-view=group]').value=group;q('[data-view=sort]').value=sort;
    const ascending=sortDirection==='asc',direction=q('[data-sort-direction]'),label=ascending?'升序':'降序';
    direction.innerHTML=`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${ascending?'M7 20V4m-4 4 4-4 4 4M15 6h2m-2 6h4m-4 6h6':'M7 4v16m-4-4 4 4 4-4M15 6h6m-6 6h4m-4 6h2'}"/></svg><span>${label}</span>`;
    direction.dataset.sortDirection=sortDirection;
    direction.setAttribute('aria-label',`排序方向：${label}，切换为${ascending?'降序':'升序'}`);
    direction.title=`当前：${orderDescription[sort][sortDirection]}；点击切换为${ascending?'降序':'升序'}`;
    save();paintResults();
  }
  function groupLabel(key){return group==='rarity'?`${key} 星`:group==='attribute'?attributeNames[key]||key:group==='character'?characters.find(c=>String(c.id)===key)?.name||`角色 ${key}`:'全部卡牌';}
  function groupImage(key){return group==='rarity'?starIconUrls(key):group==='attribute'?attributeIconUrls(key):group==='character'?characterIconUrls(key):[];}
  function paintResults(){
    observer?.disconnect();
    const cards=getCards();
    // Unknown IDs stay visible as repairable inventory records rather than disappearing.
    const unknown=cards.filter(c=>c.unknown&&c.owned);
    matched=filterCards(cards.filter(c=>!c.unknown&&candidateFilter(c)),filters,search);
    groups=groupCards(matched,group,sort,characters,sortDirection);
    q('.catalog-count').innerHTML=`<span class="match-value">${matched.length.toLocaleString()}</span><span class="match-unit">张卡牌</span>`;q('.match-detail').textContent=`${new Set(matched.map(c=>c.characterId)).size} 位角色`;q('[data-filter-description]').textContent=`已选 ${filters.character.size} 位角色 · ${filters.server===null?'全部服务器':filters.server.size+' 个服务器'} · ${filters.ownership.has('owned')?'已持有':''}${filters.ownership.has('missing')?' / 未持有':''}`;
    const host=q('.catalog-groups');host.replaceChildren();
    const jumps=q('.catalog-jumps');jumps.replaceChildren();

    observer=new IntersectionObserver(entries=>{for(const e of entries)if(e.isIntersecting)e.target.loadMore?.();},{rootMargin:'500px'});
    if(unknown.length&&!onPick){const issue=el('div','catalog-issues',`${unknown.length} 张持有卡缺少元数据，养成记录已保留。`);for(const c of unknown)issue.append(button(`#${c.id}`,()=>onOpen?.(c)));host.append(issue);}
    if(!matched.length)host.append(el('p','catalog-empty','没有符合条件的卡牌。调整筛选，或恢复档案默认。'));
    for(const g of groups){
      const section=el('section','catalog-group card-group'),heading=el('div','group-heading');
      section.id=`${onPick?'picker':'group'}-${group}-${g.key}`;
      const img=assetImage(groupImage(g.key),'group-icon',groupLabel(g.key));if(img)heading.append(img);
      heading.append(el('h3','',groupLabel(g.key)),el('span','group-amount',`${g.cards.length} 张`));
      if(bulk){const all=button('全选',()=>{selected=toggleSelectionScope(selected,g.cards.map(c=>c.id));paintSelection();});all.dataset.groupSelect=g.key;all.className='group-select filter-action bulk-group-all';all.setAttribute('aria-label',`全选${groupLabel(g.key)}分组`);heading.append(all);}
      const grid=el('div','catalog-grid card-grid'),sentinel=el('div','catalog-sentinel');let count=0;
      function more(){const fragment=document.createDocumentFragment();for(const c of g.cards.slice(count,count+60))fragment.append(tile(c));count+=60;grid.append(fragment);if(count>=g.cards.length)observer.unobserve(sentinel);}
      sentinel.loadMore=more;section.append(heading,grid,sentinel);host.append(section);more();if(count<g.cards.length)observer.observe(sentinel);
      const jump=button('',()=>section.scrollIntoView({block:'start'}));const ji=assetImage(groupImage(g.key),'',groupLabel(g.key));if(ji)jump.append(ji);else jump.append(el('span','',groupLabel(g.key)));jump.append(el('span','',String(g.cards.length)));jump.title=groupLabel(g.key);jumps.append(jump);
    }
    const bar=q('.catalog-bulk');bar.hidden=!bulk;bar.replaceChildren();
    if(bulk){const scope=el('div','bulk-scope'),selection=el('div','bulk-selection'),actions=el('div','bulk-actions');scope.append(el('strong','selection-count'),el('span','',`当前筛选 ${matched.length.toLocaleString()} 张`));for(const [action,label] of [['add','标记持有'],['edit','修改养成'],['remove','移除持有']]){const b=button(label,async()=>{await onBulk?.(action,cards.filter(c=>selected.has(c.id)));refresh();});b.dataset.bulkAction=action;actions.append(b);}const all=button('全选',()=>{selected=toggleSelectionScope(selected,matched.map(c=>c.id));paintSelection();});all.dataset.scopeSelect='';all.className='filter-action';all.setAttribute('aria-label','全选当前筛选结果');all.disabled=!matched.length;selection.append(all);bar.append(scope,selection,actions);}
    paintSelection();
  }
  function tile(c){
    const tile=el('article','cp-card catalog-card');tile.dataset.cpId=c.id;tile.dataset.owned=String(c.owned);
    tile.innerHTML=cardBriefMarkup(c);
    const reason=disabledReason(c);tile.dataset.disabled=String(!!reason);
    if(reason){tile.title=reason;tile.querySelector('.cp-main').setAttribute('aria-disabled','true');}
    bindCardBrief(tile,{resolve:()=>c,onOpen:card=>{if(bulk){selected=toggleSelectionScope(selected,[c.id]);paintSelection();}else if(onPick){if(!reason){onPick(card);paintSelection();}}else onOpen?.(card);},onIllustrationChange});
    tile.dataset.pressed=String(onPick?selectedIds().includes(c.id):selected.has(c.id));
    if(bulk){const check=el('input','card-select');check.type='checkbox';check.checked=selected.has(c.id);check.setAttribute('aria-label',`选择卡牌 ${c.id}`);check.onchange=()=>{selected=toggleSelectionScope(selected,[c.id]);paintSelection();};tile.prepend(check);}
    return tile;
  }
  function paintSelection(){
    const chosen=onPick?new Set(selectedIds()):selected;
    for(const tile of root.querySelectorAll('.catalog-card')){const has=chosen.has(Number(tile.dataset.cpId));tile.dataset.pressed=String(has);if(onPick||bulk)tile.querySelector('.cp-main').setAttribute('aria-pressed',String(has));const input=tile.querySelector('.card-select');if(input)input.checked=has;}
    const count=q('.selection-count');if(count){const visible=matched.filter(c=>selected.has(c.id)).length;count.textContent=`已选 ${selected.size} 张${selected.size>visible?` · 筛选外 ${selected.size-visible} 张`:''}`;}
    const selectedCards=getCards().filter(c=>selected.has(c.id));
    for(const b of root.querySelectorAll('[data-bulk-action]'))b.disabled=!selectedCards.some(c=>b.dataset.bulkAction==='add'?!c.owned:c.owned);
    const all=q('[data-scope-select]');if(all){all.dataset.complete=String(matched.length>0&&matched.every(c=>selected.has(c.id)));all.setAttribute('aria-pressed',all.dataset.complete);}
    for(const b of root.querySelectorAll('[data-group-select]')){b.dataset.complete=String(groups.find(g=>g.key===b.dataset.groupSelect)?.cards.every(c=>selected.has(c.id)));b.setAttribute('aria-pressed',b.dataset.complete);}
  }
  return {refresh,paintSelection,filteredIds:()=>matched.map(c=>c.id),destroy(){observer?.disconnect();clearTimeout(timer);}};
}
