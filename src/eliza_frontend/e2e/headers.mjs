// Checks the security headers of a deployed frontend, and that the Content-Security-Policy lets the app work and
// stops what it should. Run against a local asset-canister deployment (`make e2e-local` does) or the live site:
//   node e2e/headers.mjs http://<frontend canister id>.localhost:4943/
// What the signed-in app does under the policy (queries, update calls, the certified read) is tested by e2e/local.mjs,
// which `make e2e-local` runs against the same deployment.
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
check(!/unsafe-inline|unsafe-eval|\*/.test(csp), 'CSP has no unsafe-inline, unsafe-eval or wildcard');
const connectSrc = csp.split(';').map((d) => d.trim()).find((d) => d.startsWith('connect-src ')) ?? '';
check(
  connectSrc === "connect-src 'self' https://icp-api.io",
  `connect-src is the app's own origin and the IC API only (${connectSrc || 'missing'})`,
);
check(header('x-frame-options') === 'DENY', 'X-Frame-Options: DENY');
check(header('x-content-type-options') === 'nosniff', 'X-Content-Type-Options: nosniff');
check(header('referrer-policy') === 'no-referrer', 'Referrer-Policy: no-referrer');
check(/max-age=\d+/.test(header('strict-transport-security')), 'Strict-Transport-Security');
const permissions = header('permissions-policy');
check(/clipboard-write=\(self\)/.test(permissions), 'Permissions-Policy allows clipboard-write for the app');
for (const feature of ['camera', 'publickey-credentials-create', 'publickey-credentials-get', 'window-management']) {
  check(permissions.includes(`${feature}=()`), `Permissions-Policy turns off ${feature}`);
}

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

// Tries to connect to `target` and tells "blocked by the policy" (the browser reports a violation for it) from "the
// network failed" (no violation, the request just did not succeed): a failed fetch alone says neither.
const connect = (target) =>
  page.evaluate(async (t) => {
    const url = new URL(t, location.href).href;
    const violations = [];
    const listener = (e) => violations.push(e.blockedURI);
    document.addEventListener('securitypolicyviolation', listener);
    let fetched = false;
    try {
      // no-cors: whether the other side allows reading the answer does not matter here, only whether the request
      // may leave.
      await fetch(url, { mode: 'no-cors', cache: 'no-store' });
      fetched = true;
    } catch {
      // Blocked, or the network failed: the violation (or its absence) tells which.
    }
    // The violation event is dispatched in a task of its own, after the fetch has failed.
    await new Promise((resolve) => setTimeout(resolve, 300));
    document.removeEventListener('securitypolicyviolation', listener);
    // Browsers report the blocked URL in full, or only its origin.
    const blocked = violations.some((v) => v && url.startsWith(v.replace(/\/$/, '')));
    return { fetched, blocked };
  }, target);
const own = await connect('/api/v2/status');
check(own.fetched && !own.blocked, 'the app can call the API on its own origin');
const other = await connect('https://example.com/');
check(other.blocked, 'the policy blocks connecting to other sites');
// Mainnet's API: allowed by the policy. Judged by the absence of a violation, so it holds offline too.
const ic = await connect('https://icp-api.io/api/v2/status');
check(
  !ic.blocked,
  `the policy allows https://icp-api.io${ic.blocked ? ' (blocked)' : ic.fetched ? '' : ' (allowed; not reached: offline?)'}`,
);
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
