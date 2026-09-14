import {mountResultLayout} from './result-layout.js';
import {mountFeedbackLayout} from './feedback-layout.js';
import { language, savePreferences } from './preferences.js';
import { mountAcceptedShell, updateAcceptedProfile } from './fidelity.js';

export const pages = {
  activity: ['活动配置', 'EVENT & LIVE', '选择活动，设置目标与演出条件'],
  cards: ['我的卡牌', 'CARD LIBRARY', '管理持有卡牌与养成记录'],
  player: ['道具与角色', 'ITEMS & MEMBERS', '配置区域道具与角色加成'],
  result: ['计算结果', 'RESULTS', '查看队伍、演出收益与历史方案'],
  archive: ['档案与数据', 'PROFILES & DATA', '管理玩家档案、交换配置与游戏资源'],
};
export const icon = (path) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
const pageIcons = {
  activity: 'M4 6h16v14H4zM8 3v6m8-6v6M4 11h16m-11 4h2m3 0h2',
  cards: 'M6 4h13v16H6zM3 8v14h12M10 8h5m-5 4h5',
  player: 'M4 9h16v11H4zM3 6h18v4H3zM12 6v14M12 6C2 7 6-2 12 6c6-8 10 1 0 0',
  result: 'M4 4v16h17M8 16v-5m5 5V6m5 10v-7',
  archive: 'M3 6h7l2 3h9v11H3zM3 6V4h7l2 2h7v3',
};

