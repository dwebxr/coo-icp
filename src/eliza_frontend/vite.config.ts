import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath, URL } from 'url';

export default defineConfig({
  plugins: [react()],
  // dfx writes the ids of the locally deployed canisters to .env at the repository root. Only CANISTER_ID_* is
  // exposed (that file also holds absolute local paths), and only what the source references is inlined; the
  // app uses these ids on a local host only (src/network.ts).
  envDir: fileURLToPath(new URL('../..', import.meta.url)),
  envPrefix: ['VITE_', 'CANISTER_ID_'],
  // The agent's CBOR decoder refers to Node's `global`.
  define: { global: 'globalThis' },
  server: {
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:4943',
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
