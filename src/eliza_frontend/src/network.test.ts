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
      const network = networkFor(host);
      expect(network.local, host).toBe(false);
      expect(network.backendCanisterId).toBe('4wfup-gqaaa-aaaas-qdqca-cai');
      expect(network.host).toBe('https://icp-api.io');
      expect(network.identityProvider).toBe('https://identity.internetcomputer.org');
    }
  });

  it('uses the local replica on a local host', () => {
    const network = networkFor('localhost');
    expect(network.local).toBe(true);
    expect(network.host).toBeUndefined();
    expect(network.identityProvider).toMatch(/^http:\/\/[a-z0-9-]+\.localhost:4943$/);
  });
});
