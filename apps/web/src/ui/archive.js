import {confirmDialog} from './confirm.js';
import {designIcon} from './fidelity.js';
import {assetImage,cardIconUrls,cardArtUrls,serverIconUrls,characterIconUrls} from '../assets/index.js';
import {itemArtUrls} from '../views/player-library.js';
import {cardModel} from './cards/model.js';
import {icon} from './shell.js';
import {designFragment} from './approved/templates.js';
import {hydrateDesignIcons} from './design-controls.js';

const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};
const customCount=p=>Object.keys(p.customCards||{}).length;
const cardSummary=p=>`${Object.keys(p.cardList||{}).length.toLocaleString()} 张卡牌${customCount(p)?' · 自定义 '+customCount(p):''}`;
export function createArchiveUI({state,elements,readPlayer,writePlayer,refreshProfiles,renderForms,setError,setStatus}){
  const portal=el('div');portal.id='archive-dialog-host';portal.dataset.archiveSkin='refined';document.body.append(portal);
  const manager=document.querySelector('.config-manager-panel');
  manager.querySelector('.config-toolbar-primary').hidden=true;
  const layout=el('div','profile-layout'),directory=designFragment('archive-directory'),detail=el('div','profile-detail');detail.id='profile-detail';
  directory.querySelector('#profile-count').dataset.count='';directory.querySelector('#profile-list').className='profile-list';directory.querySelector('input').type='search';directory.querySelector('input').setAttribute('aria-label','搜索档案');hydrateDesignIcons(directory);
  const newProfile=el('button');newProfile.type='button';newProfile.dataset.create='';directory.append(newProfile);
  detail.innerHTML='<header class="detail-heading"><h2></h2><span class="detail-state"></span><button type="button" class="primary archive-use-profile" data-use>切换到此档案 →</button></header><section class="archive-overview" aria-label="养成概览"></section><div class="profile-form"><h3 class="archive-section-title archive-form-title">'+designIcon('activity')+'基本资料</h3><div class="form-row"><label class="field"><span>档案名称</span><input name="name" maxlength="80" required></label><label class="field"><span>玩家 ID <small>（选填）</small></span><input name="playerId" type="number" min="0" step="1" placeholder="用于从游戏账号导入"></label></div><fieldset class="server-field"><legend>对应服务器</legend><div class="server-choices"></div></fieldset><p class="server-hint">卡牌列表默认使用此服务器，并只显示已持有的卡牌。</p><div class="form-save-row archive-save"><button type="button" class="secondary" data-save>保存资料</button><small role="status"></small></div></div>';
  const profileForm=designFragment('archive-form');profileForm.removeAttribute('id');
  profileForm.querySelector('#profile-name').name='name';profileForm.querySelector('#player-id').name='playerId';profileForm.querySelector('#player-id').type='number';profileForm.querySelector('#player-id').min='0';profileForm.querySelector('#player-id').step='1';
  profileForm.querySelector('#save-profile').type='button';profileForm.querySelector('#save-profile').dataset.save='';profileForm.querySelector('.form-save-row').classList.add('archive-save');
  profileForm.insertAdjacentHTML('afterbegin','<h3 class="archive-section-title archive-form-title">'+designIcon('activity')+'基本资料</h3>');
  for(const input of profileForm.querySelectorAll('[id]')){input.dataset.designId=input.id;input.removeAttribute('id');input.removeAttribute('aria-describedby');}
  detail.querySelector('.profile-form').replaceWith(profileForm);
  for(const [code,name]of Object.entries({cn:'国服',jp:'日服',en:'国际服',tw:'台服',kr:'韩服'})){const label=el('label'),input=el('input');input.type='radio';input.name='archive-server';input.value=code;const span=el('span');span.append(assetImage(serverIconUrls(code),'',name)||el('span'),document.createTextNode(name));label.append(input,span);detail.querySelector('.server-choices').append(label);}

  layout.append(directory,detail);manager.prepend(layout);manager.querySelector(':scope>.panel-header')?.remove();
  const actions=manager.querySelector('.config-toolbar-actions');actions.classList.add('profile-actions');detail.append(actions);
  for(const [id,heading]of[['bestdori-profile-dialog','导入配置'],['export-profile-dialog','导出配置']]){
    const dialog=document.getElementById(id),content=dialog.querySelector('.bestdori-profile-dialog-content'),area=dialog.querySelector('textarea'),buttons=dialog.querySelector('.bestdori-profile-dialog-actions');document.querySelector('#archive-design').append(dialog);dialog.classList.add('flow-dialog');content.replaceChildren();const header=el('header','profile-flow-head');header.innerHTML='<div><p>档案管理</p><h2>'+heading+'</h2></div>';const body=el('div','flow-body');body.append(el('p','intro',id==='bestdori-profile-dialog'?'粘贴 Base64 配置或 Bestdori Profile，读取后先核对变化。':'选择导出格式。Base64 保留完整配置，Bestdori Profile 用于交换对应的游戏资料。'));area.setAttribute('aria-label',id==='bestdori-profile-dialog'?'粘贴配置内容':'导出配置内容');body.append(area);buttons.className='dialog-bottom';content.append(header,body,buttons);
    if(id==='bestdori-profile-dialog'){const account=el('div','import-account');account.append(el('span','','也可以根据所选档案的服务器与玩家 ID，通过 Bestdori 读取主乐队公开资料。'),elements.importMainBand);body.append(account);}
  }
  const guide=designFragment('archive-import-guide');hydrateDesignIcons(guide);guide.querySelector('button').type='button';guide.querySelector('button').onclick=()=>elements.openBestdoriProfileDialog.click();manager.append(guide);
  let selectedId,shownActive,sequence=0,draft,dirty=false,lastSource='',search='',profileInfo=new Map();
  const list=directory.querySelector('.profile-list'),q=s=>detail.querySelector(s);
  const create=directory.querySelector('[data-create]');create.textContent='＋ 新建档案';create.className='secondary archive-create';document.querySelector('#archive-design .page-hero').append(create);create.onclick=()=>elements.newPlayerProfile.click();elements.newPlayerProfile.hidden=true;
  directory.querySelector('input').oninput=e=>{search=e.target.value;paintDirectory();};
  detail.querySelectorAll('input,select').forEach(n=>n.oninput=()=>{dirty=true;q('[data-save]').disabled=false;q('.archive-save small').textContent='有未保存的资料';});
  function paintDirectory(){
    list.replaceChildren();directory.querySelector('[data-count]').textContent=state.playerProfiles?.length||0;
    for(const p of state.playerProfiles||[]){
      if(!`${p.name} ${profileInfo.get(p.id)?.playerId||''}`.toLowerCase().includes(search.toLowerCase()))continue;
      const b=el('button','profile-entry');b.type='button';b.setAttribute('aria-pressed',String(p.id===selectedId));
      const info=profileInfo.get(p.id),name=el('strong','',p.name);if(p.id===state.activePlayerProfileId)name.append(el('span','current','使用中'));if(info)b.append(assetImage(serverIconUrls(info.server),'server-flag','')||el('span'));b.append(name,el('small','',info?`${{cn:'国服',jp:'日服',en:'国际服',tw:'台服',kr:'韩服'}[info.server]} · ${cardSummary(info)}`:'读取档案资料…'));
      if(!profileInfo.has(p.id)){profileInfo.set(p.id,null);state.runtime.loadPlayerConfig(p.id).then(data=>{profileInfo.set(p.id,data);paintDirectory();}).catch(()=>{});}
      b.onclick=()=>show(p.id);list.append(b);
    }
    if(!list.children.length)list.append(el('p','muted','没有匹配的档案'));
  }
  async function show(id){
    if(dirty&&!await confirmDialog({title:'放弃尚未保存的修改？',lines:['档案资料尚未保存。放弃后查看另一份档案。'],confirmText:'放弃修改'}))return;
    const token=++sequence;selectedId=id;state.viewedPlayerProfileId=id;dirty=false;lastSource='';paintDirectory();q('.archive-save small').textContent='读取档案…';
    try{const player=id===state.activePlayerProfileId?readPlayer():await state.runtime.loadPlayerConfig(id);if(token!==sequence)return;draft=structuredClone(player);paint();}
    catch(error){if(token===sequence){q('.archive-save small').textContent='读取失败，请重新选择重试';setError(error);}}
  }
  function paint(){
    if(!draft)return;
    const profile=state.playerProfiles.find(p=>p.id===selectedId),active=selectedId===state.activePlayerProfileId;
    q('h2').textContent=profile?.name||'玩家档案';const serverImage=assetImage(serverIconUrls(draft.server),'archive-server-image',draft.server);if(serverImage)q('h2').prepend(serverImage);q('.detail-state').textContent=active?'正在使用':'查看中';q('[data-use]').hidden=active;
    q('[name=name]').value=profile?.name||'';q('[name=playerId]').value=draft.playerId||0;q(`[name=archive-server][value=${draft.server||'cn'}]`).checked=true;q('.archive-save small').textContent='';
    actions.hidden=false;
    q('[data-save]').disabled=!dirty;
    const overview=q('.archive-overview');overview.replaceChildren();const title=el('h3','archive-section-title archive-overview-title');title.innerHTML=designIcon('cards')+'养成概览';overview.append(title);
    const cards=Object.keys(draft.cardList||{}),items=Object.entries(draft.areaItem||{}).filter(([,c])=>c.level>0),characters=Object.entries(draft.characterBouns||{}).filter(([,c])=>Object.values(c.potential||{}).some(v=>v>0)||Object.values(c.characterTask||{}).some(v=>v>0));
    const strip=el('div','inventory-strip');for(const [label,count,kind,unit]of[['已持有卡牌',cards.length,'cards','张'],['已配置道具',items.length,'items','/ '+Object.keys(state.core?.areaItems||{}).length],['有加成的角色',characters.length,'characters','/ '+Object.keys(state.core?.characters||{}).length]]){const stat=el('div','archive-stat');stat.dataset.kind=kind;const content=el('div','archive-stat-content');content.append(el('strong','',count.toLocaleString()),el('span','',unit),el('p','',label));if(kind!=='cards'){const art=el('div','archive-stat-art');const img=kind==='items'&&items.length?assetImage(itemArtUrls(items[0][0],draft.server),'',''):kind==='characters'&&characters.length?assetImage(characterIconUrls(characters[0][0]),'',''):null;if(img)art.append(img);else art.innerHTML=designIcon(kind==='items'?'equipment':'player');stat.append(art);}stat.append(content);strip.append(stat);}overview.append(strip);if(customCount(draft)){const summary=el('p','muted',`自定义卡牌 ${customCount(draft)} 张 · 已启用 ${Object.values(draft.customCards).filter(c=>c.enabled).length} 张`);overview.append(summary);}
    if(cards.length){const samples=el('div','sample-cards'),images=el('div','archive-card-images');for(const id of cards.slice(-5).reverse()){const model=cardModel(state.core,draft,id),asset={cardId:Number(id),card:model.record,illustTrainingStatus:model.growth.illustTrained};const img=assetImage([...cardIconUrls(asset),...cardArtUrls(asset)],'',model.name);if(img)images.append(img);}samples.append(images,el('span','','最近持有的卡牌'));overview.append(samples);}else{const empty=el('p','empty-profile',customCount(draft)?'游戏卡牌列表为空；自定义卡牌在独立页签中管理。':'还没有卡牌，可以导入已有资料，也可以手动选择卡牌。');overview.append(empty);}
    profileInfo.set(selectedId,draft);paintDirectory();
    lastSource=JSON.stringify(draft);
  }
  q('[data-use]').onclick=()=>{elements.playerProfile.value=selectedId;elements.playerProfile.dispatchEvent(new Event('change',{bubbles:true}));};
  q('[data-save]').onclick=async()=>{
    const id=selectedId,token=sequence,name=q('[name=name]').value.trim(),playerId=q('[name=playerId]'),server=q('[name=archive-server]:checked').value;
    if(!name||!playerId.reportValidity())return;
    q('[data-save]').disabled=true;
    try{
      const latest=id===state.activePlayerProfileId?readPlayer():await state.runtime.loadPlayerConfig(id);
      const next={...latest,playerId:Number(playerId.value)||0,server};
      await state.runtime.savePlayerConfig(next,id);await state.runtime.renamePlayerConfig(id,name);
      if(id===state.activePlayerProfileId)writePlayer(next,{autosave:false});
      await refreshProfiles();if(token===sequence){draft=next;dirty=false;paint();q('.archive-save small').textContent='已保存';}
      renderForms(readPlayer());setStatus('档案资料已保存');
    }catch(error){q('.archive-save small').textContent='保存失败，输入已保留，可重试';setError(error);}finally{q('[data-save]').disabled=!dirty;}
  };
  function render(){
    renderResources();
    const active=state.activePlayerProfileId;if(!active)return;
    if(shownActive!==active||!state.playerProfiles.some(p=>p.id===selectedId)){shownActive=active;void show(active);return;}
    paintDirectory();
    if(selectedId===active&&!dirty){const next=readPlayer();if(JSON.stringify(next)!==lastSource){draft=next;paint();}}
  }
  document.addEventListener('profile-content-changed',e=>{profileInfo.delete(e.detail.id);if(selectedId===e.detail.id)void show(selectedId);});
  document.querySelector('#workspace-profile').onclick=()=>{
    const dialog=el('dialog','flow-dialog');dialog.setAttribute('aria-label','切换档案');dialog.innerHTML='<header class="profile-flow-head"><div><p>随时切换，继续当前工作</p><h2>切换档案</h2></div><button type="button" aria-label="关闭档案切换">×</button></header><div class="flow-body"><div class="switch-list"></div><p class="dialog-note">切换前保存当前档案；卡牌筛选随目标档案更新。</p></div><footer class="dialog-bottom"><button type="button" class="text-button" data-manage>管理档案</button><button type="button" class="text-button accent" data-new>新建档案</button></footer>';
    for(const profile of state.playerProfiles){const b=el('button','switch-item'),copy=el('span'),info=profileInfo.get(profile.id);b.type='button';if(info)b.append(assetImage(serverIconUrls(info.server),'',''));copy.append(el('b','',profile.name),el('small','',info?`${cardSummary(info)}`:''));b.append(copy);if(profile.id===state.activePlayerProfileId)b.append(el('span','','✓'));b.onclick=()=>{elements.playerProfile.value=profile.id;elements.playerProfile.dispatchEvent(new Event('change',{bubbles:true}));dialog.close();};dialog.querySelector('.switch-list').append(b);}
    dialog.querySelector('header button').onclick=()=>dialog.close();dialog.querySelector('[data-manage]').onclick=()=>{dialog.close();document.querySelector('[data-page=archive]').click();};dialog.querySelector('[data-new]').onclick=()=>{dialog.close();document.querySelector('[data-page=archive]').click();elements.newPlayerProfile.click();};dialog.onclose=()=>dialog.remove();portal.append(dialog);dialog.showModal();
  };
  const page=document.querySelector('#archive-design'),resources=document.querySelector('.archive-resources');
  resources.querySelector('.panel-header')?.remove();resources.querySelector(':scope>.muted')?.remove();
  const block=el('section','resource-block');block.innerHTML='<header class="resource-title">'+designIcon('equipment')+'<h2>游戏资源</h2><span class="environment-label"></span></header>';
  const resourceRow=(parent,title,description,button,cls='')=>{const row=el('div','resource-row '+cls),copy=el('div');copy.append(el('h3','',title),el('p','',description));row.append(copy);if(button){button.classList.add('text-button');row.append(button);}parent.append(row);return copy;};
  const coreInfo=resourceRow(block,'游戏数据','卡牌、角色、区域道具、活动和歌曲资料。',elements.refreshCoreGameData);coreInfo.querySelector('p').className='archive-resource-description';const kinds=el('div','archive-resource-kinds'),meta=el('div','resource-meta');meta.innerHTML='<span class="status" role="status"></span><span>来源：Bestdori</span>';coreInfo.append(kinds,meta);
  resourceRow(block,'全量资源','预先拉取完整游戏资源，完成后重新构建计算数据。',elements.syncAllGameData,'desktop-resource');
  const risk=el('details','risk-details');risk.innerHTML='<summary>缓存与清理</summary>';resourceRow(risk,'游戏数据缓存','重新加载游戏数据，保留玩家档案与计算历史；新计算不复用旧数据的结果。',elements.clearGameCache);resourceRow(risk,'玩家档案缓存','删除本地全部玩家档案和计算历史，并恢复默认档案。请先导出档案及需要保留的结果。',elements.clearLocalCache,'desktop-resource');block.append(risk);
  const download=elements.openDesktopDownloads;
  download.className='side-help side-download';
  download.innerHTML=icon('M12 3v12m-4-4 4 4 4-4M4 15v6h16v-6')+'<span>下载桌面端</span><span class="help-arrow">'+designIcon('chevron')+'</span>';
  document.querySelector('#open-feedback').before(download);
  // Shared entries must open outside page panels, including when resources are hidden.
  document.body.append(elements.desktopDownloadsDialog);
  const app=el('section','resource-block archive-app-resources');app.innerHTML='<header class="resource-title">'+designIcon('archive')+'<h2>应用与支持</h2></header>';resourceRow(app,'反馈与问题报告','遇到问题时可以附上配置与计算诊断。',resources.querySelector('.archive-feedback'));
  resources.prepend(block,app);resources.querySelector('.topbar-actions')?.remove();
  const log=page.querySelector('.log-panel');if(log){const disclosure=el('details','risk-details');disclosure.innerHTML='<summary>运行日志</summary>';disclosure.append(log);app.append(disclosure);}
  function renderResources(){
    block.querySelector('.environment-label').textContent=elements.refreshCoreGameData.hidden?'网页端':'桌面端';kinds.replaceChildren();for(const [key,label,mark]of[['cards','卡牌','cards'],['characters','角色','player'],['areaItems','道具','equipment'],['events','活动','activity'],['songs','歌曲','song']]){const span=el('span');span.innerHTML=designIcon(mark);span.append(document.createTextNode(`${label} ${Object.keys(state.core?.[key]||{}).length.toLocaleString()}`));kinds.append(span);}const status=meta.querySelector('.status');status.textContent=state.core?'资源可用':'等待加载';status.dataset.state=state.core?'ready':'pending';
  }
  const nav=el('nav','section-nav');nav.setAttribute('aria-label','档案与数据分区');for(const [key,title,iconName]of[['profiles','玩家档案','archive'],['resources','资源管理','equipment']]){const b=el('button','text-button');b.type='button';b.innerHTML=designIcon(iconName)+title;b.dataset.archiveTab=key;b.onclick=()=>{manager.hidden=key!=='profiles';resources.hidden=key!=='resources';for(const tab of nav.children)tab.setAttribute('aria-selected',String(tab===b));};nav.append(b);}page.querySelector('.page-hero').after(nav);nav.children[0].setAttribute('aria-selected','true');resources.hidden=true;
  return {render,selectedProfileId:()=>selectedId};
}
