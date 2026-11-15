// The thread the user sees: the saved conversation plus what this tab knows that the canister does not (yet).
// Pure functions, used by useCoo.

import type { Message } from '../backend';

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  /** Identifies a user message to the canister, which saves it at most once for its base revision. */
  requestId?: string;
  /** The conversation revision the message was sent after. */
  base?: number;
  /** Set on a user message that has no reply. */
  error?: string;
  /** The response was lost, so the canister may still have answered. Retrying is safe. */
  unconfirmed?: boolean;
  /** The first attempt went unanswered; the outcome is being checked. */
  checking?: boolean;
}

let seq = 0;
export const nextId = () => `m${++seq}`;

const isTurn = (m: { role: string }) => m.role === 'user' || m.role === 'assistant';

/** What the conversation looks like, to tell whether anything visible changed. */
export const turnsKey = (list: { role: string; content: string }[]) =>
  list
    .filter(isTurn)
    .map((m) => `${m.role}:${m.content}`)
    .join('\u0000');

/** Saved history as chat messages, reusing ids of unchanged leading messages so they do not re-render. */
export function mergeHistory(prev: ChatMessage[], raw: Message[]): ChatMessage[] {
  let same = true;
  return raw.filter(isTurn).map((m, i) => {
    const old = prev[i];
    same =
      same && !!old && !old.error && old.role === m.role && old.content === m.content && old.requestId === m.requestId;
    if (!same) {
      return { id: nextId(), role: m.role as ChatMessage['role'], content: m.content, requestId: m.requestId };
    }
    return old.checking ? { ...old, checking: undefined } : old;
  });
}

/**
 * Saved history followed by the thread's user messages the canister does not have: failed ones, and `activeId`
 * (one being sent). A message whose request id is in the history was saved after all, so its copy is dropped.
 */
export function reconcile(prev: ChatMessage[], raw: Message[], activeId?: string): ChatMessage[] {
  const merged = mergeHistory(prev, raw);
  const savedIds = new Set(raw.flatMap((m) => (m.requestId ? [m.requestId] : [])));
  const shown = new Set(merged.map((m) => m.id));
  const unsaved = prev.filter(
    (m) =>
      m.role === 'user' &&
      (m.error || m.id === activeId) &&
      !shown.has(m.id) &&
      !(m.requestId && savedIds.has(m.requestId)),
  );
  return [...merged, ...unsaved];
}

/**
 * What a clear whose answer was lost came to, judged by the conversation loaded through consensus afterwards (null:
 * none could be loaded). Empty, at a revision after the one the clear was made from: the conversation is cleared now
 * — by this clear or by something else — which is what was asked for. Not empty: it is not cleared. Nothing loaded:
 * not known yet.
 */
export function judgeLostClear(
  loaded: { messages: unknown[]; revision: number } | null,
  startRevision: number,
): 'cleared' | 'failed' | 'unconfirmed' {
  if (!loaded) return 'unconfirmed';
  return loaded.messages.length === 0 && loaded.revision > startRevision ? 'cleared' : 'failed';
}
