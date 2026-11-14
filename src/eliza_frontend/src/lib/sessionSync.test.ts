import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

let data: Map<string, string>;
let writes: number;
let sync: typeof import('./sessionSync');

/** A fresh copy of the module: another tab of the same browser (same storage, same clocks). */
async function openTab(): Promise<typeof import('./sessionSync')> {
  vi.resetModules();
  return import('./sessionSync');
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
    expect(data.get('coo:last-active')).toBe('100000');
    at(114_999);
    expect(data.get('coo:last-active')).toBe('100000');
    at(115_000);
    expect(data.get('coo:last-active')).toBe('114000');
    // A sign-out in between cancels it.
    at(120_000);
    sync.noteActivity(true);
    at(121_000);
    sync.noteActivity();
    sync.clearActivity();
    at(200_000);
    expect(data.has('coo:last-active')).toBe(false);
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
    const stop = sync.onSignOutElsewhere(heard);
    storageEvent('coo:signed-out', 'g1');
    expect(heard).toHaveBeenCalledTimes(1);
    // Other keys, a removal and an unchanged value are not sign-outs.
    storageEvent('coo:last-active', '5');
    storageEvent('coo:floor:x', '0:1');
    storageEvent('coo:signed-out', null, 'g1');
    storageEvent('coo:signed-out', 'g1', 'g1');
    expect(heard).toHaveBeenCalledTimes(1);
    storageEvent('coo:signed-out', 'g2', 'g1');
    expect(heard).toHaveBeenCalledTimes(2);
    stop();
    storageEvent('coo:signed-out', 'g3', 'g2');
    expect(heard).toHaveBeenCalledTimes(2);
  });

  it('works without BroadcastChannel', () => {
    vi.stubGlobal('BroadcastChannel', undefined);
    const heard = vi.fn();
    sync.onSignOutElsewhere(heard);
    storageEvent('coo:signed-out', 'g1');
    expect(heard).toHaveBeenCalledTimes(1);
  });
});
