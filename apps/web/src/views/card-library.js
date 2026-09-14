import { createCardCatalog } from '../ui/cards/catalog.js';
import { catalogModels, cardModel, cardConfig } from '../ui/cards/model.js';
import { createCardDetails } from '../ui/cards/presentation.js';
import { profilePreference, saveProfilePreference, gameText } from '../ui/preferences.js';
import { reviewBulk } from '../ui/cards/bulk.js';

export function createCardView({rows,getCore,getPlayer,getProfileId,writePlayer,normalizedCardConfig,loadCardDetail}) {
  let catalog, detailProfileId;
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
