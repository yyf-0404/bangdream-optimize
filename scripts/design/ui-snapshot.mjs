#!/usr/bin/env node
// Visual regression helper for style changes (e.g. moving a page onto tokens).
//
//   node scripts/design/ui-snapshot.mjs capture http://127.0.0.1:8080 tmp/ui-before
//   …edit CSS…
//   node scripts/design/ui-snapshot.mjs capture http://127.0.0.1:8080 tmp/ui-after
//   node scripts/design/ui-snapshot.mjs compare tmp/ui-before tmp/ui-after
//
// capture: opens the five pages (desktop 1400px and phone 390px) and the
// styleguide in a fresh browser profile with a fixed clock and external
// requests blocked, then saves full-page screenshots plus the computed style
// of every visible element. compare: lists every element/property that changed.
//
// Needs Playwright, which the app itself does not depend on:
//   npm install --no-save playwright && npx playwright install chromium
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const PAGES = ['activity', 'cards', 'player', 'result', 'archive'];
const VIEWPORTS = { desktop: { width: 1400, height: 900 }, phone: { width: 390, height: 844 } };
const PROPS = [
  'display', 'color', 'backgroundColor', 'backgroundImage', 'fontFamily', 'fontSize', 'fontWeight', 'lineHeight',
  'letterSpacing', 'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft', 'marginTop', 'marginBottom',
  'borderTopWidth', 'borderTopColor', 'borderRightColor', 'borderBottomColor', 'borderLeftColor', 'borderRadius',
  'boxShadow', 'opacity', 'transform', 'width', 'height', 'gap', 'transitionDuration', 'animationName',
];

async function loadPlaywright() {
  try {
    return await import('playwright');
  } catch {
    console.error('需要 Playwright：npm install --no-save playwright && npx playwright install chromium');
    process.exit(2);
  }
}

function dumpStyles(props) {
  const result = [];
  const visit = (el, key) => {
    const rect = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    if (cs.display === 'none') return;
    if (rect.width || rect.height) {
      const entry = { key, cls: String(el.className?.baseVal ?? el.className ?? '').slice(0, 80) };
      for (const p of props) entry[p] = p === 'width' || p === 'height' ? Math.round(rect[p] * 10) / 10 : cs[p];
      result.push(entry);
    }
    let index = 0;
    for (const child of el.children) visit(child, `${key}>${child.tagName.toLowerCase()}:${index++}`);
  };
  visit(document.body, 'body');
  return result;
}

async function capture(base, outDir) {
  const { chromium } = await loadPlaywright();
  await mkdir(outDir, { recursive: true });
  const browser = await chromium.launch();
  const origin = new URL(base).host;
  try {
    for (const [label, viewport] of Object.entries(VIEWPORTS)) {
      const context = await browser.newContext({ viewport, deviceScaleFactor: 1, locale: 'zh-CN', timezoneId: 'Asia/Shanghai', reducedMotion: 'reduce' });
      const page = await context.newPage();
      await page.clock.setFixedTime(new Date('2026-11-08T12:00:00+08:00'));
      await page.route('**/*', (route) => (new URL(route.request().url()).host === origin ? route.continue() : route.abort()));
      const targets = [...PAGES.map((name) => [name, `${base}/#${name}`]), ['styleguide', `${base}/styleguide/`]];
      let appLoaded = false;
      for (const [name, url] of targets) {
        if (name === 'styleguide' || !appLoaded) {
          await page.goto(url, { waitUntil: 'networkidle' });
          await page.waitForTimeout(name === 'styleguide' ? 500 : 2500);
          appLoaded = name !== 'styleguide';
        } else {
          await page.evaluate((hash) => { location.hash = hash; }, name);
          await page.waitForTimeout(1200);
        }
        await page.evaluate(() => window.scrollTo(0, 0));
        await page.screenshot({ path: path.join(outDir, `${label}-${name}.png`), fullPage: true });
        const styles = await page.evaluate(dumpStyles, PROPS);
        await writeFile(path.join(outDir, `${label}-${name}.json`), JSON.stringify(styles));
        console.log(`captured ${label}-${name} (${styles.length} elements)`);
      }
      await context.close();
    }
  } finally {
    await browser.close();
  }
}

async function compare(dirA, dirB) {
  let changed = 0;
  for (const label of Object.keys(VIEWPORTS)) {
    for (const name of [...PAGES, 'styleguide']) {
      const file = `${label}-${name}`;
      let a; let b;
      try {
        a = JSON.parse(await readFile(path.join(dirA, `${file}.json`), 'utf8'));
        b = JSON.parse(await readFile(path.join(dirB, `${file}.json`), 'utf8'));
      } catch {
        console.log(`${file}: missing in one of the snapshots`);
        changed += 1;
        continue;
      }
      const byKey = new Map(b.map((entry) => [entry.key, entry]));
      // Group identical property changes so one token edit reads as one line.
      const groups = new Map();
      const structural = [];
      for (const entry of a) {
        const other = byKey.get(entry.key);
        if (!other) { structural.push(`  - ${entry.key} (${entry.cls}) disappeared`); continue; }
        byKey.delete(entry.key);
        for (const p of PROPS) {
          if (entry[p] === other[p]) continue;
          const change = `${p}: ${entry[p]} → ${other[p]}`;
          if (!groups.has(change)) groups.set(change, []);
          groups.get(change).push(entry.cls ? `.${entry.cls.trim().split(/\s+/).join('.')}` : entry.key.split('>').slice(-2).join('>'));
        }
      }
      for (const entry of byKey.values()) structural.push(`  + ${entry.key} (${entry.cls}) appeared`);
      const lines = [
        ...[...groups].sort((x, y) => y[1].length - x[1].length)
          .map(([change, where]) => `  ${change}  ×${where.length}  e.g. ${[...new Set(where)].slice(0, 3).join(', ')}`),
        ...structural,
      ];
      const sameImage = Buffer.compare(await readFile(path.join(dirA, `${file}.png`)), await readFile(path.join(dirB, `${file}.png`))) === 0;
      console.log(`${file}: ${lines.length ? `${lines.length} kinds of style change` : 'styles identical'}, screenshot ${sameImage ? 'identical' : 'changed'}`);
      for (const line of lines.slice(0, 30)) console.log(line);
      if (lines.length > 30) console.log(`  … ${lines.length - 30} more`);
      if (lines.length || !sameImage) changed += 1;
    }
  }
  process.exitCode = changed ? 1 : 0;
}

const [command, first, second] = process.argv.slice(2);
if (command === 'capture' && first && second) await capture(first.replace(/\/$/, ''), second);
else if (command === 'compare' && first && second) await compare(first, second);
else {
  console.error('usage: ui-snapshot.mjs capture <baseUrl> <outDir> | compare <dirA> <dirB>');
  process.exit(2);
}
