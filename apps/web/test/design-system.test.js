import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import test from 'node:test';
import { parseDuration } from '../src/design/motion.js';

const web = new URL('../', import.meta.url);
const read = (path) => readFile(new URL(path, web), 'utf8');

const TOKEN_FILE = 'src/design/tokens.css';

async function stylesheets(dir = '', found = []) {
  for (const entry of await readdir(new URL(dir || '.', web), { withFileTypes: true })) {
    const path = dir + entry.name;
    if (entry.isDirectory()) {
      if (!['test', 'pkg', 'vendor', 'node_modules'].includes(entry.name)) await stylesheets(`${path}/`, found);
    } else if (entry.name.endsWith('.css')) {
      found.push(path);
    }
  }
  return found;
}

const stripComments = (css) => css.replace(/\/\*[\s\S]*?\*\//g, '');

function declarations(css) {
  const result = [];
  for (const block of stripComments(css).matchAll(/\{([^{}]*)\}/g)) {
    for (const part of block[1].split(';')) {
      const colon = part.indexOf(':');
      if (colon > 0) result.push({ property: part.slice(0, colon).trim(), value: part.slice(colon + 1).trim() });
    }
  }
  return result;
}

const NAMED_COLORS = /\b(white|black|red|green|blue|gray|grey|silver|pink|purple|orange|yellow|navy|teal|maroon|olive|lime|aqua|fuchsia|gold|beige|brown|crimson|indigo|violet|magenta|cyan)\b/i;
const COLOR_FUNCTION = /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch)\(/i;
const HEX = /#[0-9a-f]{3,8}\b/i;
const NONZERO_DURATION = /(?<![\w-])(?:\d*\.\d+|[1-9]\d*)m?s\b/;
const CSS_URL = /\burl\(\s*(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^)]*)\s*\)/gi;

function literalProblems(css) {
  const problems = [];
  for (const { property, value } of declarations(css)) {
    // URL contents belong to the asset, including the alpha mask of inline SVG icons.
    const styleValue = value.replace(CSS_URL, '');
    if (HEX.test(styleValue) || COLOR_FUNCTION.test(styleValue) || NAMED_COLORS.test(styleValue)) problems.push(`${property}: ${value}`);
    if (/^(transition|animation|--)/.test(property) && NONZERO_DURATION.test(styleValue)) problems.push(`${property}: ${value}`);
  }
  return problems;
}

function definedTokens(css) {
  return new Set([...stripComments(css).matchAll(/(--[\w-]+)\s*:/g)].map((match) => match[1]));
}

test('stylesheets use design tokens instead of literal colours and durations', async () => {
  const files = (await stylesheets()).filter((file) => file !== TOKEN_FILE);
  assert.ok(files.includes('src/design/components.css'));
  assert.ok(files.includes('src/design/motion.css'));
  for (const file of files) {
    assert.deepEqual(literalProblems(await read(file)), [], `${file} must use var(--…) tokens`);
  }
});

test('tokens.css parses as a single :root rule plus the reduced-motion block', async () => {
  // A stray "*/" inside a comment silently drops the whole :root rule.
  const css = stripComments(await read(TOKEN_FILE)).trim();
  assert.ok(css.startsWith(':root {'), 'only comments may precede :root in tokens.css');
  assert.doesNotMatch(css, /\*\//, 'unbalanced comment terminator in tokens.css');
  for (const file of ['src/design/components.css', 'src/design/motion.css', 'styleguide/styleguide.css']) {
    assert.doesNotMatch(stripComments(await read(file)), /\*\//, `unbalanced comment terminator in ${file}`);
  }
});

test('every CSS custom property has a definition', async () => {
  const files = await stylesheets();
  const sources = await Promise.all(files.map(read));
  const definitions = definedTokens(sources.join('\n'));
  for (const [index, file] of files.entries()) {
    const css = stripComments(sources[index]);
    for (const [, name, separator] of css.matchAll(/var\((--[\w-]+)\s*([,)])/g)) {
      // motion.js supplies the stagger index on each entering element.
      const hasInlineDefault = name === '--ui-motion-index' && separator === ',';
      assert.ok(definitions.has(name) || hasInlineDefault, `${file} uses undefined custom property ${name}`);
    }
  }
});

test('every base component has an example on the styleguide page', async () => {
  const components = stripComments(await read('src/design/components.css'));
  const styleguide = await read('styleguide/index.html');
  const blocks = new Set([...components.matchAll(/\.(ui-[a-z]+(?:-[a-z]+)*)(?![\w-])/g)].map((match) => match[1]));
  blocks.delete('ui-visually-hidden');
  blocks.delete('ui-mono');
  assert.ok(blocks.size >= 10);
  for (const block of blocks) {
    assert.match(styleguide, new RegExp(`class="[^"]*\\b${block}\\b`), `${block} needs an example in styleguide/index.html`);
  }
});

test('reduced motion zeroes every motion duration and distance token', async () => {
  const css = await read(TOKEN_FILE);
  const [root, reduced] = css.split('@media (prefers-reduced-motion: reduce)');
  assert.ok(reduced, 'tokens.css needs a prefers-reduced-motion block');
  for (const [, name] of root.matchAll(/(--motion-(?:duration|distance|loop)-[\w-]+)\s*:/g)) {
    assert.match(reduced, new RegExp(`${name}\\s*:\\s*0(?:ms|px)?;`), `${name} must drop to zero under reduced motion`);
  }
});

test('components load in the last cascade layer so page styles cannot override them', async () => {
  const design = await read('design.css');
  const order = /@layer\s+([^;]+);/.exec(design)[1].split(',').map((name) => name.trim());
  assert.equal(order[0], 'tokens');
  assert.equal(order.at(-1), 'components');
  assert.match(design, /@import url\('\.\/src\/design\/tokens\.css'\) layer\(tokens\);/);
  assert.match(design, /@import url\('\.\/src\/design\/components\.css'\) layer\(components\);/);
  assert.match(design, /@import url\('\.\/src\/design\/motion\.css'\) layer\(components\);/);
});

test('motion helpers read token durations in milliseconds', () => {
  assert.equal(parseDuration('150ms'), 150);
  assert.equal(parseDuration(' 0.22s '), 220);
  assert.equal(parseDuration('0ms'), 0);
  assert.equal(parseDuration(''), 0);
  assert.equal(parseDuration('fast'), 0);
});
