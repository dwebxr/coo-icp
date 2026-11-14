// Development only: signs in with an identity made in this browser, so the real canister client (backend.ts) can
// be run against a local replica without Internet Identity. Loaded from main.tsx only on a local host, when the URL
// has ?dev-identity, in the dev server or in the bundle `make e2e-local` builds for itself; it is not part of any
// other build.
import { DelegationChain, DelegationIdentity, Ed25519KeyIdentity } from '@dfinity/identity';
import type { Identity } from '@dfinity/agent';
import type { Auth } from '../backend';

const SESSION_MS = 8 * 60 * 60 * 1000;

/** `name` tells apart several identities in one browser (?dev-identity=<name>). */
export function devAuth(name: string): Auth {
  const key = `coo:dev-identity:${name}`;

  const stored = (): Ed25519KeyIdentity | null => {
    const raw = localStorage.getItem(key);
    return raw ? Ed25519KeyIdentity.fromJSON(raw) : null;
  };

  // The same shape Internet Identity gives the app: a session key with a delegation from the identity.
  const session = async (root: Ed25519KeyIdentity): Promise<Identity> => {
    const sessionKey = Ed25519KeyIdentity.generate();
    const chain = await DelegationChain.create(root, sessionKey.getPublicKey(), new Date(Date.now() + SESSION_MS));
    return DelegationIdentity.fromDelegation(sessionKey, chain);
  };

  return {
    async restore() {
      const root = stored();
      return root ? session(root) : null;
    },
    async signIn() {
      const root = stored() ?? Ed25519KeyIdentity.generate();
      localStorage.setItem(key, JSON.stringify(root.toJSON()));
      return session(root);
    },
    async signOut() {
      localStorage.removeItem(key);
    },
  };
}
