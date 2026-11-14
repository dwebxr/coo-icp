import { useEffect, useState } from 'react';
import { AlertIcon, CheckIcon, CopyIcon } from './icons';

// Always resolves to a boolean: the fallback can throw (blocked or unsupported), and the textarea must not be left behind.
async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Fallback for browsers that block the async clipboard API.
    const area = document.createElement('textarea');
    try {
      area.value = text;
      area.setAttribute('readonly', '');
      area.style.position = 'fixed';
      area.style.opacity = '0';
      document.body.appendChild(area);
      area.select();
      return document.execCommand('copy');
    } catch {
      return false;
    } finally {
      area.remove();
    }
  }
}

export function CopyButton({ text, label = 'Copy', compact = false }: { text: string; label?: string; compact?: boolean }) {
  const [result, setResult] = useState<'copied' | 'failed' | null>(null);

  useEffect(() => {
    if (!result) return;
    const timer = setTimeout(() => setResult(null), 1600);
    return () => clearTimeout(timer);
  }, [result]);

  const name = result === 'copied' ? 'Copied' : result === 'failed' ? 'Copy failed' : label;
  const size = compact ? 14 : 16;

  return (
    <button
      type="button"
      className={compact ? 'copy-btn compact' : 'icon-btn'}
      onClick={async () => setResult((await writeClipboard(text)) ? 'copied' : 'failed')}
      aria-label={name}
      title={name}
    >
      {result === 'copied' ? <CheckIcon size={size} /> : result === 'failed' ? <AlertIcon size={size} /> : <CopyIcon size={size} />}
      {compact && <span>{result === 'copied' ? 'Copied' : result === 'failed' ? 'Failed' : 'Copy'}</span>}
    </button>
  );
}
