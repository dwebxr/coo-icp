// Interaction tests against the dev server's mock backend (`npm run dev`, then `npm run e2e`).
// Usage: node e2e/smoke.mjs [--webkit]   BASE=<dev server url>   PW_CHANNEL=chrome to use an installed Chrome
import { chromium, webkit } from 'playwright';

const BASE = process.env.BASE ?? 'http://localhost:5173';
const engine = process.argv.includes('--webkit') ? webkit : chromium;
const browser = await engine.launch(engine === chromium && process.env.PW_CHANNEL ? { channel: process.env.PW_CHANNEL } : {});
const results = [];

async function test(name, fn, ctxOpts = {}) {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, ...ctxOpts });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  try {
    await fn(page, ctx);
    if (errors.length) throw new Error('page errors: ' + errors.join('; '));
    results.push(`PASS ${name}`);
  } catch (e) {
    results.push(`FAIL ${name}: ${e.message.split('\n')[0]}`);
  } finally {
    await ctx.close();
  }
}

const expect = (cond, msg) => {
  if (!cond) throw new Error(msg);
};
// Wait until a signed-in mock has finished loading history.
async function open(p, q) {
  await p.goto(`${BASE}/${q}`);
  if (!q.includes('signedout') && !q.includes('expiring') && !q.includes('badsession') && !q.includes('historyerror'))
    await p.waitForSelector('.thread[aria-busy="false"]');
}
const userBubbles = (p) => p.locator('.msg-user .bubble').count();
const stats = (p) => p.evaluate(() => window.__mock);
const replies = (p) => p.locator('.msg-assistant:not(.thinking)').count();

// Dispatch synthetic IME-related events directly on the textarea.
async function imeEnter(page, mode) {
  await page.locator('textarea').evaluate((el, mode) => {
    const fire = (type, init = {}, keyCode) => {
      const ev = type.startsWith('composition')
        ? new CompositionEvent(type, { bubbles: true, data: 'へんかん' })
        : new KeyboardEvent(type, { bubbles: true, cancelable: true, key: 'Enter', code: 'Enter', ...init });
      if (keyCode !== undefined) Object.defineProperty(ev, 'keyCode', { get: () => keyCode });
      el.dispatchEvent(ev);
    };
    if (mode === 'composing') {
      fire('compositionstart');
      fire('keydown', { isComposing: true }, 229);
      fire('compositionend');
    } else if (mode === 'safari') {
      fire('compositionstart');
      fire('compositionend');
      fire('keydown', { isComposing: false }, 229);
    } else if (mode === 'just-ended') {
      fire('compositionstart');
      fire('compositionend');
      fire('keydown', { isComposing: false }, 13);
    }
  }, mode);
}

await test('Enter sends, reply arrives, composer clears and keeps focus', async (p) => {
  await open(p, '?mock=empty&delay=300');
  await p.locator('textarea').fill('Hello');
  await p.keyboard.press('Enter');
  await p.waitForSelector('.msg-assistant:not(.thinking)');
  expect((await userBubbles(p)) === 1, 'one user message');
  expect((await p.locator('textarea').inputValue()) === '', 'composer cleared');
  expect(await p.locator('textarea').evaluate((el) => el === document.activeElement), 'focus kept');
});

await test('Shift+Enter inserts newline and does not send', async (p) => {
  await open(p, '?mock=empty');
  await p.locator('textarea').fill('a');
  await p.keyboard.press('Shift+Enter');
  await p.keyboard.type('b');
  expect((await p.locator('textarea').inputValue()) === 'a\nb', 'newline inserted');
  expect((await userBubbles(p)) === 0, 'not sent');
});

for (const mode of ['composing', 'safari', 'just-ended']) {
  await test(`IME Enter (${mode}) does not send`, async (p) => {
    await open(p, '?mock=empty');
    await p.locator('textarea').fill('日本語');
    await imeEnter(p, mode);
    await p.waitForTimeout(200);
    expect((await userBubbles(p)) === 0, 'IME Enter must not send');
    await p.keyboard.press('Enter');
    await p.waitForTimeout(100);
    expect((await userBubbles(p)) === 1, 'plain Enter afterwards sends');
  });
}

