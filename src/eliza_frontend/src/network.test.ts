import { describe, expect, it } from 'vitest';
import { isLocalHost, networkFor } from './network';

describe('network', () => {
  it('treats only this machine as local', () => {
    for (const host of ['localhost', 'LOCALHOST', '127.0.0.1', '[::1]', 'uxrrr-q7777-77774-qaaaq-cai.localhost']) {
      expect(isLocalHost(host), host).toBe(true);
    }
    for (const host of [
      '4res3-liaaa-aaaas-qdqcq-cai.icp0.io',
      '4res3-liaaa-aaaas-qdqcq-cai.ic0.app',
      '4res3-liaaa-aaaas-qdqcq-cai.raw.icp0.io',
      'coo.example.com',
      'localhost.example.com',
      'notlocalhost',
      '127.0.0.1.example.com',
      '192.168.1.10',
      '',
    ]) {
      expect(isLocalHost(host), host).toBe(false);
    }
  });

  it('gives every other host mainnet settings', () => {
    for (const host of ['4res3-liaaa-aaaas-qdqcq-cai.icp0.io', 'coo.example.com', '192.168.1.10']) {
      const network = networkFor(host, `https://${host}`);
      expect(network.local, host).toBe(false);
      expect(network.backendCanisterId).toBe('4wfup-gqaaa-aaaas-qdqca-cai');
      expect(network.host).toBe('https://icp-api.io');
      expect(network.identityProvider).toBe('https://identity.internetcomputer.org');
    }
  });

  it('uses the local replica on a local host', () => {
    const network = networkFor('localhost', 'http://localhost:5173');
    expect(network.local).toBe(true);
    expect(network.identityProvider).toMatch(/^http:\/\/[a-z0-9-]+\.localhost:4943$/);
  });

  // Review finding 11: the agent's own guess sends the calls of a page on [::1] to mainnet.
  it("sends a local page's calls to the page's own origin, never to the agent's guess", () => {
    for (const [hostname, origin] of [
      ['localhost', 'http://localhost:5173'],
      ['127.0.0.1', 'http://127.0.0.1:4943'],
      ['[::1]', 'http://[::1]:4943'],
      ['uzt4z-lp777-77774-qaabq-cai.localhost', 'http://uzt4z-lp777-77774-qaabq-cai.localhost:4943'],
    ]) {
      const network = networkFor(hostname, origin);
      expect(network.local, hostname).toBe(true);
      expect(network.host, hostname).toBe(origin);
    }
  });
});
