// The newest conversation revision the canister has confirmed for each principal. History older than this comes from
// a replica that is behind (for example, from before a clear) and must not be shown. It only ever grows. It is kept
// in memory for the page's lifetime, so a failing storage or a sign-out cannot lose it, and in localStorage across
// reloads and tabs. Only writers holding a cross-tab Web Lock store it (read, then write the larger value), so tabs
// can never lower it; without Web Locks it stays in memory.

const PREFIX = 'coo:revision:';
const memory = new Map<string, number>();

function stored(principal: string): number {
  try {
    return Number(localStorage.getItem(PREFIX + principal)) || 0;
  } catch {
    return 0;
  }
}

function write(principal: string, revision: number) {
  try {
    // A read that fails or returns something unreadable must not be mistaken for "nothing stored yet".
    const raw = localStorage.getItem(PREFIX + principal);
    const current = raw === null ? 0 : Number(raw);
    if (Number.isFinite(current) && revision > current) localStorage.setItem(PREFIX + principal, String(revision));
  } catch {
    // Storage unavailable or full: the in-memory floor still holds.
  }
}

function persist(principal: string, revision: number) {
  const locks = typeof navigator === 'undefined' ? undefined : navigator.locks;
  locks?.request(PREFIX + principal, () => write(principal, revision)).catch(() => {});
}

export function floorOf(principal: string): number {
  const floor = Math.max(memory.get(principal) ?? 0, stored(principal));
  memory.set(principal, floor);
  if (floor > stored(principal)) persist(principal, floor);
  return floor;
}

export function raiseFloor(principal: string, revision: number) {
  const floor = Math.max(revision, memory.get(principal) ?? 0);
  memory.set(principal, floor);
  persist(principal, floor);
}

/** Another tab changed a stored floor: adopt a higher one. */
export function onStorage(key: string | null, value: string | null) {
  if (!key?.startsWith(PREFIX)) return;
  const principal = key.slice(PREFIX.length);
  const theirs = Number(value) || 0;
  if (theirs > (memory.get(principal) ?? 0)) memory.set(principal, theirs);
  // Make sure the larger of the two is what is stored.
  persist(principal, memory.get(principal) ?? 0);
}

if (typeof window !== 'undefined') window.addEventListener('storage', (event) => onStorage(event.key, event.newValue));
