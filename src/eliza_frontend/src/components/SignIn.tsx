import { useEffect, useState } from 'react';
import type { Notice } from '../useCoo';
import { AlertIcon, GitHubIcon, InfoIcon, KeyIcon } from './icons';
import { CooAvatar } from './CooAvatar';

interface Props {
  /** Session check in progress: show the brand only, in the same position, so nothing jumps. */
  checking?: boolean;
  signingIn: boolean;
  ready: boolean;
  notice: Notice | null;
  onSignIn: () => void;
  onCancel: () => void;
}

const REPO_URL = 'https://github.com/dwebxr/coo-icp';

export function SignIn({ checking = false, signingIn, ready, notice, onSignIn, onCancel }: Props) {
  // If the identity window never appeared (pop-up blocked, opened behind), offer a way out.
  const [stalled, setStalled] = useState(false);
  useEffect(() => {
    setStalled(false);
    if (!signingIn) return;
    const timer = setTimeout(() => setStalled(true), 3000);
    return () => clearTimeout(timer);
  }, [signingIn]);

  return (
    <div className="screen signin" aria-busy={checking || undefined}>
      <main className="signin-main">
        <CooAvatar size={96} className="signin-logo" />
        <h1>Coo</h1>
        <p className="signin-tagline">Your on-chain AI agent.</p>

        <div className={checking ? 'signin-actions is-hidden' : 'signin-actions'} aria-hidden={checking || undefined}>
          {notice && (
            <p className={`notice notice-${notice.tone}`} role={notice.tone === 'error' ? 'alert' : 'status'}>
              {notice.tone === 'error' ? <AlertIcon size={16} /> : <InfoIcon size={16} />}
              {notice.text}
            </p>
          )}

          <button type="button" className="btn btn-primary btn-lg" onClick={onSignIn} disabled={signingIn || !ready}>
            {signingIn ? <span className="spinner" aria-hidden="true" /> : <KeyIcon size={18} />}
            {signingIn ? 'Waiting for Internet Identity…' : 'Sign in with Internet Identity'}
          </button>

          <p className="signin-hint" aria-live="polite">
            {signingIn ? (
              stalled ? (
                <>
                  Didn't see a window? Allow pop-ups and try again.{' '}
                  <button type="button" className="link-btn" onClick={onCancel}>
                    Cancel
                  </button>
                </>
              ) : (
                'Finish signing in in the new window.'
              )
            ) : (
              'No password needed.'
            )}
          </p>
        </div>
      </main>

      <footer className={checking ? 'signin-footer is-hidden' : 'signin-footer'}>
        <a href={REPO_URL} target="_blank" rel="noopener noreferrer">
          <GitHubIcon size={15} />
          Open source
        </a>
      </footer>
    </div>
  );
}
