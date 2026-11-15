import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { createIcBackend, type Backend } from './backend';
import { trackViewport } from './lib/viewport';
import { network } from './network';
import './index.css';

async function createBackend(): Promise<Backend> {
  // Development only. import.meta.env.DEV is statically false in production builds, and so is
  // VITE_ALLOW_DEV_IDENTITY unless the build was made with it set to 1 (only `make e2e-local` does that, for a
  // bundle it deploys to the local replica and replaces afterwards). Where both are false, these branches and the
  // modules they import are removed from the bundle; `make build` and CI check that they are.
  //   ?mock=<scenario>        previews UI states without a canister or Internet Identity
  //   ?dev-identity[=<name>]  the real client against a local replica, signed in with a locally made identity —
  //                           and only on a local host, so even a bundle built with the switch cannot use it elsewhere
  const params = new URLSearchParams(window.location.search);
  if (import.meta.env.DEV) {
    const scenario = params.get('mock');
    if (scenario !== null) {
      const { createMockBackend } = await import('./dev/mockBackend');
      return createMockBackend(scenario);
    }
  }
  if ((import.meta.env.DEV || import.meta.env.VITE_ALLOW_DEV_IDENTITY === '1') && network.local) {
    const devIdentity = params.get('dev-identity');
    if (devIdentity !== null) {
      const { devAuth } = await import('./dev/devIdentity');
      return createIcBackend(devAuth(devIdentity || 'default'));
    }
  }
  return createIcBackend();
}

trackViewport();

createBackend().then((backend) => {
  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App backend={backend} />
    </React.StrictMode>,
  );
});
