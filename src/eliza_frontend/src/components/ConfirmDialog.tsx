import { useEffect, useRef, useState } from 'react';

/**
 * done: closes the dialog. failed: it did not happen. unknown: not confirmed, so it may have happened. pending: not
 * settled yet; the promise resolves with one of the other three, and the dialog follows (closes, says it failed, or
 * keeps saying it is not confirmed) if it is still open.
 */
export type ConfirmSettled = 'done' | 'failed' | 'unknown';
export type ConfirmOutcome = ConfirmSettled | { pending: Promise<ConfirmSettled> };

interface Props {
  open: boolean;
  title: string;
  body: string;
  confirmLabel: string;
  /** Shown inside the dialog when it did not happen. */
  failedText: string;
  /** Shown inside the dialog while it is not settled or not confirmed. */
  pendingText: string;
  /** False while confirming cannot be tried again (an earlier attempt is still out). */
  canConfirm?: boolean;
  onConfirm: () => Promise<ConfirmOutcome>;
  onClose: () => void;
}

/** Modal confirmation built on <dialog>, which provides focus trapping, Esc and inert background. */
export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  failedText,
  pendingText,
  canConfirm = true,
  onConfirm,
  onClose,
}: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<'failed' | 'pending' | null>(null);
  // Bumped when the dialog opens and on every confirmation, so a late answer only changes the dialog it belongs to.
  const attempt = useRef(0);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      attempt.current += 1;
      setOutcome(null);
      dialog.showModal();
    }
    if (!open && dialog.open) dialog.close();
  }, [open]);

  const confirm = async () => {
    const mine = ++attempt.current;
    setBusy(true);
    setOutcome(null);
    try {
      const result = await onConfirm();
      const show = (settled: ConfirmSettled) => {
        if (settled === 'done') onClose();
        else setOutcome(settled === 'failed' ? 'failed' : 'pending');
      };
      if (typeof result === 'string') show(result);
      else {
        setOutcome('pending');
        void result.pending.then((final) => {
          if (mine === attempt.current && ref.current?.open) show(final);
        });
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-labelledby="dialog-title"
      aria-describedby="dialog-body"
      onClose={onClose}
      onCancel={(e) => {
        if (busy) e.preventDefault();
      }}
      onClick={(e) => {
        // Clicking the backdrop (the dialog element itself, outside its content) cancels.
        if (e.target === ref.current && !busy) onClose();
      }}
    >
      <div className="dialog-content">
        <h2 id="dialog-title">{title}</h2>
        <p id="dialog-body">{body}</p>
        {outcome === 'failed' && (
          <p className="dialog-error" role="alert">
            {failedText}
          </p>
        )}
        {/* Not an error: the outcome is not known yet. */}
        {outcome === 'pending' && (
          <p className="dialog-note" role="status">
            {pendingText}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="btn btn-ghost" onClick={onClose} disabled={busy} autoFocus>
            {outcome === 'pending' ? 'Close' : 'Cancel'}
          </button>
          <button type="button" className="btn btn-danger" onClick={confirm} disabled={busy || !canConfirm}>
            {busy ? <span className="spinner" aria-hidden="true" /> : null}
            {confirmLabel}
          </button>
        </div>
      </div>
    </dialog>
  );
}
