// Development-only backend used to preview and test UI states without Internet Identity.
// Loaded from main.tsx only when import.meta.env.DEV is true and the URL has ?mock=<scenario>.
// It mirrors the canister's semantics: one chat at a time, a request runs only after the revision it names, and a
// repeated request id replays the saved reply. (Taking over a chat stuck for 5 minutes is not modelled.)
//
// Scenarios: signedout | empty | chat | long | rich | slow | error | busy | offline | lostreply | inprogress | hang |
//            latesave | conflict | stale | laggy (one history read from before the last clear) |
//            clearerror (the canister refuses the clear) | clearhang (the clear call never returns) |
//            clearlost (the clear runs, its answer is lost) | clearfail (the clear is lost before it runs) |
//            otherclear (before the first send, another tab's clear has run and its answer was lost: the canister is
//            cleared, and the floor that tab raised before sending is in storage) |
//            clearovertaken (right after the clear runs, and before its answer arrives, another tab sends a message
//            and stores the floor it was confirmed) |
//            floorahead (the canister starts at revision 10; before the first send, another tab's exchange is saved
//            (11), and a clear of that tab that never ran left the floor at 12) |
//            clearthrows (the clear call throws instead of returning a promise; nothing is cleared) |
//            confirmedmeanwhile (this tab shows revision 9; before its first send, another tab saves a message (10) and
//            starts a clear (floor 11, expected); this tab's read through consensus runs before that clear and answers
//            (10) only after the clear was confirmed at 11) |
//            historyerror | expiring | badsession |
//            racyclear (this device has seen revision 5, replicas still say 4, and while the certified read — which
//            reads 5 — is on its way, another tab clears the conversation: revision 6) |
//            racyreset (restored from a snapshot to revision 4, this device saw 9, replicas say 3; while the first
//            certified read — which reads 4 — is on its way, another tab resets the floor to 1:3)
// Extra params: signin=cancel|error|hang|late (succeeds after 4 s)|lateerror (fails after 4 s); a comma-separated
//                 list gives each sign-in attempt its own mode (the last one repeats): signin=late,hang
//               delay=<ms>  restore=<ms> (how long restoring the session takes; 250 by default)
//               clear=<ms> (the clear call answers, and clears, only after this long)
//               stale=<n> (the next n queries after a clear return the conversation from before it; laggy is stale=1)
//               certified=<ms> (how long a read through consensus takes; 600 by default)
//               certifiedfail=<n> (the first n reads through consensus fail as if the network were down)
//               relogin=<principal> (who signs in after the first session has ended: someone else at the same tab;
//               every principal has a conversation of its own, starting as the scenario's)
import {
  SessionInvalid,
  SignInCancelled,
  type Backend,
  type Message,
  type ReplyStatus,
  type SendOutcome,
  type Sent,
  type Session,
} from '../backend';
// What the canister accepts (UTF-8 bytes); a unit test checks it against the canister's own constant.
import { MAX_MESSAGE_BYTES } from '../lib/limits';
import { floorOwner, raiseBeforeClear, raiseFloorByUpdate } from '../lib/floors';
import { network } from '../network';

const PRINCIPAL = 'h7xqe-2mlpn-aaaab-qbm3a-cai4f-ufp5q-tdqbv-wv6jf-3pgv3-zkm4p-aqe';

const SAMPLE: Message[] = [
  { role: 'user', content: 'What is the Internet Computer?' },
  {
    role: 'assistant',
    content:
      'The **Internet Computer** is a blockchain that runs smart contracts, called *canisters*, at web speed.\n\n' +
      'A few things make it different:\n\n' +
      '1. **Full-stack on-chain** — frontends, backends and data all live in canisters.\n' +
      '2. **Reverse gas** — apps pay for compute, so users need no tokens.\n' +
      '3. **Chain-key crypto** — canisters can sign for Bitcoin and Ethereum.\n\n' +
      'Want to see how I run here?',
  },
  { role: 'user', content: 'Yes — how do you work?' },
  {
    role: 'assistant',
    content:
      'I run inside a canister. Each message I get is sent to the LLM canister in a bounded-wait call:\n\n' +
      '```rust\nlet response = Call::bounded_wait(llm, "v1_chat")\n    .change_timeout(MODEL_CALL_TIMEOUT_SECS)\n    .with_arg(&request)\n    .await;\n```\n\n' +
      'Your conversation is stored per Internet Identity principal, so only you see it. ' +
      'Learn more at https://internetcomputer.org.',
  },
];

