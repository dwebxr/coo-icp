// What the tabs of this origin share about the session: when the user was last active (so "signed out after 10
// minutes of inactivity" also holds when the tab was closed or the computer slept), and that one of them signed
// out (so the others do too).

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

function readStored(): number {
  try {
    const stored = Number(localStorage.getItem(ACTIVITY_KEY));
    return Number.isFinite(stored) && stored > 0 ? stored : 0;
  } catch {
    return 0;
  }
}

function writeActivity() {
  if (trailing !== null) clearTimeout(trailing);
  trailing = null;
  writtenMono = monotonic();
  try {
    localStorage.setItem(ACTIVITY_KEY, String(lastWall));
  } catch {
    // Storage unavailable: this tab's own activity still counts.
  }
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
  // Another tab's activity, by the wall clock. A time in the future (this clock was set back since, or the other
  // tab's clock is ahead) says nothing about how long ago it was, and is ignored rather than read as "active now".
  const stored = readStored();
  const theirs = stored === 0 || stored > wall ? null : wall - stored;
  if (own === null) return theirs;
  return theirs === null ? own : Math.min(own, theirs);
}

/** Forgets recorded activity (after a sign-out, so the next sign-in starts fresh). */
export function clearActivity() {
  if (trailing !== null) clearTimeout(trailing);
  trailing = null;
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

function newGeneration(): string {
  const random =
    typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function'
      ? Array.from(crypto.getRandomValues(new Uint32Array(2)), (n) => n.toString(36)).join('')
      : Math.random().toString(36).slice(2);
  return `${Date.now().toString(36)}-${random}`;
}

/** Tells the other tabs, now and later, that this one signed out. */
export function announceSignOut() {
  try {
    localStorage.setItem(SIGN_OUT_KEY, newGeneration());
  } catch {
    // Storage unavailable: the other tabs cannot be told, and each still ends its session when idle or expired.
  }
}

/** Calls `listener` when another tab signs out. Returns the function that stops listening. */
export function onSignOutElsewhere(listener: () => void): () => void {
  if (typeof window === 'undefined') return () => {};
  // The storage event fires in the other tabs of this origin only, never in the one that wrote.
  const onStorage = (event: StorageEvent) => {
    if (event.key === SIGN_OUT_KEY && event.newValue !== null && event.newValue !== event.oldValue) listener();
  };
  window.addEventListener('storage', onStorage);
  return () => window.removeEventListener('storage', onStorage);
}
