import { describe, expect, it } from 'vitest';
import { Principal } from '@dfinity/principal';
import { UpdateCallRejectedError } from '@dfinity/agent';
import { IDL } from '@dfinity/candid';
import { isAuthError, isCanisterReject } from './callErrors';

const http = (body: string) => new Error(`Server returned an error:\n  Code: 400 (Bad Request)\n  Body: ${body}\n`);

describe('isAuthError', () => {
  it('detects rejected user signatures and delegations', () => {
    expect(isAuthError(http('Invalid signature: Invalid basic signature: EcdsaP256 signature could not be verified'))).toBe(true);
    expect(isAuthError(http('Invalid delegation: Invalid delegation expiry: Specified sender delegation has expired'))).toBe(true);
  });

  it('ignores replica response verification failures', () => {
    const certErr = new Error('Invalid certificate: Invalid signature from replica signed query: no matching node key found.');
    expect(isAuthError(certErr)).toBe(false);
    const named = new Error('anything');
    named.name = 'CertificateVerificationError';
    expect(isAuthError(named)).toBe(false);
  });

  it('ignores ordinary failures', () => {
    expect(isAuthError(new Error('Failed to fetch'))).toBe(false);
    expect(isAuthError(http('Forbidden'))).toBe(false);
    expect(isAuthError('IC LLM canister is temporarily unavailable.')).toBe(false);
  });
});

describe('isCanisterReject', () => {
  const rejectedCall = (body: Record<string, unknown>) =>
    new UpdateCallRejectedError(
      Principal.anonymous(),
      'send_message',
      new Uint8Array(32) as never,
      { ok: true, status: 200, statusText: 'OK', headers: [], body } as never,
      5,
      'Canister trapped',
      'IC0503',
    );

  it('detects every shape the agent uses for a rejected update call', () => {
    expect(isCanisterReject(rejectedCall({ certificate: new Uint8Array(8) }))).toBe(true);
    expect(
      isCanisterReject(rejectedCall({ status: 'non_replicated_rejection', reject_code: 5, reject_message: 'x', error_code: 'IC0503' })),
    ).toBe(true);
    expect(isCanisterReject(new Error('Call was rejected:\n  Request ID: 00\n  Reject code: 5\n  Reject text: trapped\n'))).toBe(true);
  });

  it('does not treat transport failures or a done call as rejects', () => {
    expect(isCanisterReject(new Error('Failed to fetch'))).toBe(false);
    expect(isCanisterReject(new Error('Timed out'))).toBe(false);
    expect(isCanisterReject(new Error('Call was marked as done but we never saw the reply:\n  Request ID: 00\n'))).toBe(false);
    expect(isCanisterReject('Call was rejected:')).toBe(false);
  });
});

describe('clear_conversation result', () => {
  it('is ignored by clients built when it returned nothing', () => {
    // The canister now returns the new revision; candid lets an older client expecting no result skip it.
    const reply = IDL.encode([IDL.Nat64], [BigInt(3)]);
    expect(IDL.decode([], reply)).toEqual([]);
  });
});
