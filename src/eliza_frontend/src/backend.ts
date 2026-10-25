import { Actor, HttpAgent, type Identity } from '@dfinity/agent';
import { AuthClient } from '@dfinity/auth-client';
import { isDelegationValid, type DelegationChain } from '@dfinity/identity';
import { idlFactory, canisterId, type _SERVICE, type Message as CandidMessage } from './declarations/eliza_backend';
import { isAuthError, isCanisterReject } from './lib/callErrors';

export interface Message {
  role: string;
  content: string;
  /** Set on user messages sent with a request id. */
  requestId?: string;
}

/** The saved conversation. `revision` changes with every saved exchange and every clear. */
export interface Conversation {
  messages: Message[];
  revision: number;
}

export interface Sent {
  reply: string;
  /** The conversation's revision after the canister answered. */
  revision: number;
}

export interface Session {
  principal: string;
}

/**
 * What the canister did with this call (submitted exactly once). `err` is short, user-facing text: nothing was
 * saved, and no other attempt with the same id and base can be saved later either. Busy, conflict (the
 * conversation changed since `base`), failed (the model could not reply) and rejected (the canister refused or
 * failed it) only say that this call saved nothing.
 */
export type SendOutcome =
  | { ok: Sent }
  | { err: string }
  | { busy: true }
  | { inProgress: true }
  | { conflict: true }
  | { failed: true }
  | { rejected: true };

export type ReplyStatus = 'unknown' | 'pending' | { done: Sent };

export const NO_REPLY = "Coo couldn't reply.";

/** The user closed the Internet Identity window. */
export class SignInCancelled extends Error {}

/** The stored delegation is expired, revoked or does not match the session key. */
export class SessionInvalid extends Error {}

export interface Backend {
  /** Prepares sign-in and restores a previous session, if any. Must resolve before signIn() is called. */
  restore(): Promise<Session | null>;
  /** Must be called synchronously from a click handler so the identity window is not blocked. */
  signIn(): Promise<Session>;
  signOut(): Promise<void>;
  history(): Promise<Conversation>;
  /**
   * Sends a message as the next exchange after revision `base`. A (request id, base) pair is saved at most once:
   * repeating it replays the saved reply, and a changed conversation is a conflict.
   */
  send(requestId: string, base: number, text: string): Promise<SendOutcome>;
  status(requestId: string): Promise<ReplyStatus>;
  /** Resolves with the conversation's new revision. */
  clear(): Promise<number>;
  /** Epoch ms when the current session's delegation expires, or null when signed out. */
  sessionExpiresAt(): number | null;
}

const isMainnet =
  typeof window !== 'undefined' &&
  (window.location.hostname.endsWith('.icp0.io') ||
    window.location.hostname.endsWith('.ic0.app') ||
    window.location.hostname.endsWith('.raw.ic0.app'));

const iiCanisterId =
  (import.meta as { env?: Record<string, string | undefined> }).env?.CANISTER_ID_INTERNET_IDENTITY ||
  'rdmx6-jaaaa-aaaaa-aaadq-cai';

const identityProvider = isMainnet ? 'https://identity.internetcomputer.org' : `http://${iiCanisterId}.localhost:4943`;

function delegationOf(identity: Identity): DelegationChain | null {
  const maybe = identity as Identity & { getDelegation?: () => DelegationChain };
  return typeof maybe.getDelegation === 'function' ? maybe.getDelegation() : null;
}

function popupFeatures(): string | undefined {
  // A centered window keeps the app visible behind it. Phones ignore features and open a tab.
  if (window.matchMedia('(pointer: coarse)').matches) return undefined;
  const width = 500;
  const height = 720;
  const left = Math.round(window.screenX + (window.outerWidth - width) / 2);
  const top = Math.round(window.screenY + (window.outerHeight - height) / 2);
  return `width=${width},height=${height},left=${left},top=${top},toolbar=0,location=0,menubar=0`;
}

const toMessage = (m: CandidMessage): Message => ({
  role: m.role,
  content: m.content,
  ...(m.request_id.length ? { requestId: m.request_id[0] } : {}),
});

const toSent = (sent: { reply: string; revision: bigint }): Sent => ({ reply: sent.reply, revision: Number(sent.revision) });

