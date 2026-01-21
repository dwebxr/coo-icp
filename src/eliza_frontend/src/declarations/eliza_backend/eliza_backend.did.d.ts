import type { Principal } from '@dfinity/principal';
import type { ActorMethod } from '@dfinity/agent';

export interface Message {
  role: string;
  content: string;
}

export interface Character {
  name: string;
  system_prompt: string;
  bio: Array<string>;
  style: Array<string>;
}

export type LlmProvider = { OnChain: null } | { OpenAI: null };

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
  get_conversation_history: ActorMethod<[], Array<Message>>;
  clear_conversation: ActorMethod<[], void>;
  get_conversation_count: ActorMethod<[], bigint>;
  store_encrypted_api_key: ActorMethod<[Uint8Array], Result<null, string>>;
  health: ActorMethod<[], string>;
  version: ActorMethod<[], string>;
}
