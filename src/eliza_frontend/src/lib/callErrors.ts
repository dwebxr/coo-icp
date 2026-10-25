// The replica rejects a request whose user signature or delegation it does not accept with an HTTP 4xx body
// such as "Invalid signature: Invalid basic signature: …" or "Invalid delegation: Invalid delegation expiry: …".
// Failures to verify the replica's own response (agent CertificateVerificationError: "Invalid certificate:
// Invalid signature from replica …") are not session problems; signing in again would not fix them.
const AUTH_REJECTION = /invalid (basic )?signature:|invalid delegation|delegation.{0,40}expir/i;
const REPLICA_VERIFICATION = /certificate|from replica|node key/i;

export function isAuthError(err: unknown): boolean {
  if (err instanceof Error && err.name === 'CertificateVerificationError') return false;
  const text = err instanceof Error ? err.message : String(err);
  return AUTH_REJECTION.test(text) && !REPLICA_VERIFICATION.test(text);
}

/**
 * The canister refused or failed an update call (it trapped, or was stopped or out of cycles), as opposed to a
 * network failure, so the rejected request saved nothing. (Only meaningful for an agent that does not re-submit
 * calls on its own; see backend.ts.) The agent throws UpdateCallRejectedError for a synchronous answer and a plain
 * Error from polling. "Call was marked as done but we never saw the reply" is not a reject: the call did run.
 */
export function isCanisterReject(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  // agent 2.4.1's AgentError resets the prototype, so `instanceof UpdateCallRejectedError` is always false; match
  // the fields it sets instead.
  const rejected = err as Error & { type?: unknown; reject_code?: unknown };
  if (rejected.type === 'update' && rejected.reject_code !== undefined) return true;
  return /^Call was rejected:/.test(err.message);
}
