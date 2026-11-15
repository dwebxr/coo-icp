// Interaction tests against the dev server's mock backend (`npm run dev`, then `npm run e2e`).
// Usage: node e2e/smoke.mjs [--webkit]   BASE=<dev server url>   PW_CHANNEL=chrome to use an installed Chrome
//        ONLY=<text> runs only the tests whose name contains it
import { chromium, webkit } from 'playwright';

const BASE = process.env.BASE ?? 'http://localhost:5173';
const ONLY = process.env.ONLY;
const engine = process.argv.includes('--webkit') ? webkit : chromium;
const browser = await engine.launch(engine === chromium && process.env.PW_CHANNEL ? { channel: process.env.PW_CHANNEL } : {});
const results = [];

async function test(name, fn, ctxOpts = {}) {
  if (ONLY && !name.includes(ONLY)) return;
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
// Confirms the clear dialog and waits until the clear call has reached the mock: a test that then moves the installed
// clock on must not get ahead of the call (the click returns before the app has handled it).
async function confirmClear(p) {
  const before = (await stats(p)).clears;
  await p.locator('dialog').getByRole('button', { name: 'Clear' }).click();
  await p.waitForFunction((n) => window.__mock.clears > n, before);
}

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
  // Wait for the old request to settle (the canister saves it), then really send.
  await p.waitForSelector('.msg-assistant:not(.thinking)', { timeout: 6000 });
  const before = await replies(p);
  await p.locator('textarea').fill('again');
  expect(!(await p.locator('.send-btn').isDisabled()), 'send enabled (pending reset)');
  await p.keyboard.press('Enter');
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, before, { timeout: 6000 });
  expect((await p.locator('.msg-user .bubble').last().innerText()) === 'again', 'the new message is in the thread');
  expect((await stats(p)).answered === before + 1, `answered ${(await stats(p)).answered}`);
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

await test('A link shows where it goes after its text, inside the link', async (p) => {
  await open(p, '?mock=rich');
  const labelled = p.locator('.md a', { hasText: 'the canister overview' });
  expect((await labelled.innerText()) === 'the canister overview (internetcomputer.org)', await labelled.innerText());
  expect((await labelled.locator('.md-link-host').count()) === 1, 'the destination is part of the link');
  // A label that seems to name the host is not read for it: the destination is shown all the same.
  const named = p.locator('.md a', { hasText: /^internetcomputer\.org/ });
  expect((await named.innerText()) === 'internetcomputer.org (internetcomputer.org)', await named.innerText());
  // A bare URL already shows its destination.
  expect((await p.locator('.md a', { hasText: 'https://internetcomputer.org/docs' }).locator('.md-link-host').count()) === 0, 'no suffix on a bare URL');
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
  const reads = (await stats(p)).historyReads;
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('dialog[open] .dialog-error');
  expect((await userBubbles(p)) > 0, 'messages kept');
  // The canister refused it: not run. The floor was raised before the call, so the conversation is read through
  // consensus once, which puts the floor back at the canister's revision (in a new epoch).
  expect((await stats(p)).historyReads === reads + 1, 'one read through consensus after the refusal');
  const floor = await p.evaluate(() =>
    localStorage.getItem(Object.keys(localStorage).find((name) => name.startsWith('coo:floor:'))),
  );
  expect(floor === '1:0', `the floor is back at the canister's revision (${floor})`);
  await p.locator('dialog').getByRole('button', { name: 'Cancel' }).click();
  await p.locator('textarea').fill('still here');
  expect(!(await p.locator('.send-btn').isDisabled()), 'and sending works');
});

// Review round 2, I4: an error in transit says nothing about whether the clear ran; the conversation tells.
await test('A clear whose answer is lost after it ran is reported as cleared', async (p) => {
  await open(p, '?mock=clearlost');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('dialog[open]', { state: 'detached', timeout: 8000 });
  await p.waitForSelector('.empty');
  expect((await userBubbles(p)) === 0, 'the thread follows the canister');
});

// Review round 3, Important ①: after a lost answer, queries still return the conversation from before the clear.
await test('A clear whose answer is lost is judged by the canister through consensus, not by a stale query', async (p) => {
  await open(p, '?mock=clearlost&stale=3');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('dialog[open]', { state: 'detached', timeout: 8000 });
  await p.waitForSelector('.empty');
  expect((await userBubbles(p)) === 0, 'the cleared conversation is not shown again');
});

