import { describe, expect, it, vi } from 'vitest';
import { NO_REPLY, SessionInvalid, type ReplyStatus, type SendOutcome } from '../backend';
import { BUSY_REASON, deliver, type DeliveryDeps } from './delivery';

type Step<T> = T | Error;

/** Deps with scripted answers and a virtual clock; the last scripted answer repeats. */
function fake(sends: Step<SendOutcome>[], statuses: Step<ReplyStatus>[] = []) {
  let clock = 0;
  let active = true;
  const calls: [string, number][] = [];
  // [time sent, timeout given]
  const timeouts: [number, number][] = [];
  const next = <T>(list: Step<T>[]): T => {
    const step = list.length > 1 ? list.shift()! : list[0];
    if (step instanceof Error) throw step;
    return step;
  };
  const deps: DeliveryDeps = {
    send: vi.fn(async (id: string, base: number, _text: string, timeoutMs: number) => {
      calls.push([id, base]);
      timeouts.push([clock, timeoutMs]);
      return next(sends);
    }),
    status: vi.fn(async () => next(statuses)),
    sleep: async (ms) => {
      clock += ms;
    },
    now: () => clock,
    active: () => active,
    onChecking: vi.fn(),
  };
  return { deps, calls, timeouts, deactivate: () => (active = false), elapsed: () => clock };
}

const lost = () => new Error('Failed to fetch');
const ok = (reply: string, revision = 1): SendOutcome => ({ ok: { reply, revision } });

