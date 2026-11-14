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
    floors.raiseFloorByUpdate(ALICE, 6);
    floors.raiseFloorByUpdate(ALICE, 5);
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
    floors.raiseFloorByUpdate(ALICE, 6);
    expect(floors.floorOf(ALICE)).toBe(6);
  });

  it('never writes a lower value over a higher one stored by another tab', () => {
    floors.raiseFloorByUpdate(ALICE, 4);
    store.data.set(KEY, '0:6');
    floors.raiseFloorByUpdate(ALICE, 5);
    expect(store.data.get(KEY)).toBe('0:6');
    expect(floors.floorOf(ALICE)).toBe(6);
  });

  it('does not write when the stored value cannot be read', () => {
    store.data.set(KEY, '0:6');
    store.failingReads = 2; // the read that works out the floor, and the one before writing
    floors.raiseFloorByUpdate(ALICE, 5);
    expect(store.data.get(KEY)).toBe('0:6');
    store.data.set(KEY, 'garbage');
    floors.raiseFloorByUpdate(ALICE, 7);
    expect(store.data.get(KEY)).toBe('garbage');
    expect(floors.floorOf(ALICE)).toBe(7);
  });

  it('puts its floor back if the stored value is lower', () => {
    floors.raiseFloorByUpdate(ALICE, 6);
    store.data.set(KEY, '0:5');
    floors.onStorage(KEY, '0:5');
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it('adopts a higher value from another tab', () => {
    floors.raiseFloorByUpdate(ALICE, 3);
    store.data.set(KEY, '0:9');
    floors.onStorage(KEY, '0:9');
    store.data.delete(KEY);
    expect(floors.floorOf(ALICE)).toBe(9);
    floors.onStorage('unrelated', '0:100');
    floors.onStorage('coo:revision:alice', '100');
    expect(floors.floorOf(ALICE)).toBe(9);
  });

  it('is lowered only by a reset, which every tab then follows', async () => {
    floors.raiseFloorByUpdate(ALICE, 10);
    expect(await floors.resetFloor(ALICE, 3, floors.readFloor(ALICE))).toBe(true);
    expect(floors.floorOf(ALICE)).toBe(3);
    expect(store.data.get(KEY)).toBe('1:3');
    // From there it grows again, in the new epoch.
    floors.raiseFloorByUpdate(ALICE, 2);
    expect(floors.floorOf(ALICE)).toBe(3);
    floors.raiseFloorByUpdate(ALICE, 4);
    expect(store.data.get(KEY)).toBe('1:4');
  });

  it('adopts another tab\'s reset instead of writing its own higher floor back', async () => {
    // This tab remembers revision 10; another tab learnt from the canister that it is at 3.
    floors.raiseFloorByUpdate(ALICE, 10);
    store.data.set(KEY, '1:3');
    floors.onStorage(KEY, '1:3');
    expect(store.data.get(KEY)).toBe('1:3');
    expect(floors.floorOf(ALICE)).toBe(3);
    // Even without the storage event, the stored epoch wins the next time the floor is read or raised.
    await floors.resetFloor('canister:bob', 0, floors.readFloor('canister:bob'));
    store.data.set('coo:floor:canister:carol', '2:1');
    floors.raiseFloorByUpdate('canister:carol', 5);
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
    floors.raiseFloorByUpdate(ALICE, 5); // waits for the lock
    queue.unshift(() => store.data.set(KEY, '0:6')); // another tab held it first and stored 6
    for (const run of queue) run();
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it('keeps the floor in memory only when Web Locks are unavailable', async () => {
    vi.stubGlobal('navigator', {});
    vi.resetModules();
    floors = await import('./floors');
    floors.raiseFloorByUpdate(ALICE, 6);
    expect(store.data.has(KEY)).toBe(false);
    expect(floors.floorOf(ALICE)).toBe(6);
  });
});

// A late answer must not lower a floor that moved while it was on its way (review finding 1).
describe('a reset is compare-and-set', () => {
  it('refuses a late certified read when this tab confirmed something newer meanwhile', async () => {
    // Floor 0:5. A certified read starts, and reads revision 4 (an answer from before the last exchange).
    floors.raiseFloorByUpdate(ALICE, 5);
    const before = floors.readFloor(ALICE);
    // While it is on its way, this tab's own clear is confirmed at 6.
    floors.raiseFloorByUpdate(ALICE, 6);
    expect(await floors.resetFloor(ALICE, 4, before)).toBe(false);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 0, revision: 6 });
    expect(store.data.get(KEY)).toBe('0:6');
  });

  it("refuses it when another tab's clear arrived through the storage event (the reviewer's case)", async () => {
    // Tab A: floor 0:5; its queries said 4 three times, so it asks through consensus, which reads 5 but answers late.
    floors.raiseFloorByUpdate(ALICE, 5);
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
    floors.raiseFloorByUpdate(ALICE, 5);
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
    floors.raiseFloorByUpdate(ALICE, 9);
    const before = floors.readFloor(ALICE);
    // Another tab reset first, from the same 0:9.
    store.data.set(KEY, '1:3');
    expect(await floors.resetFloor(ALICE, 2, before)).toBe(false);
    expect(store.data.get(KEY)).toBe('1:3');
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 3 });
  });

  it('resets when nothing moved, and with storage unreadable compares with memory only', async () => {
    floors.raiseFloorByUpdate(ALICE, 9);
    expect(await floors.resetFloor(ALICE, 2, floors.readFloor(ALICE))).toBe(true);
    expect(store.data.get(KEY)).toBe('1:2');
    // Storage that cannot be read: nothing is stored, the reset holds in memory.
    floors.raiseFloorByUpdate('canister:bob', 9);
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
    floors.raiseFloorByUpdate(ALICE, 5);
    const before = floors.readFloor(ALICE);
    floors.raiseFloorByUpdate(ALICE, 6);
    expect(await floors.resetFloor(ALICE, 4, before)).toBe(false);
    expect(await floors.resetFloor(ALICE, 4, floors.readFloor(ALICE))).toBe(true);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 4 });
  });
});

