import { useCallback, useEffect, useRef, useState } from 'react';
import { SessionInvalid, SignInCancelled, type Backend, type Conversation, type Session } from './backend';
import { sleep, within } from './lib/async';
import { mergeHistory, nextId, reconcile, turnsKey, type ChatMessage } from './lib/thread';
import { BUSY_REASON, DELIVERY_WINDOW_MS, deliver } from './lib/delivery';
import { adoptLegacyFloor, floorOf, floorOwner, raiseFloor, resetFloor } from './lib/floors';
import { MAX_MESSAGE_BYTES, byteLength } from './lib/limits';
import { announceSignOut, clearActivity, idleFor, noteActivity, onSignOutElsewhere } from './lib/sessionSync';
import { network } from './network';

export type Status = 'checking' | 'signedOut' | 'signingIn' | 'ready';
export type HistoryState = 'loading' | 'loaded' | 'error';

export type { ChatMessage };

export interface Notice {
  tone: 'info' | 'error';
  text: string;
}

/** 'elsewhere': another tab of this browser signed out. */
type EndReason = 'user' | 'idle' | 'expired' | 'elsewhere';

const IDLE_TIMEOUT_MS = 10 * 60 * 1000;
const RESTORE_TIMEOUT_MS = 8000;
const IDLE_EVENTS = ['pointerdown', 'pointermove', 'keydown', 'wheel', 'touchstart', 'scroll'] as const;
const READ_TIMEOUT_MS = 10_000;
// A read through consensus takes a few seconds on a good day.
const CERTIFIED_READ_TIMEOUT_MS = 30_000;
const CLEAR_TIMEOUT_MS = 30_000;
// A history read older than what this device already saw is asked again this many times, this far apart.
const STALE_READ_ATTEMPTS = 3;
const STALE_READ_DELAY_MS = 1000;
const CONNECTION_ERROR = 'Connection lost.';
const CONFLICT_ERROR = 'Conversation changed.';

export const NOTICES = {
  expired: { tone: 'info', text: 'Session expired. Sign in again.' },
  idle: { tone: 'info', text: 'Signed out after 10 minutes of inactivity.' },
  elsewhere: { tone: 'info', text: 'Signed out in another tab.' },
  signInFailed: { tone: 'error', text: 'Sign-in failed. Please try again.' },
  startFailed: { tone: 'error', text: "Couldn't start sign-in. Reload the page." },
} satisfies Record<string, Notice>;

const END_NOTICE: Record<EndReason, Notice | null> = {
  user: null,
  idle: NOTICES.idle,
  expired: NOTICES.expired,
  elsewhere: NOTICES.elsewhere,
};

/** The floor (lib/floors) of a principal's conversation in the canister this app talks to. */
const ownerOf = (principal: string) => floorOwner(network.backendCanisterId ?? 'local', principal);

function newRequestId(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, '0')).join('');
}

/** One attempt to get a message answered: the canister saves a (request id, base) pair at most once. */
interface Attempt {
  requestId: string;
  base: number;
  /** An earlier attempt with this id may have run. */
  uncertain: boolean;
}