// Recompose existing controls before binding. Their identities and runtime handlers survive.
export function mountShell(document) {
  const q = s => document.querySelector(s);
  q('#controls').noValidate=true;
  document.addEventListener('ui-preference-save-error',()=>{q('#status').textContent='界面偏好暂时无法保存，当前会话仍可使用';q('#status').classList.add('error');});
  const aside = q('.app-sidebar');
  const nav = q('.app-nav');
  const version = q('#app-version');
  aside.id = 'unified-sidebar';
  aside.classList.add('design-sidebar');
  aside.dataset.style = 'b-color';
  q('.sidebar-brand').className = 'sidebar-brand side-brand';
  q('.side-brand').innerHTML = `<span class="side-brand-mark">${icon('M5 17V7l7 4 7-4v10l-7-4z')}</span><div class="side-brand-name"><b>邦邦组队计算工具</b><small>BanG Dream! Optimize</small></div>`;
  q('.side-brand-name').append(version);
  const navSection = document.createElement('div');
  navSection.className = 'side-navigation';
  nav.before(navSection);
  navSection.innerHTML = '<p class="side-caption">工作空间</p>';
  navSection.append(nav);
  nav.replaceChildren(...Object.entries(pages).map(([key, [title]]) => {
    const button = document.createElement('button');
    button.type = 'button'; button.dataset.page = key;
    button.className = `side-link page-tab${key === 'activity' ? ' active' : ''}`;
    button.innerHTML = `<span class="side-icon">${icon(pageIcons[key])}</span><span class="side-label"><b>${title}</b></span><i class="location-mark"></i>`;
    if (key === 'activity') button.setAttribute('aria-current', 'page');
    return button;
  }));
  q('#toggle-topbar').hidden = true;
  const bottom = document.createElement('div'); bottom.className = 'side-bottom';
  aside.append(bottom);
  q('.sidebar-status').classList.add('side-state');
  bottom.append(q('.sidebar-status'), q('.sidebar-actions'));
  q('#open-feedback').classList.add('side-help');
  q('.sidebar-actions .calculate-submit').classList.add('side-calculate');

  const archive = document.createElement('section');
  archive.className = 'page-panel archive-page'; archive.dataset.pagePanel = 'archive'; archive.hidden = true;
  q('#controls').append(archive);
  const manager = q('.config-manager-panel');
  manager.classList.add('panel');
  manager.querySelector('h2').textContent = '当前档案';
  archive.append(manager);
  const resources = document.createElement('section');
  resources.className = 'panel archive-resources';
  resources.innerHTML = `<div class="panel-header"><h2>${icon('M4 5h16v5H4zm0 9h16v5H4zM7 7.5h.01M7 16.5h.01')}游戏资源</h2></div><p class="muted">游戏数据缓存与玩家档案独立保存。</p>`;
  const resourceActions = q('.topbar-actions');
  resourceActions.querySelector('.calculate-submit').remove();
  resources.append(resourceActions, q('#desktop-downloads-dialog'));
  archive.append(resources, q('.log-panel'));
  const feedback=document.createElement('button');feedback.type='button';feedback.className='archive-feedback';feedback.textContent='反馈与问题报告';feedback.onclick=()=>q('#open-feedback').click();resourceActions.append(feedback);
  q('#app-topbar').innerHTML = `<div class="workspace-context"><span>当前档案</span><button type="button" id="workspace-profile">加载中</button><span id="workspace-server"></span></div><label class="global-language">${icon('M3 5h12M9 3v2m-4 4 8 8M13 5c0 6-4 10-10 13m11 2 4-11 4 11m-6-4h4')}<span>文本语言</span><select id="global-language" aria-label="游戏文本语言"><option value="zh-CN">简体中文</option><option value="zh-TW">繁體中文</option><option value="ja">日本語</option><option value="en">English</option><option value="ko">한국어</option></select></label>`;
  q('#workspace-profile').onclick = () => nav.querySelector('[data-page=archive]').click();
  const quickImport = document.createElement('button');
  quickImport.type = 'button'; quickImport.id = 'quick-import-profile'; quickImport.disabled = true;
  quickImport.title = '导入到当前档案'; quickImport.setAttribute('aria-label', '导入到当前档案');
  quickImport.innerHTML = icon('M12 3v12m-4-4 4 4 4-4M4 15v6h16v-6') + '<span>导入</span>';
  q('.workspace-context').append(quickImport);
  q('#global-language').value = language();
  q('#global-language').onchange = e => { savePreferences({ language: e.target.value }); document.dispatchEvent(new CustomEvent('game-language-change')); };

  for (const [key, [title, english, description]] of Object.entries(pages)) {
    const hero = document.createElement('header'); hero.className = 'page-hero'; hero.dataset.hero = key;
    hero.innerHTML = `<div class="hero-visual" aria-hidden="true"></div><div class="hero-copy"><p>${english}</p><h2>${title}</h2><span>${description}</span></div>`;
    q(`[data-page-panel=${key}]`).prepend(hero);
  }
  const selector = q('.activity-selector-card');
  selector.querySelector('h2').textContent = '活动与目标';
  q('.card-add-card').hidden = true;
  const stale = document.createElement('div'); stale.className = 'result-stale'; stale.hidden = true;
  stale.innerHTML = '<span>配置已变化，以下为上次计算结果。</span><button class="calculate-submit" type="submit" form="controls">重新计算</button>';
  q('#result-summary').before(stale);
  // A mobile calculation entry keeps the same form and worker alive.
  const mobile = document.createElement('button'); mobile.type='submit'; mobile.setAttribute('form','controls');
  mobile.className='primary calculate-submit mobile-calculate';mobile.innerHTML=icon('m9 5 10 7-10 7z')+'<span class="button-label">计算</span>';document.body.append(mobile);
  const mobileStatus=document.createElement('div');mobileStatus.className='mobile-status';mobileStatus.setAttribute('role','status');mobileStatus.setAttribute('aria-live','polite');
  document.body.append(mobileStatus);
  const source=q('#status'),syncStatus=()=>{mobileStatus.textContent=source.textContent;mobileStatus.classList.toggle('error',source.classList.contains('error'));mobileStatus.hidden=!source.textContent.trim();};
  new MutationObserver(syncStatus).observe(source,{childList:true,subtree:true,attributes:true,attributeFilter:['class']});syncStatus();
  for (const heading of document.querySelectorAll('.page-panel > .panel > .panel-header h2,.page-panel > .panel > .panel-header h3')) {
    if (!heading.querySelector('svg')) heading.insertAdjacentHTML('afterbegin', icon(pageIcons[heading.closest('[data-page-panel]').dataset.pagePanel]));
  }
  mountAcceptedShell(document);
  mountResultLayout();
  mountFeedbackLayout();
}

export function updateShell(player, profileName) {
  document.querySelector('#workspace-profile').textContent = profileName || '当前档案';
  document.querySelector('#quick-import-profile').disabled = false;
  document.querySelector('#workspace-server').textContent = { cn: '国服', jp: '日服', en: '国际服', tw: '台服', kr: '韩服' }[player.server] || player.server;
  updateAcceptedProfile(player);
}