describe("a query's answer counts only in the epoch it started in", () => {
  it('ignores an answer from before a reset in this tab', async () => {
    floors.raiseFloorByQuery(ALICE, 10, 0);
    // A status query starts in epoch 0 …
    const queryStarted = floors.readFloor(ALICE).epoch;
    // … then the canister turns out to be at 3 (restored), and the floor is reset.
    expect(await floors.resetFloor(ALICE, 3, floors.readFloor(ALICE))).toBe(true);
    // The query's late answer (revision 11, from a replica still on the old state) must not raise the new floor.
    expect(floors.raiseFloorByQuery(ALICE, 11, queryStarted)).toBe(false);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 3 });
    expect(store.data.get(KEY)).toBe('1:3');
    // An answer to an exchange that started after the reset counts.
    expect(floors.raiseFloorByQuery(ALICE, 4, 1)).toBe(true);
    expect(store.data.get(KEY)).toBe('1:4');
  });

  it("ignores it after another tab's reset, with or without the storage event", () => {
    floors.raiseFloorByQuery(ALICE, 10, 0);
    store.data.set(KEY, '1:3');
    expect(floors.raiseFloorByQuery(ALICE, 12, 0)).toBe(false);
    expect(store.data.get(KEY)).toBe('1:3');
    floors.onStorage(KEY, '1:3');
    expect(floors.raiseFloorByQuery(ALICE, 12, 0)).toBe(false);
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
    floors.raiseFloorByQuery(ALICE, 12, 0); // waits for the lock
    store.data.set(KEY, '1:3'); // another tab's reset got the lock first
    for (const run of queue) run();
    expect(store.data.get(KEY)).toBe('1:3');
  });
});