export function useCoo(backend: Backend) {
  const [status, setStatus] = useState<Status>('checking');
  const [principal, setPrincipal] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [history, setHistory] = useState<HistoryState>('loading');
  // A reply for the current session is on its way (drives the typing indicator).
  const [pending, setPending] = useState(false);
  // Any chat request is still running, including one started before a sign-out. The canister saves a chat
  // after it finishes, so new writes wait for it.
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  // Survives an expired session so the user does not lose what they were typing — if the same user signs in again.
  const [draft, setDraft] = useState('');
  const draftOwner = useRef<string | null>(null);
  // False until the auth client exists, and while it is replaced after sign-out.
  const [authReady, setAuthReady] = useState(false);

  // Bumped on every session change; async results from an older session are dropped.
  const gen = useRef(0);
  // Bumped on every local change to the thread; a background history refresh that started earlier is dropped.
  const rev = useRef(0);
  const inflight = useRef(0);
  const authReadyRef = useRef(false);
  const statusRef = useRef<Status>('checking');
  statusRef.current = status;
  const historyRef = useRef<HistoryState>('loading');
  historyRef.current = history;
  const signInAttempt = useRef(0);
  const alive = useRef(true);
  // The revision of the saved conversation the thread shows; new messages are sent after it.
  const revision = useRef(0);
  // The signed-in principal's floor owner; history older than its confirmed revision (lib/floors) is never applied.
  const principalRef = useRef<string | null>(null);
  // The thread may lack something the canister saved (a message whose outcome was unknown left the thread, or
  // another tab was answered); the next reply re-syncs. Cleared only once a re-sync has been applied.
  const needsResync = useRef(false);

  // Keeps the ref in sync immediately so back-to-back events see the new status before re-render.
  const go = (next: Status) => {
    statusRef.current = next;
    setStatus(next);
  };

  // Drop async results once unmounted.
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      gen.current += 1;
    };
  }, []);

  const markAuthReady = (ready: boolean) => {
    authReadyRef.current = ready;
    setAuthReady(ready);
  };

  /** All thread changes except background refreshes go through here. */
  const update = (fn: (prev: ChatMessage[]) => ChatMessage[]) => {
    rev.current += 1;
    setMessages(fn);
  };

  const resetSession = () => {
    gen.current += 1;
    rev.current += 1;
    revision.current = 0;
    setPending(false);
    setMessages([]);
    setHistory('loading');
  };

  const endSession = useCallback(
    async (reason: EndReason) => {
      if (statusRef.current !== 'ready') return;
      // A draft is kept only across an expired session, and only for the principal that typed it.
      draftOwner.current = reason === 'expired' ? principalRef.current : null;
      if (reason !== 'expired') setDraft('');
      resetSession();
      setPrincipal(null);
      principalRef.current = null;
      setNotice(END_NOTICE[reason]);
      go('signedOut');
      markAuthReady(false);
      // An expired delegation ends in every tab by itself; any other sign-out is for the whole browser.
      if (reason !== 'expired') clearActivity();
      if (reason === 'user' || reason === 'idle') announceSignOut();
      try {
        await backend.signOut();
        if (alive.current) markAuthReady(true);
      } catch (err) {
        console.error('Sign-out error:', err);
        if (alive.current) setNotice(NOTICES.startFailed);
      }
    },
    [backend],
  );

  /** Shared handling for failures of authenticated calls. Returns true if the session ended. */
  const handleSessionError = useCallback(
    (err: unknown) => {
      if (!(err instanceof SessionInvalid)) return false;
      void endSession('expired');
      return true;
    },
    [endSession],
  );

  const loadHistory = useCallback(async () => {
    const myGen = gen.current;
    const who = principalRef.current;
    if (!who) return;
    setHistory('loading');
    try {
      let conversation: Conversation | null = null;
      for (let attempt = 1; attempt <= STALE_READ_ATTEMPTS; attempt++) {
        const read = await within(backend.history(), READ_TIMEOUT_MS);
        if (myGen !== gen.current) return;
        if (read.revision >= floorOf(who)) {
          conversation = read;
          break;
        }
        // From a replica that has not caught up yet; ask again.
        if (attempt < STALE_READ_ATTEMPTS) await sleep(STALE_READ_DELAY_MS);
        if (myGen !== gen.current) return;
      }
      if (!conversation) {
        // Still behind what this device has seen. Either replicas are slow to catch up, or the canister really
        // went back (restored from a snapshot, reinstalled). Its own answer, through consensus, settles it.
        conversation = await within(backend.certifiedHistory(), CERTIFIED_READ_TIMEOUT_MS);
        if (myGen !== gen.current) return;
        if (conversation.revision < floorOf(who)) resetFloor(who, conversation.revision);
      }
      update((prev) => mergeHistory(prev, conversation.messages));
      revision.current = conversation.revision;
      raiseFloor(who, conversation.revision);
      setHistory('loaded');
    } catch (err) {
      if (myGen !== gen.current) return;
      console.error('History error:', err);
      if (!handleSessionError(err)) setHistory('error');
    }
  }, [backend, handleSessionError]);

  /**
   * Quietly re-syncs the thread with what the canister saved, unless the thread changed meanwhile. Resolves with the
   * conversation when it was applied, else null.
   */
  const refreshHistory = useCallback(
    async (activeId?: string, timeoutMs = READ_TIMEOUT_MS): Promise<Conversation | null> => {
      const myGen = gen.current;
      const myRev = rev.current;
      const who = principalRef.current;
      try {
        const conversation = await within(backend.history(), timeoutMs);
        if (myGen !== gen.current || myRev !== rev.current || historyRef.current !== 'loaded' || !who) return null;
        if (conversation.revision < floorOf(who)) return null;
        update((prev) => reconcile(prev, conversation.messages, activeId));
        revision.current = conversation.revision;
        raiseFloor(who, conversation.revision);
        return conversation;
      } catch (err) {
        console.error('History refresh error:', err);
        return null;
      }
    },
    [backend],
  );

  const startSession = useCallback(
    (session: Session) => {
      resetSession();
      const owner = ownerOf(session.principal);
      // The floor an earlier version of the app kept for this principal (it only ever ran against mainnet).
      if (!network.local) adoptLegacyFloor(owner, session.principal);
      // A draft kept from an expired session is someone else's unless the same principal is back.
      if (draftOwner.current !== owner) setDraft('');
      draftOwner.current = null;
      setPrincipal(session.principal);
      principalRef.current = owner;
      setNotice(null);
      noteActivity(Date.now(), true);
      go('ready');
      void loadHistory();
    },
    [loadHistory],
  );

  // Restore an existing session on load. If the auth client is slow, show the sign-in screen with an
  // error after a while, and recover quietly if it finishes later.
  useEffect(() => {
    let active = true;
    const timer = setTimeout(() => {
      if (!active || statusRef.current !== 'checking') return;
      setNotice(NOTICES.startFailed);
      go('signedOut');
    }, RESTORE_TIMEOUT_MS);
    backend.restore().then(
      (session) => {
        clearTimeout(timer);
        if (!active) return;
        markAuthReady(true);
        if (statusRef.current === 'checking' || statusRef.current === 'signedOut') {
          setNotice((n) => (n === NOTICES.startFailed ? null : n));
          const idle = idleFor(Date.now());
          if (session && idle !== null && idle >= IDLE_TIMEOUT_MS) {
            // The stored session outlived the idle limit while no tab was open (or the computer slept).
            setNotice(NOTICES.idle);
            go('signedOut');
            markAuthReady(false);
            clearActivity();
            backend.signOut().then(
              () => active && markAuthReady(true),
              (err) => {
                console.error('Sign-out error:', err);
                if (active) setNotice(NOTICES.startFailed);
              },
            );
          } else if (session) startSession(session);
          else go('signedOut');
        }
      },
      (err) => {
        clearTimeout(timer);
        if (!active) return;
        console.error('Auth init error:', err);
        setNotice(NOTICES.startFailed);
        go('signedOut');
      },
    );
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [backend, startSession]);

  // Sign out when the delegation expires.
  useEffect(() => {
    if (status !== 'ready') return;
    const expiresAt = backend.sessionExpiresAt();
    if (expiresAt === null) return;
    const ms = Math.min(Math.max(expiresAt - Date.now(), 0), 2 ** 31 - 1);
    const timer = setTimeout(() => void endSession('expired'), ms);
    return () => clearTimeout(timer);
  }, [status, backend, endSession]);

  // Sign out after a period without activity in any tab of this browser. The time of the last activity is shared
  // (lib/sessionSync) and compared with the clock, so a tab that was in the background, or a computer that slept,
  // does not stretch the session. Waiting for a reply counts as activity, so a slow answer never signs anyone out.
  useEffect(() => {
    if (status !== 'ready') return;
    let timer = 0;
    const check = () => {
      window.clearTimeout(timer);
      const idle = idleFor(Date.now()) ?? 0;
      if (idle >= IDLE_TIMEOUT_MS) void endSession('idle');
      else timer = window.setTimeout(check, IDLE_TIMEOUT_MS - idle);
    };
    const active = () => noteActivity(Date.now());
    const visible = () => {
      if (document.visibilityState === 'visible') check();
    };
    IDLE_EVENTS.forEach((name) => window.addEventListener(name, active, { capture: true, passive: true }));
    document.addEventListener('visibilitychange', visible);
    noteActivity(Date.now(), true);
    check();
    return () => {
      window.clearTimeout(timer);
      IDLE_EVENTS.forEach((name) => window.removeEventListener(name, active, { capture: true }));
      document.removeEventListener('visibilitychange', visible);
    };
  }, [status, pending, endSession]);

  // Signing out in one tab signs out the others: they hold the same identity in memory.
  useEffect(() => {
    if (status !== 'ready') return;
    return onSignOutElsewhere(() => void endSession('elsewhere'));
  }, [status, endSession]);

  const signIn = useCallback(() => {
    if (statusRef.current !== 'signedOut' || !authReadyRef.current) return;
    const attempt = ++signInAttempt.current;
    setNotice(null);
    go('signingIn');
    // Runs synchronously inside the click so the identity window is not blocked as a pop-up.
    backend.signIn().then(
      (session) => {
        if (!alive.current) return;
        if (attempt === signInAttempt.current && statusRef.current === 'signingIn') {
          startSession(session);
          return;
        }
        // The user cancelled this attempt and it completed anyway (the identity window was still open). Cancel
        // means cancel: the delegation it produced is discarded rather than used. If a newer attempt is under
        // way, signing out now would break it, and its own outcome decides.
        if (statusRef.current === 'signedOut') {
          markAuthReady(false);
          backend.signOut().then(
            () => alive.current && markAuthReady(true),
            (err) => {
              console.error('Sign-out error:', err);
              if (alive.current) setNotice(NOTICES.startFailed);
            },
          );
        }
      },
      (err) => {
        if (!alive.current || attempt !== signInAttempt.current || statusRef.current !== 'signingIn') return;
        if (!(err instanceof SignInCancelled)) {
          console.error('Login error:', err);
          setNotice(NOTICES.signInFailed);
        }
        go('signedOut');
      },
    );
  }, [backend, startSession]);

  const cancelSignIn = useCallback(() => {
    if (statusRef.current !== 'signingIn') return;
    signInAttempt.current += 1;
    go('signedOut');
  }, []);

  const signOut = useCallback(() => endSession('user'), [endSession]);

  /** `seen`: the conversation the user saw when sending, to tell a visible change from a stale revision. */
  const runChat = useCallback(
    async (id: string, text: string, first: Attempt, seen: string) => {
      const myGen = gen.current;
      const who = principalRef.current;
      const current = () => alive.current && myGen === gen.current;
      const mark = (patch: Partial<ChatMessage>) =>
        update((prev) => prev.map((m) => (m.id === id ? { ...m, ...patch } : m)));
      // A confirmed revision counts for the sender even if the session has ended by the time it arrives.
      const confirm = (revision: number) => {
        if (who) raiseFloor(who, revision);
      };
      inflight.current += 1;
      setBusy(true);
      setPending(true);
      // One window for the whole message, automatic re-sends and the reads they need included.
      const deadline = Date.now() + DELIVERY_WINDOW_MS;
      const readTimeout = () => Math.min(READ_TIMEOUT_MS, Math.max(deadline - Date.now(), 0));
      try {
        let attempt = first;
        for (let round = 0; ; round++) {
          const result = await deliver(
            attempt.requestId,
            attempt.base,
            text,
            {
              // Confirmations are recorded from the call itself, so one that arrives after the timeout still counts.
              send: (rid, base, t, ms) => {
                const call = backend.send(rid, base, t);
                call.then((outcome) => 'ok' in outcome && confirm(outcome.ok.revision), () => {});
                return within(call, ms);
              },
              status: (rid, ms) => {
                const call = backend.status(rid);
                call.then((status) => typeof status === 'object' && confirm(status.done.revision), () => {});
                return within(call, ms);
              },
              sleep,
              now: Date.now,
              active: current,
              onChecking: () => {
                if (!current()) return;
                setPending(false);
                mark({ checking: true });
              },
            },
            { uncertain: attempt.uncertain, deadline },
          );
          if (!result || !current()) return;

          if (result.kind === 'replied') {
            update((prev) => [
              ...prev.map((m) => (m.id === id && m.checking ? { ...m, checking: undefined } : m)),
              { id: nextId(), role: 'assistant', content: result.reply },
            ]);
            if (result.revision === attempt.base + 1 && revision.current === attempt.base) {
              revision.current = result.revision;
            } else {
              // A replayed reply, or other changes in between: the thread needs the canister's version.
              needsResync.current = true;
            }
            if (needsResync.current && (await refreshHistory(undefined, readTimeout()))) needsResync.current = false;
          } else if (result.kind === 'failed') {
            mark({ checking: undefined, error: result.reason });
            if (result.reason === BUSY_REASON) needsResync.current = true;
          } else if (result.kind === 'unconfirmed') {
            mark({ checking: undefined, error: result.reason ?? CONNECTION_ERROR, unconfirmed: true });
            if (result.reason === BUSY_REASON) needsResync.current = true;
          } else {
            // Not run, and it never will be: the conversation changed since this message's revision.
            const conversation = await refreshHistory(id, readTimeout());
            if (!current()) return;
            if (conversation?.messages.some((m) => m.requestId === attempt.requestId)) break;
            if (!conversation) needsResync.current = true;
            if (result.uncertain) {
              // An earlier attempt may have been saved and then cleared or trimmed away, so this is not "not sent".
              // The old request is dead: a retry sends a new one.
              mark({ checking: undefined, error: CONFLICT_ERROR, unconfirmed: true, requestId: undefined, base: undefined });
            } else if (conversation && round === 0 && turnsKey(conversation.messages) === seen) {
              // It never ran and nothing visible changed (a stale read, or another tab's clear of an empty
              // conversation): send it as a new request after the current revision.
              attempt = { requestId: newRequestId(), base: conversation.revision, uncertain: false };
              mark({ requestId: attempt.requestId, base: attempt.base });
              continue;
            } else {
              mark({ checking: undefined, error: CONFLICT_ERROR });
            }
          }
          break;
        }
      } catch (err) {
        if (!current()) return;
        console.error('Chat error:', err);
        if (!handleSessionError(err)) mark({ checking: undefined, error: CONNECTION_ERROR, unconfirmed: true });
      } finally {
        inflight.current -= 1;
        if (alive.current) {
          if (inflight.current === 0) setBusy(false);
          if (myGen === gen.current) setPending(false);
          // A request from an earlier session finished: the canister may now hold messages this thread lacks.
          else if (statusRef.current === 'ready') void refreshHistory();
        }
      }
    },
    [backend, handleSessionError, refreshHistory],
  );

  // Sending needs the saved history: the revision a message continues comes with it.
  const canSend = status === 'ready' && history === 'loaded' && !busy;

  /**
   * Sends the text. Returns false when sending is not possible right now. `fromDraft`: the text is what the user
   * typed, so the draft is cleared; a suggestion sent with one click leaves a draft alone.
   */
  const send = useCallback(
    (raw: string, fromDraft = true) => {
      const text = raw.trim();
      if (!text || inflight.current > 0 || statusRef.current !== 'ready' || historyRef.current !== 'loaded') return false;
      // The canister refuses longer messages; the composer says so before it gets here.
      if (byteLength(text) > MAX_MESSAGE_BYTES) return false;
      const id = nextId();
      const attempt: Attempt = { requestId: newRequestId(), base: revision.current, uncertain: false };
      if (messages.some((m) => m.unconfirmed)) needsResync.current = true;
      // Failed messages were not saved (or will reappear from history if they were); drop them so the
      // thread follows the canister.
      const kept = messages.filter((m) => !m.error);
      update(() => [...kept, { id, role: 'user', content: text, requestId: attempt.requestId, base: attempt.base }]);
      if (fromDraft) setDraft('');
      void runChat(id, text, attempt, turnsKey(kept));
      return true;
    },
    [messages, runChat],
  );

  const retry = useCallback(
    (id: string) => {
      if (inflight.current > 0 || statusRef.current !== 'ready' || historyRef.current !== 'loaded') return;
      const index = messages.findIndex((m) => m.id === id);
      const target = messages[index];
      if (!target?.error) return;
      // An unconfirmed message may have run: retry the same request, which replays or is refused. A message that
      // definitely did not run is sent as a new request after the current revision.
      const attempt: Attempt =
        target.unconfirmed && target.requestId && target.base !== undefined
          ? { requestId: target.requestId, base: target.base, uncertain: true }
          : { requestId: newRequestId(), base: revision.current, uncertain: false };
      update((prev) =>
        prev.map((m) =>
          m.id === id
            ? { ...m, error: undefined, unconfirmed: undefined, requestId: attempt.requestId, base: attempt.base }
            : m,
        ),
      );
      void runChat(id, target.content, attempt, turnsKey(messages.slice(0, index).filter((m) => !m.error)));
    },
    [messages, runChat],
  );

  /**
   * Resolves true when the conversation was cleared, false when it was not or when the canister has not answered
   * in time. In the last case the call is still out: nothing can be sent until it settles, and if it did clear,
   * the thread follows.
   */
  const clear = useCallback(async () => {
    if (inflight.current > 0) return false;
    const myGen = gen.current;
    const who = principalRef.current;
    const call = backend.clear();
    inflight.current += 1;
    setBusy(true);
    const applied = call.then(
      (cleared) => {
        // An update call's answer is the canister's own: history older than the clear must never be applied
        // again (even if the session has ended meanwhile), and if the canister is behind this device's floor —
        // it was restored or reinstalled — the floor follows it.
        if (who) {
          if (cleared < floorOf(who)) resetFloor(who, cleared);
          else raiseFloor(who, cleared);
        }
        if (!alive.current || myGen !== gen.current) return false;
        update(() => []);
        revision.current = cleared;
        setHistory('loaded');
        return true;
      },
      (err) => {
        if (!alive.current || myGen !== gen.current) return false;
        console.error('Clear error:', err);
        handleSessionError(err);
        return false;
      },
    );
    void applied.finally(() => {
      inflight.current -= 1;
      if (alive.current && inflight.current === 0) setBusy(false);
    });
    try {
      return await within(applied, CLEAR_TIMEOUT_MS);
    } catch {
      // No answer yet. The dialog reports a failure and can be closed; `applied` finishes the job when it comes.
      return false;
    }
  }, [backend, handleSessionError]);

  return {
    status,
    principal,
    messages,
    history,
    pending,
    busy,
    notice,
    draft,
    authReady,
    canSend,
    setDraft,
    signIn,
    cancelSignIn,
    signOut,
    send,
    retry,
    clear,
    reloadHistory: loadHistory,
  };
}

export type Coo = ReturnType<typeof useCoo>;
