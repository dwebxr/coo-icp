// The newest conversation revision the canister has confirmed for each principal. History older than this comes from
// a replica that is behind (for example, from before a clear) and must not be shown.
//
// A floor only ever grows, with one exception: when the canister itself, asked through consensus, reports a lower
// revision (its state was restored from a snapshot, or it was reinstalled), the floor is reset to that. A reset
// starts a new epoch, and a higher epoch beats any revision of a lower one, so a tab that still remembers the old,
// higher floor adopts the reset instead of writing its value back.
//
// Answers arrive late. An answer is only as new as the moment the canister produced it, and by the time it arrives
// another tab (or this one) may have confirmed something newer. So every caller takes a snapshot of the floor before
// it asks, and hands it back with the answer:
// - a reset is compare-and-set: it happens only if the floor is still exactly that snapshot, in memory and (inside
//   the Web Lock) in storage; otherwise the answer is older than what is known and nothing is lowered;
// - a raise counts only in the epoch it started in: an answer from before a reset is never carried into the new one.
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
}

const ZERO: Floor = { epoch: 0, revision: 0 };
const memory = new Map<string, Floor>();

const newer = (a: Floor, b: Floor) => a.epoch > b.epoch || (a.epoch === b.epoch && a.revision > b.revision);
const latest = (a: Floor, b: Floor) => (newer(b, a) ? b : a);
const same = (a: Floor, b: Floor) => a.epoch === b.epoch && a.revision === b.revision;
const encode = (floor: Floor) => `${floor.epoch}:${floor.revision}`;

/** Null when the text is not a floor. */
function decode(raw: string): Floor | null {
  const match = /^(\d{1,15}):(\d{1,15})$/.exec(raw);
  return match ? { epoch: Number(match[1]), revision: Number(match[2]) } : null;
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
 * The canister confirmed `revision`, in an exchange that started when the floor was in `epoch`. Ignored when a reset
 * has happened since (the answer is from before it). Returns whether it counted.
 */
export function raiseFloor(owner: string, revision: number, epoch: number): boolean {
  const base = current(owner);
  if (base.epoch !== epoch) return false;
  const floor = { epoch, revision: Math.max(base.revision, revision) };
  memory.set(owner, floor);
  // write() only stores it over an older value, so a reset another tab stored meanwhile wins.
  persist(owner, floor);
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

/** Takes over the floor an earlier version of the app stored for this principal (it knew one canister only). */
export function adoptLegacyFloor(owner: string, principal: string) {
  try {
    const revision = Number(localStorage.getItem(LEGACY_PREFIX + principal));
    if (Number.isSafeInteger(revision) && revision > 0) raiseFloor(owner, revision, 0);
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
