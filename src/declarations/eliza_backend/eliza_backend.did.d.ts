import type { Principal } from '@dfinity/principal';
import type { ActorMethod } from '@dfinity/agent';
import type { IDL } from '@dfinity/candid';

export interface CanisterMetrics {
  'stable_memory_pages' : bigint,
  'heap_memory_bytes' : bigint,
  'version' : string,
  'conversations' : bigint,
}
export interface Character {
  'bio' : Array<string>,
  'system_prompt' : string,
  'name' : string,
  'style' : Array<string>,
}
export interface Config {
  'admin' : Principal,
  'llm_provider' : LlmProvider,
  'max_conversation_length' : bigint,
}
export interface HttpHeader { 'value' : string, 'name' : string }
export interface HttpResponse {
  'status' : bigint,
  'body' : Uint8Array | number[],
  'headers' : Array<HttpHeader>,
}
export type LlmProvider = { 'Fallback' : null } |
  { 'OpenAI' : null } |
  { 'OnChain' : null };
export interface Message { 'content' : string, 'role' : string }
export type Result = { 'Ok' : string } |
  { 'Err' : string };
export type Result_1 = { 'Ok' : null } |
  { 'Err' : string };
export interface TransformArgs {
  'context' : Uint8Array | number[],
  'response' : HttpResponse,
}
export interface _SERVICE {
  'chat' : ActorMethod<[string], Result>,
  'clear_conversation' : ActorMethod<[], undefined>,
  'get_character' : ActorMethod<[], [] | [Character]>,
  'get_config' : ActorMethod<[], [] | [Config]>,
  'get_conversation_count' : ActorMethod<[], bigint>,
  'get_conversation_history' : ActorMethod<[], Array<Message>>,
  'get_metrics' : ActorMethod<[], CanisterMetrics>,
  'health' : ActorMethod<[], string>,
  /**
   * Transfer admin rights to a new principal
   */
  'set_admin' : ActorMethod<[Principal], Result_1>,
  'set_llm_provider' : ActorMethod<[LlmProvider], Result_1>,
  'store_encrypted_api_key' : ActorMethod<[Uint8Array | number[]], Result_1>,
  'transform_openai_response' : ActorMethod<[TransformArgs], HttpResponse>,
  'update_character' : ActorMethod<[Character], Result_1>,
  'version' : ActorMethod<[], string>,
}
export declare const idlFactory: IDL.InterfaceFactory;
export declare const init: (args: { IDL: typeof IDL }) => IDL.Type[];
