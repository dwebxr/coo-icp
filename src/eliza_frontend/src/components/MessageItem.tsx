import { memo } from 'react';
import type { ChatMessage } from '../useCoo';
import { CopyButton } from './CopyButton';
import { AlertIcon, RetryIcon } from './icons';
import { CooAvatar } from './CooAvatar';
import { Markdown } from './Markdown';

interface Props {
  message: ChatMessage;
  /** The newest reply keeps its actions visible. */
  latest?: boolean;
  /** Part of the loaded history: shown without the enter animation. */
  initial?: boolean;
  canRetry: boolean;
  onRetry: (id: string) => void;
}

export const MessageItem = memo(function MessageItem({ message, latest = false, initial = false, canRetry, onRetry }: Props) {
  const motion = initial ? ' is-initial' : '';
  if (message.role === 'user') {
    return (
      <div className={`msg msg-user${motion}`}>
        <div className={message.error ? 'bubble failed' : 'bubble'}>{message.content}</div>
        {message.checking && (
          <div className="msg-status" role="status">
            <span className="spinner sm" aria-hidden="true" />
            Checking for a reply…
          </div>
        )}
        {message.error && (
          <div className="msg-failed" role="alert">
            <AlertIcon size={15} />
            <span className="msg-failed-text" title={message.error}>
              {/* A lost response may still have been saved, so it is not called "not sent". */}
              {message.unconfirmed ? 'No reply' : 'Not sent'}
              <span className="msg-failed-reason"> · {message.error}</span>
            </span>
            <button type="button" className="text-btn" onClick={() => onRetry(message.id)} disabled={!canRetry}>
              <RetryIcon size={14} />
              Retry
            </button>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className={`msg msg-assistant${latest ? ' is-latest' : ''}${motion}`}>
      <CooAvatar size={28} className="avatar" />
      <div className="msg-body">
        <Markdown text={message.content} />
        <div className="msg-actions">
          <CopyButton text={message.content} label="Copy reply" />
        </div>
      </div>
    </div>
  );
});

export function Thinking({ slow }: { slow: boolean }) {
  return (
    <div className="msg msg-assistant thinking">
      <CooAvatar size={28} className="avatar" />
      <div className="msg-body">
        <span className="dots" aria-hidden="true">
          <i />
          <i />
          <i />
        </span>
        <span className="sr-only" role="status">
          Coo is typing
        </span>
        {slow && <span className="thinking-hint">On-chain replies can take a moment…</span>}
      </div>
    </div>
  );
}
