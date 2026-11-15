// What the tabs of this origin share about the session: when the user was last active (so "signed out after 10
// minutes of inactivity" also holds when the tab was closed or the computer slept), and that one of them signed
// out (so the others do too).

// The last activity in any tab: "<wall-clock ms>|<tab id>". One rule: it is only ever replaced by a later time, so
// a write that was held back (or a tab whose clock is behind) cannot push another tab's newer activity back. Each
// write reads, compares and writes under a cross-tab Web Lock (best effort where there are none).
const ACTIVITY_KEY = 'coo:last-active';
/**
 * Changes on every sign-out in any tab. Durable, so a tab that was busy restoring a session when it changed still
 * finds out afterwards; the `storage` event tells the tabs that are open at the time.
 */
const SIGN_OUT_KEY = 'coo:signed-out';
/** Activity is written to storage at most this often; other tabs learn of it through storage, not events. */
export const WRITE_EVERY_MS = 15_000;

// This tab's last activity on both clocks. The wall clock is what tabs can compare (and it keeps running while the
// computer sleeps); the monotonic clock cannot be set back.
let lastWall = 0;
let lastMono = 0;
// When activity was last written, on the monotonic clock, and the timer that writes activity the throttle held back.
let writtenMono: number | null = null;
let trailing: ReturnType<typeof setTimeout> | null = null;

const monotonic = () => performance.now();

function randomId(): string {
  return typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function'
    ? Array.from(crypto.getRandomValues(new Uint32Array(2)), (n) => n.toString(36)).join('')
    : Math.random().toString(36).slice(2);
}

/** This page load, as the writer of the stored activity. */
const TAB = randomId();

interface Stored {
  wall: number;
  tab: string;
}

/** Null when nothing (or nothing readable) is stored. A value without a tab id is another tab's. */
function readStored(): Stored | null {
  try {
    const [time, tab = ''] = (localStorage.getItem(ACTIVITY_KEY) ?? '').split('|');
    const wall = Number(time);
    return Number.isFinite(wall) && wall > 0 ? { wall, tab } : null;
  } catch {
    return null;
  }
}

const webLocks = () => (typeof navigator === 'undefined' ? undefined : navigator.locks);

// Bumped by clearActivity: a write that was waiting for the lock when this tab signed out is dropped.
let cleared = 0;

/** Only a later time replaces the stored one. A stored time in the future says nothing and may be replaced. */
function compareAndWrite(time: number) {
  const stored = readStored();
  if (stored && stored.wall >= time && stored.wall <= Date.now()) return;
  try {
    localStorage.setItem(ACTIVITY_KEY, `${time}|${TAB}`);
  } catch {
    // Storage unavailable: this tab's own activity still counts.
  }
}

function writeActivity() {
  if (trailing !== null) clearTimeout(trailing);
  trailing = null;
  writtenMono = monotonic();
  const time = lastWall;
  const generation = cleared;
  const write = () => {
    if (generation === cleared) compareAndWrite(time);
  };
  // Read, compare and write inside a cross-tab Web Lock, so no other tab can write between this tab's read and its
  // write. Without Web Locks — or when the lock cannot be had — it is best effort: two tabs writing at the same
  // moment can leave the earlier time. (Dropping the write instead would let other tabs see this one as idle.)
  const locks = webLocks();
  if (locks) locks.request(ACTIVITY_KEY, write).catch(write);
  else write();
}

/**
 * Records activity in this tab. `force` writes it through at once (sign-in, a reply arriving). Otherwise it is
 * written at most every WRITE_EVERY_MS; activity the limit held back is written when the interval ends, so other
 * tabs never see this one as idle for longer than it was, plus at most that interval.
 */
export function noteActivity(force = false) {
  lastWall = Date.now();
  lastMono = monotonic();
  const since = writtenMono === null ? Infinity : lastMono - writtenMono;
  if (force || since >= WRITE_EVERY_MS) writeActivity();
  else trailing ??= setTimeout(writeActivity, WRITE_EVERY_MS - since);
}

/** Milliseconds since the last activity in any tab; null when none was ever recorded. */
export function idleFor(): number | null {
  const wall = Date.now();
  // This tab's own idle time: the larger of the two clocks' readings, so setting the wall clock back cannot shorten
  // it, and a computer that slept (the wall clock ran on) cannot either.
  const own = lastWall === 0 ? null : Math.max(wall - lastWall, monotonic() - lastMono, 0);
  // Another tab's activity, by the wall clock. What this tab wrote itself is left out: its own two clocks above know
  // better (a clock set back after the write would make the stored time look recent). A time in the future (this
  // clock was set back since, or the other tab's clock is ahead) says nothing about how long ago it was, and is
  // ignored rather than read as "active now".
  const stored = readStored();
  const theirs = !stored || stored.tab === TAB || stored.wall > wall ? null : wall - stored.wall;
  if (own === null) return theirs;
  return theirs === null ? own : Math.min(own, theirs);
}

/** Forgets recorded activity (after a sign-out, so the next sign-in starts fresh). */
export function clearActivity() {
  if (trailing !== null) clearTimeout(trailing);
  trailing = null;
  cleared += 1;
  lastWall = 0;
  lastMono = 0;
  writtenMono = null;
  try {
    localStorage.removeItem(ACTIVITY_KEY);
  } catch {
    // Nothing stored.
  }
}

/** The current sign-out generation: compare two readings to learn whether a tab signed out in between. */
export function signOutGeneration(): string | null {
  try {
    return localStorage.getItem(SIGN_OUT_KEY);
  } catch {
    return null;
  }
}

const newGeneration = () => `${Date.now().toString(36)}-${randomId()}`;

/** Tells the other tabs, now and later, that this one signed out. */
export function announceSignOut() {
  try {
    localStorage.setItem(SIGN_OUT_KEY, newGeneration());
  } catch {
    // Storage unavailable: the other tabs cannot be told, and each still ends its session when idle or expired.
  }
}

/**
 * Calls `listener` when another tab has signed out since the current session began. `seen` is the generation the
 * session started with (see signOutGeneration). A notification only says "look": what is compared is the generation
 * stored NOW, not the one the event carries — events arrive late and in any order, and one about a sign-out from
 * before the session began finds the stored generation unchanged. Returns the function that stops listening.
 */
export function onSignOutElsewhere(listener: () => void, seen: () => string | null): () => void {
  if (typeof window === 'undefined') return () => {};
  // The storage event fires in the other tabs of this origin only, never in the one that wrote.
  const onStorage = (event: StorageEvent) => {
    if (event.key !== SIGN_OUT_KEY) return;
    const now = signOutGeneration();
    if (now !== null && now !== seen()) listener();
  };
  window.addEventListener('storage', onStorage);
  return () => window.removeEventListener('storage', onStorage);
}