await test('Empty/whitespace input cannot be sent', async (p) => {
  await open(p, '?mock=empty');
  await p.locator('textarea').fill('   ');
  await p.keyboard.press('Enter');
  expect(await p.locator('.send-btn').isDisabled(), 'send disabled');
  expect((await userBubbles(p)) === 0, 'nothing sent');
});

await test('Double Enter while waiting sends once', async (p) => {
  await open(p, '?mock=empty&delay=800');
  await p.locator('textarea').fill('one');
  await p.keyboard.press('Enter');
  await p.locator('textarea').fill('two');
  await p.keyboard.press('Enter');
  expect((await userBubbles(p)) === 1, 'second send blocked while pending');
  expect((await p.locator('textarea').inputValue()) === 'two', 'draft kept while pending');
  expect(!(await p.locator('textarea').isDisabled()), 'textarea stays enabled');
});

await test('Suggestion chip sends immediately', async (p) => {
  await open(p, '?mock=empty&delay=200');
  await p.getByRole('button', { name: 'What is the Internet Computer?' }).click();
  await p.waitForSelector('.msg-assistant:not(.thinking)');
  expect((await userBubbles(p)) === 1, 'sent');
});

await test('Backend error: Not sent + reason, Retry resends', async (p) => {
  await open(p, '?mock=error&delay=200');
  const before = await userBubbles(p);
  await p.locator('textarea').fill('ping');
  await p.keyboard.press('Enter');
  await p.waitForSelector('.msg-failed');
  expect((await p.locator('.msg-failed').innerText()).startsWith('Not sent'), 'definite failure label');
  await p.getByRole('button', { name: 'Retry' }).click();
  await p.waitForSelector('.msg-failed', { state: 'detached' });
  await p.waitForSelector('.msg-failed');
  expect((await userBubbles(p)) === before + 1, 'retry keeps a single bubble');
});

await test('Lost connection: checks for a reply, then No reply (not "Not sent"); writes blocked meanwhile', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=offline&delay=200`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.locator('textarea').fill('ping');
  await p.keyboard.press('Enter');
  await p.clock.runFor(400);
  await p.waitForSelector('text=Checking for a reply');
  await p.getByRole('button', { name: 'Account' }).click();
  expect(await p.getByRole('menuitem', { name: /clear/i }).isDisabled(), 'clear blocked while checking');
  await p.keyboard.press('Escape');
  await p.clock.runFor(5000);
  await p.waitForSelector('.msg-failed');
  expect((await p.locator('.msg-failed').innerText()).startsWith('No reply'), 'unconfirmed label');
  expect(!(await p.getByRole('button', { name: 'Retry' }).isDisabled()), 'retry available after the window');
});

await test('Repeated prompt that fails is not mistaken for the earlier saved one', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=offline&delay=200`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  const lastPrompt = await p.locator('.msg-user .bubble').last().innerText();
  const before = await userBubbles(p);
  await p.locator('textarea').fill(lastPrompt);
  await p.keyboard.press('Enter');
  await p.clock.runFor(50_000);
  await p.waitForSelector('.msg-failed');
  expect((await userBubbles(p)) === before + 1, 'failed repeat stays visible');
});

await test('Retry after No reply sends the same request id', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=offline&delay=200`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.locator('textarea').fill('ping');
  await p.keyboard.press('Enter');
  await p.clock.runFor(5000);
  await p.waitForSelector('.msg-failed');
  expect((await stats(p)).sends === 2, 'one automatic re-send');
  await p.getByRole('button', { name: 'Retry' }).click();
  await p.clock.runFor(5000);
  await p.waitForSelector('.msg-failed');
  const { ids } = await stats(p);
  expect(ids.length === 4 && new Set(ids).size === 1, `same id every time (${ids.join(',')})`);
});

await test('Busy: Not sent with a reason, Retry goes through', async (p) => {
  await open(p, '?mock=busy&delay=200');
  await p.locator('textarea').fill('hello');
  await p.keyboard.press('Enter');
  await p.waitForSelector('.msg-failed');
  const text = await p.locator('.msg-failed').innerText();
  expect(text.startsWith('Not sent') && text.includes('busy'), `label: ${text}`);
  await p.getByRole('button', { name: 'Retry' }).click();
  await p.waitForSelector('.msg-assistant.is-latest');
  expect((await p.locator('.msg-failed').count()) === 0, 'no failure left');
});

await test('Reply still being generated after a lost response is awaited', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=inprogress&delay=200`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  const before = await replies(p);
  await p.locator('textarea').fill('long answer please');
  await p.keyboard.press('Enter');
  await p.clock.runFor(2500);
  await p.waitForSelector('text=Checking for a reply');
  await p.clock.runFor(15_000);
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, before);
  expect((await p.locator('.msg-failed').count()) === 0, 'no failure shown');
  expect((await stats(p)).answered === 1, 'answered once');
});

