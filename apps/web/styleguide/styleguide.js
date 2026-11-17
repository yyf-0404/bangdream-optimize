// Styleguide behaviour: token tables generated from tokens.css, markup
// snippets for every component example, and the motion demos.
import { enter, flash, flip, prefersReducedMotion, removeWithMotion, setOpen } from '../src/design/motion.js';

const TOKEN_FILE = new URL('../src/design/tokens.css', import.meta.url);
const GROUP_TITLES = {
  Text: '文字', 'Brand and accents': '品牌与强调色', Status: '状态', Surfaces: '背景', Borders: '边框',
  Gradients: '渐变', Navigation: '导航（侧栏）', Typography: '字体', Sizing: '尺寸', Radius: '圆角', Elevation: '阴影', Focus: '焦点', Motion: '动效',
};
const groupTitle = (title) => GROUP_TITLES[title]
  ?? (title.startsWith('Game semantics') ? '游戏语义色（乐队 / 属性 / 难度）' : title.startsWith('Spacing') ? '间距（数字即像素）' : title);

// ---------------------------------------------------------------- Tokens
function parseTokens(css) {
  const groups = [];
  let current = null;
  // Join multi-line values (e.g. layered gradients) onto their declaration line.
  const root = css.slice(css.indexOf(':root'), css.indexOf('@media')).replace(/(:|,)[ \t]*\r?\n\s*/g, '$1 ');
  for (const line of root.split(/\r?\n/)) {
    const heading = /\/\*\s*-+\s*(.+?)\s*-+\s*\*\//.exec(line);
    if (heading) {
      current = { title: heading[1], tokens: [] };
      groups.push(current);
      continue;
    }
    const token = /^\s*(--[\w-]+)\s*:\s*([^;]+);?\s*(?:\/\*\s*(.*?)\s*\*\/)?/.exec(line);
    if (token && current) current.tokens.push({ name: token[1], value: token[2].trim(), note: token[3] ?? '' });
  }
  return groups.filter((group) => group.tokens.length);
}

function sample(name) {
  const box = document.createElement('span');
  box.className = 'sg-token-sample';
  const value = `var(${name})`;
  if (/^--(color|gradient)-/.test(name)) {
    box.style.background = value;
    box.style.boxShadow = 'inset 0 0 0 1px var(--color-border)';
  } else if (name.startsWith('--shadow-')) {
    box.style.background = 'var(--color-surface)';
    box.style.boxShadow = value;
  } else if (name.startsWith('--radius-')) {
    box.style.background = 'var(--color-surface-accent)';
    box.style.borderRadius = value;
  } else if (name.startsWith('--space-')) {
    const bar = document.createElement('span');
    bar.style.cssText = `display:block;width:${value};height:20px;background:var(--color-accent-solid)`;
    box.append(bar);
  } else if (name.startsWith('--text-')) {
    box.textContent = '字';
    box.style.fontSize = value;
  } else if (name.startsWith('--weight-')) {
    box.textContent = '字';
    box.style.fontWeight = value;
  } else if (name.startsWith('--font-')) {
    box.textContent = 'Ag';
    box.style.fontFamily = value;
  } else {
    box.textContent = '·';
  }
  return box;
}

async function renderTokens() {
  const host = document.querySelector('#token-groups');
  try {
    const css = await (await fetch(TOKEN_FILE, { cache: 'no-cache' })).text();
    for (const group of parseTokens(css)) {
      const section = document.createElement('section');
      section.className = 'sg-token-group';
      const title = document.createElement('h3');
      title.textContent = groupTitle(group.title);
      const list = document.createElement('ul');
      list.className = 'sg-token-list';
      for (const token of group.tokens) {
        const item = document.createElement('li');
        item.className = 'sg-token';
        const text = document.createElement('span');
        text.innerHTML = '<span class="sg-token-name"></span><br><span class="sg-token-value"></span><br><span class="sg-token-note"></span>';
        text.querySelector('.sg-token-name').textContent = token.name;
        text.querySelector('.sg-token-value').textContent = token.value;
        text.querySelector('.sg-token-note').textContent = token.note;
        item.append(sample(token.name), text);
        list.append(item);
      }
      section.append(title, list);
      host.append(section);
    }
  } catch (error) {
    host.textContent = `无法读取 tokens.css：${error.message}`;
  }
}

