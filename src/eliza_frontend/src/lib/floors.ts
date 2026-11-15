// The newest conversation revision the canister has confirmed for each principal. History older than this comes from
// a replica that is behind (for example, from before a clear) and must not be shown.
//
// A floor only ever grows, with one exception: when the canister itself, asked through consensus, reports a lower
// revision (its state was restored from a snapshot, or it was reinstalled), the floor is reset to that. A reset
// starts a new epoch, and a higher epoch beats any revision of a lower one, so a tab that still remembers the old,
// higher floor adopts the reset instead of writing its value back.
//
// Answers arrive late, and what they may do depends on what kind of answer they are:
// - An UPDATE call's answer (a send's reply, a clear's revision, a read through consensus) is the canister's own
//   state when it answered: it raises the floor of the current epoch (raiseFloorByUpdate), whatever happened since.
// - A QUERY's answer comes from one replica and can be stale: it only counts in the epoch the query started in
//   (raiseFloorByQuery), so an answer from before a reset is never carried into the new epoch.
// - A reset (lowering the floor) is compare-and-set: the caller takes a snapshot of the floor before it asks, and
//   the reset happens only if the floor is still exactly that, in memory and (inside the Web Lock) in storage.
//   Otherwise something was confirmed while the answer was on its way, and nothing is lowered.
//
// A clear raises the floor before it is sent (raiseBeforeClear): to one past the conversation's revision as this tab
// knows it, marked as only expected — the canister has not confirmed it. Whether its answer arrives or not, an answer at that revision or below is then refused — in this tab or
// another, after a reload, in a new session — and a load asks through consensus instead. If the clear did not run,
// that read finds the canister behind and resets the floor, as for any floor that is ahead. This is a lower bound,
// not the clear's own revision: when the tab that sent the clear never gets its answer, a conversation from just
// before the clear can still pass (README, "Known gaps").
//
// Floors are kept in memory for the page's lifetime, so a failing storage or a sign-out cannot lose them, and in
// localStorage across reloads and tabs. Only writers holding a cross-tab Web Lock store them (read, then write the
// newer value), so tabs can never put an older one back; without Web Locks they stay in memory.

const PREFIX = 'coo:floor:';
/** The key used before floors were per canister and had epochs: a plain revision per principal. */
const LEGACY_PREFIX = 'coo:revision:';

export interface Floor {
  epoch: number;
  revision: number;
  /**
   * Only expected, not confirmed: raised before a clear was sent (raiseBeforeClear). Any confirmation of the canister
   * — at this revision too — makes the floor confirmed, which counts as a change: a compare-and-set planned against
   * the expected floor is refused once it has been confirmed.
   */
  expected?: true;
}

const ZERO: Floor = { epoch: 0, revision: 0 };
const memory = new Map<string, Floor>();

// Newer: a higher epoch, then a higher revision, then — at the same epoch and revision — confirmed over expected.
const newer = (a: Floor, b: Floor) =>
  a.epoch > b.epoch ||
  (a.epoch === b.epoch && (a.revision > b.revision || (a.revision === b.revision && !a.expected && !!b.expected)));
const latest = (a: Floor, b: Floor) => (newer(b, a) ? b : a);
const same = (a: Floor, b: Floor) => a.epoch === b.epoch && a.revision === b.revision && !a.expected === !b.expected;
// "<epoch>:<revision>" is confirmed (the form floors were always stored in); "<epoch>:<revision>:x" is expected.
const encode = (floor: Floor) => `${floor.epoch}:${floor.revision}${floor.expected ? ':x' : ''}`;

/** Null when the text is not a floor. */
function decode(raw: string): Floor | null {
  const match = /^(\d{1,15}):(\d{1,15})(:x)?$/.exec(raw);
  if (!match) return null;
  const floor: Floor = { epoch: Number(match[1]), revision: Number(match[2]) };
  if (match[3]) floor.expected = true;
  return floor;
}

/** Whose floor: a principal's conversation in one canister. */
export function floorOwner(canisterId: string, principal: string): string {
  return `${canisterId}:${principal}`;
}

