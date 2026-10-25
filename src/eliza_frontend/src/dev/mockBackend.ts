// Development-only backend used to preview and test UI states without Internet Identity.
// Loaded from main.tsx only when import.meta.env.DEV is true and the URL has ?mock=<scenario>.
// It mirrors the canister's semantics: one chat at a time, a request runs only after the revision it names, and a
// repeated request id replays the saved reply. (Taking over a chat stuck for 5 minutes is not modelled.)
//
// Scenarios: signedout | empty | chat | long | rich | slow | error | busy | offline | lostreply | inprogress | hang |
//            latesave | conflict | stale | laggy (one history read from before the last clear) | clearerror |
//            historyerror | expiring | badsession
// Extra params: signin=cancel|error|hang  delay=<ms>
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
      'I run inside a canister. Each message I get is sent to the on-chain LLM canister:\n\n' +
      '```rust\nlet reply = ic_llm::chat(Model::Llama3_1_8B)\n    .with_messages(messages)\n    .send()\n    .await;\n```\n\n' +
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
      '---\n\n' +
      'Docs: https://internetcomputer.org/docs/building-apps/essentials/canisters-overview-and-a-very-long-path-segment',
  },
];

const REPLY =
  'Good question. Here is the short version:\n\n- Canisters hold code **and** state\n- Calls are signed by your identity\n- `query` calls are fast; `update` calls go through consensus\n\nAnything else?';

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// The canister keeps at most this many messages, its system prompt included.
const MAX_CONVERSATION_LENGTH = 50;

export function createMockBackend(scenario: string): Backend {
  const params = new URLSearchParams(window.location.search);
  const signInMode = params.get('signin');
  const delay = Number(params.get('delay') ?? (scenario === 'slow' ? 15000 : 1400));

  let signedIn = scenario !== 'signedout' && scenario !== '';
  let expiresAt = Date.now() + (scenario === 'expiring' ? 4000 : 8 * 3600 * 1000);
  let messages: Message[] =
    scenario === 'empty' || scenario === 'signedout'
      ? []
      : scenario === 'long'
        ? Array.from({ length: 6 }, () => SAMPLE).flat()
        : scenario === 'rich'
          ? [...RICH]
          : [...SAMPLE];
  let revision = 0;
  // laggy: what a replica that is behind would still return after a clear.
  let beforeClear: { messages: Message[]; revision: number } | null = null;
  let inFlight: { id: string; cleared: boolean } | null = null;
  // Tests read how many requests were sent and how many were answered.
  const stats = { sends: 0, answered: 0, ids: [] as string[] };
  (window as unknown as { __mock: typeof stats }).__mock = stats;

  const session = (): Session => ({ principal: PRINCIPAL });
  const guard = () => {
    if (!signedIn || Date.now() >= expiresAt) throw new SessionInvalid('mock session invalid');
  };

  /** The saved exchange for a request id: the user message and the reply right after it. */
  const saved = (id: string): { text: string; sent: Sent } | null => {
    const i = messages.findIndex((m) => m.role === 'user' && m.requestId === id);
    const reply = messages[i + 1];
    return i >= 0 && reply?.role === 'assistant' ? { text: messages[i].content, sent: { reply: reply.content, revision } } : null;
  };

  /** Another tab saves an exchange. */
  const otherTab = () => {
    messages = [...messages, { role: 'user', content: 'Sent from another tab' }, { role: 'assistant', content: REPLY }];
    revision += 1;
  };

  /** The canister's checks before it runs a request: replay, one chat at a time, revision. */
  function check(id: string, base: number, text: string): SendOutcome | null {
    const exchange = saved(id);
    if (exchange) return exchange.text === text ? { ok: exchange.sent } : { err: 'This request id was used for another message.' };
    if (inFlight) return inFlight.id === id ? { inProgress: true } : { busy: true };
    if (base !== revision) return { conflict: true };
    return null;
  }

  /** What the canister does with a request: generate a reply, then save the exchange. */
  async function answer(id: string, text: string, ms = delay): Promise<SendOutcome> {
    const flight = { id, cleared: false };
    inFlight = flight;
    await sleep(ms);
    inFlight = null;
    if (flight.cleared) return { err: 'Conversation cleared.' };
    if (scenario === 'error') return { failed: true };
    // Trim like the canister (which also keeps a system prompt), then add the reply.
    const next = [...messages, { role: 'user', content: text, requestId: id }];
    const kept = next.length + 1 > MAX_CONVERSATION_LENGTH ? next.slice(1 - MAX_CONVERSATION_LENGTH) : next;
    messages = [...kept, { role: 'assistant', content: REPLY }];
    revision += 1;
    stats.answered += 1;
    return { ok: { reply: REPLY, revision } };
  }

  return {
    async restore() {
      await sleep(250);
      return signedIn ? session() : null;
    },

    async signIn() {
      await sleep(1200);
      if (signInMode === 'hang') return new Promise<Session>(() => {});
      if (signInMode === 'cancel') throw new SignInCancelled();
      if (signInMode === 'error') throw new Error('mock sign-in failure');
      signedIn = true;
      expiresAt = Date.now() + 8 * 3600 * 1000;
      return session();
    },

    async signOut() {
      await sleep(80);
      signedIn = false;
    },

    async history() {
      await sleep(scenario === 'historyerror' ? 600 : 300);
      if (scenario === 'badsession') throw new SessionInvalid('mock orphan delegation');
      guard();
      if (scenario === 'historyerror') throw new Error('mock history failure');
      if (beforeClear) {
        const stale = beforeClear;
        beforeClear = null;
        return stale;
      }
      return { messages: messages.map((m) => ({ ...m })), revision };
    },

    async send(id: string, base: number, text: string): Promise<SendOutcome> {
      guard();
      const attempt = ++stats.sends;
      stats.ids.push(id);
      // Before the first send, another tab changes the conversation: visibly, or (stale) only its revision.
      if (attempt === 1 && scenario === 'conflict') otherTab();
      if (attempt === 1 && scenario === 'stale') revision += 1;
      const refused = check(id, base, text);
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
            await answer(id, text);
            throw new Error('Request timed out');
          }
          break;
        case 'inprogress':
          // The response is lost while the reply is still being generated.
          if (attempt === 1) {
            void answer(id, text, 8000);
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
            if (attempt === 1) void sleep(8000).then(() => check(id, base, text) ?? answer(id, text, 0));
            await sleep(200);
            throw new Error('Failed to fetch');
          }
          break;
      }
      return answer(id, text);
    },

    async status(id: string): Promise<ReplyStatus> {
      guard();
      await sleep(100);
      const exchange = saved(id);
      if (exchange) return { done: exchange.sent };
      return inFlight?.id === id && !inFlight.cleared ? 'pending' : 'unknown';
    },

    async clear() {
      guard();
      await sleep(300);
      if (scenario === 'clearerror') throw new Error('mock clear failure');
      if (scenario === 'laggy') beforeClear = { messages: messages.map((m) => ({ ...m })), revision };
      revision += 1;
      messages = [];
      if (inFlight) inFlight.cleared = true;
      return revision;
    },

    sessionExpiresAt() {
      return signedIn ? expiresAt : null;
    },
  };
}