// Review round 3, Important ②: the same, for a clear made in the previous session.
await test('A clear from an earlier session whose answer is lost still brings the thread in line', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=clearlost&clear=40000&stale=3`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await confirmClear(p);
  await p.clock.runFor(31_000);
  await p.waitForSelector('.dialog-note');
  await p.locator('dialog').getByRole('button', { name: 'Close' }).click();
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.clock.runFor(500);
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.clock.runFor(2000);
  await p.waitForSelector('.thread[aria-busy="false"]');
  expect((await userBubbles(p)) > 0, 'the history loaded before the clear ran');
  // At 41.5 s the clear runs and its answer is lost.
  await p.clock.runFor(10_000);
  await p.waitForSelector('.empty', { timeout: 5000 });
  await p.locator('textarea').fill('after');
  expect(!(await p.locator('.send-btn').isDisabled()), 'sending is possible again');
});

// Review round 4, Important 1: the session ends while a clear whose answer was lost is being confirmed. (Since
// round 5 the floor, raised before the clear was sent, is what keeps the old conversation off the screen.)
await test('A clear whose answer was lost is confirmed through consensus in the next session too', async (p) => {
  // The clear runs, its answer is lost; queries return the conversation from before it; consensus takes 6 s.
  await open(p, '?mock=clearlost&stale=3&certified=6000');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForTimeout(1500); // the answer was lost; the read through consensus is on its way
  // Another tab signs out (its generation, and the event for it), so this session ends; the same user signs in again.
  await p.evaluate(() => {
    localStorage.setItem('coo:signed-out', 'signed-out-elsewhere');
    window.dispatchEvent(new StorageEvent('storage', { key: 'coo:signed-out', newValue: 'signed-out-elsewhere' }));
  });
  await p.waitForSelector('.signin', { timeout: 5000 });
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.chat');
  await p.waitForSelector('.thread[aria-busy="false"]', { timeout: 15000 });
  expect((await userBubbles(p)) === 0, 'the conversation from before the clear is not shown');
  await p.waitForSelector('.empty');
});

// Review round 4, Important 2: the read that should confirm the clear fails too; later, Retry (round 5: the floor).
await test('A clear that could not be confirmed says so, and Retry reads through consensus', async (p) => {
  await open(p, '?mock=clearlost&stale=3&certifiedfail=1');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('dialog[open] .dialog-note', { timeout: 8000 });
  expect((await p.locator('.dialog-error').count()) === 0, 'not reported as a failure: it may have run');
  await p.locator('dialog').getByRole('button', { name: 'Close' }).click();
  await p.waitForSelector("text=Couldn't load your conversation");
  await p.locator('.state-card').getByRole('button', { name: 'Retry' }).click();
  await p.waitForSelector('.thread[aria-busy="false"]', { timeout: 8000 });
  expect((await userBubbles(p)) === 0, 'the conversation from before the clear is not shown');
  await p.waitForSelector('.empty');
});

// Review round 5, Important 2: tab A's clear ran, its answer was lost and A was closed; tab B still shows the
// conversation from before it. (The scenario plays tab A: on B's first send, the canister has been cleared, and the
// floor A raised before sending is in storage.)
await test("Another tab's lost clear does not block this tab: a send meets a Conflict, and the thread catches up", async (p) => {
  await open(p, '?mock=otherclear&delay=200');
  expect((await userBubbles(p)) > 0, 'this tab shows the conversation from before the clear');
  await p.locator('textarea').fill('hello');
  await p.keyboard.press('Enter');
  await p.waitForSelector('.msg-failed');
  // The conversation the canister holds now is shown: only this tab's message, which was not sent.
  expect((await userBubbles(p)) === 1, 'the cleared conversation is shown');
  expect((await p.locator('.msg-failed').innerText()).includes('changed'), 'not sent: the conversation changed');
  // And the next attempt goes through, without a reload.
  const before = await replies(p);
  await p.getByRole('button', { name: 'Retry' }).click();
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, before);
  expect((await p.locator('.msg-failed').count()) === 0, 'sent');
});

// Review round 4/5, Nit 1: the hook shows the empty thread only while the floor is where the clear left it. Here
// another tab's reply is confirmed (and its floor stored) after the clear ran and before its answer arrives.
await test('A clear overtaken by another tab before its answer arrives shows what the canister holds, not an empty thread', async (p) => {
  await open(p, '?mock=clearovertaken');
  const reads = (await stats(p)).historyReads;
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('dialog[open]', { state: 'detached', timeout: 8000 });
  await p.waitForSelector('.thread[aria-busy="false"]');
  expect((await stats(p)).historyReads > reads, 'the conversation was loaded through consensus');
  expect((await userBubbles(p)) === 1, `the other tab's message is shown (${await userBubbles(p)} messages)`);
  expect((await p.locator('.msg-user .bubble').first().innerText()) === 'Sent from another tab', 'it is that message');
});

