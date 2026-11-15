// Where the app is running, and what it talks to there.
//
// "Local" is a dfx replica on this machine, recognised by the host name; every other host is the Internet Computer.
// The check fails closed: an unknown host (a custom domain, say) gets mainnet behaviour — the IC's root key is
// never fetched from the network there, and sign-in goes to the real Internet Identity.

/** The backend canister on the Internet Computer. Public; see canister_ids.json. */
const MAINNET_BACKEND = '4wfup-gqaaa-aaaas-qdqca-cai';
const LOCAL_REPLICA_PORT = 4943;

export function isLocalHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return host === 'localhost' || host === '127.0.0.1' || host === '[::1]' || host.endsWith('.localhost');
}

export interface Network {
  local: boolean;
  /** Undefined when running locally without canister ids (no local deployment yet). */
  backendCanisterId: string | undefined;
  /** Where the agent sends calls. */
  host: string;
  identityProvider: string;
}

/** `origin`: this page's origin (scheme, host and port), where a local deployment's API is served. */
export function networkFor(hostname: string, origin: string): Network {
  if (!isLocalHost(hostname)) {
    return {
      local: false,
      backendCanisterId: MAINNET_BACKEND,
      host: 'https://icp-api.io',
      identityProvider: 'https://identity.internetcomputer.org',
    };
  }
  // dfx writes the local ids to .env at the repository root; Vite inlines these two when it serves or builds.
  // They are only ever used on a local host.
  const internetIdentity = import.meta.env.CANISTER_ID_INTERNET_IDENTITY || 'rdmx6-jaaaa-aaaaa-aaadq-cai';
  return {
    local: true,
    backendCanisterId: import.meta.env.CANISTER_ID_ELIZA_BACKEND || undefined,
    // The page's own origin, never the agent's guess: the agent knows "localhost" and "127.0.0.1" but would send the
    // calls of a page opened on [::1] to mainnet. Locally the API is served on every origin the app can be on: the
    // replica's gateway answers /api on the frontend canister's own host, and the dev server passes /api on to it.
    host: origin,
    identityProvider: `http://${internetIdentity}.localhost:${LOCAL_REPLICA_PORT}`,
  };
}

export const network: Network =
  typeof window === 'undefined'
    ? networkFor('localhost', 'http://localhost:4943')
    : networkFor(window.location.hostname, window.location.origin);
