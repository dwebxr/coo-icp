import type { Principal } from '@dfinity/principal';
import type { ActorMethod } from '@dfinity/agent';
import type { IDL } from '@dfinity/candid';

/**
 * Who Coo is. All of it — name, system prompt, bio and style — makes the system message of every model request,
 * which may be at most 3 000 bytes as a whole (`update_character` refuses a longer one).
 */
export interface Character {
  'bio' : Array<string>,
  'system_prompt' : string,
  'name' : string,
  'style' : Array<string>,
}
/**
 * Why `send_message` saved nothing.
 */
export type ChatError = {
    /**
     * The request itself is refused, whenever it is sent: anonymous caller, malformed request id, a request id
     * already used for another message, or an empty or too long message.
     */
    'Invalid' : string
  } |
  {
    /**
     * The model did not answer, or not in time.
     */
    'Failed' : string
  } |
  {
    /**
     * Try again later, with the same request id: another message from this caller is being answered, or the
     * canister is at one of its limits (chats in progress, model calls per minute, storage).
     */
    'Busy' : null
  } |
  {
    /**
     * The conversation was cleared while the reply was generated.
     */
    'Cleared' : null
  } |
  {
    /**
     * This request id is already being answered; poll `get_message_status`.
     */
    'InProgress' : null
  } |
  {
    /**
     * The base revision is not the conversation's: nothing was run; reload and send again. For a revision the
     * canister handed out (in a reply, a conversation or a clear) this is for good: the conversation has moved
     * past it, and a request for it never runs. A revision the conversation has not reached yet is refused now,
     * but not for ever: the same request would run if the conversation got there.
     */
    'Conflict' : null
  };
export interface Config { 'llm_provider' : LlmProvider }
/**
 * The caller's conversation and its revision.
 */
export interface Conversation {
  'messages' : Array<Message>,
  /**
   * Changes with every saved exchange and every clear, and never repeats for a caller.
   */
  'revision' : bigint,
}
export type LlmProvider = {
    /**
     * A canned echo, for local replicas where the LLM canister does not exist.
     */
    'Fallback' : null
  } |
  {
    /**
     * The Internet Computer's LLM canister (mainnet only).
     */
    'OnChain' : null
  };
/**
 * One turn of a conversation, as returned by `get_conversation`.
 */
export interface Message {
  /**
   * The request id a user message was sent with.
   */
  'request_id' : [] | [string],
  'content' : string,
  /**
   * "user" or "assistant".
   */
  'role' : string,
}
export type MessageStatus = { 'Done' : Sent } |
  {
    /**
     * Not being answered and not in the conversation (never received, failed, cleared, or trimmed away).
     */
    'Unknown' : null
  } |
  { 'Pending' : null };
export interface Metrics {
  /**
   * Pages of stable memory allocated (64 KiB each). Stable memory is only written during upgrades.
   */
  'stable_memory_pages' : bigint,
  /**
   * Size of the Wasm memory, which grows but never shrinks; not the bytes currently in use.
   */
  'wasm_memory_bytes' : bigint,
  /**
   * All entries, including cleared conversations that only keep their revision.
   */
  'entries' : bigint,
  'version' : string,
  /**
   * Conversations with at least one saved exchange.
   */
  'conversations' : bigint,
  /**
   * Bytes counted against the storage budget.
   */
  'stored_bytes' : bigint,
  /**
   * The revision of every caller without an entry; raised whenever an entry is removed to make room.
   */
  'revision_floor' : bigint,
  'chats_in_flight' : bigint,
}
export type Result = { 'Ok' : Sent } |
  { 'Err' : ChatError };
export type Result_1 = { 'Ok' : null } |
  { 'Err' : string };
export interface Sent {
  /**
   * The conversation's revision now.
   */
  'revision' : bigint,
  'reply' : string,
}
export interface _SERVICE {
  /**
   * Empties the caller's conversation and cancels saving a reply that is still being generated. The conversation
   * gets the next revision (even if there was none yet), so no request for a revision handed out before the clear
   * can run afterwards. Returns the new revision. Traps when the canister keeps nothing for the caller yet (not even
   * a cleared conversation) and has no room: a clear only takes the place of conversations with nothing in them,
   * never of anyone's saved exchanges.
   */
  'clear_conversation' : ActorMethod<[], bigint>,
  'get_character' : ActorMethod<[], Character>,
  'get_config' : ActorMethod<[], Config>,
  /**
   * The caller's conversation: user and assistant turns, oldest first.
   */
  'get_conversation' : ActorMethod<[], Conversation>,
  /**
   * Conversations with at least one saved exchange.
   */
  'get_conversation_count' : ActorMethod<[], bigint>,
  'get_message_status' : ActorMethod<[string], MessageStatus>,
  'get_metrics' : ActorMethod<[], Metrics>,
  /**
   * "ok | v<version> | <n> conversations": the canister answers and its state is loaded. It says nothing about
   * whether the model can be reached.
   */
  'health' : ActorMethod<[], string>,
  /**
   * Sends a message as the next exchange after `base_revision`. A (request id, base revision) pair is saved at most
   * once: repeating it returns the saved reply. A base revision other than the conversation's is a Conflict, for
   * good once the conversation has moved past it (see `ChatError::Conflict`).
   */
  'send_message' : ActorMethod<[string, bigint, string], Result>,
  /**
   * Controllers only.
   */
  'set_llm_provider' : ActorMethod<[LlmProvider], Result_1>,
  /**
   * Controllers only. The new character applies to every conversation from its next message on.
   */
  'update_character' : ActorMethod<[Character], Result_1>,
  'version' : ActorMethod<[], string>,
}
export declare const idlFactory: IDL.InterfaceFactory;
export declare const init: (args: { IDL: typeof IDL }) => IDL.Type[];
