import { useEffect, useLayoutEffect, useRef, type FormEvent, type KeyboardEvent } from 'react';
import { ArrowUpIcon } from './icons';

interface Props {
  value: string;
  onChange: (value: string) => void;
  onSend: (text: string) => boolean;
  canSend: boolean;
}

const MAX_HEIGHT = 200;
const isTouch = () => window.matchMedia('(pointer: coarse)').matches;

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

  const ready = canSend && value.trim().length > 0;

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
  );
}
