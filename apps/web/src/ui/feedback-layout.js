import {designIcon} from './fidelity.js';

const svg = path => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${path}"/></svg>`;
const icons = {
 problem: svg('M12 8v5m0 3h.01M10.3 3.8 2.2 18a2 2 0 0 0 1.7 3h16.2a2 2 0 0 0 1.7-3L13.7 3.8a2 2 0 0 0-3.4 0'),
 suggestion: svg('M9 18h6m-5 3h4M8.5 14.5a6 6 0 1 1 7 0c-1 .8-1 1.5-1 3.5h-5c0-2 0-2.7-1-3.5'),
 data: svg('M4 4h16v16H4zM4 9h16M9 9v11m0-6h11'),
 other: designIcon('message'),
 attachment: svg('m8 12 6.5-6.5a3 3 0 0 1 4.2 4.2l-8.5 8.5a5 5 0 0 1-7.1-7.1l8.5-8.5'),
 mail: svg('M3 5h18v14H3zM3 6l9 7 9-7'),
 add: svg('M12 5v14M5 12h14'),
 file: svg('M14 3H5v18h14V8l-5-5zM14 3v5h5M8 13h8m-8 4h6'),
 close: svg('m6 6 12 12M18 6 6 18'),
 send: svg('m21 3-7 18-4-7-7-4 18-7zM10 14 21 3'),
};
const kinds = {
 problem: {name:'问题反馈', subject:'例如：切换卡牌分组后，列表无法滚动', content:'做了哪些操作？\n实际出现了什么情况？\n你期望看到什么结果？', hint:'操作步骤、实际表现和预期结果，都有助于定位问题。'},
 suggestion: {name:'功能建议', subject:'用一句话说明你希望改进的地方', content:'你希望增加或改进什么？\n在什么场景下会用到？', hint:'说说使用场景，以及这个改进能解决什么问题。'},
 data: {name:'数据问题', subject:'例如：某张卡牌的技能数值不正确', content:'对应的卡牌 / 歌曲 ID 和服务器是什么？\n当前数据与正确数据分别是什么？', hint:'附上 ID、服务器和参考来源，方便核对。'},
 other: {name:'其他', subject:'用一句话概括你想反馈的内容', content:'写下你想告诉我们的事情。', hint:'补充相关背景，可以帮助维护者更好地理解。'},
};

