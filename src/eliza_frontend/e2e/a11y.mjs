// Accessibility check (axe-core) of the main UI states against the dev server's mock backend.
// Usage: node e2e/a11y.mjs   BASE=<dev server url>   PW_CHANNEL=chrome to use an installed Chrome
import { chromium } from 'playwright';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
const axe = readFileSync(createRequire(import.meta.url).resolve('axe-core/axe.min.js'), 'utf8');
const BASE = process.env.BASE ?? 'http://localhost:5173';
const browser = await chromium.launch(process.env.PW_CHANNEL ? { channel: process.env.PW_CHANNEL } : {});
let violations = 0;
const cases = [
  ['signin dark', '?mock=signedout', 'dark'], ['signin light', '?mock=signedout', 'light'],
  ['chat dark', '?mock=rich', 'dark'], ['chat light', '?mock=chat', 'light'],
  ['empty light', '?mock=empty', 'light'], ['failed', '?mock=error&delay=200', 'dark', 'fail'],
  ['menu', '?mock=chat', 'light', 'menu'], ['dialog', '?mock=chat', 'dark', 'dialog'],
];
for (const [name, q, scheme, action] of cases) {
  const ctx = await browser.newContext({ colorScheme: scheme, reducedMotion: 'reduce' });
  const p = await ctx.newPage();
  await p.goto(`${BASE}/${q}`, { waitUntil: 'networkidle' });
  await p.waitForTimeout(900);
  if (action === 'fail') { await p.locator('textarea').fill('x'); await p.keyboard.press('Enter'); await p.waitForSelector('.msg-failed'); }
  if (action === 'menu') { await p.getByRole('button', { name: 'Account' }).click(); }
  if (action === 'dialog') { await p.getByRole('button', { name: 'Account' }).click(); await p.getByRole('menuitem', { name: /clear/i }).click(); }
  await p.waitForTimeout(300);
  await p.addScriptTag({ content: axe });
  const res = await p.evaluate(async () => (await window.axe.run(document, { resultTypes: ['violations'] })).violations.map((v) => `${v.impact} ${v.id}: ${v.help} [${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}]`));
  violations += res.length;
  console.log(`${name}: ${res.length ? '\n  ' + res.join('\n  ') : 'no violations'}`);
  await ctx.close();
}
await browser.close();
process.exit(violations ? 1 : 0);
