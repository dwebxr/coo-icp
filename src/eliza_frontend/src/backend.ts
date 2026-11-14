import { Actor, HttpAgent, type ActorMethod, type Identity } from '@dfinity/agent';
import { AuthClient } from '@dfinity/auth-client';
import type { IDL } from '@dfinity/candid';
import { isDelegationValid, type DelegationChain } from '@dfinity/identity';
import {
  idlFactory,
  type _SERVICE,
  type Conversation as CandidConversation,
  type Message as CandidMessage,
} from './declarations/eliza_backend';
import { isAuthError, isCanisterReject } from './lib/callErrors';
import { network } from './network';

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
 * saved, and no other attempt with the same id and base can be saved later either. Busy (the caller has another
 * chat in progress, or the canister is at a limit), conflict (the conversation is not at `base`), failed (the model
 * could not reply) and rejected (the canister refused or failed it) only say that this call saved nothing.
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
  /** The saved conversation, as one replica reports it (fast; a replica can be a little behind). */
  history(): Promise<Conversation>;
  /** The saved conversation, as the subnet certifies it (an update call: slower, never behind). */
  certifiedHistory(): Promise<Conversation>;
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

/** Signing in and out. The default is Internet Identity; development can supply another (see dev/). */
export interface Auth {
  /** Prepares sign-in and returns the identity of an earlier session, if there is one. */
  restore(): Promise<Identity | null>;
  /** Must begin synchronously inside a click handler, so the identity window is not blocked as a pop-up. */
  signIn(): Promise<Identity>;
  signOut(): Promise<void>;
}

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