export function mountFeedbackLayout() {
 const dialog = document.querySelector('#feedback-dialog');
 const ids = ['feedback-form','feedback-category','feedback-contact-email','feedback-subject','feedback-content','feedback-attachments','feedback-attachment-summary','feedback-status','feedback-diagnostic-notice','close-feedback','submit-feedback'];
 const nodes = Object.fromEntries(ids.map(id => [id, dialog.querySelector('#'+id)]));
 const trap = dialog.querySelector('.feedback-honeypot');
 const template = document.createElement('template');
 template.innerHTML = `
  <header class="pf-top">
   <div class="pf-heading"><span class="pf-mark">${icons.other}</span><div><h2 id="pf-title">问题反馈</h2><p id="pf-intro">遇到问题，或有更好的想法，都可以告诉我们。</p></div></div>
   <span data-slot="close-feedback"></span>
  </header>
  <form>
   <div class="pf-body">
    <fieldset class="pf-types"><legend>反馈类型</legend><div>${Object.entries(kinds).map(([value, kind]) => `<label class="pf-type"><input type="radio" name="feedback-kind" value="${value}"><span>${icons[value]}<span>${kind.name}</span></span></label>`).join('')}</div></fieldset>
    <span data-slot="feedback-category"></span>
    <div class="pf-field"><label for="feedback-subject">标题 <small>必填</small></label><span data-slot="feedback-subject"></span></div>
    <div class="pf-field pf-description"><label for="feedback-content">详细描述 <small>必填</small></label><span data-slot="feedback-content"></span><div class="pf-writing-hint"><span id="pf-content-hint"></span><span id="pf-content-count" aria-hidden="true"></span></div></div>
    <section class="pf-supplements" aria-label="补充信息">
     <div class="pf-attachments"><h3>${icons.attachment}补充材料 <small>选填</small></h3><label class="pf-file">${icons.add}<span>添加附件</span><span data-slot="feedback-attachments"></span></label><p class="pf-note" id="pf-file-hint">图片、日志或文件 · 最多 3 个<br>单个 ≤ 5 MiB，合计 ≤ 10 MiB（含诊断）</p><ul class="pf-file-list" aria-label="已选附件"></ul><span data-slot="feedback-attachment-summary"></span></div>
     <div class="pf-contact pf-field"><label for="feedback-contact-email">${icons.mail}联系邮箱 <small>选填</small></label><span data-slot="feedback-contact-email"></span><p class="pf-note" id="pf-email-hint">留下邮箱，方便维护者回复你。</p></div>
    </section>
    <span data-slot="feedback-diagnostic-notice"></span><span data-slot="feedback-status"></span>
   </div>
   <footer class="pf-bottom"><small class="pf-context"><span>反馈会发送给维护者</span><span id="pf-context-detail"></span></small><div><button type="button" class="pf-cancel">取消</button><span data-slot="submit-feedback"></span></div></footer>
  </form>`;
 for (const [id, node] of Object.entries(nodes)) template.content.querySelector(`[data-slot="${id}"]`)?.replaceWith(node);
 const form = nodes['feedback-form'];
 form.className = '';
 form.replaceChildren(...template.content.querySelector('form').childNodes);
 trap.hidden = true;
 form.append(trap);
 template.content.querySelector('form').replaceWith(form);
 dialog.className = '';
 dialog.replaceChildren(...template.content.childNodes);
 dialog.setAttribute('aria-labelledby', 'pf-title');
 dialog.setAttribute('aria-describedby', 'pf-intro');
 const category = nodes['feedback-category'], subject = nodes['feedback-subject'], content = nodes['feedback-content'], files = nodes['feedback-attachments'];
 category.hidden = true;
 nodes['feedback-attachment-summary'].hidden = true;
 nodes['close-feedback'].className = 'pf-close';
 nodes['close-feedback'].innerHTML = icons.close;
 nodes['submit-feedback'].className = 'pf-submit';
 nodes['submit-feedback'].innerHTML = `<span>发送反馈</span>${icons.send}`;
 content.rows = 5;
 content.setAttribute('aria-describedby', 'pf-content-hint');
 files.setAttribute('aria-label', '添加反馈附件');
 files.setAttribute('aria-describedby', 'pf-file-hint');
 nodes['feedback-contact-email'].placeholder = 'you@example.com';
 nodes['feedback-contact-email'].setAttribute('aria-describedby', 'pf-email-hint');
 nodes['feedback-status'].setAttribute('role', 'status');
 nodes['feedback-status'].setAttribute('aria-atomic', 'true');
 dialog.querySelector('.pf-cancel').onclick = () => dialog.close();

 const syncKind = () => {
  const kind = kinds[category.value] ?? kinds.problem;
  for (const radio of dialog.querySelectorAll('[name="feedback-kind"]')) radio.checked = radio.value === category.value;
  subject.placeholder = kind.subject;
  content.placeholder = kind.content;
  dialog.querySelector('#pf-content-hint').textContent = kind.hint;
 };
 for (const radio of dialog.querySelectorAll('[name="feedback-kind"]')) radio.addEventListener('change', () => {
  category.value = radio.value;
  category.dispatchEvent(new Event('change', {bubbles:true}));
 });
 category.addEventListener('change', syncKind);
 const syncCount = () => { dialog.querySelector('#pf-content-count').textContent = `${content.value.length.toLocaleString()} / 10,000`; };
 content.addEventListener('input', syncCount);
 const syncFiles = () => {
  const list = dialog.querySelector('.pf-file-list');
  list.replaceChildren();
  [...files.files].forEach((file, index) => {
   const item = document.createElement('li');
   item.innerHTML = `${icons.file}<span class="pf-file-name"></span><small></small><button type="button">${icons.close}</button>`;
   const name = item.querySelector('.pf-file-name');
   name.textContent = name.title = file.name;
   item.querySelector('small').textContent = file.size >= 1024 * 1024 ? `${(file.size / 1024 / 1024).toFixed(1)} MiB` : `${Math.max(1, Math.ceil(file.size / 1024))} KiB`;
   const remove = item.querySelector('button');
   remove.setAttribute('aria-label', `移除附件 ${file.name}`);
   remove.onclick = () => {
    const transfer = new DataTransfer();
    [...files.files].filter((_, position) => position !== index).forEach(kept => transfer.items.add(kept));
    files.files = transfer.files;
    files.dispatchEvent(new Event('change', {bubbles:true}));
    files.focus();
   };
   list.append(item);
  });
 };
 files.addEventListener('change', syncFiles);
 const refresh = () => {
  syncKind(); syncCount(); syncFiles();
  dialog.querySelector('#pf-context-detail').textContent = [document.querySelector('#page-breadcrumb')?.textContent.trim(), document.querySelector('#app-version')?.textContent.trim()].filter(Boolean).join(' · ');
 };
 form.addEventListener('reset', () => queueMicrotask(refresh));
 new MutationObserver(() => { if (dialog.open) refresh(); }).observe(dialog, {attributes:true, attributeFilter:['open']});
 refresh();
}
