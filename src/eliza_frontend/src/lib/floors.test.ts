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

const ALICE = 'canister:alice';
const KEY = 'coo:floor:canister:alice';

describe('floors', () => {
  it('grows, and writes under a per-owner lock', () => {
    floors.raiseFloor(ALICE, 6);
    floors.raiseFloor(ALICE, 5);
    expect(floors.floorOf(ALICE)).toBe(6);
    expect(store.data.get(KEY)).toBe('0:6');
    expect(locked.every((name) => name === KEY)).toBe(true);
    expect(floors.floorOf('canister:bob')).toBe(0);
    // The same principal in another canister has its own floor.
    expect(floors.floorOwner('canister', 'alice')).toBe(ALICE);
    expect(floors.floorOf(floors.floorOwner('other', 'alice'))).toBe(0);
  });

  it('holds in memory when storage fails', () => {
    store.failing = true;
    floors.raiseFloor(ALICE, 6);
    expect(floors.floorOf(ALICE)).toBe(6);
  });

  it('never writes a lower value over a higher one stored by another tab', () => {
    floors.raiseFloor(ALICE, 4);
    store.data.set(KEY, '0:6');
    floors.raiseFloor(ALICE, 5);
    expect(store.data.get(KEY)).toBe('0:6');
    expect(floors.floorOf(ALICE)).toBe(6);
  });

  it('does not write when the stored value cannot be read', () => {
    store.data.set(KEY, '0:6');
    store.failingReads = 2; // the read that works out the floor, and the one before writing
    floors.raiseFloor(ALICE, 5);
    expect(store.data.get(KEY)).toBe('0:6');
    store.data.set(KEY, 'garbage');
    floors.raiseFloor(ALICE, 7);
    expect(store.data.get(KEY)).toBe('garbage');
    expect(floors.floorOf(ALICE)).toBe(7);
  });

  it('puts its floor back if the stored value is lower', () => {
    floors.raiseFloor(ALICE, 6);
    store.data.set(KEY, '0:5');
    floors.onStorage(KEY, '0:5');
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it('adopts a higher value from another tab', () => {
    floors.raiseFloor(ALICE, 3);
    store.data.set(KEY, '0:9');
    floors.onStorage(KEY, '0:9');
    store.data.delete(KEY);
    expect(floors.floorOf(ALICE)).toBe(9);
    floors.onStorage('unrelated', '0:100');
    floors.onStorage('coo:revision:alice', '100');
    expect(floors.floorOf(ALICE)).toBe(9);
  });

  it('is lowered only by a reset, which every tab then follows', () => {
    floors.raiseFloor(ALICE, 10);
    floors.resetFloor(ALICE, 3);
    expect(floors.floorOf(ALICE)).toBe(3);
    expect(store.data.get(KEY)).toBe('1:3');
    // From there it grows again, in the new epoch.
    floors.raiseFloor(ALICE, 2);
    expect(floors.floorOf(ALICE)).toBe(3);
    floors.raiseFloor(ALICE, 4);
    expect(store.data.get(KEY)).toBe('1:4');
  });

  it('adopts another tab\'s reset instead of writing its own higher floor back', () => {
    // This tab remembers revision 10; another tab learnt from the canister that it is at 3.
    floors.raiseFloor(ALICE, 10);
    store.data.set(KEY, '1:3');
    floors.onStorage(KEY, '1:3');
    expect(store.data.get(KEY)).toBe('1:3');
    expect(floors.floorOf(ALICE)).toBe(3);
    // Even without the storage event, the stored epoch wins the next time the floor is read or raised.
    floors.resetFloor('canister:bob', 0);
    store.data.set('coo:floor:canister:carol', '2:1');
    floors.raiseFloor('canister:carol', 5);
    expect(store.data.get('coo:floor:canister:carol')).toBe('2:5');
  });

  it('takes over the floor an earlier version stored, once', () => {
    store.data.set('coo:revision:alice', '7');
    floors.adoptLegacyFloor(ALICE, 'alice');
    expect(floors.floorOf(ALICE)).toBe(7);
    // Not after a reset: the old value is from before it.
    floors.resetFloor(ALICE, 2);
    floors.adoptLegacyFloor(ALICE, 'alice');
    expect(floors.floorOf(ALICE)).toBe(2);
    // Nothing, or nonsense, stored: no floor.
    store.data.set('coo:revision:bob', 'garbage');
    floors.adoptLegacyFloor('canister:bob', 'bob');
    floors.adoptLegacyFloor('canister:carol', 'carol');
    expect(floors.floorOf('canister:bob')).toBe(0);
    expect(floors.floorOf('canister:carol')).toBe(0);
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
    floors.raiseFloor(ALICE, 5); // waits for the lock
    queue.unshift(() => store.data.set(KEY, '0:6')); // another tab held it first and stored 6
    for (const run of queue) run();
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it('keeps the floor in memory only when Web Locks are unavailable', async () => {
    vi.stubGlobal('navigator', {});
    vi.resetModules();
    floors = await import('./floors');
    floors.raiseFloor(ALICE, 6);
    expect(store.data.has(KEY)).toBe(false);
    expect(floors.floorOf(ALICE)).toBe(6);
  });
});
