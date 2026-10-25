import type { Principal } from '@dfinity/principal';
import type { ActorMethod } from '@dfinity/agent';

export const canisterId: string = (import.meta as any).env?.CANISTER_ID_ELIZA_BACKEND ||
  (import.meta as any).env?.VITE_CANISTER_ID_ELIZA_BACKEND ||
  '4wfup-gqaaa-aaaas-qdqca-cai';

export interface Message {
  role: string;
  content: string;
  request_id: [] | [string];
}

export interface Conversation {
  messages: Message[];
  revision: bigint;
}

export interface Sent {
  reply: string;
  revision: bigint;
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

export type ChatError =
  | { Busy: null }
  | { InProgress: null }
  | { Conflict: null }
  | { Cleared: null }
  | { Invalid: string }
  | { Failed: string };

export type MessageStatus = { Unknown: null } | { Pending: null } | { Done: Sent };

export interface CanisterMetrics {
  version: string;
  conversations: bigint;
  stable_memory_pages: bigint;
  heap_memory_bytes: bigint;
}

export interface _SERVICE {
  chat: ActorMethod<[string], Result<string, string>>;
  send_message: ActorMethod<[string, bigint, string], Result<Sent, ChatError>>;
  get_conversation: ActorMethod<[], Conversation>;
  get_message_status: ActorMethod<[string], MessageStatus>;
  update_character: ActorMethod<[Character], Result<null, string>>;
  get_character: ActorMethod<[], [] | [Character]>;
  set_llm_provider: ActorMethod<[LlmProvider], Result<null, string>>;
  get_config: ActorMethod<[], [] | [Config]>;
  get_conversation_history: ActorMethod<[], Message[]>;
  clear_conversation: ActorMethod<[], bigint>;
  get_conversation_count: ActorMethod<[], bigint>;
  store_encrypted_api_key: ActorMethod<[Uint8Array | number[]], Result<null, string>>;
  health: ActorMethod<[], string>;
  version: ActorMethod<[], string>;
  get_metrics: ActorMethod<[], CanisterMetrics>;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export const idlFactory = ({ IDL }: any) => {
  const Message = IDL.Record({
    role: IDL.Text,
    content: IDL.Text,
    request_id: IDL.Opt(IDL.Text),
  });

  const Conversation = IDL.Record({
    messages: IDL.Vec(Message),
    revision: IDL.Nat64,
  });

  const Sent = IDL.Record({
    reply: IDL.Text,
    revision: IDL.Nat64,
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

  const ChatError = IDL.Variant({
    Busy: IDL.Null,
    InProgress: IDL.Null,
    Conflict: IDL.Null,
    Cleared: IDL.Null,
    Invalid: IDL.Text,
    Failed: IDL.Text,
  });

  const MessageStatus = IDL.Variant({
    Unknown: IDL.Null,
    Pending: IDL.Null,
    Done: Sent,
  });

  const CanisterMetrics = IDL.Record({
    version: IDL.Text,
    conversations: IDL.Nat64,
    stable_memory_pages: IDL.Nat64,
    heap_memory_bytes: IDL.Nat64,
  });

  return IDL.Service({
    chat: IDL.Func([IDL.Text], [Result(IDL.Text, IDL.Text)], []),
    send_message: IDL.Func([IDL.Text, IDL.Nat64, IDL.Text], [Result(Sent, ChatError)], []),
    get_conversation: IDL.Func([], [Conversation], ['query']),
    get_message_status: IDL.Func([IDL.Text], [MessageStatus], ['query']),
    update_character: IDL.Func([Character], [Result(IDL.Null, IDL.Text)], []),
    get_character: IDL.Func([], [IDL.Opt(Character)], ['query']),
    set_llm_provider: IDL.Func([LlmProvider], [Result(IDL.Null, IDL.Text)], []),
    get_config: IDL.Func([], [IDL.Opt(Config)], ['query']),
    get_conversation_history: IDL.Func([], [IDL.Vec(Message)], ['query']),
    clear_conversation: IDL.Func([], [IDL.Nat64], []),
    get_conversation_count: IDL.Func([], [IDL.Nat64], ['query']),
    store_encrypted_api_key: IDL.Func([IDL.Vec(IDL.Nat8)], [Result(IDL.Null, IDL.Text)], []),
    health: IDL.Func([], [IDL.Text], ['query']),
    version: IDL.Func([], [IDL.Text], ['query']),
    get_metrics: IDL.Func([], [CanisterMetrics], ['query']),
  });
};
