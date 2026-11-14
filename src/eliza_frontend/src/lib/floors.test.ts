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
  // Runs each locked section at once, recording the lock it held. Like the real API, the request resolves with what
  // the section returns.
  vi.stubGlobal('navigator', {
    locks: {
      request: (name: string, run: () => unknown) => {
        locked.push(name);
        return Promise.resolve(run());
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
    floors.raiseFloor(ALICE, 6, 0);
    floors.raiseFloor(ALICE, 5, 0);
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
    floors.raiseFloor(ALICE, 6, 0);
    expect(floors.floorOf(ALICE)).toBe(6);
  });

  it('never writes a lower value over a higher one stored by another tab', () => {
    floors.raiseFloor(ALICE, 4, 0);
    store.data.set(KEY, '0:6');
    floors.raiseFloor(ALICE, 5, 0);
    expect(store.data.get(KEY)).toBe('0:6');
    expect(floors.floorOf(ALICE)).toBe(6);
  });

  it('does not write when the stored value cannot be read', () => {
    store.data.set(KEY, '0:6');
    store.failingReads = 2; // the read that works out the floor, and the one before writing
    floors.raiseFloor(ALICE, 5, 0);
    expect(store.data.get(KEY)).toBe('0:6');
    store.data.set(KEY, 'garbage');
    floors.raiseFloor(ALICE, 7, 0);
    expect(store.data.get(KEY)).toBe('garbage');
    expect(floors.floorOf(ALICE)).toBe(7);
  });

  it('puts its floor back if the stored value is lower', () => {
    floors.raiseFloor(ALICE, 6, 0);
    store.data.set(KEY, '0:5');
    floors.onStorage(KEY, '0:5');
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it('adopts a higher value from another tab', () => {
    floors.raiseFloor(ALICE, 3, 0);
    store.data.set(KEY, '0:9');
    floors.onStorage(KEY, '0:9');
    store.data.delete(KEY);
    expect(floors.floorOf(ALICE)).toBe(9);
    floors.onStorage('unrelated', '0:100');
    floors.onStorage('coo:revision:alice', '100');
    expect(floors.floorOf(ALICE)).toBe(9);
  });

  it('is lowered only by a reset, which every tab then follows', async () => {
    floors.raiseFloor(ALICE, 10, 0);
    expect(await floors.resetFloor(ALICE, 3, floors.readFloor(ALICE))).toBe(true);
    expect(floors.floorOf(ALICE)).toBe(3);
    expect(store.data.get(KEY)).toBe('1:3');
    // From there it grows again, in the new epoch.
    floors.raiseFloor(ALICE, 2, 1);
    expect(floors.floorOf(ALICE)).toBe(3);
    floors.raiseFloor(ALICE, 4, 1);
    expect(store.data.get(KEY)).toBe('1:4');
  });

  it('adopts another tab\'s reset instead of writing its own higher floor back', async () => {
    // This tab remembers revision 10; another tab learnt from the canister that it is at 3.
    floors.raiseFloor(ALICE, 10, 0);
    store.data.set(KEY, '1:3');
    floors.onStorage(KEY, '1:3');
    expect(store.data.get(KEY)).toBe('1:3');
    expect(floors.floorOf(ALICE)).toBe(3);
    // Even without the storage event, the stored epoch wins the next time the floor is read or raised.
    await floors.resetFloor('canister:bob', 0, floors.readFloor('canister:bob'));
    store.data.set('coo:floor:canister:carol', '2:1');
    floors.raiseFloor('canister:carol', 5, 2);
    expect(store.data.get('coo:floor:canister:carol')).toBe('2:5');
  });

  it('takes over the floor an earlier version stored, once', async () => {
    store.data.set('coo:revision:alice', '7');
    floors.adoptLegacyFloor(ALICE, 'alice');
    expect(floors.floorOf(ALICE)).toBe(7);
    // Not after a reset: the old value is from before it.
    await floors.resetFloor(ALICE, 2, floors.readFloor(ALICE));
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
    floors.raiseFloor(ALICE, 5, 0); // waits for the lock
    queue.unshift(() => store.data.set(KEY, '0:6')); // another tab held it first and stored 6
    for (const run of queue) run();
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it('keeps the floor in memory only when Web Locks are unavailable', async () => {
    vi.stubGlobal('navigator', {});
    vi.resetModules();
    floors = await import('./floors');
    floors.raiseFloor(ALICE, 6, 0);
    expect(store.data.has(KEY)).toBe(false);
    expect(floors.floorOf(ALICE)).toBe(6);
  });
});

// A late answer must not lower a floor that moved while it was on its way (review finding 1).
describe('a reset is compare-and-set', () => {
  it('refuses a late certified read when this tab confirmed something newer meanwhile', async () => {
    // Floor 0:5. A certified read starts, and reads revision 4 (an answer from before the last exchange).
    floors.raiseFloor(ALICE, 5, 0);
    const before = floors.readFloor(ALICE);
    // While it is on its way, this tab's own clear is confirmed at 6.
    floors.raiseFloor(ALICE, 6, before.epoch);
    expect(await floors.resetFloor(ALICE, 4, before)).toBe(false);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 0, revision: 6 });
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it("refuses it when another tab's clear arrived through the storage event (the reviewer's case)", async () => {
    // Tab A: floor 0:5; its queries said 4 three times, so it asks through consensus, which reads 5 but answers late.
    floors.raiseFloor(ALICE, 5, 0);
    const before = floors.readFloor(ALICE);
    // Tab B clears: revision 6, stored, and A hears of it.
    store.data.set(KEY, '0:6');
    floors.onStorage(KEY, '0:6');
    // A's answer arrives: 5 is below the floor, but the floor is no longer what A started from.
    expect(await floors.resetFloor(ALICE, 5, before)).toBe(false);
    expect(floors.floorOf(ALICE)).toBe(6);
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it('refuses it when another tab stored something newer and the event has not arrived yet', async () => {
    floors.raiseFloor(ALICE, 5, 0);
    const before = floors.readFloor(ALICE);
    store.data.set(KEY, '0:6'); // no storage event (yet)
    expect(await floors.resetFloor(ALICE, 3, before)).toBe(false);
    expect(store.data.get(KEY)).toBe('0:6');
    expect(floors.floorOf(ALICE)).toBe(6);
  });

  it('compares with what is stored inside the lock, not only before waiting for it', async () => {
    const queue: (() => unknown)[] = [];
    vi.stubGlobal('navigator', {
      locks: {
        request: (_name: string, run: () => unknown) => {
          let resolve!: (value: unknown) => void;
          const done = new Promise((r) => (resolve = r));
          queue.push(() => resolve(run()));
          return done;
        },
      },
    });
    vi.resetModules();
    floors = await import('./floors');
    store.data.set(KEY, '0:5');
    const before = floors.readFloor(ALICE);
    const reset = floors.resetFloor(ALICE, 3, before); // passes the check in memory, then waits for the lock
    // Another tab holds the lock first and stores 0:6.
    store.data.set(KEY, '0:6');
    for (const run of queue) run();
    expect(await reset).toBe(false);
    expect(store.data.get(KEY)).toBe('0:6');
    expect(floors.floorOf(ALICE)).toBe(6);
  });

  it("does not reset twice from the same snapshot: the second tab finds the first one's reset", async () => {
    floors.raiseFloor(ALICE, 9, 0);
    const before = floors.readFloor(ALICE);
    // Another tab reset first, from the same 0:9.
    store.data.set(KEY, '1:3');
    expect(await floors.resetFloor(ALICE, 2, before)).toBe(false);
    expect(store.data.get(KEY)).toBe('1:3');
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 3 });
  });

  it('resets when nothing moved, and with storage unreadable compares with memory only', async () => {
    floors.raiseFloor(ALICE, 9, 0);
    expect(await floors.resetFloor(ALICE, 2, floors.readFloor(ALICE))).toBe(true);
    expect(store.data.get(KEY)).toBe('1:2');
    // Storage that cannot be read: nothing is stored, the reset holds in memory.
    floors.raiseFloor('canister:bob', 9, 0);
    const before = floors.readFloor('canister:bob');
    store.failingReads = 3;
    expect(await floors.resetFloor('canister:bob', 1, before)).toBe(true);
    expect(store.data.get('coo:floor:canister:bob')).toBe('0:9');
    store.failingReads = 0;
    expect(floors.readFloor('canister:bob')).toEqual({ epoch: 1, revision: 1 });
  });

  it('without Web Locks, compares with memory', async () => {
    vi.stubGlobal('navigator', {});
    vi.resetModules();
    floors = await import('./floors');
    floors.raiseFloor(ALICE, 5, 0);
    const before = floors.readFloor(ALICE);
    floors.raiseFloor(ALICE, 6, 0);
    expect(await floors.resetFloor(ALICE, 4, before)).toBe(false);
    expect(await floors.resetFloor(ALICE, 4, floors.readFloor(ALICE))).toBe(true);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 4 });
  });
});

describe('a raise counts only in the epoch it started in', () => {
  it('ignores a confirmation from before a reset in this tab', async () => {
    floors.raiseFloor(ALICE, 10, 0);
    // A send starts in epoch 0 …
    const sendStarted = floors.readFloor(ALICE).epoch;
    // … then the canister turns out to be at 3 (restored), and the floor is reset.
    expect(await floors.resetFloor(ALICE, 3, floors.readFloor(ALICE))).toBe(true);
    // The send's late answer (revision 11 of the old state) must not raise the new epoch's floor.
    expect(floors.raiseFloor(ALICE, 11, sendStarted)).toBe(false);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 3 });
    expect(store.data.get(KEY)).toBe('1:3');
    // An answer to an exchange that started after the reset counts.
    expect(floors.raiseFloor(ALICE, 4, 1)).toBe(true);
    expect(store.data.get(KEY)).toBe('1:4');
  });

  it("ignores it after another tab's reset, with or without the storage event", () => {
    floors.raiseFloor(ALICE, 10, 0);
    store.data.set(KEY, '1:3');
    expect(floors.raiseFloor(ALICE, 12, 0)).toBe(false);
    expect(store.data.get(KEY)).toBe('1:3');
    floors.onStorage(KEY, '1:3');
    expect(floors.raiseFloor(ALICE, 12, 0)).toBe(false);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 3 });
  });

  it('does not store an older epoch over a reset another tab stored while it waited for the lock', async () => {
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
    floors.raiseFloor(ALICE, 12, 0); // waits for the lock
    store.data.set(KEY, '1:3'); // another tab's reset got the lock first
    for (const run of queue) run();
    expect(store.data.get(KEY)).toBe('1:3');
  });
});