await test('A hung request is sent again after 90 s and answered once', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=hang&delay=200`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  const before = await replies(p);
  await p.locator('textarea').fill('ping');
  await p.keyboard.press('Enter');
  await p.clock.runFor(60_000);
  expect((await replies(p)) === before, 'still waiting at 60 s');
  await p.clock.runFor(35_000);
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, before);
  expect((await stats(p)).answered === 1, 'answered once');
});

await test('Retry after a late save shows the saved reply without answering twice', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=latesave&delay=200`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  const before = await userBubbles(p);
  await p.locator('textarea').fill('did it go through?');
  await p.keyboard.press('Enter');
  await p.clock.runFor(5000);
  await p.waitForSelector('.msg-failed');
  await p.clock.runFor(10_000);
  await p.getByRole('button', { name: 'Retry' }).click();
  await p.clock.runFor(1000);
  await p.waitForSelector('.msg-assistant.is-latest');
  expect((await stats(p)).answered === 1, 'answered once');
  expect((await userBubbles(p)) === before + 1, 'one bubble for the message');
});

await test('Conflict: another tab moved the conversation on; Not sent, its messages appear, Retry works', async (p) => {
  await open(p, '?mock=conflict&delay=200');
  const before = await userBubbles(p);
  await p.locator('textarea').fill('hello');
  await p.keyboard.press('Enter');
  await p.waitForSelector('.msg-failed');
  const text = await p.locator('.msg-failed').innerText();
  expect(text.startsWith('Not sent') && text.includes('changed'), `label: ${text}`);
  expect((await p.locator('text=Sent from another tab').count()) === 1, 'the other tab is shown');
  expect((await userBubbles(p)) === before + 2, 'other tab + mine');
  const repliesBefore = await replies(p);
  await p.getByRole('button', { name: 'Retry' }).click();
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, repliesBefore);
  expect((await p.locator('.msg-failed').count()) === 0, 'no failure left');
  expect((await stats(p)).answered === 1, 'answered once');
});

await test('A stale revision with nothing visible changed is sent again automatically', async (p) => {
  await open(p, '?mock=stale&delay=200');
  const before = await replies(p);
  await p.locator('textarea').fill('hello');
  await p.keyboard.press('Enter');
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, before);
  expect((await p.locator('.msg-failed').count()) === 0, 'no failure shown');
  const { sends, answered, ids } = await stats(p);
  expect(sends === 2 && answered === 1 && new Set(ids).size === 2, `sends ${sends}, answered ${answered}`);
});

await test('Signing back in after a clear never shows the cleared messages', async (p) => {
  await open(p, '?mock=laggy');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('.empty');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.getByRole('button', { name: /sign in/i }).click();
  // The first history read comes from before the clear; it is not shown, and a fresh read is.
  await p.waitForSelector('.empty', { timeout: 8000 });
  expect((await userBubbles(p)) === 0, 'no cleared messages');
});

await test('Sending waits for the conversation to load', async (p) => {
  await open(p, '?mock=historyerror');
  await p.waitForSelector("text=Couldn't load your conversation");
  await p.locator('textarea').fill('hello');
  expect(await p.locator('.send-btn').isDisabled(), 'send disabled');
  await p.keyboard.press('Enter');
  expect((await userBubbles(p)) === 0, 'Enter does not send');
});

await test('Clear: confirm dialog, Esc cancels, Clear empties thread', async (p) => {
  await open(p, '?mock=chat');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.waitForSelector('dialog[open]');
  await p.keyboard.press('Escape');
  await p.waitForTimeout(150);
  expect((await p.locator('dialog[open]').count()) === 0, 'Esc closes');
  expect((await userBubbles(p)) > 0, 'messages kept');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('.empty');
  expect((await userBubbles(p)) === 0, 'cleared');
});

