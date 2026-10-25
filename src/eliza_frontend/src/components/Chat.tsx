import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { Coo } from '../useCoo';
import { AccountMenu } from './AccountMenu';
import { Composer } from './Composer';
import { ConfirmDialog } from './ConfirmDialog';
import { AlertIcon, ArrowDownIcon, RetryIcon } from './icons';
import { CooAvatar } from './CooAvatar';
import { MessageItem, Thinking } from './MessageItem';

const SUGGESTIONS = [
  'What is the Internet Computer?',
  'How do you run on-chain?',
  'Explain elizaOS in simple terms',
  'Give me a Web3 project idea',
];

const NEAR_BOTTOM_PX = 96;
const SLOW_REPLY_MS = 8000;

const prefersReducedMotion = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

export function Chat({ coo }: { coo: Coo }) {
  const { messages, history, pending, busy, principal } = coo;
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const [atBottom, setAtBottom] = useState(true);
  const [scrolled, setScrolled] = useState(false);
  const [confirmClear, setConfirmClear] = useState(false);
  const [slow, setSlow] = useState(false);

  const scrollToBottom = useCallback((smooth: boolean) => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior: smooth && !prefersReducedMotion() ? 'smooth' : 'auto' });
  }, []);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const bottom = el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM_PX;
    stickToBottom.current = bottom;
    setAtBottom(bottom);
    setScrolled(el.scrollTop > 4);
  };

  // Jump to the latest message once history is in.
  useLayoutEffect(() => {
    if (history === 'loaded') {
      stickToBottom.current = true;
      scrollToBottom(false);
    }
  }, [history, scrollToBottom]);

  // Stay pinned to the bottom while content or the composer changes size, unless the reader scrolled up.
  useEffect(() => {
    const el = scrollRef.current;
    const content = el?.firstElementChild;
    if (!el || !content) return;
    const observer = new ResizeObserver(() => {
      if (stickToBottom.current) el.scrollTop = el.scrollHeight;
    });
    observer.observe(el);
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  // Sending always brings the conversation back to the latest message.
  const last = messages[messages.length - 1];
  useEffect(() => {
    if (last?.role === 'user' && !last.error) {
      stickToBottom.current = true;
      scrollToBottom(false);
    }
  }, [last, scrollToBottom]);

  useEffect(() => {
    setSlow(false);
    if (!pending) return;
    const timer = setTimeout(() => setSlow(true), SLOW_REPLY_MS);
    return () => clearTimeout(timer);
  }, [pending]);

  const showEmpty = history === 'loaded' && messages.length === 0 && !pending;

  // Ids that arrived with the history load; only later messages animate in.
  const initialIds = useRef<Set<string> | null>(null);
  if (history !== 'loaded') initialIds.current = null;
  else initialIds.current ??= new Set(messages.map((m) => m.id));

  return (
    <div className="screen chat">
      <header className={scrolled ? 'topbar is-scrolled' : 'topbar'}>
        <div className="brand">
          <CooAvatar size={28} />
          <span>Coo</span>
        </div>
        {principal && (
          <AccountMenu
            principal={principal}
            canClear={messages.length > 0 && !busy}
            onClear={() => setConfirmClear(true)}
            onSignOut={() => void coo.signOut()}
          />
        )}
      </header>

      <main className="thread-scroll" ref={scrollRef} onScroll={onScroll} data-scroll>
        <div className="thread" role="log" aria-label="Conversation" aria-busy={history === 'loading'}>
          {history === 'loading' && (
            <div className="skeleton" aria-hidden="true">
              <span className="sk sk-user" />
              <span className="sk sk-line" />
              <span className="sk sk-line short" />
            </div>
          )}

          {history === 'error' && (
            <div className="state-card" role="alert">
              <AlertIcon size={20} />
              <p>Couldn't load your conversation.</p>
              <button type="button" className="btn btn-ghost btn-sm" onClick={() => void coo.reloadHistory()}>
                <RetryIcon size={15} />
                Retry
              </button>
            </div>
          )}

          {showEmpty && (
            <div className="empty">
              <CooAvatar size={64} />
              <h2>How can I help?</h2>
              <div className="suggestions">
                {SUGGESTIONS.map((text) => (
                  <button key={text} type="button" className="chip" onClick={() => coo.send(text)} disabled={!coo.canSend}>
                    {text}
                  </button>
                ))}
              </div>
            </div>
          )}

          {messages.map((m) => (
            <MessageItem
              key={m.id}
              message={m}
              latest={m === last && !pending}
              initial={initialIds.current?.has(m.id)}
              canRetry={!busy}
              onRetry={coo.retry}
            />
          ))}

          {pending && <Thinking slow={slow} />}
        </div>
      </main>

      <footer className="dock">
        {!atBottom && (
          <button
            type="button"
            className="jump-btn"
            onClick={() => {
              stickToBottom.current = true;
              scrollToBottom(true);
            }}
            aria-label="Jump to latest"
          >
            <ArrowDownIcon size={18} />
          </button>
        )}
        <Composer value={coo.draft} onChange={coo.setDraft} onSend={coo.send} canSend={coo.canSend} />
        <p className="composer-note">Coo can make mistakes.</p>
      </footer>

      <ConfirmDialog
        open={confirmClear}
        title="Clear conversation?"
        body="This permanently deletes your messages with Coo."
        confirmLabel="Clear"
        errorText="Couldn't clear the conversation. Try again."
        onConfirm={coo.clear}
        onClose={() => setConfirmClear(false)}
      />
    </div>
  );
}
