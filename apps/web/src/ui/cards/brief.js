import { cardBriefMarkup, bindCardBrief } from './presentation.js';

export function createCardBrief(card,{captain=false,order,onOpen,onIllustrationChange}={}) {
  const item=document.createElement('article');item.className='cp-card';item.dataset.cpId=card.id;item.dataset.captain=String(captain);item.dataset.owned=String(card.owned);
  if(card.unknown){
    item.classList.add('cp-unknown');
    const id=document.createElement('strong');id.textContent=`#${card.id}`;
    const note=document.createElement('span');note.textContent='卡牌资料暂缺';
    const growth=document.createElement('small');growth.textContent=`Lv. ${card.level} · Skill Lv. ${card.skill} · 突破 ${card.growth.mastery}`;
    item.append(id,note,growth);return item;
  }
  item.innerHTML=cardBriefMarkup(card,{captain,order});
  if(card.custom&&card.enabled===false){item.dataset.disabled='true';const state=document.createElement('small');state.className='cp-inactive';state.textContent='未启用';item.append(state);}
  bindCardBrief(item,{resolve:()=>card,onOpen,onIllustrationChange});
  return item;
}
