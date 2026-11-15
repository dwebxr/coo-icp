import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

let data: Map<string, string>;
let writes: number;
let sync: typeof import('./sessionSync');

/** A fresh copy of the module: another tab of the same browser (same storage, same clocks). */
async function openTab(): Promise<typeof import('./sessionSync')> {
  vi.resetModules();
  return import('./sessionSync');
}

/** The time part of the stored activity ("<wall ms>|<tab id>"). */
const storedTime = () => data.get('coo:last-active')?.split('|')[0];

/**
 * A stand-in for the browser's Web Locks, shared by all "tabs" of a test: a section runs at once unless another one
 * holds the lock, in which case it runs right after that one (as navigator.locks does, but synchronously).
 */
function mutex() {
  let busy = false;
  const waiting: (() => void)[] = [];
  return {
    request(_name: string, section: () => unknown) {
      return new Promise((resolve) => {
        const run = () => {
          busy = true;
          let result: unknown;
          try {
            result = section();
          } finally {
            busy = false;
          }
          resolve(result);
          waiting.shift()?.();
        };
        if (busy) waiting.push(run);
        else run();
      });
    },
  };
}

/** Moves both clocks forward to `ms` (wall clock, epoch ms), running the timers due on the way. */
const at = (ms: number) => vi.advanceTimersByTime(ms - Date.now());

beforeEach(async () => {
  data = new Map();
  writes = 0;
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      writes += 1;
      data.set(key, value);
    },
    removeItem: (key: string) => data.delete(key),
  });
  // Node has Web Locks of its own (asynchronous); the tests use the stand-in above.
  vi.stubGlobal('navigator', { locks: mutex() });
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date', 'performance'], now: 0 });
  sync = await openTab();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('shared activity', () => {
  it('is unknown until something was recorded', () => {
    at(1_000);
    expect(sync.idleFor()).toBeNull();
    sync.noteActivity();
    at(61_000);
    expect(sync.idleFor()).toBe(60_000);
  });

  it('counts activity recorded by another tab', () => {
    at(1_000);
    sync.noteActivity();
    data.set('coo:last-active', '50000'); // another tab, later
    at(61_000);
    expect(sync.idleFor()).toBe(11_000);
  });

  it('writes to storage at most every 15 seconds, unless forced', () => {
    at(100_000);
    sync.noteActivity();
    for (let t = 100_500; t < 114_000; t += 500) {
      at(t);
      sync.noteActivity();
    }
    expect(writes).toBe(1);
    // This tab still knows its own latest activity.
    at(114_000);
    expect(sync.idleFor()).toBe(500);
    // The activity held back is written when the 15 seconds are over (see below), and a forced write goes through.
    at(115_000);
    expect(writes).toBe(2);
    sync.noteActivity(true);
    expect(writes).toBe(3);
  });

  it('is not confused by a clock set back, or by nonsense in storage', () => {
    at(100_000);
    sync.noteActivity();
    vi.setSystemTime(90_000);
    expect(sync.idleFor()).toBe(0);
    sync.clearActivity();
    expect(sync.idleFor()).toBeNull();
    data.set('coo:last-active', 'soon');
    expect(sync.idleFor()).toBeNull();
  });

  it('survives storage that throws', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('SecurityError');
      },
      setItem: () => {
        throw new Error('QuotaExceededError');
      },
      removeItem: () => {
        throw new Error('SecurityError');
      },
    });
    at(1_000);
    sync.noteActivity();
    at(2_000);
    expect(sync.idleFor()).toBe(1_000);
    sync.clearActivity();
    expect(sync.idleFor()).toBeNull();
    expect(sync.signOutGeneration()).toBeNull();
    expect(() => sync.announceSignOut()).not.toThrow();
  });
});

