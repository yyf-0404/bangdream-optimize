// Motion helpers for the patterns in motion.css.
//
// Timing always comes from the --motion-* tokens, so a reduced-motion
// preference (which zeroes those tokens) is honoured automatically. Every
// helper also resolves on a timer, because animationend never fires when
// animations are switched off.
//
//   import { enter, removeWithMotion, flip, setOpen, flash } from '../../design/motion.js';
//   list.append(row); enter(row);
//   await removeWithMotion(row);
//   await flip(list, () => sortRows(list));
//   setOpen(collapse, true);

const DEFAULT_ROOT = () => globalThis.document?.documentElement;

/** '150ms' → 150, '0.2s' → 200, anything else → 0. */
export function parseDuration(value) {
  const text = String(value ?? '').trim();
  const match = /^(-?\d*\.?\d+)(ms|s)$/.exec(text);
  if (!match) return 0;
  const amount = Number(match[1]) * (match[2] === 's' ? 1000 : 1);
  return Number.isFinite(amount) && amount > 0 ? amount : 0;
}

export function prefersReducedMotion(win = globalThis) {
  return Boolean(win.matchMedia?.('(prefers-reduced-motion: reduce)').matches);
}

function token(name, root = DEFAULT_ROOT()) {
  if (!root || !globalThis.getComputedStyle) return '';
  return getComputedStyle(root).getPropertyValue(name).trim();
}

/** Duration of --motion-duration-<name> in ms (instant | fast | normal | slow). */
export function motionDuration(name = 'normal', root) {
  return parseDuration(token(`--motion-duration-${name}`, root));
}

function waitForAnimation(el, timeoutMs) {
  return new Promise((resolve) => {
    let timer;
    const done = () => {
      clearTimeout(timer);
      el.removeEventListener('animationend', onEnd);
      el.removeEventListener('animationcancel', onEnd);
      resolve();
    };
    const onEnd = (event) => { if (event.target === el) done(); };
    el.addEventListener('animationend', onEnd);
    el.addEventListener('animationcancel', onEnd);
    timer = setTimeout(done, timeoutMs + 50);
  });
}

/** Play the enter animation on an element that was just inserted. */
export async function enter(el, { index = 0 } = {}) {
  if (!el) return;
  const delay = index * parseDuration(token('--motion-stagger'));
  el.style.setProperty('--ui-motion-index', String(index));
  el.classList.remove('ui-motion-exit');
  el.classList.add('ui-motion-enter');
  await waitForAnimation(el, motionDuration('normal') + delay);
  el.classList.remove('ui-motion-enter');
  el.style.removeProperty('--ui-motion-index');
}

/** Stagger the enter animation across several new elements. */
export function enterAll(elements) {
  return Promise.all([...elements].map((el, index) => enter(el, { index })));
}

/** Play the exit animation; the element stays in the DOM. */
export async function exit(el) {
  if (!el) return;
  el.classList.remove('ui-motion-enter');
  el.classList.add('ui-motion-exit');
  await waitForAnimation(el, motionDuration('fast'));
}

/** Exit, then remove the element. */
export async function removeWithMotion(el) {
  if (!el) return;
  await exit(el);
  el.remove();
}

/** Briefly highlight an element whose value changed. */
export async function flash(el) {
  if (!el) return;
  el.classList.remove('ui-motion-flash');
  void el.offsetWidth; // restart the animation when flashed repeatedly
  el.classList.add('ui-motion-flash');
  await waitForAnimation(el, motionDuration('slow') * 2);
  el.classList.remove('ui-motion-flash');
}

/** Open or close a .ui-collapse container (animated by CSS). */
export function setOpen(collapse, open) {
  if (!collapse) return;
  collapse.dataset.open = String(Boolean(open));
  const trigger = collapse.id && globalThis.document?.querySelector(`[aria-controls="${collapse.id}"]`);
  trigger?.setAttribute('aria-expanded', String(Boolean(open)));
}

/**
 * Animate a container's children from their old positions to new ones
 * (FLIP). Use for sorting, filtering or inserting rows. New children enter.
 */
export async function flip(container, mutate) {
  const before = new Map([...container.children].map((el) => [el, el.getBoundingClientRect()]));
  await mutate();
  const duration = motionDuration('normal');
  const easing = token('--motion-ease-standard') || 'ease';
  const running = [];
  for (const el of container.children) {
    const first = before.get(el);
    if (!first) {
      running.push(enter(el));
      continue;
    }
    if (!duration || typeof el.animate !== 'function') continue;
    const last = el.getBoundingClientRect();
    const dx = first.left - last.left;
    const dy = first.top - last.top;
    if (!dx && !dy) continue;
    running.push(
      el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }], { duration, easing })
        .finished.catch(() => {}),
    );
  }
  await Promise.all(running);
}
