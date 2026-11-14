/** The canister refuses a message longer than this many UTF-8 bytes (not characters). */
export const MAX_MESSAGE_BYTES = 4000;

const encoder = new TextEncoder();

/** Length of the text in UTF-8 bytes: Japanese is 3 bytes per character, most emoji 4. */
export function byteLength(text: string): number {
  return encoder.encode(text).length;
}
