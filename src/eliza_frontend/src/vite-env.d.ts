/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Ids of the locally deployed canisters, from the repository's .env (written by dfx). Absent without one. */
  readonly CANISTER_ID_ELIZA_BACKEND?: string;
  readonly CANISTER_ID_INTERNET_IDENTITY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
