import { describe, expect, it } from 'vitest';
import { byteLength, MAX_MESSAGE_BYTES } from './limits';

describe('byteLength', () => {
  it('counts ASCII as one byte per character', () => {
    expect(byteLength('hello')).toBe(5);
  });

  it('counts Japanese as three bytes per character', () => {
    expect(byteLength('こんにちは')).toBe(15);
  });

  it('counts emoji as four bytes (two UTF-16 units)', () => {
    expect(byteLength('😀')).toBe(4);
    expect('😀'.length).toBe(2);
  });

  it('is zero for empty text', () => {
    expect(byteLength('')).toBe(0);
  });

  it('puts the 4 000-byte limit at 1 333 Japanese characters', () => {
    expect(MAX_MESSAGE_BYTES).toBe(4000);
    expect(byteLength('あ'.repeat(1333))).toBeLessThanOrEqual(MAX_MESSAGE_BYTES);
    expect(byteLength('あ'.repeat(1334))).toBeGreaterThan(MAX_MESSAGE_BYTES);
  });
});