await test('A clear lost before it ran is reported as failed, after the conversation was checked', async (p) => {
  await open(p, '?mock=clearfail');
  const reads = (await stats(p)).historyReads;
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await p.getByRole('button', { name: 'Clear', exact: true }).click();
  await p.waitForSelector('dialog[open] .dialog-error', { timeout: 8000 });
  expect((await stats(p)).historyReads > reads, 'the conversation was loaded again');
  expect((await userBubbles(p)) > 0, 'messages kept');
  await p.locator('dialog').getByRole('button', { name: 'Cancel' }).click();
  await p.locator('textarea').fill('still here');
  expect(!(await p.locator('.send-btn').isDisabled()), 'sending is possible again');
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

// ---------- 0.7.0 ----------

await test('A draft kept from an expired session is not shown to someone else who signs in', async (p) => {
  await open(p, '?mock=expiring&relogin=aaaaa-aa');
  await p.locator('textarea').fill('private thought');
  await p.waitForSelector('.signin', { timeout: 8000 });
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.chat');
  expect((await p.locator('textarea').inputValue()) === '', 'the other principal starts with an empty composer');
});

await test('Cancel during sign-in is final: a sign-in that completes afterwards is discarded', async (p) => {
  await open(p, '?mock=signedout&signin=late');
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('text=Allow pop-ups', { timeout: 5000 });
  await p.getByRole('button', { name: 'Cancel' }).click();
  // The mock completes the sign-in about a second later.
  await p.waitForTimeout(2500);
  expect((await p.locator('.chat').count()) === 0, 'still signed out');
  expect((await p.evaluate(() => window.__mock.session)) === false, 'its delegation is discarded');
  await p.waitForFunction(() => !document.querySelector('.btn-primary')?.disabled, null, { timeout: 3000 });
  // A sign-in the user does not cancel still works.
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.chat', { timeout: 8000 });
});

await test('A tab that is still restoring its session when another tab signs out does not keep it', async (p, ctx) => {
  await open(p, '?mock=chat');
  const other = await ctx.newPage();
  // Restoring the session takes 3 s in this tab.
  await other.goto(`${BASE}/?mock=chat&restore=3000`);
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.waitForSelector('.signin');
  await other.waitForSelector('.signin-actions:not(.is-hidden)', { timeout: 6000 });
  expect((await other.locator('.notice').innerText()).includes('another tab'), 'says why');
  expect((await other.locator('.chat').count()) === 0, 'not signed in');
  await other.waitForFunction(() => window.__mock.session === false, null, { timeout: 3000 });
});

await test('A tab that finds the stored session idle at start-up signs out a tab that is restoring it', async (p, ctx) => {
  // This tab restores slowly; meanwhile the last recorded activity is 11 minutes old.
  await p.goto(`${BASE}/?mock=chat&restore=3000`);
  await p.evaluate(() => localStorage.setItem('coo:last-active', String(Date.now() - 11 * 60 * 1000)));
  // Another tab opens, finds the session idle, signs out (which forgets the recorded activity) and says so.
  const other = await ctx.newPage();
  await other.goto(`${BASE}/?mock=chat`);
  await other.waitForSelector('.signin-actions:not(.is-hidden)');
  expect((await other.locator('.notice').innerText()).includes('inactivity'), 'idle notice in the tab that found it');
  // The first tab's restore then finds no recorded activity at all — but the sign-out was announced.
  await p.waitForSelector('.signin-actions:not(.is-hidden)', { timeout: 6000 });
  expect((await p.locator('.chat').count()) === 0, 'not signed in');
  expect((await p.locator('.notice').innerText()).includes('another tab'), 'says why');
});

for (const [second, ending] of [
  ['hang', 'is cancelled'],
  ['lateerror', 'fails'],
]) {
  await test(`A cancelled sign-in that completes during a newer one is discarded when the newer one ${ending}`, async (p) => {
    await open(p, `?mock=signedout&signin=late,${second}`);
    await p.getByRole('button', { name: /sign in/i }).click();
    await p.waitForSelector('text=Allow pop-ups', { timeout: 5000 });
    await p.getByRole('button', { name: 'Cancel' }).click();
    // A newer attempt, still in progress when the first one completes (4 s after it began).
    await p.getByRole('button', { name: /sign in/i }).click();
    await p.waitForFunction(() => window.__mock.session === true, null, { timeout: 4000 });
    expect((await p.locator('.chat').count()) === 0, 'the cancelled attempt is not used');
    if (second === 'hang') {
      await p.waitForSelector('text=Allow pop-ups', { timeout: 5000 });
      await p.getByRole('button', { name: 'Cancel' }).click();
    } else {
      await p.waitForSelector('.notice-error', { timeout: 6000 });
    }
    // The delegation the cancelled attempt left behind is thrown away: a reload would not restore it.
    await p.waitForFunction(() => window.__mock.session === false, null, { timeout: 3000 });
    expect((await p.locator('.chat').count()) === 0, 'still signed out');
  });
}

// Review round 2, I7: another tab signed out → this tab signed in → the other tab's notification arrives.
await test('A sign-out notification older than this session does not end it', async (p) => {
  await open(p, '?mock=signedout');
  // Other tabs signed out twice before this one signs in …
  await p.evaluate(() => {
    localStorage.setItem('coo:signed-out', 'an-older-one');
    localStorage.setItem('coo:signed-out', 'before-this-session');
  });
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.waitForSelector('.chat');
  // … and their notifications are delivered only now, the older one last (review round 3, Nit ③).
  const deliver = (value) =>
    p.evaluate((v) => window.dispatchEvent(new StorageEvent('storage', { key: 'coo:signed-out', newValue: v, oldValue: null })), value);
  await deliver('before-this-session');
  await deliver('an-older-one');
  await p.waitForTimeout(500);
  expect((await p.locator('.chat').count()) === 1, 'still signed in');
  // A sign-out after the session began (another tab writes a new generation, and its event arrives) does end it.
  await p.evaluate(() => localStorage.setItem('coo:signed-out', 'after-this-session'));
  await deliver('after-this-session');
  await p.waitForSelector('.signin', { timeout: 3000 });
  expect((await p.locator('.notice').innerText()).includes('another tab'), 'says why');
});

await test('Signing out in one tab signs out the other', async (p, ctx) => {
  await open(p, '?mock=chat');
  const other = await ctx.newPage();
  await other.goto(`${BASE}/?mock=chat`);
  await other.waitForSelector('.thread[aria-busy="false"]');
  await other.locator('textarea').fill('half a message');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await other.waitForSelector('.signin', { timeout: 3000 });
  expect((await other.locator('.notice').innerText()).includes('another tab'), 'says why');
  await other.getByRole('button', { name: /sign in/i }).click();
  await other.waitForSelector('.chat');
  expect((await other.locator('textarea').inputValue()) === '', 'its draft is gone');
});

await test('A session that sat idle with the tab closed is not restored', async (p, ctx) => {
  await ctx.addInitScript(() => localStorage.setItem('coo:last-active', String(Date.now() - 11 * 60 * 1000)));
  await p.goto(`${BASE}/?mock=chat`);
  await p.waitForSelector('.signin');
  expect((await p.locator('.notice').innerText()).includes('inactivity'), 'idle notice');
  expect((await p.locator('.chat').count()) === 0, 'not signed in');
});

await test('Activity in another tab keeps this one signed in', async (p) => {
  await p.clock.install();
  await open(p, '?mock=chat');
  await p.clock.runFor(1500);
  await p.waitForSelector('.chat textarea');
  await p.clock.runFor(9 * 60 * 1000);
  // Another tab was used a moment ago.
  await p.evaluate(() => localStorage.setItem('coo:last-active', String(Date.now())));
  await p.clock.runFor(2 * 60 * 1000);
  expect((await p.locator('.chat').count()) === 1, 'still signed in 11 minutes after its own last activity');
  await p.clock.runFor(9 * 60 * 1000);
  await p.waitForSelector('.signin');
});

await test('A clear that gets no answer can be left, blocks sending, and is reported as neither done nor failed', async (p) => {
  await p.clock.install();
  await open(p, '?mock=clearhang');
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await confirmClear(p);
  await p.clock.runFor(31_000);
  // No answer is not a failure: the conversation may already be cleared.
  await p.waitForSelector('.dialog-note');
  const note = await p.locator('.dialog-note').innerText();
  expect(/not confirmed this yet|hasn't confirmed this yet/.test(note) && /may already be cleared/.test(note), note);
  expect(/nothing can be sent until it is confirmed/i.test(note) && /close/i.test(note), note);
  expect((await p.locator('.dialog-error').count()) === 0, 'not reported as a failure');
  expect(await p.locator('dialog').getByRole('button', { name: 'Clear' }).isDisabled(), 'not offered again while it is out');
  await p.locator('dialog').getByRole('button', { name: 'Close' }).click();
  expect((await p.locator('dialog[open]').count()) === 0, 'dialog closed');
  expect((await userBubbles(p)) > 0, 'the thread is not emptied on a guess');
  await p.locator('textarea').fill('next');
  expect(await p.locator('.send-btn').isDisabled(), 'nothing is sent while the clear is still out');
});

// Review round 2, N1: the dialog follows the answer that comes after "no answer yet".
await test('A late clear closes the dialog that still says "no answer yet", and the thread is emptied', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=chat&clear=40000`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await confirmClear(p);
  await p.clock.runFor(31_000);
  await p.waitForSelector('.dialog-note');
  await p.clock.runFor(10_000);
  await p.waitForSelector('dialog[open]', { state: 'detached', timeout: 5000 });
  await p.waitForSelector('.empty');
});

await test('A late refusal replaces "no answer yet" in the dialog with the failure', async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=clearerror&clear=40000`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await confirmClear(p);
  await p.clock.runFor(31_000);
  await p.waitForSelector('.dialog-note');
  await p.clock.runFor(10_000);
  await p.waitForSelector('dialog[open] .dialog-error', { timeout: 5000 });
  expect((await p.locator('.dialog-note').count()) === 0, 'no longer "no answer yet"');
  expect((await userBubbles(p)) > 0, 'messages kept');
});

// Review round 2, I3: the first read after that late clear comes from a replica that is behind.
await test('After a late clear from an earlier session, a stale read is not the end of it: nothing is sent until the thread matches', async (p) => {
  await p.clock.install();
  // The clear lands 60 s after it is sent; queries return the conversation from before it three times; a read
  // through consensus takes 4 s.
  await p.goto(`${BASE}/?mock=chat&clear=60000&stale=3&certified=4000`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await confirmClear(p);
  const lands = (await p.evaluate(() => Date.now())) + 60_000;
  await p.clock.runFor(31_000);
  await p.waitForSelector('.dialog-note');
  await p.locator('dialog').getByRole('button', { name: 'Close' }).click();
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.clock.runFor(500);
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.clock.runFor(2000);
  // (The floor was raised before the clear was sent, and the clear has not run yet: this load reads through
  // consensus and puts the floor back. It takes a few seconds.)
  await p.waitForSelector('.thread[aria-busy="false"]', { timeout: 20000 });
  // Just after the clear lands, the thread is being loaded again (4 s), and nothing can be sent.
  await p.clock.runFor(Math.max(lands - (await p.evaluate(() => Date.now())), 0) + 300);
  await p.locator('textarea').fill('too early');
  expect(await p.locator('.send-btn').isDisabled(), 'nothing can be sent while the thread may be stale');
  await p.clock.runFor(8_000);
  await p.waitForSelector('.empty', { timeout: 5000 });
  expect(!(await p.locator('.send-btn').isDisabled()), 'sending is possible once the thread matches');
});

// Review round 4, Nit 2: every principal has its own conversation in the mock, as in the canister.
await test("A late clear of one user does not touch the next user's conversation", async (p) => {
  await p.clock.install();
  await p.goto(`${BASE}/?mock=chat&clear=40000&relogin=aaaaa-aa`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await confirmClear(p);
  await p.clock.runFor(31_000);
  await p.waitForSelector('.dialog-note');
  await p.locator('dialog').getByRole('button', { name: 'Close' }).click();
  // Someone else signs in at this tab.
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.clock.runFor(500);
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.clock.runFor(2000);
  await p.waitForSelector('.thread[aria-busy="false"]');
  const theirs = await userBubbles(p);
  expect(theirs > 0, 'the other user has a conversation of their own');
  // The first user's clear lands (at about 41.5 s).
  await p.clock.runFor(10_000);
  await p.waitForSelector('.thread[aria-busy="false"]');
  expect((await userBubbles(p)) === theirs, "the other user's conversation is unchanged");
  const before = await replies(p);
  await p.locator('textarea').fill('mine');
  expect(!(await p.locator('.send-btn').isDisabled()), 'and they can send');
  await p.keyboard.press('Enter');
  await p.clock.runFor(3000);
  await p.waitForFunction((n) => document.querySelectorAll('.msg-assistant:not(.thinking)').length === n + 1, before);
});

await test('A clear from an earlier session that succeeds late brings the thread up to date after signing back in', async (p) => {
  await p.clock.install();
  // The clear answers (and clears) 40 s after it is sent.
  await p.goto(`${BASE}/?mock=chat&clear=40000`);
  await p.clock.runFor(1500);
  await p.waitForSelector('.thread[aria-busy="false"]');
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /clear/i }).click();
  await confirmClear(p);
  await p.clock.runFor(31_000);
  await p.waitForSelector('.dialog-note');
  await p.locator('dialog').getByRole('button', { name: 'Close' }).click();
  // Sign out and back in (the same user) while the clear is still out.
  await p.getByRole('button', { name: 'Account' }).click();
  await p.getByRole('menuitem', { name: /sign out/i }).click();
  await p.clock.runFor(500);
  await p.getByRole('button', { name: /sign in/i }).click();
  await p.clock.runFor(2000);
  await p.waitForSelector('.thread[aria-busy="false"]');
  expect((await userBubbles(p)) > 0, 'the history loaded before the clear answered');
  await p.clock.runFor(10_000);
  await p.waitForSelector('.empty', { timeout: 5000 });
  expect((await userBubbles(p)) === 0, 'the thread follows the clear');
  await p.locator('textarea').fill('after');
  expect(!(await p.locator('.send-btn').isDisabled()), 'and sending is possible again');
});

await test('History behind what this device saw is settled by asking the canister through consensus', async (p) => {
  await open(p, '?mock=chat&delay=200');
  // One exchange, so this device has a floor stored for the conversation.
  await p.locator('textarea').fill('hello');
  await p.keyboard.press('Enter');
  await p.waitForFunction(() => Object.keys(localStorage).some((name) => name.startsWith('coo:floor:')));
  // As if the canister had been restored to an older state: this device remembers a far newer revision.
  const key = await p.evaluate(() => {
    const k = Object.keys(localStorage).find((name) => name.startsWith('coo:floor:'));
    if (k) localStorage.setItem(k, '0:99');
    return k;
  });
  expect(!!key, 'a floor was stored');
  await p.reload();
  await p.waitForSelector('.thread[aria-busy="false"]', { timeout: 15000 });
  expect((await p.locator('.state-card').count()) === 0, 'no error');
  expect((await userBubbles(p)) === 2, 'the conversation is shown');
  const stored = await p.evaluate((k) => localStorage.getItem(k), key);
  expect(stored === '1:0', `the floor follows the canister in a new epoch (${stored})`);
});

await test('A late certified read does not bring back a conversation another tab cleared meanwhile', async (p) => {
  // Floor 0:5; replicas say 4; the certified read reads 5 but answers after another tab's clear (revision 6).
  await p.goto(`${BASE}/?mock=racyclear`);
  await p.waitForSelector('.thread[aria-busy="false"]', { timeout: 15000 });
  expect((await p.locator('.state-card').count()) === 0, 'no error');
  expect((await userBubbles(p)) === 0, 'the cleared conversation is not shown');
  await p.waitForSelector('.empty');
  const floor = await p.evaluate(() =>
    localStorage.getItem(Object.keys(localStorage).find((name) => name.startsWith('coo:floor:'))),
  );
  expect(floor === '0:6', `the floor stays at the other tab's clear, not lowered to the late answer (${floor})`);
});

// Review round 3, Nit ②: once a load has asked through consensus, it does not go back to queries.
await test('A load that had to ask through consensus does not then accept a stale query', async (p) => {
  await p.goto(`${BASE}/?mock=racyreset`);
  await p.waitForSelector('.thread[aria-busy="false"]', { timeout: 20000 });
  expect((await p.locator('.state-card').count()) === 0, 'no error');
  // The canister holds two exchanges (revision 4); the replicas' revision 3 has one.
  expect((await userBubbles(p)) === 2, `the canister's conversation is shown (${await userBubbles(p)} messages)`);
  const floor = await p.evaluate(() =>
    localStorage.getItem(Object.keys(localStorage).find((name) => name.startsWith('coo:floor:'))),
  );
  expect(floor === '1:4', `the floor is the canister's revision (${floor})`);
});

await test('A conversation that cannot be loaded can still be cleared', async (p) => {
  await open(p, '?mock=historyerror');
  await p.waitForSelector('text=Couldn\'t load your conversation');
  await p.getByRole('button', { name: 'Clear conversation' }).click();
  await confirmClear(p);
  await p.waitForSelector('.empty');
  expect((await p.locator('.state-card').count()) === 0, 'error gone');
  await p.locator('textarea').fill('hello again');
  expect(!(await p.locator('.send-btn').isDisabled()), 'can send');
});

await test('A message over 4 000 bytes cannot be sent, and the composer says so', async (p) => {
  await open(p, '?mock=empty');
  const area = p.locator('textarea');
  await area.fill('x'.repeat(100));
  expect((await p.locator('.composer-count').count()) === 0, 'no counter for a short message');
  await area.fill('x'.repeat(3700));
  expect((await p.locator('.composer-count').innerText()).replace(/\s/g, '') === '3700/4000bytes', 'counter near the limit');
  expect(!(await p.locator('.send-btn').isDisabled()), 'still sendable');
  // Bytes, not characters: 1 334 Japanese characters are 4 002 bytes.
  await area.fill('あ'.repeat(1334));
  expect(await p.locator('.send-btn').isDisabled(), 'send disabled over the limit');
  expect((await p.locator('.composer-count.over').count()) === 1, 'counter shows the overflow');
  await area.press('Enter');
  await p.waitForTimeout(300);
  expect((await stats(p)).sends === 0, 'Enter does not send it either');
  await area.fill('あ'.repeat(1333));
  expect(!(await p.locator('.send-btn').isDisabled()), 'sendable at the limit');
});

await test('A suggestion sent with one click leaves a typed draft alone', async (p) => {
  await open(p, '?mock=empty&delay=300');
  await p.locator('textarea').fill('my own question');
  await p.locator('.chip').first().click();
  await p.waitForSelector('.msg-assistant:not(.thinking)');
  expect((await p.locator('textarea').inputValue()) === 'my own question', 'draft kept');
});

await test('Each message names its speaker for screen readers', async (p) => {
  await open(p, '?mock=chat');
  // textContent: engines differ on whether innerText includes visually hidden text.
  expect((await p.locator('.msg-user').first().textContent()).startsWith('You'), 'user message is labelled');
  expect((await p.locator('.msg-assistant .msg-body').first().textContent()).startsWith('Coo'), 'reply is labelled');
  expect((await p.locator('.msg-user .bubble').first().innerText()) === 'What is the Internet Computer?', 'bubble text unchanged');
});


await browser.close();
console.log(results.join('\n'));
const failed = results.filter((r) => r.startsWith('FAIL')).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
// A filter that selects nothing has tested nothing.
if (ONLY && results.length === 0) console.log(`ONLY=${ONLY} matches no test`);
process.exit(failed || (ONLY && results.length === 0) ? 1 : 0);
