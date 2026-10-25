import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { CopyButton } from './CopyButton';
import { LogOutIcon, TrashIcon, UserIcon } from './icons';

interface Props {
  principal: string;
  canClear: boolean;
  onClear: () => void;
  onSignOut: () => void;
}

export function shortPrincipal(principal: string) {
  const parts = principal.split('-');
  return parts.length > 3 ? `${parts[0]}-…-${parts[parts.length - 1]}` : principal;
}

export function AccountMenu({ principal, canClear, onClear, onSignOut }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    const onPointer = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('pointerdown', onPointer);
    menuRef.current?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])')?.focus();
    return () => document.removeEventListener('pointerdown', onPointer);
  }, [open]);

  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) buttonRef.current?.focus();
  };

  const onMenuKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Home' && e.key !== 'End') return;
    e.preventDefault();
    const items = Array.from(menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? []);
    if (!items.length) return;
    const index = items.indexOf(document.activeElement as HTMLElement);
    const next =
      e.key === 'Home' ? 0
      : e.key === 'End' ? items.length - 1
      : e.key === 'ArrowDown' ? (index + 1) % items.length
      : (index - 1 + items.length) % items.length;
    items[next].focus();
  };

  return (
    <div
      className="account"
      ref={rootRef}
      onBlur={(e) => {
        // Close when keyboard focus leaves the menu entirely.
        if (open && e.relatedTarget && !rootRef.current?.contains(e.relatedTarget as Node)) setOpen(false);
      }}
    >
      <button
        ref={buttonRef}
        type="button"
        className="avatar-btn"
        aria-label="Account"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="user-avatar">
          <UserIcon size={18} />
        </span>
      </button>

      {open && (
        <div className="menu" id={menuId} ref={menuRef} onKeyDown={onMenuKeyDown}>
          <div className="menu-identity">
            <span className="user-avatar lg">
              <UserIcon size={20} />
            </span>
            <div className="menu-identity-text">
              <span className="menu-label">Signed in</span>
              <span className="mono" title={principal}>
                {shortPrincipal(principal)}
              </span>
            </div>
            <CopyButton text={principal} label="Copy principal ID" />
          </div>
          <div className="menu-sep" />
          <div role="menu" aria-label="Account actions">
            <button
              type="button"
              role="menuitem"
              className="menu-item"
              disabled={!canClear}
              onClick={() => {
                // Focus the Account button first so the dialog returns focus there when it closes.
                close();
                onClear();
              }}
            >
              <TrashIcon size={17} />
              Clear conversation
            </button>
            <button
              type="button"
              role="menuitem"
              className="menu-item"
              onClick={() => {
                close(false);
                onSignOut();
              }}
            >
              <LogOutIcon size={17} />
              Sign out
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
