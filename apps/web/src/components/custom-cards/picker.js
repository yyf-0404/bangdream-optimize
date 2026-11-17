import {cardModel} from '../cards/model.js';
import {createCardBrief} from '../cards/brief.js';

// Separate source and filters; the brief and detail presentation stays shared.
export function createCustomCardPicker({host,getCore,getPlayer,disabledReason,candidateFilter,onPick,onDetails}) {
  host.className='custom-team-picker';
  host.innerHTML='<div class="custom-picker-tools"><input type="search" aria-label="搜索指定队伍的自定义卡牌" placeholder="搜索名称、角色或 C-编号"><select aria-label="指定队伍自定义卡牌属性"><option value="all">全部属性</option><option value="powerful">Powerful</option><option value="cool">Cool</option><option value="happy">Happy</option><option value="pure">Pure</option></select><span role="status"></span></div><div class="custom-picker-grid"></div><p class="custom-picker-empty" hidden>没有符合条件的自定义卡牌。可在“我的卡牌 → 自定义卡牌”中创建或启用。</p>';
  const search=host.querySelector('input'),attribute=host.querySelector('select'),grid=host.querySelector('.custom-picker-grid');
  function refresh(){
    const player=getPlayer(),term=search.value.trim().toLowerCase();
    const cards=Object.keys(player.customCards||{}).map(id=>cardModel(getCore(),player,id)).filter(c=>(attribute.value==='all'||attribute.value===c.attribute)&&c.searchText.toLowerCase().includes(term)&&candidateFilter(c));
    grid.replaceChildren();host.querySelector('[role=status]').textContent=cards.length+' 张自定义卡牌';host.querySelector('.custom-picker-empty').hidden=cards.length>0;
    for(const card of cards){
      const reason=disabledReason(card),cell=document.createElement('div');cell.className='custom-picker-item';
      const brief=createCardBrief(card,{onOpen:()=>{if(!reason)onPick(card);}}),main=brief.querySelector('.cp-main');main.disabled=!!reason;main.setAttribute('aria-label',`选择 ${card.displayId} ${card.name} ${card.title}${reason?'，'+reason:''}`);
      const foot=document.createElement('div');foot.className='custom-picker-foot';const note=document.createElement('span');note.textContent=reason||card.title;
      const detail=document.createElement('button');detail.type='button';detail.textContent='详情';detail.setAttribute('aria-label',card.displayId+' 自定义卡牌详情');detail.onclick=()=>onDetails(card);foot.append(note,detail);cell.append(brief,foot);grid.append(cell);
    }
  }
  search.oninput=attribute.onchange=refresh;refresh();return {refresh};
}