export function createIcBackend(): Backend {
  let client: AuthClient | null = null;
  // Queries go through an agent that retries on its own. Updates go through one that never re-submits: the app
  // re-sends them itself (with a request id the canister deduplicates), so it always knows whether an earlier
  // submission might still arrive. (The agent's v3 → v2 fallback only follows a v3 404, i.e. an unprocessed call.)
  let actors: { query: _SERVICE; update: _SERVICE } | null = null;
  let restoring: Promise<Session | null> | null = null;
  // Bumped by every connect and sign-out, so a slower, superseded connect never publishes its actor.
  let connection = 0;

  // The library idle manager leaks listeners and re-fires callbacks; the app runs its own idle timer.
  const newClient = () => AuthClient.create({ idleOptions: { disableIdle: true } });

  const connect = async (identity: Identity): Promise<Session> => {
    const attempt = ++connection;
    const host = isMainnet ? 'https://icp-api.io' : undefined;
    const queryAgent = HttpAgent.createSync({ identity, host });
    const updateAgent = HttpAgent.createSync({ identity, host, retryTimes: 0 });
    if (!isMainnet) await Promise.all([queryAgent.fetchRootKey(), updateAgent.fetchRootKey()]);
    if (attempt !== connection) throw new SignInCancelled();
    actors = {
      query: Actor.createActor<_SERVICE>(idlFactory, { agent: queryAgent, canisterId }),
      update: Actor.createActor<_SERVICE>(idlFactory, { agent: updateAgent, canisterId }),
    };
    return { principal: identity.getPrincipal().toText() };
  };

  const call = async <T>(fn: (a: _SERVICE) => Promise<T>, kind: 'query' | 'update' = 'query'): Promise<T> => {
    if (!actors) throw new SessionInvalid('Not signed in');
    const current = actors[kind];
    if (!sessionValid()) throw new SessionInvalid('Session expired');
    try {
      return await fn(current);
    } catch (err) {
      if (isAuthError(err)) throw new SessionInvalid(String(err));
      throw err;
    }
  };

  const sessionValid = () => {
    const chain = client ? delegationOf(client.getIdentity()) : null;
    return !!chain && isDelegationValid(chain);
  };

  return {
    restore() {
      restoring ??= (async () => {
        client = await newClient();
        if (!(await client.isAuthenticated()) || !sessionValid()) return null;
        return connect(client.getIdentity());
      })();
      return restoring;
    },

    signIn() {
      const current = client;
      if (!current) return Promise.reject(new Error('Sign-in is not ready'));
      return new Promise<Session>((resolve, reject) => {
        void current.login({
          identityProvider,
          windowOpenerFeatures: popupFeatures(),
          onSuccess: () => {
            // Ignore a late success from a client that was replaced by sign-out in the meantime.
            if (client !== current) return reject(new SignInCancelled());
            connect(current.getIdentity()).then(resolve, reject);
          },
          onError: (reason) => {
            reject(reason === 'UserInterrupt' ? new SignInCancelled() : new Error(reason || 'Sign-in failed'));
          },
        });
      });
    },

    async signOut() {
      connection += 1;
      // The actor is dropped rather than its agent invalidated: a chat already in flight keeps polling with
      // its own identity until it settles, so the app learns whether the canister saved it. No new call can
      // be made once the actor is gone.
      actors = null;
      const previous = client;
      client = null;
      try {
        await previous?.logout();
      } finally {
        // logout() deletes the stored session key but keeps it in memory; a fresh client stores a new key
        // so the next sign-in survives a reload.
        client = await newClient();
      }
    },

    async history() {
      const conversation = await call((a) => a.get_conversation());
      return { messages: conversation.messages.map(toMessage), revision: Number(conversation.revision) };
    },

    async send(requestId, base, text) {
      let result;
      try {
        result = await call((a) => a.send_message(requestId, BigInt(base), text), 'update');
      } catch (err) {
        if (!isCanisterReject(err)) throw err;
        console.error('Chat rejected:', err);
        return { rejected: true };
      }
      if ('Ok' in result) return { ok: toSent(result.Ok) };
      const error = result.Err;
      if ('Busy' in error) return { busy: true };
      if ('InProgress' in error) return { inProgress: true };
      if ('Conflict' in error) return { conflict: true };
      if ('Cleared' in error) return { err: 'Conversation cleared.' };
      if ('Invalid' in error) return { err: error.Invalid };
      console.error('Chat failed:', error.Failed);
      return { failed: true };
    },

    async status(requestId) {
      const status = await call((a) => a.get_message_status(requestId));
      if ('Done' in status) return { done: toSent(status.Done) };
      return 'Pending' in status ? 'pending' : 'unknown';
    },

    clear: async () => Number(await call((a) => a.clear_conversation(), 'update')),

    sessionExpiresAt() {
      const chain = actors && client ? delegationOf(client.getIdentity()) : null;
      if (!chain || chain.delegations.length === 0) return null;
      const earliest = chain.delegations.reduce(
        (min, { delegation }) => (delegation.expiration < min ? delegation.expiration : min),
        chain.delegations[0].delegation.expiration,
      );
      return Number(earliest / BigInt(1_000_000));
    },
  };
}