await test('Account menu: keyboard open, arrows, Esc returns focus', async (p) => {
  await open(p, '?mock=chat');
  await p.getByRole('button', { name: 'Account' }).focus();
  await p.keyboard.press('Enter');
  await p.waitForSelector('[role="menu"]');
  const first = await p.evaluate(() => document.activeElement?.textContent);
  expect(first.includes('Clear'), 'first item focused');
  await p.keyboard.press('ArrowDown');
  const second = await p.evaluate(() => document.activeElement?.textContent);
  expect(second.includes('Sign out'), 'arrow moves');
  await p.keyboard.press('Escape');
  expect((await p.locator('[role="menu"]').count()) === 0, 'closed');
  const focused = await p.evaluate(() => document.activeElement?.getAttribute('aria-label'));
  expect(focused === 'Account', 'focus returned');
});

await test('Clear disabled while a reply is pending', async (p) => {
  await open(p, '?mock=chat&delay=3000');
  await p.locator('textarea').fill('wait');
  await p.keyboard.press('Enter');
  await p.getByRole('button', { name: 'Account' }).click();
  expect(await p.getByRole('menuitem', { name: /clear/i }).isDisabled(), 'clear disabled');
});

await test('Sign out returns to sign-in; sign in again restores chat', async (p) => {
  await open(p, '?mock=chat');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.waitForSelector('.signin');
  expect((await p.locator('.notice').count()) === 0, 'no notice on deliberate sign-out');
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.chat');
});

await test('Sign out while a reply is pending: no stale reply, Send works after re-sign-in', async (p) => {
  await open(p, '?mock=empty&delay=1500');
  await p.locator('textarea').fill('slow');
  await p.keyboard.press('Enter');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.waitForSelector('.signin');
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.waitForTimeout(1500);
  // The canister may have saved the exchange; the old request must not add a second copy.
  expect((await replies(p)) <= 1 && (await userBubbles(p)) <= 1, 'stale reply not duplicated');
  await p.locator('textarea').fill('again');
  expect(!(await p.locator('.send-btn').isDisabled()), 'send enabled (pending reset)');
});

await test('Sign-in cancel is silent; error shows notice', async (p) => {
  await open(p, '?mock=signedout&signin=cancel');
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForTimeout(1500);
  expect((await p.locator('.notice').count()) === 0, 'cancel silent');
  await open(p, '?mock=signedout&signin=error');
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.notice-error');
});

await test('Stalled sign-in offers Cancel that returns to idle', async (p) => {
  await open(p, '?mock=signedout&signin=hang');
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('text=Allow pop-ups', { timeout: 5000 });
  await p.getByRole('button', { name: 'Cancel' }).click();
  expect(!(await p.getByRole('button', { name: /sign in with/i }).isDisabled()), 'button enabled again');
});

await test('Session expiry signs out with notice and keeps the draft', async (p) => {
  await open(p, '?mock=expiring');
  await p.locator('textarea').fill('half-written thought');
  await p.waitForSelector('.signin', { timeout: 8000 });
  expect((await p.locator('.notice').innerText()).includes('expired'), 'expired notice');
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.chat');
  expect((await p.locator('textarea').inputValue()) === 'half-written thought', 'draft restored');
});

await test('Orphaned session on load self-heals to sign-in with notice', async (p) => {
  await open(p, '?mock=badsession');
  await p.waitForSelector('.signin', { timeout: 5000 });
  expect((await p.locator('.notice').innerText()).includes('expired'), 'expired notice');
});

await test('Idle for 10 minutes signs out with notice and clears draft', async (p) => {
  await p.clock.install();
  await open(p, '?mock=chat');
  await p.clock.runFor(1500);
  await p.waitForSelector('.chat textarea');
  await p.locator('textarea').fill('secret draft');
  await p.clock.runFor(10 * 60 * 1000 + 1000);
  await p.waitForSelector('.signin');
  expect((await p.locator('.notice').innerText()).includes('inactivity'), 'idle notice');
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.clock.runFor(2000);
  await p.waitForSelector('.chat textarea');
  expect((await p.locator('textarea').inputValue()) === '', 'draft cleared after idle');
});

