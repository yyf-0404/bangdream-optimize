import { emptyMessage } from '../ui/dom.js?v=3';
import { difficultyLabel,bandLabel } from '../utils.js?v=3';
import { assetImage } from '../assets/index.js';
import { bandNames, bandOrder, serverNames } from '../ui/cards/rules.js';
import { compareSongRelease, compareEventSongOrder } from '../ui/song-rules.js';
const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};
const btn=(text,fn,cls='')=>{const b=el('button',cls,text);b.type='button';b.onclick=fn;return b;};
export function createSongView({rows,selectedEventId,editableEventSnapshot,getSongRecord,normalizedActivityMode,fixedSongListForMode,eventSongsFromPreset,updateSong,getCore,getProfileId,readPlayer,writePlayer,songLabel,songCoverUrls}){
 let activeDialog;
 function current(player){const eventId=selectedEventId(player),event=editableEventSnapshot(eventId,player);return {eventId,event,songs:fixedSongListForMode(player.eventSongs[String(eventId)],normalizedActivityMode(player.activityMode),event)};}
 function renderSongs(player){
  rows.replaceChildren();rows.className='bo-songs';let selection;try{selection=current(player);}catch{rows.append(emptyMessage('未设置活动','song-error'));return;}
  const {songs,eventId}=selection;
  songs.forEach((song,index)=>{const row=el('div','bo-song');row.append(el('span','bo-song-index',String(index+1).padStart(2,'0')));const choose=btn('',()=>openPicker(index),'song-entry');const image=song.songId?assetImage(songCoverUrls(song.songId),'song-thumb',songLabel(song.songId)):null;choose.append(image||el('span','song-thumb song-placeholder','♪'));const name=el('span');name.append(el('strong','',song.songId?songLabel(song.songId):'选择歌曲'),el('small','',song.songId?'#'+song.songId:'点击选择本曲'));choose.append(name);row.append(choose);
   const diff=renderDifficultyList(getSongRecord(song.songId),song.difficulty,d=>updateSong(eventId,index,{difficulty:d}));diff.classList.add('song-difficulty-group');row.append(diff);rows.append(row);
  });
  const tools=el('div','song-section-tools');tools.append(el('p','','难度切换只影响当前曲位。'),btn('选择活动歌曲',()=>openPicker(0),'song-action'));if(eventSongsFromPreset(selection.event).length)tools.append(btn('恢复活动曲',restorePreset,'song-action'));rows.append(tools);
 }
 function restorePreset(){
  const player=readPlayer(),{event,eventId}=current(player),preset=eventSongsFromPreset(event);
  if(!preset.length)return;
  player.eventSongs[String(eventId)]=fixedSongListForMode(preset,normalizedActivityMode(player.activityMode),event);
  writePlayer(player);renderSongs(player);
 }
 function cover(id){const host=el('span','sp-jacket','♪');const image=id?assetImage(songCoverUrls(id),'song-thumb',''):null;if(image)host.append(image);return host;}
 function openPicker(slot){
  activeDialog?.close();const origin=getProfileId(),player=readPlayer(),{event,eventId,songs}=current(player),draft=structuredClone(songs);let active=slot,query='',band='all',scope='all',page=1;
  const dialog=el('dialog');dialog.id='song-picker';dialog.setAttribute('aria-labelledby','song-picker-title');dialog.innerHTML='<header class="sp-header"><div><p class="sp-eyebrow">编辑活动曲位</p><h2 id="song-picker-title">选择活动歌曲</h2></div><button type="button" class="sp-close" aria-label="关闭歌曲选择">×</button></header><div class="sp-slots" role="group" aria-label="编辑曲位"></div><div class="sp-current"></div><div class="sp-filters"><div class="sp-search"><input type="search" placeholder="搜索歌名 / ID" aria-label="搜索歌曲名称或 ID"><select aria-label="歌曲演出乐队"></select></div><div class="sp-filter-row" role="group" aria-label="歌曲范围"></div></div><div class="sp-list" aria-label="歌曲列表"></div><footer class="sp-footer"><p></p><button type="button" data-cancel>取消</button><button type="button" class="sp-apply">应用选择</button></footer>';
  const q=s=>dialog.querySelector(s),close=()=>dialog.close();q('.sp-close').onclick=close;q('[data-cancel]').onclick=close;dialog.onclose=()=>{dialog.remove();if(activeDialog===dialog)activeDialog=null;};
  const original=JSON.stringify(draft),records=Object.entries(getCore().songs||{}).map(([id,s])=>({id:Number(id),record:s})).sort((a,b)=>compareSongRelease(a,b,player.server)),presets=new Set(eventSongsFromPreset(event).map(s=>Number(s.songId??s)));
  scope=presets.size?'preset':'all';q('.sp-eyebrow').textContent=(serverNames[player.server]||player.server)+' · '+(draft.length===3?'巡回演出 · 三曲连续选择':'本次活动歌曲');
  const select=q('select');select.add(new Option('全部乐队','all'));for(const id of bandOrder.filter(id=>records.some(s=>Number(s.record.bandId)===id)))select.add(new Option(bandNames[id],String(id)));select.add(new Option('其他 / 联合','other'));select.onchange=()=>{band=select.value;page=1;list();};
  q('input').oninput=e=>{query=e.target.value.toLowerCase();page=1;list();};
  const filters=q('.sp-filter-row');for(const [id,title]of[['all','全部歌曲'],['preset','活动曲'],['selected','本次已选']]){const b=btn(title,()=>{scope=id;page=1;list();});b.dataset.scope=id;filters.append(b);}filters.append(el('span','sp-count'));
  function renderDraft(){
   const slots=q('.sp-slots');slots.replaceChildren();for(const [i,s]of draft.entries()){const b=btn('',()=>{active=i;renderDraft();list();},'sp-slot');b.setAttribute('aria-pressed',String(active===i));b.append(cover(s.songId));const name=el('span');name.append(el('small','',`第 ${i+1} 首${active===i?' · 当前':''}`),el('strong','',s.songId?songLabel(s.songId):'待选择'));b.append(name);slots.append(b);}
   const selected=draft[active],host=q('.sp-current');host.replaceChildren(el('span','sp-current-label',`第 ${active+1} 曲难度`));if(selected.songId){host.append(renderDifficultyList(getSongRecord(selected.songId),selected.difficulty,d=>{selected.difficulty=d;renderDraft();list();}));const remove=btn('移除本曲',()=>{draft[active]={songId:0,difficulty:3};renderDraft();list();},'sp-remove');host.append(remove);}else host.append(el('span','sp-empty-current','先选择歌曲，再设置难度。'));
   q('.sp-footer p').textContent=`已选 ${draft.filter(s=>s.songId).length} / ${draft.length} 首${draft.some(s=>!s.songId)?' · 可先保存，计算前需补齐':''} · 应用后生效`;q('.sp-apply').disabled=JSON.stringify(draft)===original;
  }
  function list(){
   const ids=new Set(draft.map(s=>s.songId)),matches=records.filter(s=>(band==='all'||(band==='other'?!bandNames[s.record.bandId]:String(s.record.bandId)===band))&&(scope!=='preset'||presets.has(s.id))&&(scope!=='selected'||ids.has(s.id))&&(!query||[s.id,...(s.record.musicTitle||[]),...(s.record.name||[]),songLabel(s.id)].join(' ').toLowerCase().includes(query)));
   if(scope==='preset')matches.sort((a,b)=>compareEventSongOrder(a,b,[...presets]));
   q('.sp-count').textContent=matches.length+(scope==='preset'?' 首 · 活动顺序':' 首 · 新到旧');for(const b of filters.querySelectorAll('[data-scope]'))b.setAttribute('aria-pressed',String(b.dataset.scope===scope));const host=q('.sp-list');host.replaceChildren();
   for(const s of matches.slice((page-1)*40,page*40)){const b=btn('',()=>{const difficulties=Object.keys(s.record.difficulty||{}).map(Number);draft[active]={songId:s.id,difficulty:difficulties.includes(draft[active].difficulty)?draft[active].difficulty:difficulties.includes(3)?3:difficulties.at(-1)||0};renderDraft();list();},'sp-row');b.setAttribute('aria-pressed',String(draft[active].songId===s.id));b.append(cover(s.id));const name=el('span','sp-row-main');name.append(el('span','sp-row-title',songLabel(s.id)),el('span','sp-row-meta',`#${s.id} · ${bandNames[s.record.bandId]||'其他 / 联合演出'}`));b.append(name);const levels=renderDifficultyList(s.record,draft[active].songId===s.id?draft[active].difficulty:null);levels.classList.add('sp-levels');b.append(levels);host.append(b);}
   const pages=Math.ceil(matches.length/40);if(pages>1){const pager=el('div','sp-pagination'),prev=btn('上一页',()=>{page--;list();host.scrollTop=0;}),next=btn('下一页',()=>{page++;list();host.scrollTop=0;});prev.disabled=page===1;next.disabled=page===pages;pager.append(prev,el('span','',`${page} / ${pages}`),next);host.append(pager);}if(!matches.length)host.append(el('div','sp-empty','没有找到匹配歌曲，试试其他名称、ID 或歌曲范围。'));
  }
  q('.sp-apply').onclick=()=>{if(origin!==getProfileId()||JSON.stringify(draft)===original)return;const latest=readPlayer();latest.eventSongs[String(eventId)]=draft;writePlayer(latest);renderSongs(latest);dialog.close();};
  document.querySelector('#aurora-soft-study').append(dialog);activeDialog=dialog;renderDraft();list();dialog.showModal();q('input').focus();
 }
 return {renderSongs};
}

