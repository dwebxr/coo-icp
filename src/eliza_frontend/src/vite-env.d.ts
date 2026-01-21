/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly CANISTER_ID_ELIZA_BACKEND: string;
  readonly CANISTER_ID_INTERNET_IDENTITY: string;
  readonly DFX_NETWORK: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

declare namespace NodeJS {
  interface ProcessEnv {
    CANISTER_ID_ELIZA_BACKEND?: string;
    CANISTER_ID_INTERNET_IDENTITY?: string;
    ELIZA_BACKEND_CANISTER_ID?: string;
    DFX_NETWORK?: string;
  }
}