function stored(owner: string): Floor {
  try {
    const raw = localStorage.getItem(PREFIX + owner);
    return (raw && decode(raw)) || ZERO;
  } catch {
    return ZERO;
  }
}

/**
 * What is stored, for a writer: null when it cannot be read or is not a floor, which must not be mistaken for
 * "nothing stored yet".
 */
function storedForWrite(owner: string): Floor | null {
  try {
    const raw = localStorage.getItem(PREFIX + owner);
    return raw === null ? ZERO : decode(raw);
  } catch {
    return null;
  }
}

function write(owner: string, floor: Floor) {
  try {
    const current = storedForWrite(owner);
    if (current && newer(floor, current)) localStorage.setItem(PREFIX + owner, encode(floor));
  } catch {
    // Storage unavailable or full: the in-memory floor still holds.
  }
}

const webLocks = () => (typeof navigator === 'undefined' ? undefined : navigator.locks);

function persist(owner: string, floor: Floor) {
  webLocks()
    ?.request(PREFIX + owner, () => write(owner, floor))
    .catch(() => {});
}

function current(owner: string): Floor {
  return latest(memory.get(owner) ?? ZERO, stored(owner));
}

/** The floor as it is now: take it before asking the canister, and pass it back with the answer. */
export function readFloor(owner: string): Floor {
  const floor = current(owner);
  memory.set(owner, floor);
  if (newer(floor, stored(owner))) persist(owner, floor);
  return floor;
}

export function floorOf(owner: string): number {
  return readFloor(owner).revision;
}

/**
 * Whether a conversation at `revision`, judged against a floor of `epoch`, may be shown now: the floor as it is at
 * this moment is in that epoch and not above it. The last check before anything is shown — an answer can be
 * overtaken while it is on its way, or while a reset waits for its lock.
 */
export function isCurrent(owner: string, revision: number, epoch: number): boolean {
  const now = readFloor(owner);
  return now.epoch === epoch && revision >= now.revision;
}

/**
 * The canister confirmed `revision`: the floor rises to it, confirmed. A confirmation at the floor's own revision
 * turns an expected floor into a confirmed one; one below it changes nothing (a higher expected floor stays expected).
 */
function raise(owner: string, base: Floor, revision: number) {
  if (revision < base.revision || (revision === base.revision && !base.expected)) return;
  const floor = { epoch: base.epoch, revision };
  memory.set(owner, floor);
  // write() only stores it over an older value, so a reset another tab stored meanwhile wins.
  persist(owner, floor);
}

/** An update call answered with `revision` (the canister's state when it answered): the floor of now rises to it. */
export function raiseFloorByUpdate(owner: string, revision: number) {
  raise(owner, current(owner), revision);
}

/**
 * A query, started when the floor was in `epoch`, answered with `revision`. Ignored when a reset has happened since
 * (the answer may be from before it). Returns whether it counted.
 */
export function raiseFloorByQuery(owner: string, revision: number, epoch: number): boolean {
  const base = current(owner);
  if (base.epoch !== epoch) return false;
  raise(owner, base, revision);
  return true;
}

/**
 * The canister, asked through consensus, is at `revision`, below the floor: start a new epoch there — but only if
 * the floor is still `expected`, the snapshot taken before the canister was asked. Resolves false, and changes
 * nothing, when the floor moved in the meantime: the answer is older than what has been confirmed since, so it is
 * stale rather than a sign that the canister went back. Only for an answer that cannot be stale in itself (an update
 * call) — a query's answer must never lower a floor.
 */