const RICH: Message[] = [
  {
    role: 'user',
    content:
      'Can you compare canisters and smart contracts? Also here is a long token: 0x9f8e7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b4a39281706f5e4d3c2b1a0',
  },
  {
    role: 'assistant',
    content:
      '## Canisters vs. smart contracts\n\n' +
      'Both run code on a blockchain, but canisters go further.\n\n' +
      '### Key differences\n\n' +
      '- **State**: canisters hold up to hundreds of GB\n' +
      '  - stable memory survives upgrades\n' +
      '  - heap memory is fast but reset on upgrade\n' +
      '- **Web**: canisters serve HTTP directly\n' +
      '- **Cost**: the `reverse gas` model\n\n' +
      '> Canisters are WebAssembly modules bundled with their memory.\n\n' +
      'More in [the canister overview](https://internetcomputer.org/docs/building-apps/essentials/canisters-overview), ' +
      'or start at [internetcomputer.org](https://internetcomputer.org).\n\n' +
      '---\n\n' +
      'Docs: https://internetcomputer.org/docs/building-apps/essentials/canisters-overview-and-a-very-long-path-segment',
  },
];

const REPLY =
  'Good question. Here is the short version:\n\n- Canisters hold code **and** state\n- Calls are signed by your identity\n- `query` calls are fast; `update` calls go through consensus\n\nAnything else?';

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// The canister keeps at most this many turns per conversation.
const MAX_TURNS = 50;

