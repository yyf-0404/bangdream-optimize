import { createCardCatalog } from '../ui/cards/catalog.js';
import { catalogModels, cardModel, cardConfig } from '../ui/cards/model.js';
import { createCardDetails } from '../ui/cards/presentation.js';
import { profilePreference, saveProfilePreference, gameText } from '../ui/preferences.js';
import { reviewBulk } from '../ui/cards/bulk.js';
import { createCustomCardsView } from '../ui/custom-cards/index.js';

export function createCardView({rows,getCore,getPlayer,getProfileId,writePlayer,normalizedCardConfig,loadCardDetail}) {
  let catalog, customView, sourceTabs, customPanel, detailProfileId;
  const page=rows.closest('[data-page-panel]'),hero=page.querySelector('.page-hero');
  const models=()=>catalogModels(getCore(),getPlayer(),getProfileId());
  function apply(changes){
    const player=structuredClone(getPlayer()),removed={...profilePreference(getProfileId(),'removedGrowth',{})};
    for(const {card,next} of changes){
      const config=normalizedCardConfig(card.id,cardConfig(next));
      if(next.owned){player.cardList[card.id]=config;delete removed[card.id];}
      else {removed[card.id]=config;delete player.cardList[card.id];}
    }
    const saved=saveProfilePreference(getProfileId(),'removedGrowth',removed);
    if(!saved&&changes.some(c=>!c.next.owned))return;
    writePlayer(player);catalog.refresh();
  }
  function openCard(card){
    if(!card.unknown){detailProfileId=getProfileId();details.open(card);return;}
    const dialog=document.createElement('dialog');dialog.className='catalog-bulk-dialog';
    const heading=document.createElement('h2');heading.textContent=`卡牌 #${card.id} · 资料暂缺`;
    const note=document.createElement('p');note.textContent='原有持有与养成记录已保留。更新游戏数据后可恢复显示。';
    const data=document.createElement('pre');data.textContent=JSON.stringify(card.rawConfig,null,2);
    const close=document.createElement('button');close.type='button';close.textContent='关闭';close.onclick=()=>dialog.close();
    dialog.append(heading,note,data,close);dialog.addEventListener('close',()=>dialog.remove(),{once:true});document.body.append(dialog);dialog.showModal();
  }
  const details=createCardDetails({editable:true,loadCardDetail,onApply:(card,patch)=>{if(detailProfileId===getProfileId())apply([{card,next:{...card,...patch}}]);}});
  function renderCards(){
    if(!getCore())return;
    if(!customView){
      const gamePanel=rows.closest('.panel');gamePanel.id='game-card-panel';gamePanel.setAttribute('role','tabpanel');gamePanel.setAttribute('aria-labelledby','game-card-tab');
      sourceTabs=document.createElement('div');sourceTabs.className='card-source-tabs';sourceTabs.setAttribute('role','tablist');sourceTabs.setAttribute('aria-label','卡牌来源');
      sourceTabs.innerHTML='<button type="button" id="game-card-tab" role="tab" aria-controls="game-card-panel" aria-selected="true">游戏卡牌</button><button type="button" id="custom-card-tab" role="tab" aria-controls="custom-card-panel" aria-selected="false" tabindex="-1">自定义卡牌 <small></small></button>';
      gamePanel.before(sourceTabs);customPanel=document.createElement('section');customPanel.id='custom-card-panel';customPanel.className='card-custom-panel';customPanel.setAttribute('role','tabpanel');customPanel.setAttribute('aria-labelledby','custom-card-tab');customPanel.hidden=true;gamePanel.after(customPanel);
      const select=index=>{[...sourceTabs.children].forEach((b,i)=>{b.setAttribute('aria-selected',String(i===index));b.tabIndex=i===index?0:-1;});gamePanel.hidden=index!==0;customPanel.hidden=index!==1;};
      [...sourceTabs.children].forEach((b,i)=>{b.onclick=()=>select(i);b.onkeydown=e=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(e.key))return;e.preventDefault();const next=e.key==='Home'?0:e.key==='End'?1:1-i;select(next);sourceTabs.children[next].focus();};});
      customView=createCustomCardsView({host:customPanel,getCore,getPlayer,getProfileId,writePlayer,onChange:()=>{sourceTabs.querySelector('small').textContent=Object.keys(getPlayer().customCards||{}).length;}});
    }
    customView.refresh();sourceTabs.querySelector('small').textContent=Object.keys(getPlayer().customCards||{}).length;
    const title=hero.querySelector('h2');title.textContent='我的卡牌 ';const count=document.createElement('small');count.textContent=Object.keys(getCore().cards).length.toLocaleString();title.append(count);hero.querySelector('.hero-copy>span').textContent=`${{cn:'国服',jp:'日服',en:'国际服',tw:'台服',kr:'韩服'}[getPlayer().server]}档案 · 已持有 ${Object.keys(getPlayer().cardList).length.toLocaleString()} 张 · ${new Set(models().filter(c=>c.owned).map(c=>c.characterId)).size} 位角色`;
    if(!catalog){const header=rows.closest('.panel').querySelector('.panel-header');if(header)header.hidden=true;catalog=createCardCatalog({root:rows,getCards:models,getProfileId,getServer:()=>getPlayer().server,
      getCharacters:()=>Object.entries(getCore().characters||{}).filter(([,c])=>Number(c.bandId)>0&&Number(c.bandId)<100).map(([id,c])=>({id:Number(id),band:Number(c.bandId),name:gameText(c.characterName,`角色 ${id}`)})),
      onOpen:openCard,
      onIllustrationChange:(card,illustTrained)=>{if(!card.owned)return;const player=structuredClone(getPlayer());player.cardList[card.id].illustTrainingStatus=illustTrained;writePlayer(player);},
      onBulk:async(action,cards)=>{const id=getProfileId();const changes=await reviewBulk(cards,action);if(id===getProfileId()&&changes.length)apply(changes);},
    });}
    catalog.refresh();
  }
  return {renderCards,expandCardGroupForCard(){},models,details,model:id=>cardModel(getCore(),getPlayer(),id,undefined,getProfileId())};
}
