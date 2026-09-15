import {designFragment} from './approved/templates.js';
import {assetImage} from '../assets/index.js';
import {itemArtUrls} from '../views/player-library.js';
import {areaItemGroupIconUrls,formatAreaItemRate} from '../domain/area.js';
import {bandOrder,attributeNames} from './cards/rules.js';

const categories={band:'乐队道具',attribute:'属性道具',magazine:'杂志'};
const el=(tag,cls,text)=>{const node=document.createElement(tag);node.className=cls||'';if(text!==undefined)node.textContent=text;return node;};
const keyOf=group=>group.key.split(':').slice(1).join(':');
import {equipmentAvailable} from '../domain/area.js';
export {equipmentAvailable} from '../domain/area.js';

export function createEquipmentUI({elements,getPlayer,getProfileId,writePlayer,renderForms,areaItemGroups,areaItemLabel}){
 const root=document.querySelector('#aurora-soft-study'),panel=elements.ptEvaluateItemPanel;
 const legacy=panel.querySelector('.config-content');if(legacy)legacy.hidden=true;
 const heading=panel.querySelector('.bo-section-heading'),manage=el('a','pc-link','管理道具等级 ↗');manage.href='#player';heading.append(manage);
 const note=el('p','pc-note'),summary=el('div','pc-equipment-summary'),issue=el('p','pc-issue');issue.setAttribute('role','alert');panel.append(note,summary,issue);
 const selects={band:elements.ptEvaluateBandItem,attribute:elements.ptEvaluateAttributeItem,magazine:elements.ptEvaluateMagazineItem};
 const dialog=designFragment('equipment-dialog');dialog.querySelector('#pc-cancel').dataset.cancel='';dialog.querySelector('#pc-category-note').dataset.categoryNote='';dialog.querySelector('header p').textContent='道具等级沿用当前档案';
 root.append(dialog);const q=s=>dialog.querySelector(s);let category='band',draft={},original={},profile,opener,groups=[],player;
 const name=g=>g?.isAll?'通用':g?.category==='magazine'?(areaItemLabel?.(g.areaItemIds[0])||g.label.replace(/^杂志 /,'')):g?.label.replace(/^属性 /,'')||'请选择';
 const art=g=>assetImage(g.category==='magazine'||!areaItemGroupIconUrls(g).length?itemArtUrls(g.areaItemIds[0],player.server):areaItemGroupIconUrls(g),'pc-group-art',name(g))||el('span','pc-missing-art','＋');
 const selected=(key,values)=>groups.find(g=>g.category===key&&keyOf(g)===String(values[key]));
 const tiles=g=>g.areaItemIds.map(id=>{const span=el('span');span.title=`Lv. ${player.areaItem?.[id]?.level||0}`;const image=assetImage(itemArtUrls(id,player.server),'',`道具 ${id}`);if(image)span.append(image);span.append(el('small','',span.title));return span;});
 function render(current){
  player=current;const order=g=>g.isAll?99:g.category==='band'?bandOrder.indexOf(Number(g.bandId)):g.category==='attribute'?Object.keys(attributeNames).indexOf(g.attribute):0;groups=areaItemGroups(player).sort((a,b)=>a.category===b.category?order(a)-order(b):0);const values=Object.fromEntries(Object.entries(selects).map(([k,s])=>[k,s.value]));
  note.textContent=player.activityMode==='medley'?'三队共用一套道具；等级来自当前档案。':'选择本次装备的三类道具；等级来自当前档案。';summary.replaceChildren();
  for(const [key,label]of Object.entries(categories)){
   const g=selected(key,values),button=el('button','pc-summary-card');button.type='button';button.setAttribute('aria-label',`选择${label}，当前${name(g)}`);const caption=el('span','pc-summary-caption',label);caption.append(el('span','','更换 ›'));
   const main=el('span','pc-summary-main'),text=el('span');text.append(el('strong','',name(g)),el('small','',equipmentAvailable(g,player)?'加成 '+formatAreaItemRate(g.rate):'含 0 级道具，请先配置'));main.append(g?art(g):el('span','pc-missing-art','＋'),text);
   const items=el('span','pc-summary-items');items.setAttribute('aria-hidden','true');if(g)for(const tile of tiles(g)){tile.querySelector('small').remove();items.append(tile.firstChild||tile);}button.append(caption,main,items);button.onclick=()=>open(key,button);summary.append(button);
  }
  const unavailable=Object.keys(categories).filter(key=>!equipmentAvailable(selected(key,values),player));issue.textContent=unavailable.length?`请先配置${unavailable.map(k=>categories[k]).join('、')}的必需道具等级。`:'';issue.hidden=!unavailable.length;
 }
 function paint(){
  const tabs=q('.pc-category-tabs');tabs.replaceChildren();for(const[key,label]of Object.entries(categories)){const tab=el('button','',label);tab.type='button';tab.id='pc-tab-'+key;tab.setAttribute('role','tab');tab.setAttribute('aria-selected',String(key===category));tab.setAttribute('aria-controls','pc-item-options');tab.tabIndex=key===category?0:-1;tab.append(el('small','',selected(key,draft)?'已选':'待选'));tab.onclick=()=>{category=key;paint();q('#pc-tab-'+key).focus();};tabs.append(tab);}
  q('[data-category-note]').textContent={band:'选择一组乐队道具，七件一起装备。',attribute:'通用道具也在属性分类；海螺包与咖啡取较高加成，盆栽叠加。',magazine:'三本杂志选择一本，对应演出、技巧或形象加成。'}[category];
  const host=q('.pc-item-options');host.id='pc-item-options';host.setAttribute('aria-labelledby','pc-tab-'+category);host.replaceChildren();
  for(const g of groups.filter(g=>g.category===category)){const button=el('button','pc-option'),chosen=String(draft[category])===keyOf(g),available=equipmentAvailable(g,player);button.type='button';button.setAttribute('aria-pressed',String(chosen));button.setAttribute('aria-disabled',String(!available));const head=el('span','pc-option-heading');head.append(art(g),el('strong','',name(g)),el('span','pc-check',chosen?'✓':''));const items=el('span','pc-option-items');items.append(...tiles(g));button.append(head,items,el('span','pc-option-rate',available?'加成 '+formatAreaItemRate(g.rate):'含 0 级道具，需先配置'));button.onclick=()=>{if(!available)return;draft[category]=keyOf(g);paint();};host.append(button);}
  q('footer p').textContent=Object.keys(categories).map(k=>name(selected(k,draft))).join(' · ');q('.pc-primary').disabled=Object.keys(categories).some(k=>!equipmentAvailable(selected(k,draft),player))||JSON.stringify(draft)===JSON.stringify(original);
 }
 function open(key,button){render(getPlayer());category=key;profile=getProfileId();opener=button;original=Object.fromEntries(Object.entries(selects).map(([k,s])=>[k,s.value]));draft={...original};paint();dialog.showModal();q('#pc-tab-'+key).focus();}
 const close=()=>dialog.close();q('.pc-close').onclick=q('[data-cancel]').onclick=close;dialog.onclose=()=>opener?.focus();q('a').onclick=close;
 q('.pc-category-tabs').onkeydown=e=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(e.key))return;e.preventDefault();const keys=Object.keys(categories),i=keys.indexOf(category);category=e.key==='Home'?keys[0]:e.key==='End'?keys[2]:keys[(i+(e.key==='ArrowRight'?1:2))%3];paint();q('#pc-tab-'+category).focus();};
 q('.pc-primary').onclick=()=>{if(q('.pc-primary').disabled||profile!==getProfileId())return;const latest=getPlayer();if(Object.keys(categories).some(k=>!equipmentAvailable(selected(k,draft),latest)))return;latest.ptEvaluate.items={...draft};writePlayer(latest);renderForms(latest);close();};
 return {render};
}