export function createMockBackend(scenario: string): Backend {
  const params = new URLSearchParams(window.location.search);
  const signInModes = (params.get('signin') ?? '').split(',');
  let signIns = 0;
  const delay = Number(params.get('delay') ?? (scenario === 'slow' ? 15000 : 1400));
  const restoreMs = Number(params.get('restore') ?? 250);
  const clearMs = params.get('clear') === null ? null : Number(params.get('clear'));
  const staleReads = scenario === 'laggy' ? 1 : Number(params.get('stale') ?? 0);
  const certifiedMs = Number(params.get('certified') ?? 600);
  let certifiedFailures = Number(params.get('certifiedfail') ?? 0);

  let signedIn = scenario !== 'signedout' && scenario !== '';
  let expiresAt = Date.now() + (scenario === 'expiring' ? 4000 : 8 * 3600 * 1000);
  let principal = PRINCIPAL;
  const session = (): Session => ({ principal });
  const guard = () => {
    if (!signedIn || Date.now() >= expiresAt) throw new SessionInvalid('mock session invalid');
  };

  /** One caller's conversation, as the canister keeps it. */
  interface Conversation {
    messages: Message[];
    revision: number;
    // laggy, stale=<n>: what a replica that is behind would still return after a clear, and how many more times.
    beforeClear: { messages: Message[]; revision: number } | null;
    staleLeft: number;
    inFlight: { id: string; cleared: boolean } | null;
  }
  const conversations = new Map<string, Conversation>();
  /** The conversation of `who` (by default whoever is signed in now); every caller starts with the scenario's. */
  const conv = (who = principal): Conversation => {
    let found = conversations.get(who);
    if (!found) {
      const messages =
        scenario === 'empty' || scenario === 'signedout'
          ? []
          : scenario === 'long'
            ? Array.from({ length: 6 }, () => SAMPLE).flat()
            : scenario === 'rich'
              ? [...RICH]
              : [...SAMPLE];
      found = { messages, revision: 0, beforeClear: null, staleLeft: 0, inFlight: null };
      conversations.set(who, found);
    }
    return found;
  };
  const copy = (c: Conversation) => ({ messages: c.messages.map((m) => ({ ...m })), revision: c.revision });

  // racyclear, racyreset: the floor this device stored, and whether the other tab has acted yet.
  const floorKeyOf = (who: string) => `coo:floor:${floorOwner(network.backendCanisterId ?? 'local', who)}`;
  const floorKey = floorKeyOf(PRINCIPAL);
  let otherTabCleared = false;
  if (scenario === 'racyclear') {
    conv().revision = 5;
    localStorage.setItem(floorKey, '0:5');
  }
  if (scenario === 'floorahead') conv().revision = 10;
  if (scenario === 'confirmedmeanwhile') conv().revision = 9;
  let otherTabConfirmed = false;
  // What another tab does to this device's floors, done with the app's own functions (lib/floors).
  const ownerOf = (who: string) => floorOwner(network.backendCanisterId ?? 'local', who);
  let otherTabReset = false;
  if (scenario === 'racyreset') {
    conv().revision = 4;
    localStorage.setItem(floorKey, '0:9');
  }
  // Tests read how many requests were sent and how many were answered, and whether a delegation is held (what a
  // reload would restore).
  const stats = {
    sends: 0,
    answered: 0,
    ids: [] as string[],
    // Reads of the conversation: by query, and through consensus.
    queryReads: 0,
    certifiedReads: 0,
    clears: 0,
    get session() {
      return signedIn;
    },
  };
  (window as unknown as { __mock: typeof stats }).__mock = stats;

  /** The saved exchange for a request id: the user message and the reply right after it. */
  const saved = (c: Conversation, id: string): { text: string; sent: Sent } | null => {
    const i = c.messages.findIndex((m) => m.role === 'user' && m.requestId === id);
    const reply = c.messages[i + 1];
    return i >= 0 && reply?.role === 'assistant'
      ? { text: c.messages[i].content, sent: { reply: reply.content, revision: c.revision } }
      : null;
  };

  /** Another tab saves an exchange. */
  const otherTab = (c: Conversation) => {
    c.messages = [...c.messages, { role: 'user', content: 'Sent from another tab' }, { role: 'assistant', content: REPLY }];
    c.revision += 1;
  };

  /** The canister's checks before it runs a request: replay, one chat at a time, revision. */
  function check(c: Conversation, id: string, base: number, text: string): SendOutcome | null {
    const exchange = saved(c, id);
    if (exchange) return exchange.text === text ? { ok: exchange.sent } : { err: 'This request id was used for another message.' };
    const bytes = new TextEncoder().encode(text).length;
    if (bytes > MAX_MESSAGE_BYTES) return { err: `Message is too long (${bytes} bytes; the limit is ${MAX_MESSAGE_BYTES}).` };
    if (c.inFlight) return c.inFlight.id === id ? { inProgress: true } : { busy: true };
    if (base !== c.revision) return { conflict: true };
    return null;
  }

  /** What the canister does with a request: generate a reply, then save the exchange. */
  async function answer(c: Conversation, id: string, text: string, ms = delay): Promise<SendOutcome> {
    const flight = { id, cleared: false };
    c.inFlight = flight;
    await sleep(ms);
    c.inFlight = null;
    if (flight.cleared) return { err: 'Conversation cleared.' };
    if (scenario === 'error') return { failed: true };
    // Trim like the canister: the newest turns, never starting with a reply.
    const next = [...c.messages, { role: 'user', content: text, requestId: id }, { role: 'assistant', content: REPLY }];
    const kept = next.slice(Math.max(next.length - MAX_TURNS, 0));
    c.messages = kept.slice(kept.findIndex((m) => m.role === 'user'));
    c.revision += 1;
    stats.answered += 1;
    return { ok: { reply: REPLY, revision: c.revision } };
  }

  // Every call works on the conversation of whoever was signed in when it was made, as the canister does (the
  // caller is part of the call).
  return {
    async restore() {
      await sleep(restoreMs);
      return signedIn ? session() : null;
    },

    async signIn() {
      const mode = signInModes[Math.min(signIns++, signInModes.length - 1)];
      await sleep(mode === 'late' || mode === 'lateerror' ? 4000 : 1200);
      if (mode === 'hang') return new Promise<Session>(() => {});
      if (mode === 'cancel') throw new SignInCancelled();
      if (mode === 'error' || mode === 'lateerror') throw new Error('mock sign-in failure');
      signedIn = true;
      expiresAt = Date.now() + 8 * 3600 * 1000;
      principal = params.get('relogin') ?? principal;
      return session();
    },

    async signOut() {
      await sleep(80);
      signedIn = false;
    },

    async history() {
      const c = conv();
      stats.queryReads += 1;
      await sleep(scenario === 'historyerror' ? 600 : 300);
      if (scenario === 'badsession') throw new SessionInvalid('mock orphan delegation');
      guard();
      if (scenario === 'historyerror') throw new Error('mock history failure');
      if (c.beforeClear && c.staleLeft > 0) {
        c.staleLeft -= 1;
        return { messages: c.beforeClear.messages.map((m) => ({ ...m })), revision: c.beforeClear.revision };
      }
      // Replicas that have not caught up with revision 5 yet.
      if (scenario === 'racyclear' && !otherTabCleared) return { ...copy(c), revision: 4 };
      // Replicas still one exchange behind the restored state.
      if (scenario === 'racyreset') return { messages: c.messages.slice(0, -2).map((m) => ({ ...m })), revision: 3 };
      return copy(c);
    },

    async certifiedHistory() {
      const c = conv();
      stats.certifiedReads += 1;
      // Through consensus: slower, and never the stale copy a lagging replica returns.
      if (scenario === 'racyclear' && !otherTabCleared) {
        // Read now, answered late: meanwhile another tab clears, and stores the floor it was confirmed.
        const read = copy(c);
        await sleep(300);
        c.messages = [];
        c.revision += 1;
        otherTabCleared = true;
        localStorage.setItem(floorKey, `0:${c.revision}`);
        await sleep(300);
        return read;
      }
      if (scenario === 'confirmedmeanwhile' && !otherTabConfirmed && c.revision === 10) {
        // Read now, answered late: meanwhile the other tab's clear runs and is confirmed at 11.
        const read = copy(c);
        await sleep(300);
        otherTabConfirmed = true;
        c.messages = [];
        c.revision += 1;
        raiseFloorByUpdate(ownerOf(principal), c.revision);
        await sleep(300);
        return read;
      }
      if (scenario === 'racyreset' && !otherTabReset) {
        // Read now, answered late: meanwhile another tab resets the floor to 1:3.
        const read = copy(c);
        await sleep(300);
        otherTabReset = true;
        localStorage.setItem(floorKey, '1:3');
        await sleep(300);
        return read;
      }
      await sleep(certifiedMs);
      // certifiedfail=<n>: the first n reads through consensus are lost too.
      if (certifiedFailures > 0) {
        certifiedFailures -= 1;
        throw new Error('Failed to fetch');
      }
      guard();
      return copy(c);
    },

    async send(id: string, base: number, text: string): Promise<SendOutcome> {
      guard();
      const c = conv();
      const attempt = ++stats.sends;
      stats.ids.push(id);
      // Before the first send, another tab changes the conversation: visibly, or (stale) only its revision.
      if (attempt === 1 && scenario === 'conflict') otherTab(c);
      if (attempt === 1 && scenario === 'stale') c.revision += 1;
      if (attempt === 1 && scenario === 'confirmedmeanwhile') {
        otherTab(c);
        raiseBeforeClear(ownerOf(principal), c.revision);
      }
      if (attempt === 1 && scenario === 'floorahead') {
        otherTab(c);
        localStorage.setItem(floorKeyOf(principal), `0:${c.revision + 1}`);
      }
      if (attempt === 1 && scenario === 'otherclear') {
        localStorage.setItem(floorKeyOf(principal), `0:${c.revision + 1}`);
        c.messages = [];
        c.revision += 1;
      }
      const refused = check(c, id, base, text);
      if (refused) return refused;
      switch (scenario) {
        case 'busy':
          // Another tab is being answered at first.
          if (attempt === 1) return { busy: true };
          break;
        case 'offline':
          // Never reaches the canister.
          await sleep(delay);
          throw new Error('Failed to fetch');
        case 'lostreply':
          // Answered and saved, but the response is lost.
          if (attempt === 1) {
            await answer(c, id, text);
            throw new Error('Request timed out');
          }
          break;
        case 'inprogress':
          // The response is lost while the reply is still being generated.
          if (attempt === 1) {
            void answer(c, id, text, 8000);
            await sleep(200);
            throw new Error('Failed to fetch');
          }
          break;
        case 'hang':
          // The connection hangs and the request never arrives.
          if (attempt === 1) return new Promise<SendOutcome>(() => {});
          break;
        case 'latesave':
          // Both attempts fail in transit; the first one is executed 8 s later anyway.
          if (attempt <= 2) {
            if (attempt === 1) void sleep(8000).then(() => check(c, id, base, text) ?? answer(c, id, text, 0));
            await sleep(200);
            throw new Error('Failed to fetch');
          }
          break;
      }
      return answer(c, id, text);
    },

    async status(id: string): Promise<ReplyStatus> {
      guard();
      const c = conv();
      await sleep(100);
      const exchange = saved(c, id);
      if (exchange) return { done: exchange.sent };
      return c.inFlight?.id === id && !c.inFlight.cleared ? 'pending' : 'unknown';
    },

    clear() {
      stats.clears += 1;
      // As a backend that fails before it has a promise to return could.
      if (scenario === 'clearthrows') throw new Error('Failed to fetch');
      return clearConversation();
    },

    sessionExpiresAt() {
      return signedIn ? expiresAt : null;
    },
  };

  /** What the canister does with a clear of the caller's conversation. */
  async function clearConversation(): Promise<number> {
    guard();
    const c = conv();
    await sleep(clearMs ?? 300);
    // What the agent reports when the canister rejects a call: certainly not run.
    if (scenario === 'clearerror') throw new Error('Call was rejected: mock clear refused');
    if (scenario === 'clearhang') return new Promise<number>(() => {});
    if (scenario === 'clearfail') throw new Error('Failed to fetch');
    if (staleReads > 0) {
      c.beforeClear = copy(c);
      c.staleLeft = staleReads;
    }
    c.revision += 1;
    c.messages = [];
    if (c.inFlight) c.inFlight.cleared = true;
    const cleared = c.revision;
    if (scenario === 'clearovertaken') {
      otherTab(c);
      localStorage.setItem(floorKeyOf(principal), `0:${c.revision}`);
    }
    if (scenario === 'clearlost') throw new Error('Failed to fetch');
    return cleared;
  }
}