export function renderDifficultyList(songRecord, selectedDifficulty, onSelect) {
  const list = document.createElement('div');
  list.className = 'activity-song-difficulty-list';

  const entries = Object.entries(songRecord?.difficulty ?? {})
    .map(([difficulty, detail]) => ({
      difficulty: Number.parseInt(difficulty, 10),
      level: Number.parseInt(detail?.playLevel, 10),
    }))
    .filter((entry) =>
      Number.isInteger(entry.difficulty)
      && entry.difficulty >= 0
      && Number.isInteger(entry.level),
    )
    .sort((left, right) => left.difficulty - right.difficulty);

  if (selectedDifficulty != null && !entries.some(entry => selectedDifficulty != null && entry.difficulty === Number(selectedDifficulty))) {
    const missing = emptyMessage(`已选 ${difficultyLabel(Number(selectedDifficulty))}（${selectedDifficulty}）资料暂缺，请重新选择难度`, 'activity-song-difficulty-empty', 'span');
    missing.setAttribute('role', 'status');list.append(missing);
  }

  for (const entry of entries) {
    const interactive = typeof onSelect === 'function';
    const item = document.createElement(interactive ? 'button' : 'span');
    if (interactive) {
      item.type = 'button';
      item.addEventListener('click', () => onSelect(entry.difficulty));
    }
    item.className = `activity-song-difficulty difficulty-${entry.difficulty} difficulty-badge preview-difficulty`;
    item.dataset.levelId=String(entry.difficulty);
    item.setAttribute('style',`--diff:${['#8eb4fd','#72c960','#d6a921','#ff6d70','#e35fb0'][entry.difficulty]};--diff-ink:${['#456dae','#39753b','#876411','#b03e50','#a33c7d'][entry.difficulty]}`);
    item.classList.toggle('is-selected', selectedDifficulty != null && entry.difficulty === Number(selectedDifficulty));
    item.setAttribute(
      'aria-label',
      `${difficultyLabel(entry.difficulty)} ${entry.level}`,
    );
    item.title = `${difficultyLabel(entry.difficulty)} ${entry.level}`;
    item.dataset.selected = String(selectedDifficulty != null && entry.difficulty === Number(selectedDifficulty));
    if (interactive) item.setAttribute('aria-pressed', item.dataset.selected);
    const name = document.createElement('span'); name.className = 'difficulty-name'; name.textContent = ['EZ','NM','HD','EX','SP'][entry.difficulty] || '?';
    const number = document.createElement('span'); number.className = 'difficulty-number'; number.textContent = String(entry.level);
    item.append(name, number);
    list.append(item);
  }
  return list;
}
