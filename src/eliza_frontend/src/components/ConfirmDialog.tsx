import { useEffect, useRef, useState } from 'react';

interface Props {
  open: boolean;
  title: string;
  body: string;
  confirmLabel: string;
  /** Shown inside the dialog when onConfirm resolves false. */
  errorText: string;
  onConfirm: () => Promise<boolean>;
  onClose: () => void;
}

/** Modal confirmation built on <dialog>, which provides focus trapping, Esc and inert background. */
export function ConfirmDialog({ open, title, body, confirmLabel, errorText, onConfirm, onClose }: Props) {
  const ref = useRef<HTMLDialogElement>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      setFailed(false);
      dialog.showModal();
    }
    if (!open && dialog.open) dialog.close();
  }, [open]);

  const confirm = async () => {
    setBusy(true);
    setFailed(false);
    try {
      if (await onConfirm()) onClose();
      else setFailed(true);
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
        {failed && (
          <p className="dialog-error" role="alert">
            {errorText}
          </p>
        )}
        <div className="dialog-actions">
          <button type="button" className="btn btn-ghost" onClick={onClose} disabled={busy} autoFocus>
            Cancel
          </button>
          <button type="button" className="btn btn-danger" onClick={confirm} disabled={busy}>
            {busy ? <span className="spinner" aria-hidden="true" /> : null}
            {confirmLabel}
          </button>
        </div>
      </div>
    </dialog>
  );
}