describe('deliver', () => {
  vi.spyOn(console, 'error').mockImplementation(() => {});

  it('returns the reply and the new revision', async () => {
    const { deps } = fake([ok('hi', 4)]);
    expect(await deliver('r1', 3, 'hello', deps)).toEqual({ kind: 'replied', reply: 'hi', revision: 4 });
    expect(deps.onChecking).not.toHaveBeenCalled();
  });

  it('reports definite failures, Busy and conflicts', async () => {
    expect(await deliver('r1', 0, 'x', fake([{ err: 'nope' }]).deps)).toEqual({ kind: 'failed', reason: 'nope' });
    expect(await deliver('r1', 0, 'x', fake([{ busy: true }]).deps)).toEqual({ kind: 'failed', reason: BUSY_REASON });
    expect(await deliver('r1', 0, 'x', fake([{ conflict: true }]).deps)).toEqual({ kind: 'conflict', uncertain: false });
  });

  it('keeps the uncertainty of an earlier lost attempt through Busy and conflicts', async () => {
    expect(await deliver('r1', 0, 'x', fake([lost(), { busy: true }]).deps)).toEqual({
      kind: 'unconfirmed',
      reason: BUSY_REASON,
    });
    expect(await deliver('r1', 0, 'x', fake([{ busy: true }]).deps, { uncertain: true })).toEqual({
      kind: 'unconfirmed',
      reason: BUSY_REASON,
    });
    expect(await deliver('r1', 0, 'x', fake([lost(), { conflict: true }]).deps)).toEqual({ kind: 'conflict', uncertain: true });
    expect(await deliver('r1', 0, 'x', fake([{ conflict: true }]).deps, { uncertain: true })).toEqual({
      kind: 'conflict',
      uncertain: true,
    });
  });

  it('re-sends once with the same id and base after a lost response', async () => {
    const { deps, calls } = fake([lost(), ok('saved reply')]);
    expect(await deliver('r1', 7, 'hello', deps)).toEqual({ kind: 'replied', reply: 'saved reply', revision: 1 });
    expect(calls).toEqual([
      ['r1', 7],
      ['r1', 7],
    ]);
    expect(deps.onChecking).toHaveBeenCalled();
  });

  it('is unconfirmed when the re-send is lost too, without sending a third time', async () => {
    const { deps, calls } = fake([lost(), lost(), ok('never')]);
    expect(await deliver('r1', 0, 'hello', deps)).toEqual({ kind: 'unconfirmed' });
    expect(calls).toHaveLength(2);
  });

  it('treats a failed or refused call as a failure only when no earlier attempt can still be saved', async () => {
    for (const outcome of [{ rejected: true }, { failed: true }] as SendOutcome[]) {
      expect(await deliver('r1', 0, 'x', fake([outcome]).deps)).toEqual({ kind: 'failed', reason: NO_REPLY });
      // The first attempt may still be in transit and be saved after this one failed.
      expect(await deliver('r1', 0, 'x', fake([lost(), outcome]).deps)).toEqual({ kind: 'unconfirmed', reason: NO_REPLY });
      // A retry of an unconfirmed message.
      expect(await deliver('r1', 0, 'x', fake([outcome]).deps, { uncertain: true })).toEqual({
        kind: 'unconfirmed',
        reason: NO_REPLY,
      });
    }
  });

  it('passes session errors through', async () => {
    await expect(deliver('r1', 0, 'x', fake([new SessionInvalid('expired')]).deps)).rejects.toBeInstanceOf(SessionInvalid);
    await expect(deliver('r1', 0, 'x', fake([lost(), new SessionInvalid('expired')]).deps)).rejects.toBeInstanceOf(
      SessionInvalid,
    );
    await expect(
      deliver('r1', 0, 'x', fake([{ inProgress: true }], [new SessionInvalid('expired')]).deps),
    ).rejects.toBeInstanceOf(SessionInvalid);
  });

  it('waits for a reply that is still being generated', async () => {
    const { deps, calls } = fake(
      [{ inProgress: true }],
      ['pending', new Error('offline'), 'pending', { done: { reply: 'late', revision: 2 } }],
    );
    expect(await deliver('r1', 1, 'hello', deps)).toEqual({ kind: 'replied', reply: 'late', revision: 2 });
    expect(calls).toHaveLength(1);
  });

  it('treats an unknown status as no verdict and asks the canister again', async () => {
    const { deps, calls } = fake([{ inProgress: true }, { inProgress: true }, ok('replayed')], ['unknown']);
    expect(await deliver('r1', 0, 'hello', deps)).toEqual({ kind: 'replied', reply: 'replayed', revision: 1 });
    expect(calls.map(([id, base]) => `${id}@${base}`)).toEqual(['r1@0', 'r1@0', 'r1@0']);
  });

  it('gives up as unconfirmed after five minutes, however the canister keeps answering', async () => {
    for (const statuses of [['pending'], ['unknown']] as ReplyStatus[][]) {
      const { deps, calls, timeouts, elapsed } = fake([{ inProgress: true }], statuses);
      expect(await deliver('r1', 0, 'hello', deps)).toEqual({ kind: 'unconfirmed' });
      expect(elapsed()).toBeLessThanOrEqual(5 * 60 * 1000);
      expect(calls.length).toBeLessThan(60);
      // No call may outlast the window.
      for (const [at, timeout] of timeouts) expect(at + timeout).toBeLessThanOrEqual(5 * 60 * 1000);
    }
  });

  it('never starts or waits for a call past the window', async () => {
    // Every send hangs until its timeout.
    const f = fake([lost()]);
    f.deps.send = vi.fn(async (_id: string, _base: number, _text: string, timeoutMs: number) => {
      await f.deps.sleep(timeoutMs);
      throw new Error('Timed out');
    });
    expect(await deliver('r1', 0, 'hello', f.deps)).toEqual({ kind: 'unconfirmed' });
    expect(f.elapsed()).toBeLessThanOrEqual(5 * 60 * 1000);
  });

  it('keeps to a deadline it is given, including the wait before a re-send', async () => {
    const f = fake([lost(), ok('late')]);
    expect(await deliver('r1', 0, 'hello', f.deps, { deadline: 1000 })).toEqual({ kind: 'unconfirmed' });
    expect(f.elapsed()).toBeLessThanOrEqual(1000);
    expect(f.calls).toHaveLength(1);
  });

  it('stops once the result is no longer wanted', async () => {
    const f = fake([lost(), ok('too late')]);
    f.deps.sleep = async () => {
      f.deactivate();
    };
    expect(await deliver('r1', 0, 'hello', f.deps)).toBeNull();
    expect(f.calls).toHaveLength(1);
  });
});