export function internetIdentityAuth(identityProvider: string): Auth {
  let client: AuthClient | null = null;
  // The library idle manager leaks listeners and re-fires callbacks; the app runs its own idle timer.
  const newClient = () => AuthClient.create({ idleOptions: { disableIdle: true } });

  return {
    async restore() {
      client = await newClient();
      // isAuthenticated() ignores expiry; the caller checks the delegation itself.
      return (await client.isAuthenticated()) ? client.getIdentity() : null;
    },

    signIn() {
      const current = client;
      if (!current) return Promise.reject(new Error('Sign-in is not ready'));
      return new Promise<Identity>((resolve, reject) => {
        void current.login({
          identityProvider,
          windowOpenerFeatures: popupFeatures(),
          onSuccess: () => {
            // Ignore a late success from a client that was replaced by sign-out in the meantime.
            if (client !== current) reject(new SignInCancelled());
            else resolve(current.getIdentity());
          },
          onError: (reason) => {
            reject(reason === 'UserInterrupt' ? new SignInCancelled() : new Error(reason || 'Sign-in failed'));
          },
        });
      });
    },

    async signOut() {
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
  };
}

const toMessage = (m: CandidMessage): Message => ({
  role: m.role,
  content: m.content,
  ...(m.request_id.length ? { requestId: m.request_id[0] } : {}),
});

const toConversation = (c: CandidConversation): Conversation => ({
  messages: c.messages.map(toMessage),
  revision: Number(c.revision),
});

const toSent = (sent: { reply: string; revision: bigint }): Sent => ({ reply: sent.reply, revision: Number(sent.revision) });

/**
 * `get_conversation` declared as an update method. Called that way it goes through consensus and the subnet
 * certifies the answer, where a query is answered by a single replica that may be a few seconds behind.
 */
interface CertifiedReads {
  get_conversation: ActorMethod<[], CandidConversation>;
}

const certifiedReads: IDL.InterfaceFactory = (args) => {
  const [, read] = idlFactory(args)._fields.find(([name]) => name === 'get_conversation')!;
  return args.IDL.Service({ get_conversation: args.IDL.Func(read.argTypes, read.retTypes, []) });
};

interface Actors {
  query: _SERVICE;
  update: _SERVICE;
  certified: CertifiedReads;
}

export function createIcBackend(auth: Auth = internetIdentityAuth(network.identityProvider)): Backend {
  // The identity of the current session, and the actors that call the canister as it.
  // Queries go through an agent that retries on its own. Updates go through one that never re-submits: the app
  // re-sends them itself (with a request id the canister deduplicates), so it always knows whether an earlier
  // submission might still arrive. (The agent's v3 → v2 fallback only follows a v3 404, i.e. an unprocessed call.)
  let session: { identity: Identity; actors: Actors } | null = null;
  let restoring: Promise<Session | null> | null = null;
  // Bumped by every connect and sign-out, so a slower, superseded connect never publishes its actors.
  let connection = 0;

  const valid = (identity: Identity) => {
    const chain = delegationOf(identity);
    return !!chain && isDelegationValid(chain);
  };

  const connect = async (identity: Identity): Promise<Session> => {
    const attempt = ++connection;
    const canisterId = network.backendCanisterId;
    if (!canisterId) throw new Error('No backend canister id for the local replica: deploy locally first.');
    const queryAgent = HttpAgent.createSync({ identity, host: network.host });
    const updateAgent = HttpAgent.createSync({ identity, host: network.host, retryTimes: 0 });
    // Only a local replica has its own root key. On the Internet Computer the agent's built-in key is the trust
    // anchor and must never be replaced by one fetched from the network.
    if (network.local) await Promise.all([queryAgent.fetchRootKey(), updateAgent.fetchRootKey()]);
    if (attempt !== connection) throw new SignInCancelled();
    session = {
      identity,
      actors: {
        query: Actor.createActor<_SERVICE>(idlFactory, { agent: queryAgent, canisterId }),
        update: Actor.createActor<_SERVICE>(idlFactory, { agent: updateAgent, canisterId }),
        certified: Actor.createActor<CertifiedReads>(certifiedReads, { agent: queryAgent, canisterId }),
      },
    };
    return { principal: identity.getPrincipal().toText() };
  };

  const call = async <T>(fn: (actors: Actors) => Promise<T>): Promise<T> => {
    if (!session) throw new SessionInvalid('Not signed in');
    if (!valid(session.identity)) throw new SessionInvalid('Session expired');
    try {
      return await fn(session.actors);
    } catch (err) {
      if (isAuthError(err)) throw new SessionInvalid(String(err));
      throw err;
    }
  };

  return {
    restore() {
      restoring ??= (async () => {
        const identity = await auth.restore();
        return identity && valid(identity) ? connect(identity) : null;
      })();
      return restoring;
    },

    signIn() {
      // auth.signIn() runs synchronously inside the caller's click.
      return auth.signIn().then(connect);
    },

    async signOut() {
      connection += 1;
      // The session is dropped rather than its agents invalidated: an update call already submitted still
      // completes on its own (the canister may save it), but no new call can be made, and the app only learns
      // about that request again from the history it loads after the next sign-in.
      session = null;
      await auth.signOut();
    },

    async history() {
      return toConversation(await call((a) => a.query.get_conversation()));
    },

    async certifiedHistory() {
      return toConversation(await call((a) => a.certified.get_conversation()));
    },

    async send(requestId, base, text) {
      let result;
      try {
        result = await call((a) => a.update.send_message(requestId, BigInt(base), text));
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
      const status = await call((a) => a.query.get_message_status(requestId));
      if ('Done' in status) return { done: toSent(status.Done) };
      return 'Pending' in status ? 'pending' : 'unknown';
    },

    clear: async () => Number(await call((a) => a.update.clear_conversation())),

    sessionExpiresAt() {
      const chain = session ? delegationOf(session.identity) : null;
      if (!chain || chain.delegations.length === 0) return null;
      const earliest = chain.delegations.reduce(
        (min, { delegation }) => (delegation.expiration < min ? delegation.expiration : min),
        chain.delegations[0].delegation.expiration,
      );
      return Number(earliest / BigInt(1_000_000));
    },
  };
}
