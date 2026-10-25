import { beforeEach, describe, expect, it, vi } from 'vitest';

/** A localStorage stand-in whose writes can be made to fail. */
function storage() {
  const data = new Map<string, string>();
  const store = {
    failing: false,
    failingReads: 0,
    getItem: (key: string) => {
      if (store.failingReads > 0) {
        store.failingReads -= 1;
        throw new Error('SecurityError');
      }
      return data.get(key) ?? null;
    },
    setItem: (key: string, value: string) => {
      if (store.failing) throw new Error('QuotaExceededError');
      data.set(key, value);
    },
    data,
  };
  return store;
}

let store: ReturnType<typeof storage>;
let floors: typeof import('./floors');
let locked: string[];

beforeEach(async () => {
  store = storage();
  locked = [];
  vi.stubGlobal('localStorage', store);
  // Runs each locked section at once, recording the lock it held.
  vi.stubGlobal('navigator', {
    locks: {
      request: (name: string, run: () => void) => {
        locked.push(name);
        run();
        return Promise.resolve();
      },
    },
  });
  vi.resetModules();
  floors = await import('./floors');
});

describe('floors', () => {
  it('only ever grows, and writes under a per-principal lock', () => {
    floors.raiseFloor('alice', 6);
    floors.raiseFloor('alice', 5);
    expect(floors.floorOf('alice')).toBe(6);
    expect(store.data.get('coo:revision:alice')).toBe('6');
    expect(locked.every((name) => name === 'coo:revision:alice')).toBe(true);
    expect(floors.floorOf('bob')).toBe(0);
  });

  it('holds in memory when storage fails', () => {
    store.failing = true;
    floors.raiseFloor('alice', 6);
    expect(floors.floorOf('alice')).toBe(6);
  });

  it('never writes a lower value over a higher one stored by another tab', () => {
    floors.raiseFloor('alice', 4);
    store.data.set('coo:revision:alice', '6');
    floors.raiseFloor('alice', 5);
    expect(store.data.get('coo:revision:alice')).toBe('6');
    expect(floors.floorOf('alice')).toBe(6);
  });

  it('does not write when the stored value cannot be read', () => {
    store.data.set('coo:revision:alice', '6');
    store.failingReads = 1;
    floors.raiseFloor('alice', 5);
    expect(store.data.get('coo:revision:alice')).toBe('6');
    store.data.set('coo:revision:alice', 'garbage');
    floors.raiseFloor('alice', 5);
    expect(store.data.get('coo:revision:alice')).toBe('garbage');
  });

  it('puts its floor back if the stored value is lower', () => {
    floors.raiseFloor('alice', 6);
    store.data.set('coo:revision:alice', '5');
    floors.onStorage('coo:revision:alice', '5');
    expect(store.data.get('coo:revision:alice')).toBe('6');
  });

  it('adopts a higher value from another tab', () => {
    floors.raiseFloor('alice', 3);
    store.data.set('coo:revision:alice', '9');
    floors.onStorage('coo:revision:alice', '9');
    store.data.delete('coo:revision:alice');
    expect(floors.floorOf('alice')).toBe(9);
    floors.onStorage('unrelated', '100');
    expect(floors.floorOf('alice')).toBe(9);
  });

  it('serializes writers through the lock, so a slower tab cannot lower the stored value', async () => {
    const queue: (() => void)[] = [];
    vi.stubGlobal('navigator', {
      locks: {
        request: (_name: string, run: () => void) => {
          queue.push(run);
          return Promise.resolve();
        },
      },
    });
    vi.resetModules();
    floors = await import('./floors');
    floors.raiseFloor('alice', 5); // waits for the lock
    queue.unshift(() => store.data.set('coo:revision:alice', '6')); // another tab held it first and stored 6
    for (const run of queue) run();
    expect(store.data.get('coo:revision:alice')).toBe('6');
  });

  it('keeps the floor in memory only when Web Locks are unavailable', async () => {
    vi.stubGlobal('navigator', {});
    vi.resetModules();
    floors = await import('./floors');
    floors.raiseFloor('alice', 6);
    expect(store.data.has('coo:revision:alice')).toBe(false);
    expect(floors.floorOf('alice')).toBe(6);
  });
});
