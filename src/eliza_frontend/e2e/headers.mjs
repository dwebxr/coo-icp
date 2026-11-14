// Checks the security headers of a deployed frontend, and that the Content-Security-Policy lets the app work and
// stops what it should. Run against a local asset-canister deployment (`make e2e-local` does) or the live site:
//   node e2e/headers.mjs http://<frontend canister id>.localhost:4943/
import { chromium } from 'playwright';

const url = process.argv[2];
if (!url) {
  console.error('usage: node e2e/headers.mjs <url of the deployed frontend>');
  process.exit(2);
}
const failures = [];
const check = (ok, what) => {
  console.log(`${ok ? 'PASS' : 'FAIL'} ${what}`);
  if (!ok) failures.push(what);
};

const response = await fetch(url);
const header = (name) => response.headers.get(name) ?? '';
const csp = header('content-security-policy');
for (const directive of [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self'",
  "frame-ancestors 'none'",
  "base-uri 'none'",
  "form-action 'none'",
  "object-src 'none'",
]) {
  check(csp.split(';').map((d) => d.trim()).includes(directive), `CSP has ${directive}`);
}
// (`http://localhost:*` is a port wildcard on this machine's own replica, not a host wildcard.)
check(!/unsafe-inline|unsafe-eval|[\s/]\*[\s;.]|\s\*$/.test(csp), 'CSP has no unsafe-inline, unsafe-eval or host wildcard');
check(header('x-frame-options') === 'DENY', 'X-Frame-Options: DENY');
check(header('x-content-type-options') === 'nosniff', 'X-Content-Type-Options: nosniff');
check(header('referrer-policy') === 'no-referrer', 'Referrer-Policy: no-referrer');
check(/max-age=\d+/.test(header('strict-transport-security')), 'Strict-Transport-Security');
check(/clipboard-write=\(self\)/.test(header('permissions-policy')), 'Permissions-Policy allows clipboard-write for the app');
check(/camera=\(\)/.test(header('permissions-policy')), 'Permissions-Policy turns the rest off');

const browser = await chromium.launch(process.env.PW_CHANNEL ? { channel: process.env.PW_CHANNEL } : {});
const context = await browser.newContext();
await context.grantPermissions(['clipboard-write']);
const page = await context.newPage();
const problems = [];
page.on('console', (m) => m.type() === 'error' && problems.push(m.text()));
page.on('pageerror', (e) => problems.push(e.message));
await page.goto(url);
await page.waitForSelector('.signin-actions:not(.is-hidden)', { timeout: 20000 });
const button = page.getByRole('button', { name: /sign in with/i });

check(problems.length === 0, `the page loads without errors or policy violations${problems.length ? `: ${problems[0]}` : ''}`);
check((await button.evaluate((el) => getComputedStyle(el).borderRadius)) !== '0px', 'the stylesheet is applied');
check(await page.locator('img').first().evaluate((el) => el.complete && el.naturalWidth > 0), 'images load');

const reachable = (target) =>
  page.evaluate(async (t) => {
    try {
      await fetch(t);
      return true;
    } catch {
      return false;
    }
  }, target);
check(await reachable('/api/v2/status'), 'the app can call the API on its own origin');
check(!(await reachable('https://example.com/')), 'it cannot connect to other sites');
await button.focus();
check(
  await page.evaluate(() => navigator.clipboard.writeText('x').then(() => true, () => false)),
  'writing to the clipboard is allowed (the Copy buttons)',
);
check(
  await page.evaluate(() => {
    const script = document.createElement('script');
    script.textContent = 'window.__inline = 1';
    document.head.appendChild(script);
    return window.__inline !== 1;
  }),
  'an injected inline script does not run',
);

await browser.close();
console.log(failures.length ? `\n${failures.length} failed` : '\nall passed');
process.exit(failures.length ? 1 : 0);
