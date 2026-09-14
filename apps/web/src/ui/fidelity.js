import {sidebarMarkup,sidebarIcon} from './approved/sidebar-template.js';
import { assetImage, serverIconUrls } from '../assets/index.js';

const paths={music:'M9 18V5l12-3v13M9 9l12-3M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0m12-3a3 3 0 1 1-6 0 3 3 0 0 1 6 0',activity:'M3 7h8m5 0h5M3 17h4m5 0h9M16 7a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0M12 17a2.5 2.5 0 1 1-5 0 2.5 2.5 0 0 1 5 0',cards:'M7 5h13v16H7zM4 17H3V3h12',player:'M12 8a3 3 0 1 1-6 0 3 3 0 0 1 6 0M3 21v-3a6 6 0 0 1 12 0v3M16 5a3 3 0 0 1 0 6m1 3a5 5 0 0 1 4 5v2',result:'M4 3v18h17M8 15l4-5 4 2 5-6',archive:'M3 7V4h6l2 3h10v13H3z',chevron:'m9 6 6 6-6 6',message:'M3 3h18v14H8l-5 4zM7 7h10M7 11h7',target:'M20 12a8 8 0 1 1-8-8m4 8a4 4 0 1 1-4-4m0 4 8-8m-4 0h4v4',calendar:'M4 5h16v15H4zM8 3v4m8-4v4M4 10h16',bonus:'M12 8v8m-4-4h8M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0',song:'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0',equipment:'M4 8h16v13H4zM3 5h18v4H3zM12 5v16'};
Object.assign(paths,{search:'M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0',arrow:'M12 4v16m-6-6 6 6 6-6',layers:'m12 3 9 5-9 5-9-5zM3 12l9 5 9-5M3 16l9 5 9-5',help:'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3m.1 3h.01',transfer:'M4 8h14m-4-4 4 4-4 4M20 16H6m4-4-4 4 4 4'});
export const designIcon=name=>`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${paths[name]||paths.activity}"/></svg>`;
const names={activity:'活动配置',cards:'我的卡牌',player:'道具与角色',result:'计算结果',archive:'档案与数据'};
export function mountAcceptedShell(document){
 const q=s=>document.querySelector(s),side=q('#unified-sidebar');
 const status=q('#status'),help=q('#open-feedback'),calculate=q('.sidebar-actions .calculate-submit'),version=q('#app-version');
 const frame=document.createElement('div');frame.id='unified-sidebar';side.removeAttribute('id');
 frame.innerHTML=sidebarMarkup({session:{names,url:id=>'#'+(id==='shared'?'archive':id)},page:'activity',svg:n=>sidebarIcon(n==='archive'?'shared':n)});side.replaceWith(frame);
 const nav=frame.querySelector('nav');nav.className='accepted-nav';
 for(const link of nav.children){link.dataset.page=link.getAttribute('href').slice(1);link.addEventListener('click',e=>e.preventDefault());}
 const state=frame.querySelector('.side-state-value');state.lastElementChild.replaceWith(status);
 const originalHelp=frame.querySelector('.side-help');help.className=originalHelp.className;help.innerHTML=originalHelp.innerHTML;originalHelp.replaceWith(help);
 calculate.className='side-calculate calculate-submit';calculate.innerHTML='<span class="button-label">计算</span>'+designIcon('chevron');frame.querySelector('.side-bottom').append(calculate);
 frame.querySelector('.side-brand-name').append(version);
 const brand=frame.querySelector('.side-brand-mark'),brandImage=document.createElement('img');brandImage.src=new URL('../../assets/brand.svg',import.meta.url).href;brandImage.alt='';brand.replaceChildren(brandImage);
 frame.querySelector('.side-brand').onclick=e=>{e.preventDefault();nav.querySelector('[data-page=activity]').click();};
 const top=q('#app-topbar'),language=q('.global-language'),context=q('.workspace-context');
 top.className='accepted-topbar';top.replaceChildren();top.innerHTML='<span>工作台</span><span>/</span><b id="page-breadcrumb">活动配置</b>';top.append(language);
 const menu=document.createElement('button');menu.type='button';menu.className='workspace-menu';menu.innerHTML='···';menu.setAttribute('aria-label','打开档案与数据');menu.onclick=()=>q('[data-page=archive]').click();top.append(menu);
 context.className='accepted-profile';top.after(context);
 for(const [page,id] of Object.entries({activity:'aurora-soft-study',cards:'card-library-unified',player:'player-library',result:'result-design',archive:'archive-design'})){
  const panel=q(`[data-page-panel=${page}]`);panel.id=id;panel.dataset.resultSkin='coherent';panel.dataset.archiveSkin='refined';
  const content=document.createElement('div');content.className=page==='activity'?'bo-content accepted-content':page==='result'?'result-content accepted-content':page==='archive'?'shared-content accepted-content':'content accepted-content';
  for(const child of [...panel.children])if(!child.matches('.page-hero'))content.append(child);panel.append(content);
  const hero=panel.querySelector('.page-hero');hero.classList.add('heading');
  hero.querySelector('.hero-copy>p').hidden=true;
  hero.querySelector('.hero-copy>span').textContent={activity:'设置计算目标、活动加成与歌曲。',cards:'在同一张图鉴里管理持有与未持有的卡牌。',player:'从排练室到每一位成员，整理档案中的加成配置。',result:'查看队伍、歌曲表现与本次计算方案。',archive:'管理玩家档案、交换配置与游戏资源。'}[page];
  for(const section of content.querySelectorAll(':scope>.panel')){section.classList.add(page==='activity'?'bo-section':'accepted-section');const header=section.querySelector(':scope>.panel-header');if(header)header.classList.add(page==='activity'?'bo-section-heading':'section-heading');}
 }
 q('#card-library-unified .accepted-content>.panel:not(.card-add-card)>.panel-header')?.remove();
 for(const button of q('.accepted-nav').children)button.addEventListener('click',()=>{q('#page-breadcrumb').textContent=names[button.dataset.page];});
}

export function updateAcceptedProfile(player){
 const host=document.querySelector('#workspace-server');if(!host)return;host.prepend(assetImage(serverIconUrls(player.server),'','')||document.createTextNode(''));
 const page=document.querySelector('[data-page-panel]:not([hidden])')?.dataset.pagePanel;document.querySelector('#page-breadcrumb').textContent=names[page]||names.activity;
}
