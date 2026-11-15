import { describe, expect, it } from 'vitest';
import type { Message } from '../backend';
import { judgeLostClear, mergeHistory, reconcile, turnsKey, type ChatMessage } from './thread';

const saved = (role: string, content: string, requestId?: string): Message => ({ role, content, requestId });
const shown = (id: string, role: 'user' | 'assistant', content: string, extra: Partial<ChatMessage> = {}): ChatMessage => ({
  id,
  role,
  content,
  ...extra,
});
const texts = (list: ChatMessage[]) => list.map((m) => `${m.role}:${m.content}`);

describe('mergeHistory', () => {
  it('keeps the ids of unchanged leading messages and renews the rest', () => {
    const prev = [shown('a', 'user', 'hi', { requestId: 'r1' }), shown('b', 'assistant', 'hello'), shown('c', 'user', 'old')];
    const next = mergeHistory(prev, [saved('user', 'hi', 'r1'), saved('assistant', 'hello'), saved('user', 'new', 'r2')]);
    expect(next.slice(0, 2)).toEqual(prev.slice(0, 2));
    expect(next[2]).toMatchObject({ role: 'user', content: 'new', requestId: 'r2' });
    expect(next[2].id).not.toBe('c');
  });

  it('shows only user and assistant turns', () => {
    const next = mergeHistory([], [saved('system', 'prompt'), saved('user', 'hi'), saved('tool', 'x'), saved('assistant', 'yo')]);
    expect(texts(next)).toEqual(['user:hi', 'assistant:yo']);
  });

  it('does not keep a message that was marked as failed, even with the same text', () => {
    const prev = [shown('a', 'user', 'hi', { error: 'Not sent' })];
    const next = mergeHistory(prev, [saved('user', 'hi')]);
    expect(next[0].id).not.toBe('a');
    expect(next[0].error).toBeUndefined();
  });

  it('clears the "checking" mark of a message that turned out to be saved', () => {
    const prev = [shown('a', 'user', 'hi', { requestId: 'r1', checking: true })];
    expect(mergeHistory(prev, [saved('user', 'hi', 'r1')])[0]).toEqual({ id: 'a', role: 'user', content: 'hi', requestId: 'r1' });
  });
});

describe('reconcile', () => {
  it('keeps failed messages and the one being sent after the saved history', () => {
    const prev = [
      shown('a', 'user', 'hi', { requestId: 'r1' }),
      shown('b', 'assistant', 'hello'),
      shown('c', 'user', 'lost', { requestId: 'r2', error: 'Connection lost.', unconfirmed: true }),
      shown('d', 'user', 'sending', { requestId: 'r3' }),
    ];
    const next = reconcile(prev, [saved('user', 'hi', 'r1'), saved('assistant', 'hello')], 'd');
    expect(next.map((m) => m.id)).toEqual(['a', 'b', 'c', 'd']);
  });

  it('drops the local copy of a message that was saved after all', () => {
    const prev = [shown('c', 'user', 'lost', { requestId: 'r2', error: 'Connection lost.', unconfirmed: true })];
    const next = reconcile(prev, [saved('user', 'lost', 'r2'), saved('assistant', 'found')]);
    expect(texts(next)).toEqual(['user:lost', 'assistant:found']);
    expect(next.every((m) => !m.error)).toBe(true);
  });

  it('drops messages that were neither saved, failed nor being sent', () => {
    // After a clear in another tab: what this tab showed is gone.
    const prev = [shown('a', 'user', 'hi', { requestId: 'r1' }), shown('b', 'assistant', 'hello')];
    expect(reconcile(prev, [])).toEqual([]);
  });
});

describe('turnsKey', () => {
  it('changes with what is visible, and only with that', () => {
    const a = turnsKey([saved('system', 'p'), saved('user', 'hi', 'r1'), saved('assistant', 'yo')]);
    expect(turnsKey([saved('user', 'hi', 'other-id'), saved('assistant', 'yo')])).toBe(a);
    expect(turnsKey([saved('user', 'hi'), saved('assistant', 'yo!')])).not.toBe(a);
    // "ab" + "c" is not "a" + "bc".
    expect(turnsKey([saved('user', 'ab'), saved('user', 'c')])).not.toBe(turnsKey([saved('user', 'a'), saved('user', 'bc')]));
  });
});

// Review round 5: a clear whose answer was lost, judged by the conversation loaded through consensus afterwards.
describe('judgeLostClear', () => {
  it('cleared when empty at a later revision, failed when not empty, not known when nothing was loaded', () => {
    expect(judgeLostClear({ messages: [], revision: 11 }, 10)).toBe('cleared');
    expect(judgeLostClear({ messages: [{}], revision: 10 }, 10)).toBe('failed');
    // Empty but not later: the conversation was already empty, and this clear did not run.
    expect(judgeLostClear({ messages: [], revision: 10 }, 10)).toBe('failed');
    expect(judgeLostClear(null, 10)).toBe('unconfirmed');
  });
});
