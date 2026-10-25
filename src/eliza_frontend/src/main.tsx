import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { createIcBackend, type Backend } from './backend';
import { trackViewport } from './lib/viewport';
import './index.css';

async function createBackend(): Promise<Backend> {
  // Development only: ?mock=<scenario> previews UI states without Internet Identity.
  // import.meta.env.DEV is statically false in production builds, so this branch and the mock are removed.
  if (import.meta.env.DEV) {
    const scenario = new URLSearchParams(window.location.search).get('mock');
    if (scenario !== null) {
      const { createMockBackend } = await import('./dev/mockBackend');
      return createMockBackend(scenario);
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