// ------------------------------------------------------- Markup snippets
function addSnippets() {
  for (const component of document.querySelectorAll('#components .sg-component')) {
    const markup = [...component.querySelectorAll(':scope > .sg-demo')]
      .map((demo) => demo.innerHTML.replace(/^\n/, '').replace(/\s+$/, ''))
      .join('\n\n');
    const lines = markup.split('\n');
    const indent = Math.min(...lines.filter((line) => line.trim()).map((line) => line.match(/^\s*/)[0].length));
    const details = document.createElement('details');
    details.className = 'sg-code';
    details.innerHTML = '<summary>HTML</summary><pre><code></code></pre>';
    details.querySelector('code').textContent = lines.map((line) => line.slice(indent)).join('\n');
    component.append(details);
  }
}

// ------------------------------------------------------------- Demos
function wireComponentDemos() {
  document.addEventListener('click', (event) => {
    const chip = event.target.closest('[data-chip-demo] .ui-chip');
    if (chip) chip.setAttribute('aria-pressed', String(chip.getAttribute('aria-pressed') !== 'true'));
    const opener = event.target.closest('[data-open-dialog]');
    if (opener) document.getElementById(opener.dataset.openDialog)?.showModal();
    const closer = event.target.closest('[data-close-dialog]');
    if (closer) closer.closest('dialog')?.close();
  });
}

const songs = ['Yes! BanG_Dream!', 'ときめきエクスペリエンス！', '走り始めたばかりのキミに', 'しゅわりん☆どり~みん', 'Returns', 'R・I・O・T', '迷星叫', 'Sophie'];
let songIndex = 0;
function songRow() {
  const row = document.createElement('li');
  row.className = 'ui-list-row';
  row.innerHTML = '<span class="ui-list-row__main"></span><span class="ui-list-row__meta"></span>';
  row.querySelector('.ui-list-row__main').textContent = songs[songIndex % songs.length];
  row.querySelector('.ui-list-row__meta').textContent = `#${String(++songIndex).padStart(3, '0')}`;
  return row;
}

function wireMotionDemos() {
  const list = document.querySelector('#motion-list');
  for (let i = 0; i < 4; i += 1) list.append(songRow());
  document.querySelector('#motion-status').textContent = prefersReducedMotion()
    ? '当前系统开启了"减少动态效果"，下方示例只切换状态、不播放动画。'
    : '';
  document.addEventListener('click', async (event) => {
    const action = event.target.closest('[data-motion]')?.dataset.motion;
    if (action === 'add') {
      const row = songRow();
      list.prepend(row);
      enter(row);
    } else if (action === 'remove' && list.firstElementChild) {
      await removeWithMotion(list.firstElementChild);
    } else if (action === 'shuffle') {
      await flip(list, () => {
        const rows = [...list.children];
        for (let i = rows.length - 1; i > 0; i -= 1) {
          const j = Math.floor(Math.random() * (i + 1));
          [rows[i], rows[j]] = [rows[j], rows[i]];
        }
        list.append(...rows);
      });
    } else if (action === 'collapse') {
      const collapse = document.querySelector('#motion-collapse');
      setOpen(collapse, collapse.dataset.open !== 'true');
    } else if (action === 'flash') {
      const value = document.querySelector('#motion-value');
      value.textContent = (4_800_000 + Math.floor(Math.random() * 120_000)).toLocaleString('en-US');
      flash(value);
    } else if (action === 'page') {
      const panel = document.querySelector('#motion-page');
      panel.classList.remove('ui-motion-page');
      void panel.offsetWidth;
      panel.classList.add('ui-motion-page');
    }
  });
}

addSnippets();
wireComponentDemos();
wireMotionDemos();
await renderTokens();