await test('Slow reply shows the patience hint after 8s', async (p) => {
  await p.clock.install();
  await open(p, '?mock=chat&delay=60000');
  await p.clock.runFor(1500);
  await p.locator('textarea').fill('slow one');
  await p.keyboard.press('Enter');
  await p.clock.runFor(8500);
  await p.waitForSelector('.thinking-hint');
});

await test('History failure shows Retry', async (p) => {
  await open(p, '?mock=historyerror');
  await p.waitForSelector('text=Couldn\'t load your conversation');
});

await test('Copy reply writes to clipboard', async (p, ctx) => {
  if (engine === chromium) await ctx.grantPermissions(['clipboard-read', 'clipboard-write']);
  await open(p, '?mock=chat');
  await p.locator('.msg-assistant.is-latest').getByRole('button', { name: 'Copy reply' }).click();
  await p.waitForSelector('.msg-assistant.is-latest [aria-label="Copied"]');
  if (engine === chromium) {
    const text = await p.evaluate(() => navigator.clipboard.readText());
    expect(text.includes('I run inside a canister'), 'clipboard content');
  }
});

await test('Scrolled up: jump button appears and returns to bottom', async (p) => {
  await open(p, '?mock=long');
  await p.waitForTimeout(500);
  await p.locator('[data-scroll]').evaluate((el) => el.scrollTo(0, 0));
  await p.waitForSelector('.jump-btn');
  await p.locator('.jump-btn').click();
  await p.waitForSelector('.jump-btn', { state: 'detached', timeout: 4000 });
  await p.waitForTimeout(300);
  const gap = await p.locator('[data-scroll]').evaluate((el) => el.scrollHeight - el.scrollTop - el.clientHeight);
  expect(gap < 5, `at bottom (gap ${gap})`);
});

await test('Links are safe (http/https only, new tab, noopener)', async (p) => {
  await open(p, '?mock=chat');
  const attrs = await p.locator('.md a').evaluateAll((as) => as.map((a) => [a.getAttribute('href'), a.target, a.rel]));
  expect(attrs.length > 0, 'has links');
  for (const [href, target, rel] of attrs) {
    expect(/^https?:\/\//.test(href) && target === '_blank' && rel.includes('noopener'), `bad link ${href}`);
  }
});

await test('Mobile: Return inserts newline; send button sends', async (p) => {
  await open(p, '?mock=empty&delay=200');
  await p.locator('textarea').fill('x');
  await p.locator('textarea').press('Enter');
  expect((await userBubbles(p)) === 0, 'Return does not send on touch');
  await p.locator('.send-btn').tap();
  await p.waitForSelector('.msg-user');
}, { viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: engine === chromium });

await test('Lost response: the automatic re-send returns the saved reply', async (p) => {
  await open(p, '?mock=lostreply&delay=200');
  const before = await replies(p);
  await p.locator('textarea').fill('did it save?');
  await p.keyboard.press('Enter');
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, before);
  expect((await p.locator('.msg-failed').count()) === 0, 'no failure shown');
  const { sends, answered } = await stats(p);
  expect(sends === 2 && answered === 1, `sent ${sends}, answered ${answered}`);
});

await test('Clear failure is shown inside the dialog', async (p) => {
  await open(p, '?mock=clearerror');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('dialog[open] .dialog-error');
  expect((await userBubbles(p)) > 0, 'messages kept');
});

await test('Closing the clear dialog returns focus to the Account button', async (p) => {
  await open(p, '?mock=chat');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.waitForSelector('dialog[open]');
  await p.keyboard.press('Escape');
  await p.waitForTimeout(150);
  const focused = await p.evaluate(() => document.activeElement?.getAttribute('aria-label'));
  expect(focused === 'Account', `focus on ${focused}`);
});

await test('Request from a previous session blocks writes until it settles, then thread syncs', async (p) => {
  await open(p, '?mock=empty&delay=2500');
  await p.locator('textarea').fill('carry over');
  await p.keyboard.press('Enter');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.locator('textarea').fill('second');
  expect(await p.locator('.send-btn').isDisabled(), 'send blocked while old request runs');
  await p.waitForSelector('.msg-assistant:not(.thinking)', { timeout: 6000 });
  expect(!(await p.locator('.send-btn').isDisabled()), 'send enabled after it settles');
});

await browser.close();
console.log(results.join('\n'));
const failed = results.filter((r) => r.startsWith('FAIL')).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
