import {assetImage,cardArtUrls,cardIconUrls,cardTrainingStatusList} from '../../shared/assets.js';
import {allCardModels} from '../../components/cards/model.js';
import {imageSources} from '../../components/cards/images.js';
import {comparator,normalize,resolveCardCover,cardReleaseOrder} from '../../components/cards/rules.js';
import {profilePreference,saveProfilePreference} from '../../shared/preferences.js';
import {icon} from '../../shared/ui/icons.js';

// The cover has its own draft: browsing artwork never changes card training.
export function openCoverPicker({host,getCore,getPlayer,getProfileId,onApply,opener}) {
  const profile=getProfileId(),player=getPlayer();
  const allCards=allCardModels(getCore(),player,profile);
  const cards=allCards.filter(c=>c.owned&&!c.unknown).sort(comparator('release-desc',undefined,cardReleaseOrder(allCards)));
  let draft={...resolveCardCover(allCards,player.server,profilePreference(profile,'cover',{}))},search='',limit=48;
  let source=draft.card?.custom?'custom':'game';
  draft={mode:draft.mode,cardId:draft.card?.id||null,variant:draft.variant};
  const dialog=document.createElement('dialog');dialog.id='cover-dialog';dialog.setAttribute('aria-labelledby','cover-dialog-title');
  dialog.innerHTML=`<div class="editor-header"><h3 id="cover-dialog-title">卡牌封面</h3><button type="button" class="icon-button" aria-label="关闭封面设置">${icon('M6 6l12 12M18 6 6 18')}</button></div><div class="cover-body"><div id="cover-sample" class="page-hero" data-hero="cards"><div class="hero-visual" aria-hidden="true"><div class="hero-cover-scene"></div><div class="hero-cover-frame"></div></div><div class="hero-copy"><h2>我的卡牌</h2><p>封面效果预览</p></div></div><div class="cover-caption"><strong id="cover-card-label"></strong><span id="cover-art-status" role="status"></span></div><fieldset class="cover-options"><legend>选择方式</legend><label><input type="radio" name="cover-mode" value="auto">持有中最新</label><label><input type="radio" name="cover-mode" value="manual">手动选择</label></fieldset><p id="cover-rule" class="cover-hint">自动优先选择最新持有的游戏卡牌；没有时使用自定义卡牌，跳过 2 星卡。</p><fieldset class="cover-options" data-cover-variant><legend>使用原画</legend><label><input type="radio" name="cover-variant" value="after_training">特训后</label><label><input type="radio" name="cover-variant" value="normal">特训前</label><span id="cover-variant-hint" class="cover-hint"></span></fieldset><section id="cover-picker" aria-label="从已持有卡牌选择封面"><div class="card-source-tabs" role="tablist" aria-label="封面卡牌来源"><button type="button" role="tab" id="cover-game-tab" aria-controls="cover-card-list" aria-selected="true">游戏卡牌</button><button type="button" role="tab" id="cover-custom-tab" aria-controls="cover-card-list" aria-selected="false" tabindex="-1">自定义卡牌</button></div><div id="cover-card-list" role="tabpanel" aria-labelledby="cover-game-tab"><label class="search"><input type="search" placeholder="搜索已持有卡牌的角色、名称或 ID" aria-label="搜索封面卡牌"></label><p id="cover-grid-count" class="cover-hint" role="status"></p><div class="cover-grid"></div><button id="cover-more" type="button" class="text-button">加载更多卡牌</button></div></section></div><div class="editor-footer"><span>仅用于当前档案</span><div class="cover-footer-actions"><button type="button" class="text-button" data-cancel>取消</button><button type="button" class="primary" data-apply>应用封面</button></div></div>`;
  const q=s=>dialog.querySelector(s);
  function preview(){
    const selected=resolveCardCover(allCards,player.server,draft),card=selected.card;
    q('#cover-picker').hidden=draft.mode!=='manual';q('#cover-rule').hidden=draft.mode!=='auto';
    for(const n of dialog.querySelectorAll('[name=cover-mode]'))n.checked=n.value===draft.mode;
    for(const n of dialog.querySelectorAll('[name=cover-variant]'))n.checked=n.value===draft.variant;
    const statuses=card&&!card.custom?cardTrainingStatusList(card.record):[];
    q('[data-cover-variant]').hidden=!!card?.custom;
    q('#cover-variant-hint').textContent=card&&statuses.length===1?`此卡只有特训${statuses[0]?'后':'前'}图，自动使用可用原画。`:'';
    q('#cover-card-label').textContent=card?`${card.name} · ${card.title} · ${card.displayId||'#'+card.id}`:'暂无持有卡牌';
    const status=q('#cover-art-status');status.textContent=card?.custom?'自定义卡牌图片':card?`特训${draft.variant==='normal'?'前':'后'}原画`:'';
    const image=card?assetImage(card.custom?imageSources(card):cardArtUrls({card:card.record,illustTrainingStatus:draft.variant!=='normal'}),'',''):null;
    q('.hero-cover-scene').replaceChildren(...(image?[image]:[]));
    if(image){image.loading='eager';image.addEventListener('load',()=>{if(!image.isConnected||card.custom)return;status.textContent=image.src.includes('after_training')?'特训后原画':'特训前原画';});image.addEventListener('error',()=>{if(image.hidden&&image.isConnected)status.textContent='原画暂不可用，选择已保留。';});}
    q('[data-apply]').disabled=!card;
    for(const n of dialog.querySelectorAll('[data-cover-card]'))n.setAttribute('aria-pressed',String(Number(n.dataset.coverCard)===card?.id));
  }
  function grid(){
    const matched=cards.filter(c=>!!c.custom===(source==='custom')&&(!search||normalize(`${c.id} ${c.name} ${c.title} ${c.searchText||''}`).includes(normalize(search))));
    q('.cover-grid').replaceChildren(...matched.slice(0,limit).map(c=>{
      const button=document.createElement('button');button.type='button';button.className='cover-choice';button.dataset.coverCard=c.id;
      button.setAttribute('aria-label',`${c.name}，${c.title}，${c.displayId||'ID '+c.id}，${c.rarity} 星`);
      const art=document.createElement('span');art.className='art-wrap';const image=assetImage(c.custom?imageSources(c):cardIconUrls({cardId:c.id,card:c.record,illustTrainingStatus:draft.variant!=='normal'}),'',c.name);if(image)art.append(image);
      const name=document.createElement('span');name.className='cover-choice-name';name.textContent=c.custom?`${c.displayId} · ${c.title}`:c.name;button.append(art,name);
      button.onclick=()=>{draft.cardId=c.id;preview();};return button;
    }));
    q('#cover-grid-count').textContent=source==='custom'&&!matched.length&&!search?'暂无自定义卡牌，可在“我的卡牌”中创建。':`${matched.length} 张${source==='custom'?'自定义':'已持有游戏'}卡牌 · 已显示 ${Math.min(limit,matched.length)} 张`;
    q('#cover-more').hidden=limit>=matched.length;preview();
  }
  const tabs=[...dialog.querySelectorAll('[role=tab]')];
  const selectSource=(next,focus=false)=>{
    source=next;search='';limit=48;q('input[type=search]').value='';
    for(const tab of tabs){const active=tab.id===`cover-${next}-tab`;tab.setAttribute('aria-selected',String(active));tab.tabIndex=active?0:-1;if(active&&focus)tab.focus();}
    q('[role=tabpanel]').setAttribute('aria-labelledby',`cover-${next}-tab`);
    grid();
  };
  for(const tab of tabs){
    tab.onclick=()=>selectSource(tab.id==='cover-custom-tab'?'custom':'game');
    tab.onkeydown=e=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(e.key))return;e.preventDefault();selectSource(e.key==='Home'?'game':e.key==='End'?'custom':source==='game'?'custom':'game',true);};
  }
  dialog.addEventListener('change',e=>{if(e.target.name==='cover-mode')draft.mode=e.target.value;if(e.target.name==='cover-variant')draft.variant=e.target.value;grid();});
  q('input[type=search]').oninput=e=>{search=e.target.value;limit=48;grid();};q('#cover-more').onclick=()=>{limit+=48;grid();};
  q('[data-apply]').onclick=()=>{if(profile===getProfileId()){saveProfilePreference(profile,'cover',draft);onApply();}dialog.close();};
  q('[data-cancel]').onclick=q('.icon-button').onclick=()=>dialog.close();
  dialog.addEventListener('close',()=>{dialog.remove();opener?.focus({preventScroll:true});},{once:true});
  host.append(dialog);selectSource(source);dialog.showModal();
}