// Review round 2, I2: an update call's answer is the canister's state when it answered, whatever epoch it started in.
describe("an update call's answer raises the floor of now", () => {
  it('counts in the current epoch, also when the call started before a reset', async () => {
    floors.raiseFloorByUpdate(ALICE, 10);
    expect(await floors.resetFloor(ALICE, 3, floors.readFloor(ALICE))).toBe(true);
    floors.raiseFloorByUpdate(ALICE, 4);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 4 });
    expect(store.data.get(KEY)).toBe('1:4');
    floors.raiseFloorByUpdate(ALICE, 2);
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 4 });
  });

  it("the reviewer's case: a clear started before the reset to 1:3 answers 4, so a query's 3 is stale", async () => {
    // Before the rollback this device had seen 10. A clear starts (snapshot 0:10) …
    floors.raiseFloorByUpdate(ALICE, 10);
    const beforeClear = floors.readFloor(ALICE);
    const queryStarted = floors.readFloor(ALICE).epoch;
    // … meanwhile a load (after signing in again) finds the canister restored to 3 and resets the floor …
    expect(await floors.resetFloor(ALICE, 3, floors.readFloor(ALICE))).toBe(true);
    // … and then the clear answers: it ran on the restored state, which is at 4 now.
    expect(await floors.settleClear(ALICE, 4, beforeClear)).toEqual({ epoch: 1, revision: 4 });
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 4 });
    // A query that answers 3 (from before the clear) is below the floor, so a load does not apply it, and it does not
    // raise anything either.
    expect(floors.floorOf(ALICE)).toBeGreaterThan(3);
    expect(floors.raiseFloorByQuery(ALICE, 3, queryStarted)).toBe(false);
    expect(store.data.get(KEY)).toBe('1:4');
  });

  it('a clear: reset if nothing moved, else raise if at or above the floor of now, else leave it', async () => {
    // Nothing moved, and the canister is behind: a reset.
    floors.raiseFloorByUpdate(ALICE, 9);
    expect(await floors.settleClear(ALICE, 2, floors.readFloor(ALICE))).toEqual({ epoch: 1, revision: 2 });
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 2 });
    // At or above the floor: a raise.
    expect(await floors.settleClear(ALICE, 5, floors.readFloor(ALICE))).toEqual({ epoch: 1, revision: 5 });
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 5 });
    // Below the floor, which moved during the call to 7 (another tab's reply): left alone, and reported.
    const before = floors.readFloor(ALICE);
    store.data.set(KEY, '1:7');
    expect(await floors.settleClear(ALICE, 6, before)).toBeNull();
    expect(floors.readFloor(ALICE)).toEqual({ epoch: 1, revision: 7 });
  });
});

// Review round 3, Important ③: between the clear's answer and showing its result, another tab can confirm more.
describe("a clear's result is shown only while the floor is where the clear left it", () => {
  it('not after another tab confirmed a newer revision (the reviewer\'s case, no snapshot involved)', async () => {
    floors.raiseFloorByUpdate(ALICE, 1);
    const before = floors.readFloor(ALICE);
    // The clear answers 2: raised to 0:2 …
    const left = await floors.settleClear(ALICE, 2, before);
    expect(left).toEqual({ epoch: 0, revision: 2 });
    expect(floors.floorIs(ALICE, left!)).toBe(true);
    // … and before this tab shows "empty at 2", another tab's reply is confirmed at 3.
    store.data.set(KEY, '0:3');
    expect(floors.floorIs(ALICE, left!)).toBe(false);
  });

  it('not after a reset that happened meanwhile either', async () => {
    floors.raiseFloorByUpdate(ALICE, 9);
    const left = await floors.settleClear(ALICE, 2, floors.readFloor(ALICE)); // a reset to 1:2
    expect(floors.floorIs(ALICE, left!)).toBe(true);
    store.data.set(KEY, '2:2'); // another tab reset again, to the same revision
    expect(floors.floorIs(ALICE, left!)).toBe(false);
  });
});

// Review round 2, I1: the last check before a conversation is shown compares epoch AND revision with the floor of now.
describe('the last check before a conversation is shown', () => {
  it("refuses an answer overtaken while its reset waited (the reviewer's case)", async () => {
    floors.raiseFloorByUpdate(ALICE, 5);
    const start = floors.readFloor(ALICE);
    // The certified read says 3: the canister went back, and the reset goes through (1:3) …
    expect(await floors.resetFloor(ALICE, 3, start)).toBe(true);
    // … but before the load gets on with it, another tab confirms a clear: 1:4.
    store.data.set(KEY, '1:4');
    expect(floors.isCurrent(ALICE, 3, 1)).toBe(false);
    expect(floors.isCurrent(ALICE, 4, 1)).toBe(true);
    // An answer judged against another epoch is never current.
    expect(floors.isCurrent(ALICE, 9, 0)).toBe(false);
  });
});
