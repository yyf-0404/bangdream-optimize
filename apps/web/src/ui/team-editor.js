import {designFragment} from './approved/templates.js';
import { createCardCatalog } from './cards/catalog.js';
import { catalogModels, cardModel } from './cards/model.js';
import { createCardBrief } from './cards/brief.js';
import { createCardDetails } from './cards/presentation.js';
import { gameText } from './preferences.js';
import { teamChoiceReason, placeTeamCard } from './team-rules.js';
import { createCustomCardPicker } from './custom-cards/picker.js';

const el=(tag,cls,text)=>{const n=document.createElement(tag);n.className=cls||'';if(text!==undefined)n.textContent=text;return n;};
const btn=(text,fn,cls='')=>{const b=el('button',cls,text);b.type='button';b.onclick=fn;return b;};
export function createTeamEditor({getCore,getPlayer,getProfileId,writePlayer,onApply,importDraft,openDetails}) {
  let dialog,catalog,customPicker,draft,index,slot,profileId,trigger,original,busy=false,generation=0,activeTeamCount=1,onlyAvailable=false;
  const details=createCardDetails(),q=s=>dialog.querySelector(s),teams=()=>draft.ptEvaluate.teams.slice(0,activeTeamCount);
  function close(){generation++;busy=false;catalog?.destroy();dialog?.close();dialog?.remove();trigger?.focus({preventScroll:true});}
  function invalid(c){return teamChoiceReason(c,teams(),index,slot,id=>cardModel(getCore(),draft,id).characterId);}
  function changeTeam(value){if(busy)return;index=value;slot=Math.max(0,draft.ptEvaluate.teams[index].findIndex(id=>!id));refresh();}
  function refresh(){slots();catalog?.refresh();customPicker?.refresh();}
  function slots(){
    const host=q('.pt-slots'),team=draft.ptEvaluate.teams[index];host.replaceChildren();
    const tabs=q('.pt-tabs');tabs.replaceChildren();tabs.hidden=activeTeamCount===1;
    for(let i=0;i<activeTeamCount;i++){const b=btn('',()=>changeTeam(i),'pt-tab');b.append(el('span','','第 '+(i+1)+' 队'),el('small','',draft.ptEvaluate.teams[i].filter(Boolean).length+' / 5'));b.setAttribute('aria-pressed',String(i===index));b.disabled=busy;tabs.append(b);}
    for(let at=0;at<5;at++){
      const id=team[at],c=id?cardModel(getCore(),draft,id):null,choose=()=>{if(busy)return;slot=at;refresh();};let cell;
      if(c&&!c.unknown){cell=createCardBrief(c,{captain:at===2,onOpen:choose});cell.classList.add('pt-card-slot');cell.dataset.active=String(slot===at);cell.querySelector('.cp-main').setAttribute('aria-pressed',String(slot===at));}
      else {cell=btn('',choose,'pt-slot');cell.dataset.captain=String(at===2);cell.setAttribute('aria-pressed',String(slot===at));cell.setAttribute('aria-label','卡位 '+(at+1)+(at===2?'，队长':'')+'，'+(id?'卡牌 '+id+' 资料暂缺':'待选择'));cell.append(el('span','pt-image pt-empty-slot','+'));const text=el('span');text.append(el('small','',at===2?'队长':'卡位 '+(at+1)),el('strong','',id?'#'+id+' · 资料暂缺':'待选择'));cell.append(text);}
      cell.dataset.slotIndex=at;host.append(cell);
    }
    const id=team[slot],card=id?cardModel(getCore(),draft,id):null;
    q('.pt-current>p').textContent='正在编辑第 '+(index+1)+' 队 · 卡位 '+(slot+1)+(card?' · '+card.name+' '+(card.displayId||'#'+id):' · 选择后移到下一空位');
    q('[data-team-detail]').disabled=busy||!card||card.unknown;
    q('[data-team-captain]').disabled=busy||!id||slot===2;q('[data-team-remove]').disabled=busy||!id;q('[data-team-clear]').disabled=busy||!team.some(Boolean);
    q('[data-team-import]').disabled=busy;q('[data-team-import]').setAttribute('aria-label','为第 '+(index+1)+' 队导入主乐队');
    q('[data-team-next]').hidden=activeTeamCount===1;q('[data-team-next]').disabled=busy||index===activeTeamCount-1;
    const count=teams().flat().filter(Boolean).length;
    q('.pt-footer p').textContent='已选 '+count+' / '+(activeTeamCount*5)+' 张'+(count<activeTeamCount*5?' · 可先保存，计算前需补齐':'');
    q('[data-team-save]').disabled=busy||JSON.stringify(draft)===original;
  }
  function open(teamIndex,medley,initialSlot){
    generation++;activeTeamCount=medley?3:1;onlyAvailable=false;
    trigger=document.activeElement;draft=structuredClone(getPlayer());index=teamIndex;slot=initialSlot;profileId=getProfileId();
    draft.ptEvaluate.teams??=[];while(draft.ptEvaluate.teams.length<activeTeamCount)draft.ptEvaluate.teams.push([]);
    for(let i=0;i<activeTeamCount;i++)draft.ptEvaluate.teams[i]=Array.from({length:5},(_,at)=>draft.ptEvaluate.teams[i]?.[at]||0);
    slot=initialSlot??Math.max(0,draft.ptEvaluate.teams[index].findIndex(id=>!id));
    original=JSON.stringify(draft);
    dialog=designFragment('team-dialog');dialog.classList.add('team-editor-dialog');dialog.setAttribute('aria-label','选择指定队伍');
    q('.pt-eyebrow').textContent=medley?'队伍内角色不重复；三队不能重复使用同一卡牌。':'队伍内角色不重复，第三卡位为队长。';
    const bindings={close:'close',detail:'card-detail',import:'import',captain:'captain',remove:'remove',clear:'clear',next:'next-team',cancel:'cancel',save:'apply'};
    for(const [key,id]of Object.entries(bindings))q('#pt-'+id).setAttribute('data-team-'+key,'');
    const scroll=q('.pt-scroll'),available=q('#pt-only-available').closest('label');available.className='pt-available';
    scroll.classList.add('team-editor-scroll');scroll.replaceChildren(available);const library=el('div','team-editor-catalog accepted-card-picker');scroll.append(library);q('#pt-feedback').classList.add('team-editor-status');
    const sourceTabs=el('div','card-source-tabs'),customHost=el('section');customHost.hidden=true;
    sourceTabs.setAttribute('role','tablist');sourceTabs.setAttribute('aria-label','指定队伍卡牌来源');
    sourceTabs.innerHTML='<button type="button" role="tab" id="team-game-tab" aria-controls="team-game-panel" aria-selected="true">游戏卡牌</button><button type="button" role="tab" id="team-custom-tab" aria-controls="team-custom-panel" aria-selected="false" tabindex="-1">自定义卡牌</button>';
    library.id='team-game-panel';customHost.id='team-custom-panel';[library,customHost].forEach((n,i)=>{n.setAttribute('role','tabpanel');n.setAttribute('aria-labelledby',i?'team-custom-tab':'team-game-tab');});
    library.before(sourceTabs);library.after(customHost);
    const selectSource=i=>{library.hidden=i!==0;customHost.hidden=i!==1;[...sourceTabs.children].forEach((b,at)=>{b.setAttribute('aria-selected',String(i===at));b.tabIndex=i===at?0:-1;});};
    [...sourceTabs.children].forEach((b,i)=>{b.onclick=()=>selectSource(i);b.onkeydown=e=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(e.key))return;e.preventDefault();const next=e.key==='Home'?0:e.key==='End'?1:1-i;selectSource(next);sourceTabs.children[next].focus();};});
    const pick=c=>{if(busy||invalid(c))return;const choice=placeTeamCard(draft.ptEvaluate.teams[index],slot,c.id);draft.ptEvaluate.teams[index]=choice.team;slot=choice.slot;refresh();};
    customPicker=createCustomCardPicker({host:customHost,getCore,getPlayer:()=>draft,disabledReason:invalid,candidateFilter:c=>!onlyAvailable||!invalid(c),onPick:pick,onDetails:c=>details.open(c,{context:'队伍草稿'})});
    document.querySelector('#aurora-soft-study').append(dialog);
    q('[data-team-close]').onclick=q('[data-team-cancel]').onclick=close;
    dialog.addEventListener('cancel',e=>{e.preventDefault();close();});
    q('[data-team-save]').onclick=()=>{if(busy||q('[data-team-save]').disabled||profileId!==getProfileId())return;writePlayer(draft);onApply(draft);close();};
    q('[data-team-detail]').onclick=()=>details.open(cardModel(getCore(),draft,draft.ptEvaluate.teams[index][slot]),{context:'队伍草稿'});
    q('[data-team-captain]').onclick=()=>{const team=draft.ptEvaluate.teams[index];[team[slot],team[2]]=[team[2],team[slot]];slot=2;refresh();};
    q('[data-team-remove]').onclick=()=>{draft.ptEvaluate.teams[index][slot]=0;refresh();};
    q('[data-team-clear]').onclick=()=>{draft.ptEvaluate.teams[index].fill(0);slot=0;refresh();};
    q('[data-team-next]').onclick=()=>changeTeam(index+1);
    q('.pt-available input').onchange=e=>{onlyAvailable=e.target.checked;catalog.refresh();customPicker.refresh();};
    q('[data-team-import]').onclick=async()=>{
      const token=generation;busy=true;slots();const status=q('.team-editor-status');status.textContent='读取主乐队配置…';
      try{const imported=await importDraft(structuredClone(draft),index);if(token!==generation)return;if(profileId!==getProfileId())throw new Error('档案已切换，请重新打开队伍编辑');draft=imported;status.textContent='已载入主乐队草稿。应用后保存队伍、养成与道具，取消将放弃导入。';}
      catch(error){if(token===generation)status.textContent=error.message||String(error);}finally{if(token===generation){busy=false;refresh();}}
    };
    catalog=createCardCatalog({root:q('.team-editor-catalog'),selectionMode:'team',getCards:()=>catalogModels(getCore(),draft,profileId),getProfileId:()=>profileId,getServer:()=>draft.server,
      getCharacters:()=>Object.entries(getCore().characters||{}).filter(([,c])=>c.bandId>0&&c.bandId<100).map(([id,c])=>({id:Number(id),band:Number(c.bandId),name:gameText(c.characterName,'角色 '+id)})),
      selectedIds:()=>draft.ptEvaluate.teams[index],disabledReason:invalid,candidateFilter:c=>!onlyAvailable||!invalid(c),onReset:()=>{onlyAvailable=false;q('.pt-available input').checked=false;},
      onPick:c=>{if(busy)return;const choice=placeTeamCard(draft.ptEvaluate.teams[index],slot,c.id);draft.ptEvaluate.teams[index]=choice.team;slot=choice.slot;refresh();},
    });
    refresh();dialog.showModal();
  }
  function renderTeams(host,player,selected,medley,active){
    host.replaceChildren();host.append(el('p','pt-help','每队五位不同角色，第三卡位为队长。'+(medley?'三队不能重复使用同一张卡；同角色不同卡可跨队使用。':'')));
    for(let ti=0;ti<(medley?3:1);ti++){
      const section=el('section','specified-team'),head=el('div','specified-team-header pt-team-heading');
      const title=el('h4','',medley?'第 '+(ti+1)+' 曲队伍':'指定队伍');title.append(el('small','',(selected[ti]?.filter(Boolean).length||0)+' / 5'));head.append(title);const edit=btn('编辑队伍',()=>open(ti,medley));edit.disabled=!active;head.append(edit);
      const grid=el('div','specified-team-grid pt-summary-grid');
      for(let ci=0;ci<5;ci++){
        const id=selected[ti]?.[ci]||0;
        if(id)grid.append(createCardBrief(cardModel(getCore(),player,id),{captain:ci===2,onOpen:openDetails}));
        else {const empty=btn(ci===2?'选择队长':'选择卡牌',()=>open(ti,medley,ci),'team-slot-empty');empty.disabled=!active;grid.append(empty);}
        const input=el('input','pt-evaluate-card-input');input.type='hidden';input.dataset.teamIndex=ti;input.dataset.cardIndex=ci;input.value=id||'';section.append(input);
      }
      section.append(head,grid);host.append(section);
    }
  }
  return {open,renderTeams};
}
