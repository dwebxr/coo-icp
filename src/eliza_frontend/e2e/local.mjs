// End-to-end tests of the real canister client (src/backend.ts) against a canister on a local replica.
// The mock is not involved: every message goes through the agent, candid, canister_inspect_message and the
// canister's own checks. Sign-in uses the development-only `?dev-identity` (Internet Identity needs a passkey).
//
// `make e2e-local` runs them against the local asset canister, which serves a bundle built for this test (with
// ?dev-identity) under the security headers it sends in production: any Content-Security-Policy violation, and any
// console error, fails the test it happens in. By hand, against the dev server (no CSP there):
//   dfx start --background && dfx deploy eliza_backend --network local
//   dfx canister call eliza_backend set_llm_provider '(variant { Fallback })' --network local
//   npm run dev        (start it after the deploy: it reads the canister id from the repository's .env)
//   npm run e2e:local
// Usage: node e2e/local.mjs   BASE=<url of the app>   PW_CHANNEL=chrome to use an installed Chrome
import { chromium } from 'playwright';

const BASE = process.env.BASE ?? 'http://localhost:5173';
const browser = await chromium.launch(process.env.PW_CHANNEL ? { channel: process.env.PW_CHANNEL } : {});
const results = [];

/** Each test gets its own browser context, and so its own identity. */
async function test(name, fn) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const errors = [];
  // Violations of the Content-Security-Policy, reported from every page and every reload of the context.
  await ctx.exposeBinding('__cspViolation', (_source, text) => errors.push(`CSP violation: ${text}`));
  await ctx.addInitScript(() =>
    document.addEventListener('securitypolicyviolation', (e) =>
      window.__cspViolation(`${e.effectiveDirective} blocked ${e.blockedURI || '(inline)'}`),
    ),
  );
  const page = await ctx.newPage();
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(`console error: ${m.text()}`));
  try {
    await fn(page, ctx);
    if (errors.length) throw new Error('page errors: ' + errors.join('; '));
    results.push(`PASS ${name}`);
  } catch (e) {
    const first = e.message.split('\n')[0];
    // A timeout is often only the consequence: say what went wrong on the page first.
    const cause = errors.length && !first.startsWith('page errors') ? ` (on the page: ${errors[0]})` : '';
    results.push(`FAIL ${name}: ${first}${cause}`);
  } finally {
    await ctx.close();
  }
}

const expect = (cond, msg) => {
  if (!cond) throw new Error(msg);
};
const loaded = (p) => p.waitForSelector('.thread[aria-busy="false"]', { timeout: 20000 });
const userBubbles = (p) => p.locator('.msg-user .bubble').count();
const replies = (p) => p.locator('.msg-assistant:not(.thinking)').count();

async function signIn(p, name = 'default') {
  await p.goto(`${BASE}/?dev-identity=${name}`);
  await p.getByRole('button', { name: /sign in/i }).click();
  await loaded(p);
}

/** Sends a message and waits for its reply (an update call: a few seconds on a local replica). */
async function send(p, text) {
  const before = await replies(p);
  await p.locator('textarea').fill(text);
  await p.locator('.send-btn').click();
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, before, {
    timeout: 30000,
  });
}

await test('Sign in, chat, and find the conversation again after a reload', async (p) => {
  await signIn(p);
  expect((await p.locator('.empty').count()) === 1, 'a new identity starts with an empty conversation');
  await send(p, 'hello canister');
  const reply = await p.locator('.msg-assistant .md').last().innerText();
  expect(reply.includes('Local fallback') && reply.includes('hello canister'), `fallback reply: ${reply.slice(0, 80)}`);
  await send(p, 'second message');
  expect((await userBubbles(p)) === 2 && (await replies(p)) === 2, 'two exchanges');

  await p.reload();
  await loaded(p);
  expect((await userBubbles(p)) === 2 && (await replies(p)) === 2, 'the session and the conversation are restored');
  expect((await p.locator('.msg-user .bubble').first().innerText()) === 'hello canister', 'in order');
  await send(p, 'third message');
  expect((await replies(p)) === 3, 'the conversation continues from the restored revision');
});

await test('Another identity has its own conversation', async (p) => {
  await signIn(p, 'someone-else');
  expect((await p.locator('.empty').count()) === 1, 'nothing of the other identity is visible');
});

await test('Clear empties the conversation on the canister', async (p) => {
  await signIn(p);
  await send(p, 'to be cleared');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.locator('dialog').getByRole('button', { name: 'Clear' }).click();
  await p.waitForSelector('.empty', { timeout: 20000 });
  await p.reload();
  await loaded(p);
  expect((await p.locator('.empty').count()) === 1, 'still empty after a reload');
  await send(p, 'after the clear');
  expect((await userBubbles(p)) === 1, 'the next message continues the cleared conversation');
});

await test('The longest message the canister accepts is accepted; the composer stops a longer one', async (p) => {
  await signIn(p);
  // Exactly the limit, in ASCII: a canister whose limit were lower would refuse it.
  await p.locator('textarea').fill('x'.repeat(4001));
  expect(await p.locator('.send-btn').isDisabled(), '4 001 bytes cannot be sent');
  await send(p, 'x'.repeat(4000));
  expect((await p.locator('.msg-failed').count()) === 0, '4 000 bytes are not refused');
  expect((await userBubbles(p)) === 1, '4 000 bytes went through');
  // In UTF-8: bytes, not characters.
  await p.locator('textarea').fill('あ'.repeat(1334));
  expect(await p.locator('.send-btn').isDisabled(), '4 002 bytes cannot be sent');
  await send(p, 'あ'.repeat(1333));
  expect((await p.locator('.msg-failed').count()) === 0, '3 999 bytes are not refused');
  expect((await userBubbles(p)) === 2, '3 999 bytes went through');
});

await test('A floor above the canister\'s revision is settled by a certified read', async (p) => {
  await signIn(p);
  await send(p, 'one');
  const key = await p.evaluate(() => Object.keys(localStorage).find((name) => name.startsWith('coo:floor:')));
  expect(!!key, 'a floor was stored');
  // As if the canister had gone back to an older state.
  await p.evaluate((k) => localStorage.setItem(k, '0:99'), key);
  await p.reload();
  await loaded(p);
  expect((await p.locator('.state-card').count()) === 0, 'no error');
  expect((await userBubbles(p)) === 1, 'the conversation is shown');
  const stored = await p.evaluate((k) => localStorage.getItem(k), key);
  expect(stored === '1:1', `the floor follows the canister (${stored})`);
  await send(p, 'two');
  expect((await replies(p)) === 2, 'and the conversation goes on');
});

await test('Signing out ends the session for good', async (p) => {
  await signIn(p);
  await send(p, 'remember me');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.waitForSelector('.signin');
  await p.reload();
  await p.waitForSelector('.signin-actions:not(.is-hidden)');
  expect((await p.locator('.chat').count()) === 0, 'a reload does not bring the session back');
});

await browser.close();
console.log(results.join('\n'));
const failed = results.filter((r) => r.startsWith('FAIL')).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
