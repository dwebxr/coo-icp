export const idlFactory = ({ IDL }) => {
  const Character = IDL.Record({
    'bio' : IDL.Vec(IDL.Text),
    'system_prompt' : IDL.Text,
    'name' : IDL.Text,
    'style' : IDL.Vec(IDL.Text),
  });
  const LlmProvider = IDL.Variant({
    'Fallback' : IDL.Null,
    'OnChain' : IDL.Null,
  });
  const Config = IDL.Record({ 'llm_provider' : LlmProvider });
  const Message = IDL.Record({
    'request_id' : IDL.Opt(IDL.Text),
    'content' : IDL.Text,
    'role' : IDL.Text,
  });
  const Conversation = IDL.Record({
    'messages' : IDL.Vec(Message),
    'revision' : IDL.Nat64,
  });
  const Sent = IDL.Record({ 'revision' : IDL.Nat64, 'reply' : IDL.Text });
  const MessageStatus = IDL.Variant({
    'Done' : Sent,
    'Unknown' : IDL.Null,
    'Pending' : IDL.Null,
  });
  const Metrics = IDL.Record({
    'stable_memory_pages' : IDL.Nat64,
    'wasm_memory_bytes' : IDL.Nat64,
    'entries' : IDL.Nat64,
    'version' : IDL.Text,
    'conversations' : IDL.Nat64,
    'stored_bytes' : IDL.Nat64,
    'revision_floor' : IDL.Nat64,
    'chats_in_flight' : IDL.Nat64,
  });
  const ChatError = IDL.Variant({
    'Invalid' : IDL.Text,
    'Failed' : IDL.Text,
    'Busy' : IDL.Null,
    'Cleared' : IDL.Null,
    'InProgress' : IDL.Null,
    'Conflict' : IDL.Null,
  });
  const Result = IDL.Variant({ 'Ok' : Sent, 'Err' : ChatError });
  const Result_1 = IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text });
  return IDL.Service({
    'clear_conversation' : IDL.Func([], [IDL.Nat64], []),
    'get_character' : IDL.Func([], [Character], ['query']),
    'get_config' : IDL.Func([], [Config], ['query']),
    'get_conversation' : IDL.Func([], [Conversation], ['query']),
    'get_conversation_count' : IDL.Func([], [IDL.Nat64], ['query']),
    'get_message_status' : IDL.Func([IDL.Text], [MessageStatus], ['query']),
    'get_metrics' : IDL.Func([], [Metrics], ['query']),
    'health' : IDL.Func([], [IDL.Text], ['query']),
    'send_message' : IDL.Func([IDL.Text, IDL.Nat64, IDL.Text], [Result], []),
    'set_llm_provider' : IDL.Func([LlmProvider], [Result_1], []),
    'update_character' : IDL.Func([Character], [Result_1], []),
    'version' : IDL.Func([], [IDL.Text], ['query']),
  });
};
export const init = ({ IDL }) => { return []; };
