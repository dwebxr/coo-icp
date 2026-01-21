import { Actor, HttpAgent } from '@dfinity/agent';
import type { Principal } from '@dfinity/principal';
import type { ActorMethod, ActorSubclass } from '@dfinity/agent';

// Canister ID - mainnet deployment
export const canisterId: string = (import.meta as any).env?.CANISTER_ID_ELIZA_BACKEND ||
  (import.meta as any).env?.VITE_CANISTER_ID_ELIZA_BACKEND ||
  '4wfup-gqaaa-aaaas-qdqca-cai'; // Mainnet canister ID

export interface Message {
  role: string;
  content: string;
}

export interface Character {
  name: string;
  system_prompt: string;
  bio: string[];
  style: string[];
}

export type LlmProvider = { OnChain: null } | { OpenAI: null } | { Fallback: null };

export interface Config {
  llm_provider: LlmProvider;
  max_conversation_length: bigint;
  admin: Principal;
}

export type Result<T, E> = { Ok: T } | { Err: E };

export interface _SERVICE {
  chat: ActorMethod<[string], Result<string, string>>;
  update_character: ActorMethod<[Character], Result<null, string>>;
  get_character: ActorMethod<[], [] | [Character]>;
  set_llm_provider: ActorMethod<[LlmProvider], Result<null, string>>;
  get_config: ActorMethod<[], [] | [Config]>;
  get_conversation_history: ActorMethod<[], Message[]>;
  clear_conversation: ActorMethod<[], void>;
  get_conversation_count: ActorMethod<[], bigint>;
  store_encrypted_api_key: ActorMethod<[Uint8Array | number[]], Result<null, string>>;
  health: ActorMethod<[], string>;
  version: ActorMethod<[], string>;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const idlFactory = ({ IDL }: any) => {
  const Message = IDL.Record({
    role: IDL.Text,
    content: IDL.Text,
  });

  const Character = IDL.Record({
    name: IDL.Text,
    system_prompt: IDL.Text,
    bio: IDL.Vec(IDL.Text),
    style: IDL.Vec(IDL.Text),
  });

  const LlmProvider = IDL.Variant({
    OnChain: IDL.Null,
    OpenAI: IDL.Null,
    Fallback: IDL.Null,
  });

  const Config = IDL.Record({
    llm_provider: LlmProvider,
    max_conversation_length: IDL.Nat64,
    admin: IDL.Principal,
  });

  const Result = (ok: any, err: any) => IDL.Variant({ Ok: ok, Err: err });

  return IDL.Service({
    chat: IDL.Func([IDL.Text], [Result(IDL.Text, IDL.Text)], []),
    update_character: IDL.Func([Character], [Result(IDL.Null, IDL.Text)], []),
    get_character: IDL.Func([], [IDL.Opt(Character)], ['query']),
    set_llm_provider: IDL.Func([LlmProvider], [Result(IDL.Null, IDL.Text)], []),
    get_config: IDL.Func([], [IDL.Opt(Config)], ['query']),
    get_conversation_history: IDL.Func([], [IDL.Vec(Message)], ['query']),
    clear_conversation: IDL.Func([], [], []),
    get_conversation_count: IDL.Func([], [IDL.Nat64], ['query']),
    store_encrypted_api_key: IDL.Func([IDL.Vec(IDL.Nat8)], [Result(IDL.Null, IDL.Text)], []),
    health: IDL.Func([], [IDL.Text], ['query']),
    version: IDL.Func([], [IDL.Text], ['query']),
  });
};

export const createActor = (
  cid: string | Principal,
  options?: {
    agentOptions?: { host?: string };
    actorOptions?: Record<string, unknown>;
  }
): ActorSubclass<_SERVICE> => {
  const agent = HttpAgent.createSync(options?.agentOptions);

  // Fetch root key for local development
  const network = (import.meta as any).env?.DFX_NETWORK;
  if (network !== 'ic') {
    agent.fetchRootKey().catch(console.error);
  }

  return Actor.createActor(idlFactory, {
    agent,
    canisterId: cid,
    ...options?.actorOptions,
  });
};
