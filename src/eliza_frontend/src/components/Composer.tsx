import { useEffect, useLayoutEffect, useRef, type FormEvent, type KeyboardEvent } from 'react';
import { byteLength, MAX_MESSAGE_BYTES } from '../lib/limits';
import { ArrowUpIcon } from './icons';

interface Props {
  value: string;
  onChange: (value: string) => void;
  onSend: (text: string) => boolean;
  canSend: boolean;
}

const MAX_HEIGHT = 200;
const isTouch = () => window.matchMedia('(pointer: coarse)').matches;
// The counter appears once the message is close to the limit.
const COUNTER_FROM = MAX_MESSAGE_BYTES * 0.9;
const grouped = (n: number) => String(n).replace(/\B(?=(\d{3})+(?!\d))/g, '\u00a0');

export function Composer({ value, onChange, onSend, canSend }: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false);
  const compositionEndedAt = useRef(0);

  // Grow with the content up to MAX_HEIGHT, then scroll.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, MAX_HEIGHT)}px`;
  }, [value]);

  useEffect(() => {
    if (!isTouch()) ref.current?.focus();
  }, []);

  // The app sends the trimmed text, so that is what the canister measures.
  const trimmed = value.trim();
  const bytes = byteLength(trimmed);
  const over = bytes > MAX_MESSAGE_BYTES;
  const ready = canSend && trimmed.length > 0 && !over;

  const submit = () => {
    if (!ready) return;
    if (onSend(value) && !isTouch()) ref.current?.focus();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== 'Enter' || e.shiftKey || e.altKey) return;
    // Enter that confirms an IME conversion (Japanese, Chinese, …) must not send. Safari reports it after
    // compositionend with keyCode 229, so all three signals are checked.
    const ime =
      composing.current ||
      e.nativeEvent.isComposing ||
      e.keyCode === 229 ||
      performance.now() - compositionEndedAt.current < 100;
    if (ime) return;
    // On touch keyboards Return inserts a new line; the send button sends.
    if (isTouch()) return;
    e.preventDefault();
    submit();
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    submit();
  };

  return (
    <>
      <form className="composer" onSubmit={onSubmit}>
        <label htmlFor="composer-input" className="sr-only">
          Message Coo
        </label>
        <textarea
          id="composer-input"
          ref={ref}
          rows={1}
          value={value}
          placeholder="Message Coo"
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKeyDown}
          onCompositionStart={() => {
            composing.current = true;
          }}
          onCompositionEnd={() => {
            composing.current = false;
            compositionEndedAt.current = performance.now();
          }}
          autoComplete="off"
          spellCheck
        />
        <button type="submit" className="send-btn" disabled={!ready} aria-label="Send message" title="Send">
          <ArrowUpIcon size={18} />
        </button>
      </form>
      {bytes >= COUNTER_FROM && (
        <div className={over ? 'composer-count over' : 'composer-count'} aria-hidden="true">
          {grouped(bytes)} / {grouped(MAX_MESSAGE_BYTES)} bytes
        </div>
      )}
      {/* Always mounted so the change to "too long" is announced; below the limit it stays silent. */}
      <div className="sr-only" role="status">
        {over ? `Message is too long: ${grouped(bytes)} of ${grouped(MAX_MESSAGE_BYTES)} bytes. Shorten it to send.` : ''}
      </div>
    </>
  );
}