export async function resetFloor(owner: string, revision: number, expected: Floor): Promise<boolean> {
  const floor = { epoch: expected.epoch + 1, revision };
  // Synchronous from the check to the change, so nothing in this tab can come in between.
  const apply = (storedNow: Floor | null): boolean => {
    const now = storedNow ? latest(memory.get(owner) ?? ZERO, storedNow) : (memory.get(owner) ?? ZERO);
    if (!same(now, expected)) return false;
    memory.set(owner, floor);
    if (storedNow) {
      try {
        localStorage.setItem(PREFIX + owner, encode(floor));
      } catch {
        // Storage full: the in-memory floor still holds.
      }
    }
    return true;
  };
  if (!same(current(owner), expected)) return false;
  const locks = webLocks();
  // Without Web Locks floors are not stored (see above), so memory is all there is to compare with.
  if (!locks) return apply(null);
  try {
    // Inside the lock, so no other tab can store a floor between the comparison and the write. If storage cannot be
    // read, only this tab's memory is compared and nothing is stored.
    return await locks.request(PREFIX + owner, () => apply(storedForWrite(owner)));
  } catch {
    return false;
  }
}

/** Whether the floor is still exactly `floor` (nothing was confirmed, and nothing reset, since it was left there). */
export function floorIs(owner: string, floor: Floor): boolean {
  return same(readFloor(owner), floor);
}

/**
 * A clear answered with `cleared`; `before` is the floor taken before the call. Below the floor, it is a reset if
 * nothing moved meanwhile. If something did, a revision at or above the floor as it is now still raises it (the
 * clear's own answer), and one below it is left: something newer was confirmed.
 *
 * Resolves with the floor this left at `cleared`, or null when the canister holds something newer than this clear.
 * The thread may show the cleared conversation only while the floor is still exactly that (floorIs): the caller
 * checks after its own await, since another tab can confirm something newer at any time in between.
 */
export async function settleClear(owner: string, cleared: number, before: Floor): Promise<Floor | null> {
  const raiseTo = (): Floor => {
    const base = current(owner);
    raise(owner, base, cleared);
    return { epoch: base.epoch, revision: cleared };
  };
  if (cleared >= floorOf(owner)) return raiseTo();
  if (await resetFloor(owner, cleared, before)) return { epoch: before.epoch + 1, revision: cleared };
  if (cleared >= floorOf(owner)) return raiseTo();
  return null;
}

/**
 * Before a clear of `owner`'s conversation is sent (see above): the floor rises past `revision` (the revision the
 * thread shows) or its own revision, whichever is higher. Returns the floor as it is then — the snapshot settleClear
 * judges the clear's answer against.
 */
export function raiseBeforeClear(owner: string, revision: number): Floor {
  const base = current(owner);
  const floor: Floor = { epoch: base.epoch, revision: Math.max(base.revision, revision) + 1, expected: true };
  memory.set(owner, floor);
  persist(owner, floor);
  return floor;
}

/**
 * What to show once a clear has been answered and settled (`left`: what settleClear resolved with): the cleared,
 * empty conversation only while the floor is still exactly where the clear left it; otherwise the conversation has
 * to be loaded, because something newer was confirmed (or the canister holds something newer than the clear).
 */
export function afterClear(owner: string, left: Floor | null): 'show-cleared' | 'load' {
  return left && floorIs(owner, left) ? 'show-cleared' : 'load';
}

/** Takes over the floor an earlier version of the app stored for this principal (it knew one canister only). */
export function adoptLegacyFloor(owner: string, principal: string) {
  try {
    const revision = Number(localStorage.getItem(LEGACY_PREFIX + principal));
    // A value from before epochs: only for a floor that was never reset.
    if (Number.isSafeInteger(revision) && revision > 0) raiseFloorByQuery(owner, revision, 0);
  } catch {
    // Nothing to adopt.
  }
}

/** Another tab changed a stored floor: adopt a newer one. */
export function onStorage(key: string | null, value: string | null) {
  if (!key?.startsWith(PREFIX)) return;
  const owner = key.slice(PREFIX.length);
  const theirs = (value && decode(value)) || ZERO;
  const mine = memory.get(owner) ?? ZERO;
  if (newer(theirs, mine)) memory.set(owner, theirs);
  // Make sure the newer of the two is what is stored.
  persist(owner, memory.get(owner) ?? ZERO);
}

if (typeof window !== 'undefined') window.addEventListener('storage', (event) => onStorage(event.key, event.newValue));