// Review finding 12.
describe('idle time and the clocks', () => {
  it('a clock set back an hour does not stretch this tab\'s idle time', () => {
    const start = 10 * 3600_000;
    at(start);
    sync.noteActivity(true);
    vi.setSystemTime(start - 3600_000); // the wall clock goes back an hour; the monotonic clock does not
    vi.advanceTimersByTime(10 * 60_000);
    // Ten real minutes later the tab is ten minutes idle, not still "active" for another hour.
    expect(sync.idleFor()).toBe(10 * 60_000);
  });

  it('a computer that slept (the wall clock ran, the monotonic clock did not) counts the sleep', () => {
    at(1_000);
    sync.noteActivity(true);
    vi.setSystemTime(1_000 + 30 * 60_000);
    expect(sync.idleFor()).toBe(30 * 60_000);
  });

  it("ignores another tab's time that lies in the future instead of reading it as active now", () => {
    at(1_000_000);
    sync.noteActivity(true);
    data.set('coo:last-active', String(1_000_000 + 3600_000));
    at(1_000_000 + 11 * 60_000);
    expect(sync.idleFor()).toBe(11 * 60_000);
    // A tab with no activity of its own has nothing to go by.
    sync.clearActivity();
    data.set('coo:last-active', String(Date.now() + 60_000));
    expect(sync.idleFor()).toBeNull();
  });

  it('writes activity the throttle held back when the interval ends', () => {
    at(100_000);
    sync.noteActivity(); // written
    at(114_000);
    sync.noteActivity(); // held back
    expect(storedTime()).toBe('100000');
    at(114_999);
    expect(storedTime()).toBe('100000');
    at(115_000);
    expect(storedTime()).toBe('114000');
    // A sign-out in between cancels it.
    at(120_000);
    sync.noteActivity(true);
    at(121_000);
    sync.noteActivity();
    sync.clearActivity();
    at(200_000);
    expect(data.has('coo:last-active')).toBe(false);
  });

  // Review round 2, I5.
  it("a clock set back five minutes after this tab's own activity does not stretch it to fifteen", () => {
    const start = 10 * 3600_000;
    at(start);
    sync.noteActivity(true); // stored, by this tab
    vi.setSystemTime(start - 5 * 60_000);
    vi.advanceTimersByTime(10 * 60_000);
    // The stored time (this tab's own) looks five minutes old by the wall clock; this tab knows better.
    expect(sync.idleFor()).toBe(10 * 60_000);
  });

  // Review round 2, I9.
  it("a write the throttle held back does not replace another tab's later activity", async () => {
    const a = sync;
    const b = await openTab();
    at(1_000);
    a.noteActivity(true);
    at(15_000);
    a.noteActivity(); // held back until 16 s
    at(15_500);
    b.noteActivity(true); // B, later
    expect(data.get('coo:last-active')).toMatch(/^15500\|/);
    const bWrote = data.get('coo:last-active');
    at(16_000); // A's trailing write: 15 s is older than B's 15.5 s
    expect(data.get('coo:last-active')).toBe(bWrote);
    // A later activity of A's does replace it.
    at(40_000);
    a.noteActivity();
    expect(storedTime()).toBe('40000');
  });

  // Review round 3, Important ④: tab A reads the stored time, tab B writes a later one, then A writes. Under the
  // lock, B's write waits until A's read-and-write is done, and then still finds A's time older than its own.
  it("another tab's later write is not lost between this tab's read and its write", async () => {
    const a = sync;
    const b = await openTab();
    at(1_000);
    a.noteActivity(true);
    let interleaved = false;
    vi.stubGlobal('localStorage', {
      getItem: (key: string) => {
        const value = data.get(key) ?? null;
        if (key === 'coo:last-active' && !interleaved && Date.now() === 15_000) {
          // While A is between its read and its write, B (another tab, half a second later) writes.
          interleaved = true;
          vi.setSystemTime(15_500);
          b.noteActivity(true);
          vi.setSystemTime(15_000);
        }
        return value;
      },
      setItem: (key: string, value: string) => data.set(key, value),
      removeItem: (key: string) => data.delete(key),
    });
    at(15_000);
    a.noteActivity(true);
    expect(interleaved).toBe(true);
    expect(data.get('coo:last-active')).toMatch(/^15500\|/);
  });

  it('without Web Locks it still only writes a later time (best effort)', async () => {
    vi.stubGlobal('navigator', {});
    const a = await openTab();
    const b = await openTab();
    at(1_000);
    a.noteActivity(true);
    at(15_000);
    a.noteActivity(); // held back until 16 s
    at(15_500);
    b.noteActivity(true);
    at(16_000);
    expect(data.get('coo:last-active')).toMatch(/^15500\|/);
  });

  // Review round 4: a lock that cannot be had must not lose the write.
  it('writes directly when the lock request is refused', async () => {
    vi.stubGlobal('navigator', { locks: { request: () => Promise.reject(new Error('SecurityError')) } });
    at(1_000);
    sync.noteActivity(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(storedTime()).toBe('1000');
  });

  it('a write still waiting for the lock when the tab signs out is dropped', () => {
    const waiting: (() => unknown)[] = [];
    vi.stubGlobal('navigator', {
      locks: { request: (_name: string, section: () => unknown) => (waiting.push(section), Promise.resolve()) },
    });
    at(1_000);
    sync.noteActivity(true); // waits for the lock
    sync.clearActivity(); // signs out
    for (const section of waiting) section();
    expect(data.has('coo:last-active')).toBe(false);
  });

  it('a stored time in the future can be replaced (it says nothing)', () => {
    at(1_000_000);
    data.set('coo:last-active', String(1_000_000 + 3600_000));
    sync.noteActivity(true);
    expect(storedTime()).toBe('1000000');
  });

  it("the reviewer's 14 seconds: neither tab signs out before its own ten minutes", async () => {
    const TEN_MINUTES = 10 * 60_000;
    const a = sync;
    const b = await openTab();
    at(1_000);
    a.noteActivity(true); // both tabs signed in
    b.noteActivity(true);
    at(15_000);
    a.noteActivity(); // 14 s after the last write: held back by the throttle
    // B has not been used since; A's ten minutes end at 615 s.
    at(1_000 + TEN_MINUTES);
    expect(b.idleFor()!).toBeLessThan(TEN_MINUTES);
    expect(a.idleFor()!).toBeLessThan(TEN_MINUTES);
    at(15_000 + TEN_MINUTES - 1);
    expect(b.idleFor()!).toBeLessThan(TEN_MINUTES);
    expect(a.idleFor()!).toBeLessThan(TEN_MINUTES);
    at(15_000 + TEN_MINUTES);
    expect(a.idleFor()).toBe(TEN_MINUTES);
    expect(b.idleFor()).toBe(TEN_MINUTES);
  });
});

// Review finding 2.
describe('sign-out across tabs', () => {
  let target: EventTarget;
  const storageEvent = (key: string | null, newValue: string | null, oldValue: string | null = null) =>
    target.dispatchEvent(Object.assign(new Event('storage'), { key, newValue, oldValue }));
  /** Another tab signs out: it writes a new generation, and this tab gets the event for it. */
  const signsOut = (generation: string) => {
    const old = data.get('coo:signed-out') ?? null;
    data.set('coo:signed-out', generation);
    storageEvent('coo:signed-out', generation, old);
  };

  beforeEach(() => {
    target = new EventTarget();
    vi.stubGlobal('window', target);
  });

  it('changes the generation on every sign-out, and keeps it', () => {
    expect(sync.signOutGeneration()).toBeNull();
    sync.announceSignOut();
    const first = sync.signOutGeneration();
    expect(first).not.toBeNull();
    sync.announceSignOut();
    expect(sync.signOutGeneration()).not.toBe(first);
    // Durable: a tab that reads it later (after restoring a session, say) sees the change.
    const later = sync.signOutGeneration();
    expect(later).toBe(data.get('coo:signed-out'));
  });

  it('tells a tab when another tab announces a sign-out', () => {
    const heard = vi.fn();
    let sessionSaw: string | null = null;
    const stop = sync.onSignOutElsewhere(heard, () => sessionSaw);
    signsOut('g1');
    expect(heard).toHaveBeenCalledTimes(1);
    // A new session begins (it sees g1). Other keys, a removal and an event repeating g1 are not sign-outs.
    sessionSaw = 'g1';
    storageEvent('coo:last-active', '5');
    storageEvent('coo:floor:x', '0:1');
    data.delete('coo:signed-out');
    storageEvent('coo:signed-out', null, 'g1');
    data.set('coo:signed-out', 'g1');
    storageEvent('coo:signed-out', 'g1', 'g1');
    expect(heard).toHaveBeenCalledTimes(1);
    signsOut('g2');
    expect(heard).toHaveBeenCalledTimes(2);
    stop();
    signsOut('g3');
    expect(heard).toHaveBeenCalledTimes(2);
  });

  it('works without BroadcastChannel', () => {
    vi.stubGlobal('BroadcastChannel', undefined);
    const heard = vi.fn();
    sync.onSignOutElsewhere(heard, () => null);
    signsOut('g1');
    expect(heard).toHaveBeenCalledTimes(1);
  });

  // Review round 2, I7: A signs out (writes g1) → B's sign-in completes (its session saw g1) → the event reaches B.
  it('ignores a notification of the generation the current session started with', () => {
    const heard = vi.fn();
    let sessionSaw: string | null = null;
    sync.onSignOutElsewhere(heard, () => sessionSaw);
    data.set('coo:signed-out', 'g1'); // tab A signs out
    sessionSaw = sync.signOutGeneration(); // tab B's new session starts, after that
    storageEvent('coo:signed-out', 'g1', null); // A's event, delivered late
    expect(heard).not.toHaveBeenCalled();
    signsOut('g2'); // a sign-out after the session began
    expect(heard).toHaveBeenCalledTimes(1);
  });

  // Review round 3, Nit ③: two sign-outs (g1, g2) before the session began; it saw g2; g1's event arrives last.
  it('ignores a late notification of an older sign-out too: what counts is the generation stored now', () => {
    const heard = vi.fn();
    let sessionSaw: string | null = null;
    sync.onSignOutElsewhere(heard, () => sessionSaw);
    data.set('coo:signed-out', 'g1');
    data.set('coo:signed-out', 'g2');
    sessionSaw = sync.signOutGeneration();
    storageEvent('coo:signed-out', 'g1', null); // the older event, delivered late
    storageEvent('coo:signed-out', 'g2', 'g1');
    expect(heard).not.toHaveBeenCalled();
    signsOut('g3');
    expect(heard).toHaveBeenCalledTimes(1);
  });
});
