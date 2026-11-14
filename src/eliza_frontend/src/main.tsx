import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { createIcBackend, type Backend } from './backend';
import { trackViewport } from './lib/viewport';
import './index.css';

async function createBackend(): Promise<Backend> {
  // Development only. import.meta.env.DEV is statically false in production builds, so this branch and the
  // modules it imports are removed from them.
  //   ?mock=<scenario>        previews UI states without a canister or Internet Identity
  //   ?dev-identity[=<name>]  the real client against a local replica, signed in with a locally made identity
  if (import.meta.env.DEV) {
    const params = new URLSearchParams(window.location.search);
    const scenario = params.get('mock');
    if (scenario !== null) {
      const { createMockBackend } = await import('./dev/mockBackend');
      return createMockBackend(scenario);
    }
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
