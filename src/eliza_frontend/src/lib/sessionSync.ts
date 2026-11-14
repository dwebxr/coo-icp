// What the tabs of this origin share about the session: when the user was last active (so "signed out after 10
// minutes of inactivity" also holds when the tab was closed or the computer slept), and that one of them signed
// out (so the others do too).

const ACTIVITY_KEY = 'coo:last-active';
const CHANNEL = 'coo:session';
/** Activity is written to storage at most this often; other tabs learn of it through storage, not events. */
const WRITE_EVERY_MS = 15_000;

let lastSeen = 0;
let lastWritten = 0;

function readStored(): number {
  try {
    const stored = Number(localStorage.getItem(ACTIVITY_KEY));
    return Number.isFinite(stored) && stored > 0 ? stored : 0;
  } catch {
    return 0;
  }
}

/** Records activity in this tab. `force` writes it through at once (sign-in, a reply arriving). */
export function noteActivity(now: number, force = false) {
  lastSeen = Math.max(lastSeen, now);
  if (!force && now - lastWritten < WRITE_EVERY_MS) return;
  lastWritten = now;
  try {
    localStorage.setItem(ACTIVITY_KEY, String(now));
  } catch {
    // Storage unavailable: this tab's own activity still counts.
  }
}

/** Milliseconds since the last activity in any tab; null when none was ever recorded. */
export function idleFor(now: number): number | null {
  const last = Math.max(lastSeen, readStored());
  // A clock that was set back must not sign the user out, nor keep the session alive for longer than it should.
  return last === 0 ? null : Math.max(now - last, 0);
}

/** Forgets recorded activity (after a sign-out, so the next sign-in starts fresh). */
export function clearActivity() {
  lastSeen = 0;
  lastWritten = 0;
  try {
    localStorage.removeItem(ACTIVITY_KEY);
  } catch {
    // Nothing stored.
  }
}

type Listener = () => void;

/** Tells the other tabs that this one signed out. */
export function announceSignOut() {
  if (typeof BroadcastChannel === 'undefined') return;
  const channel = new BroadcastChannel(CHANNEL);
  channel.postMessage('signed-out');
  channel.close();
}

/** Calls `listener` when another tab signs out. Returns the function that stops listening. */
export function onSignOutElsewhere(listener: Listener): () => void {
  if (typeof BroadcastChannel === 'undefined') return () => {};
  const channel = new BroadcastChannel(CHANNEL);
  channel.onmessage = (event) => {
    if (event.data === 'signed-out') listener();
  };
  return () => channel.close();
}
