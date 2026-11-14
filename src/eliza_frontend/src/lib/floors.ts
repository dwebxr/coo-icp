// The newest conversation revision the canister has confirmed for each principal. History older than this comes from
// a replica that is behind (for example, from before a clear) and must not be shown.
//
// A floor only ever grows, with one exception: when the canister itself, asked through consensus, reports a lower
// revision (its state was restored from a snapshot, or it was reinstalled), the floor is reset to that. A reset
// starts a new epoch, and a higher epoch beats any revision of a lower one, so a tab that still remembers the old,
// higher floor adopts the reset instead of writing its value back.
//
// Floors are kept in memory for the page's lifetime, so a failing storage or a sign-out cannot lose them, and in
// localStorage across reloads and tabs. Only writers holding a cross-tab Web Lock store them (read, then write the
// newer value), so tabs can never put an older one back; without Web Locks they stay in memory.

const PREFIX = 'coo:floor:';
/** The key used before floors were per canister and had epochs: a plain revision per principal. */
const LEGACY_PREFIX = 'coo:revision:';

interface Floor {
  epoch: number;
  revision: number;
}

const ZERO: Floor = { epoch: 0, revision: 0 };
const memory = new Map<string, Floor>();

const newer = (a: Floor, b: Floor) => a.epoch > b.epoch || (a.epoch === b.epoch && a.revision > b.revision);
const latest = (a: Floor, b: Floor) => (newer(b, a) ? b : a);
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

function write(owner: string, floor: Floor) {
  try {
    // A read that fails or returns something unreadable must not be mistaken for "nothing stored yet".
    const raw = localStorage.getItem(PREFIX + owner);
    const current = raw === null ? ZERO : decode(raw);
    if (current && newer(floor, current)) localStorage.setItem(PREFIX + owner, encode(floor));
  } catch {
    // Storage unavailable or full: the in-memory floor still holds.
  }
}

function persist(owner: string, floor: Floor) {
  const locks = typeof navigator === 'undefined' ? undefined : navigator.locks;
  locks?.request(PREFIX + owner, () => write(owner, floor)).catch(() => {});
}

function current(owner: string): Floor {
  return latest(memory.get(owner) ?? ZERO, stored(owner));
}

export function floorOf(owner: string): number {
  const floor = current(owner);
  memory.set(owner, floor);
  if (newer(floor, stored(owner))) persist(owner, floor);
  return floor.revision;
}

export function raiseFloor(owner: string, revision: number) {
  const base = current(owner);
  const floor = { epoch: base.epoch, revision: Math.max(base.revision, revision) };
  memory.set(owner, floor);
  persist(owner, floor);
}

/**
 * The canister, asked through consensus, is at `revision`, below the floor: start a new epoch there. Only for an
 * answer that cannot be stale (an update call) — a query's answer must never lower a floor.
 */
export function resetFloor(owner: string, revision: number) {
  const floor = { epoch: current(owner).epoch + 1, revision };
  memory.set(owner, floor);
  persist(owner, floor);
}

/** Takes over the floor an earlier version of the app stored for this principal (it knew one canister only). */
export function adoptLegacyFloor(owner: string, principal: string) {
  try {
    const revision = Number(localStorage.getItem(LEGACY_PREFIX + principal));
    if (Number.isSafeInteger(revision) && revision > 0 && current(owner).epoch === 0) raiseFloor(owner, revision);
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
