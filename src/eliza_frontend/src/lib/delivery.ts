import { NO_REPLY, SessionInvalid, type ReplyStatus, type SendOutcome } from '../backend';

// Each message is sent with a request id and the revision of the conversation it continues. The canister saves it
// at most once: a repeated id replays the saved reply, and a request whose revision is out of date is a conflict.
// So after a lost response the message can simply be sent again with the same id and revision.

// Another message (perhaps from another tab) is still being answered.
export const BUSY_REASON = 'Coo is busy.';

// Replies usually take 10-30 s. A request that takes longer is treated as lost and sent again.
const SEND_TIMEOUT_MS = 90_000;
const STATUS_TIMEOUT_MS = 10_000;
const RESEND_DELAY_MS = 2000;
const STATUS_DELAYS_MS = [1500, 2500, 4000, 6000, 8000];
// The longest a message is waited for in total (an agent also gives up on an update call after 5 minutes).
export const DELIVERY_WINDOW_MS = 5 * 60 * 1000;

export type Delivery =
  | { kind: 'replied'; reply: string; revision: number }
  /** Definitely not saved. */
  | { kind: 'failed'; reason: string }
  /**
   * Not run now because the conversation changed since its revision, so it can never run any more. If `uncertain`,
   * an earlier attempt may have been saved (look for its request id in the history).
   */
  | { kind: 'conflict'; uncertain: boolean }
  /** It may or may not have been saved. Retrying with the same id and revision is safe. */
  | { kind: 'unconfirmed'; reason?: string };

export interface DeliveryDeps {
  /** Rejects if the call has not completed within `timeoutMs`. */
  send(requestId: string, base: number, text: string, timeoutMs: number): Promise<SendOutcome>;
  /** Rejects if the query has not completed within `timeoutMs`. */
  status(requestId: string, timeoutMs: number): Promise<ReplyStatus>;
  sleep(ms: number): Promise<void>;
  now(): number;
  /** False once the result is no longer wanted (the session changed). */
  active(): boolean;
  /** An attempt went unanswered; the outcome is being checked. */
  onChecking(): void;
}

interface State {
  deadline: number;
  /** An earlier attempt may have been saved. */
  uncertain: boolean;
  /** Keeps the polling backoff growing across rounds. */
  polls: number;
}

/**
 * Sends a message and resolves with what happened to it, or null once no longer active. Throws SessionInvalid.
 * `uncertain`: an earlier attempt may have been saved (a retry of an unconfirmed message).
 * Only the canister's own answers decide the outcome; status queries are only used to wait. Everything, including
 * the calls themselves, ends by `deadline` (five minutes from now by default).
 */
export async function deliver(
  requestId: string,
  base: number,
  text: string,
  deps: DeliveryDeps,
  { uncertain = false, deadline = deps.now() + DELIVERY_WINDOW_MS }: { uncertain?: boolean; deadline?: number } = {},
): Promise<Delivery | null> {
  const state: State = { deadline, uncertain, polls: 0 };
  const unconfirmed = (reason?: string): Delivery => ({ kind: 'unconfirmed', ...(reason ? { reason } : {}) });
  let outcome = await sendWithResend(requestId, base, text, deps, state);
  for (;;) {
    if (outcome === null || !deps.active()) return null;
    if (outcome === 'lost') return unconfirmed();
    if ('ok' in outcome) return { kind: 'replied', ...outcome.ok };
    if ('err' in outcome) return { kind: 'failed', reason: outcome.err };
    if ('conflict' in outcome) return { kind: 'conflict', uncertain: state.uncertain };
    // These only say that this call saved nothing: an earlier attempt still in transit could arrive and be saved.
    if ('busy' in outcome) return state.uncertain ? unconfirmed(BUSY_REASON) : { kind: 'failed', reason: BUSY_REASON };
    if ('failed' in outcome || 'rejected' in outcome) {
      return state.uncertain ? unconfirmed(NO_REPLY) : { kind: 'failed', reason: NO_REPLY };
    }

    // Still being answered: wait for it.
    state.uncertain = true;
    deps.onChecking();
    const status = await awaitStatus(requestId, deps, state);
    if (status === null) return null;
    if (status === 'timeout') return unconfirmed();
    if (status !== 'unknown') return { kind: 'replied', ...status.done };
    // A query may come from a replica that is behind, so "unknown" is no verdict. Ask the canister again with the
    // same id and revision: it replays a saved reply, reports it still in progress, or answers it now.
    outcome = await sendWithResend(requestId, base, text, deps, state);
  }
}

/**
 * One send, and one re-send after a lost response. 'lost' when the re-send was lost too or no time is left;
 * null once inactive.
 */
async function sendWithResend(
  requestId: string,
  base: number,
  text: string,
  deps: DeliveryDeps,
  state: State,
): Promise<SendOutcome | 'lost' | null> {
  for (let attempt = 0; attempt < 2; attempt++) {
    if (attempt > 0) {
      deps.onChecking();
      await deps.sleep(Math.min(RESEND_DELAY_MS, Math.max(state.deadline - deps.now(), 0)));
      if (!deps.active()) return null;
    }
    const left = state.deadline - deps.now();
    if (left <= 0) return 'lost';
    try {
      return await deps.send(requestId, base, text, Math.min(SEND_TIMEOUT_MS, left));
    } catch (err) {
      if (err instanceof SessionInvalid) throw err;
      console.error('Chat error:', err);
      state.uncertain = true;
    }
  }
  return 'lost';
}

/** Polls until the request is no longer pending. */
async function awaitStatus(
  requestId: string,
  deps: DeliveryDeps,
  state: State,
): Promise<Exclude<ReplyStatus, 'pending'> | 'timeout' | null> {
  for (;;) {
    const left = state.deadline - deps.now();
    if (left <= 0) return 'timeout';
    await deps.sleep(Math.min(STATUS_DELAYS_MS[Math.min(state.polls++, STATUS_DELAYS_MS.length - 1)], left));
    if (!deps.active()) return null;
    const remaining = state.deadline - deps.now();
    if (remaining <= 0) return 'timeout';
    try {
      const status = await deps.status(requestId, Math.min(STATUS_TIMEOUT_MS, remaining));
      if (!deps.active()) return null;
      if (status !== 'pending') return status;
    } catch (err) {
      if (err instanceof SessionInvalid) throw err;
    }
  }
}
