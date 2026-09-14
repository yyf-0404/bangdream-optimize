// Copied sidebar HTML and SVG; hrefs and labels are supplied by the SPA.
 const paths={
  music:'<path d="M9 18V5l12-3v13M9 9l12-3"/><ellipse cx="6" cy="18" rx="3" ry="3"/><ellipse cx="18" cy="15" rx="3" ry="3"/>',
  activity:'<path d="M3 7h8m5 0h5M3 17h4m5 0h9"/><circle cx="13.5" cy="7" r="2.5"/><circle cx="9.5" cy="17" r="2.5"/>',
  cards:'<rect x="7" y="5" width="13" height="16" rx="2"/><path d="M4 17H3V3h12"/>',
  player:'<circle cx="9" cy="8" r="3"/><path d="M3 21v-3a6 6 0 0 1 12 0v3M16 5a3 3 0 0 1 0 6m1 3a5 5 0 0 1 4 5v2"/>',
  result:'<path d="M4 3v18h17M8 15l4-5 4 2 5-6"/>',
  shared:'<path d="M3 7V4h6l2 3h10v13H3z"/>',
  message:'<path d="M3 3h18v14H8l-5 4zM7 7h10M7 11h7"/>',
  chevron:'<path d="m9 6 6 6-6 6"/>',check:'<path d="m5 12 4 4 10-10"/>',
  idle:'<circle cx="12" cy="12" r="6"/>',running:'<path d="M12 3a9 9 0 1 1-9 9"/>',
  error:'<circle cx="12" cy="12" r="9"/><path d="M12 7v6m0 3v1"/>'
 };
 export const sidebarIcon=n=>`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths[n]||paths.idle}</svg>`;

export function sidebarMarkup({session,page,svg}) {return `<aside class="design-sidebar" data-style="b-color" aria-label="工作台导航"><a class="side-brand" href="${session.url('activity')}"><span class="side-brand-mark">${svg('music')}</span><span class="side-brand-name"><b>邦邦组队计算</b><small>BANDORI OPTIMIZE</small></span></a><div class="side-navigation"><p class="side-caption">工作台</p><nav aria-label="主要页面">${Object.entries(session.names).map(([id,name])=>`<a class="side-link" href="${session.url(id)}" ${id===page?'aria-current="page"':''}><span class="side-icon">${svg(id)}</span><span class="side-label"><b>${name}</b></span><span class="location-mark" aria-hidden="true"></span></a>`).join('')}</nav></div><div class="side-bottom"><div class="side-state" data-state="idle"><span class="side-state-caption">运行状态</span><span class="side-state-value" role="status">${svg('idle')}<span>未开始计算</span></span></div><a class="side-help" href="${session.url('shared',{action:'feedback',return:page})}">${svg('message')}<span>问题反馈</span><span class="help-arrow">${svg('chevron')}</span></a></div></aside>`;}
