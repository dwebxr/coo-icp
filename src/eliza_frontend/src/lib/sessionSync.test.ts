import { beforeEach, describe, expect, it, vi } from 'vitest';

let data: Map<string, string>;
let writes: number;
let sync: typeof import('./sessionSync');

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
  vi.resetModules();
  sync = await import('./sessionSync');
});

describe('shared activity', () => {
  it('is unknown until something was recorded', () => {
    expect(sync.idleFor(1_000)).toBeNull();
    sync.noteActivity(1_000);
    expect(sync.idleFor(61_000)).toBe(60_000);
  });

  it('counts activity recorded by another tab', () => {
    sync.noteActivity(1_000);
    data.set('coo:last-active', '50000'); // another tab, later
    expect(sync.idleFor(61_000)).toBe(11_000);
  });

  it('writes to storage at most every 15 seconds, unless forced', () => {
    sync.noteActivity(100_000);
    for (let t = 100_001; t < 114_000; t += 500) sync.noteActivity(t);
    expect(writes).toBe(1);
    // This tab still knows its own latest activity.
    expect(sync.idleFor(114_000)).toBe(499);
    sync.noteActivity(115_000);
    expect(writes).toBe(2);
    sync.noteActivity(115_001, true);
    expect(writes).toBe(3);
  });

  it('is not confused by a clock set back, or by nonsense in storage', () => {
    sync.noteActivity(100_000);
    expect(sync.idleFor(90_000)).toBe(0);
    sync.clearActivity();
    expect(sync.idleFor(100_000)).toBeNull();
    data.set('coo:last-active', 'soon');
    expect(sync.idleFor(100_000)).toBeNull();
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
    sync.noteActivity(1_000);
    expect(sync.idleFor(2_000)).toBe(1_000);
    sync.clearActivity();
    expect(sync.idleFor(2_000)).toBeNull();
  });
});
