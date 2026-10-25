import { useEffect, useState } from 'react';
import { CheckIcon, CopyIcon } from './icons';

async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Fallback for browsers that block the async clipboard API.
    const area = document.createElement('textarea');
    area.value = text;
    area.setAttribute('readonly', '');
    area.style.position = 'fixed';
    area.style.opacity = '0';
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand('copy');
    area.remove();
    return ok;
  }
}

export function CopyButton({ text, label = 'Copy', compact = false }: { text: string; label?: string; compact?: boolean }) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1600);
    return () => clearTimeout(timer);
  }, [copied]);

  return (
    <button
      type="button"
      className={compact ? 'copy-btn compact' : 'icon-btn'}
      onClick={async () => setCopied(await writeClipboard(text))}
      aria-label={copied ? 'Copied' : label}
      title={copied ? 'Copied' : label}
    >
      {copied ? <CheckIcon size={compact ? 14 : 16} /> : <CopyIcon size={compact ? 14 : 16} />}
      {compact && <span>{copied ? 'Copied' : 'Copy'}</span>}
    </button>
  );
}
