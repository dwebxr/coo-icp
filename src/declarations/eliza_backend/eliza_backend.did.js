export const idlFactory = ({ IDL }) => {
  const Result = IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text });
  const Character = IDL.Record({
    'bio' : IDL.Vec(IDL.Text),
    'system_prompt' : IDL.Text,
    'name' : IDL.Text,
    'style' : IDL.Vec(IDL.Text),
  });
  const LlmProvider = IDL.Variant({
    'Fallback' : IDL.Null,
    'OpenAI' : IDL.Null,
    'OnChain' : IDL.Null,
  });
  const Config = IDL.Record({
    'admin' : IDL.Principal,
    'llm_provider' : LlmProvider,
    'max_conversation_length' : IDL.Nat64,
  });
  const Message = IDL.Record({ 'content' : IDL.Text, 'role' : IDL.Text });
  const CanisterMetrics = IDL.Record({
    'stable_memory_pages' : IDL.Nat64,
    'heap_memory_bytes' : IDL.Nat64,
    'version' : IDL.Text,
    'conversations' : IDL.Nat64,
  });
  const Result_1 = IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text });
  const HttpHeader = IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text });
  const HttpResponse = IDL.Record({
    'status' : IDL.Nat,
    'body' : IDL.Vec(IDL.Nat8),
    'headers' : IDL.Vec(HttpHeader),
  });
  const TransformArgs = IDL.Record({
    'context' : IDL.Vec(IDL.Nat8),
    'response' : HttpResponse,
  });
  return IDL.Service({
    'chat' : IDL.Func([IDL.Text], [Result], []),
    'clear_conversation' : IDL.Func([], [], []),
    'get_character' : IDL.Func([], [IDL.Opt(Character)], ['query']),
    'get_config' : IDL.Func([], [IDL.Opt(Config)], ['query']),
    'get_conversation_count' : IDL.Func([], [IDL.Nat64], ['query']),
    'get_conversation_history' : IDL.Func([], [IDL.Vec(Message)], ['query']),
    'get_metrics' : IDL.Func([], [CanisterMetrics], ['query']),
    'health' : IDL.Func([], [IDL.Text], ['query']),
    'set_admin' : IDL.Func([IDL.Principal], [Result_1], []),
    'set_llm_provider' : IDL.Func([LlmProvider], [Result_1], []),
    'store_encrypted_api_key' : IDL.Func([IDL.Vec(IDL.Nat8)], [Result_1], []),
    'transform_openai_response' : IDL.Func(
        [TransformArgs],
        [HttpResponse],
        ['query'],
      ),
    'update_character' : IDL.Func([Character], [Result_1], []),
    'version' : IDL.Func([], [IDL.Text], ['query']),
  });
};
export const init = ({ IDL }) => { return []; };
