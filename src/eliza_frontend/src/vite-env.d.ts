/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly CANISTER_ID_ELIZA_BACKEND: string;
  readonly CANISTER_ID_INTERNET_IDENTITY: string;
  readonly DFX_NETWORK: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
