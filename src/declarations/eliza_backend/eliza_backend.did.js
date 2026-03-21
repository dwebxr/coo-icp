export const idlFactory = ({ IDL }) => {
  const ChainType = IDL.Variant({
    'EVM' : IDL.Record({ 'chain_id' : IDL.Nat64 }),
    'ICP' : IDL.Null,
    'Solana' : IDL.Record({ 'network' : IDL.Text }),
  });
  const D2AConfig = IDL.Record({
    'enabled' : IDL.Bool,
    'x402_usdc_contract' : IDL.Text,
    'aegis_url' : IDL.Text,
    'fetch_interval_seconds' : IDL.Nat64,
    'x402_network' : IDL.Text,
    'cache_ttl_seconds' : IDL.Nat64,
    'max_context_items' : IDL.Nat64,
    'min_composite_score' : IDL.Float64,
    'x402_enabled' : IDL.Bool,
  });
  const DiscordConfig = IDL.Record({
    'webhook_url' : IDL.Opt(IDL.Text),
    'channel_ids' : IDL.Vec(IDL.Text),
    'bot_token' : IDL.Vec(IDL.Nat8),
  });
  const Erc8004RegistryConfig = IDL.Record({
    'reputation_registry' : IDL.Text,
    'name' : IDL.Text,
    'validation_registry' : IDL.Text,
    'is_default' : IDL.Bool,
    'chain_id' : IDL.Nat64,
    'registry_id' : IDL.Text,
    'identity_registry' : IDL.Text,
  });
  const EvmChainConfig = IDL.Record({
    'decimals' : IDL.Nat8,
    'native_symbol' : IDL.Text,
    'chain_id' : IDL.Nat64,
    'rpc_url' : IDL.Text,
    'chain_name' : IDL.Text,
  });
  const SolanaNetworkConfig = IDL.Record({
    'network_name' : IDL.Text,
    'rpc_url' : IDL.Text,
  });
  const TwitterCredentials = IDL.Record({
    'access_token_secret' : IDL.Vec(IDL.Nat8),
    'api_key' : IDL.Vec(IDL.Nat8),
    'api_secret' : IDL.Vec(IDL.Nat8),
    'user_id' : IDL.Opt(IDL.Text),
    'access_token' : IDL.Vec(IDL.Nat8),
  });
  const Erc8004AgentInfo = IDL.Record({
    'owner' : IDL.Text,
    'agent_uri' : IDL.Text,
    'agent_id' : IDL.Nat64,
    'chain_id' : IDL.Nat64,
    'wallet' : IDL.Opt(IDL.Text),
    'registry' : IDL.Text,
  });
  const Erc8004ReputationSummary = IDL.Record({
    'count' : IDL.Nat64,
    'tag1' : IDL.Text,
    'tag2' : IDL.Text,
    'summary_value' : IDL.Int64,
    'agent_id' : IDL.Nat64,
    'summary_value_decimals' : IDL.Nat8,
  });
  const Erc8004ValidationStatus = IDL.Record({
    'tag' : IDL.Text,
    'request_hash' : IDL.Text,
    'response_hash' : IDL.Text,
    'validator_address' : IDL.Text,
    'agent_id' : IDL.Nat64,
    'response' : IDL.Nat8,
    'last_update' : IDL.Nat64,
  });
  const Erc8004Feedback = IDL.Record({
    'value_decimals' : IDL.Nat8,
    'feedback_index' : IDL.Nat64,
    'value' : IDL.Int64,
    'tag1' : IDL.Text,
    'tag2' : IDL.Text,
    'client_address' : IDL.Text,
    'is_revoked' : IDL.Bool,
  });
  const Erc8004MetadataEntry = IDL.Record({
    'key' : IDL.Text,
    'value' : IDL.Vec(IDL.Nat8),
  });
  const SocialPlatform = IDL.Variant({
    'Discord' : IDL.Null,
    'Twitter' : IDL.Null,
  });
  const AutoPostConfig = IDL.Record({
    'platform' : SocialPlatform,
    'enabled' : IDL.Bool,
    'interval_seconds' : IDL.Nat64,
    'topics' : IDL.Vec(IDL.Text),
    'last_post_time' : IDL.Nat64,
  });
  const D2ABriefingSummary = IDL.Record({
    'total_burned' : IDL.Nat64,
    'total_evaluated' : IDL.Nat64,
    'quality_rate' : IDL.Float64,
  });
  const BriefingScores = IDL.Record({
    'credibility' : IDL.Float64,
    'composite' : IDL.Float64,
    'l_slop' : IDL.Float64,
    'originality' : IDL.Float64,
    'v_signal' : IDL.Float64,
    'insight' : IDL.Float64,
    'c_context' : IDL.Float64,
  });
  const D2ABriefingItem = IDL.Record({
    'title' : IDL.Text,
    'content' : IDL.Text,
    'source' : IDL.Text,
    'scores' : BriefingScores,
    'briefing_score' : IDL.Float64,
    'verdict' : IDL.Text,
    'source_url' : IDL.Text,
    'topics' : IDL.Vec(IDL.Text),
    'reason' : IDL.Text,
  });
  const D2ABriefingResponse = IDL.Record({
    'generated_at' : IDL.Text,
    'version' : IDL.Text,
    'summary' : D2ABriefingSummary,
    'items' : IDL.Vec(D2ABriefingItem),
    'serendipity_pick' : IDL.Opt(D2ABriefingItem),
  });
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
  const D2AStatus = IDL.Record({
    'last_error' : IDL.Opt(IDL.Text),
    'enabled' : IDL.Bool,
    'polling_active' : IDL.Bool,
    'total_fetches' : IDL.Nat64,
    'aegis_url' : IDL.Text,
    'cache_age_seconds' : IDL.Nat64,
    'cache_item_count' : IDL.Nat64,
    'x402_enabled' : IDL.Bool,
    'last_fetch_time' : IDL.Nat64,
  });
  const Erc8004Status = IDL.Record({
    'default_registry' : IDL.Opt(IDL.Text),
    'agent_uri' : IDL.Opt(IDL.Text),
    'agent_id' : IDL.Opt(IDL.Nat64),
    'configured_registries' : IDL.Nat32,
    'registered' : IDL.Bool,
  });
  const EvmTransactionStatus = IDL.Variant({
    'Failed' : IDL.Text,
    'Confirmed' : IDL.Nat64,
    'Submitted' : IDL.Text,
    'Pending' : IDL.Null,
  });
  const EvmTransactionRecord = IDL.Record({
    'id' : IDL.Nat64,
    'to' : IDL.Text,
    'status' : EvmTransactionStatus,
    'data' : IDL.Opt(IDL.Text),
    'chain_id' : IDL.Nat64,
    'value_wei' : IDL.Text,
    'timestamp' : IDL.Nat64,
    'tx_hash' : IDL.Opt(IDL.Text),
  });
  const EvmWalletInfo = IDL.Record({
    'chain_id' : IDL.Nat64,
    'address' : IDL.Text,
    'chain_name' : IDL.Text,
  });
  const ExecutionStatus = IDL.Variant({
    'Failed' : IDL.Text,
    'Success' : IDL.Null,
  });
  const CommandType = IDL.Variant({
    'ExecuteJupiterSwap' : IDL.Null,
    'SendEvmNative' : IDL.Null,
    'SendIcp' : IDL.Null,
    'ExecuteUniswapSwap' : IDL.Null,
    'SendSplToken' : IDL.Null,
    'SendSolana' : IDL.Null,
    'SendErc20' : IDL.Null,
    'ExecuteLifiBridge' : IDL.Null,
  });
  const ExecutionRecord = IDL.Record({
    'id' : IDL.Nat64,
    'status' : ExecutionStatus,
    'result' : IDL.Opt(IDL.Text),
    'params_json' : IDL.Text,
    'command_type' : CommandType,
    'timestamp' : IDL.Nat64,
    'caller' : IDL.Principal,
    'tx_hash' : IDL.Opt(IDL.Text),
  });
  const IncomingMessage = IDL.Record({
    'id' : IDL.Text,
    'content' : IDL.Text,
    'author_name' : IDL.Text,
    'conversation_id' : IDL.Opt(IDL.Text),
    'platform' : SocialPlatform,
    'timestamp' : IDL.Nat64,
    'replied' : IDL.Bool,
    'author_id' : IDL.Text,
    'processed' : IDL.Bool,
  });
  const JupiterQuote = IDL.Record({
    'price_impact_pct' : IDL.Text,
    'in_amount' : IDL.Text,
    'slippage_bps' : IDL.Nat64,
    'output_mint' : IDL.Text,
    'input_mint' : IDL.Text,
    'out_amount' : IDL.Text,
  });
  const LiFiBridgeQuote = IDL.Record({
    'to_token' : IDL.Text,
    'from_amount' : IDL.Text,
    'from_token' : IDL.Text,
    'tool' : IDL.Text,
    'to_chain_id' : IDL.Nat64,
    'from_chain_id' : IDL.Nat64,
    'to_amount' : IDL.Text,
    'estimated_gas' : IDL.Text,
  });
  const SolanaNetworkBalance = IDL.Record({
    'network_name' : IDL.Text,
    'address' : IDL.Text,
    'balance_lamports' : IDL.Nat64,
  });
  const EvmChainBalance = IDL.Record({
    'chain_id' : IDL.Nat64,
    'address' : IDL.Text,
    'balance_wei' : IDL.Text,
    'chain_name' : IDL.Text,
  });
  const PlatformStatus = IDL.Record({
    'cycles_balance' : IDL.Nat,
    'solana_balances' : IDL.Vec(SolanaNetworkBalance),
    'evm_balances' : IDL.Vec(EvmChainBalance),
    'icp_balance' : IDL.Nat64,
  });
  const PortfolioAsset = IDL.Record({
    'balance' : IDL.Text,
    'chain' : IDL.Text,
    'token_address' : IDL.Opt(IDL.Text),
    'address' : IDL.Text,
    'symbol' : IDL.Text,
  });
  const Portfolio = IDL.Record({
    'icp' : PortfolioAsset,
    'total_chains' : IDL.Nat32,
    'solana_assets' : IDL.Vec(PortfolioAsset),
    'evm_assets' : IDL.Vec(PortfolioAsset),
    'last_updated' : IDL.Nat64,
  });
  const PostStatus = IDL.Variant({
    'Failed' : IDL.Text,
    'Processing' : IDL.Null,
    'Completed' : IDL.Null,
    'Pending' : IDL.Null,
  });
  const PostMetadata = IDL.Record({
    'reply_to_id' : IDL.Opt(IDL.Text),
    'result_id' : IDL.Opt(IDL.Text),
    'discord_channel_id' : IDL.Opt(IDL.Text),
  });
  const ScheduledPost = IDL.Record({
    'id' : IDL.Nat64,
    'scheduled_time' : IDL.Nat64,
    'status' : PostStatus,
    'content' : IDL.Text,
    'retry_count' : IDL.Nat32,
    'metadata' : IDL.Opt(PostMetadata),
    'platform' : SocialPlatform,
    'created_at' : IDL.Nat64,
  });
  const SocialStatus = IDL.Record({
    'unprocessed_messages' : IDL.Nat32,
    'discord_configured' : IDL.Bool,
    'last_discord_poll' : IDL.Nat64,
    'pending_posts' : IDL.Nat32,
    'polling_active' : IDL.Bool,
    'last_twitter_poll' : IDL.Nat64,
    'twitter_configured' : IDL.Bool,
    'enabled_platforms' : IDL.Vec(SocialPlatform),
  });
  const SolanaTransactionStatus = IDL.Variant({
    'Failed' : IDL.Text,
    'Confirmed' : IDL.Nat64,
    'Submitted' : IDL.Text,
    'Pending' : IDL.Null,
  });
  const SolanaTransactionRecord = IDL.Record({
    'id' : IDL.Nat64,
    'to' : IDL.Text,
    'status' : SolanaTransactionStatus,
    'signature' : IDL.Opt(IDL.Text),
    'timestamp' : IDL.Nat64,
    'amount_lamports' : IDL.Nat64,
  });
  const SolanaWalletInfo = IDL.Record({
    'network' : IDL.Text,
    'address' : IDL.Text,
  });
  const TransactionStatus = IDL.Variant({
    'Failed' : IDL.Text,
    'Completed' : IDL.Null,
    'Pending' : IDL.Null,
  });
  const TransactionType = IDL.Variant({
    'Send' : IDL.Null,
    'Receive' : IDL.Null,
  });
  const TransactionRecord = IDL.Record({
    'id' : IDL.Nat64,
    'to' : IDL.Opt(IDL.Text),
    'status' : TransactionStatus,
    'from' : IDL.Opt(IDL.Text),
    'memo' : IDL.Nat64,
    'timestamp' : IDL.Nat64,
    'tx_type' : TransactionType,
    'amount' : IDL.Nat64,
    'block_height' : IDL.Opt(IDL.Nat64),
  });
  const DexSwapQuote = IDL.Record({
    'token_in' : IDL.Text,
    'amount_out' : IDL.Text,
    'chain_id' : IDL.Nat64,
    'price_impact' : IDL.Text,
    'amount_in' : IDL.Text,
    'token_out' : IDL.Text,
  });
  const WalletInfo = IDL.Record({
    'icp_address' : IDL.Text,
    'last_balance_update' : IDL.Nat64,
    'principal_id' : IDL.Text,
    'icp_balance' : IDL.Nat64,
  });
  const WhitelistEntry = IDL.Record({
    'id' : IDL.Nat64,
    'chain' : ChainType,
    'added_at' : IDL.Nat64,
    'added_by' : IDL.Principal,
    'label' : IDL.Opt(IDL.Text),
    'address' : IDL.Text,
  });
  return IDL.Service({
    'add_whitelist_address' : IDL.Func(
        [IDL.Text, ChainType, IDL.Opt(IDL.Text)],
        [IDL.Variant({ 'Ok' : IDL.Nat64, 'Err' : IDL.Text })],
        [],
      ),
    'cancel_scheduled_post' : IDL.Func(
        [IDL.Nat64],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'chat' : IDL.Func(
        [IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'check_icp_balance' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Nat64, 'Err' : IDL.Text })],
        [],
      ),
    'clear_conversation' : IDL.Func([], [], []),
    'configure_d2a' : IDL.Func(
        [D2AConfig],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'configure_discord' : IDL.Func(
        [DiscordConfig],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'configure_erc8004_registry' : IDL.Func(
        [Erc8004RegistryConfig],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'configure_evm_chain' : IDL.Func(
        [EvmChainConfig],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'configure_solana_network' : IDL.Func(
        [SolanaNetworkConfig],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'configure_twitter' : IDL.Func(
        [TwitterCredentials],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_get_agent_info' : IDL.Func(
        [IDL.Nat64],
        [IDL.Variant({ 'Ok' : Erc8004AgentInfo, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_get_metadata' : IDL.Func(
        [IDL.Nat64, IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Vec(IDL.Nat8), 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_get_reputation_summary' : IDL.Func(
        [IDL.Nat64, IDL.Text, IDL.Text],
        [IDL.Variant({ 'Ok' : Erc8004ReputationSummary, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_get_validation_status' : IDL.Func(
        [IDL.Text],
        [IDL.Variant({ 'Ok' : Erc8004ValidationStatus, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_give_feedback' : IDL.Func(
        [
          IDL.Nat64,
          IDL.Int64,
          IDL.Nat8,
          IDL.Text,
          IDL.Text,
          IDL.Text,
          IDL.Text,
        ],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_read_feedback' : IDL.Func(
        [IDL.Nat64, IDL.Text, IDL.Nat64],
        [IDL.Variant({ 'Ok' : Erc8004Feedback, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_register_agent' : IDL.Func(
        [IDL.Text, IDL.Vec(Erc8004MetadataEntry)],
        [IDL.Variant({ 'Ok' : IDL.Nat64, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_request_validation' : IDL.Func(
        [IDL.Text, IDL.Nat64, IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_respond_validation' : IDL.Func(
        [IDL.Text, IDL.Nat8, IDL.Text, IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_revoke_feedback' : IDL.Func(
        [IDL.Nat64, IDL.Nat64],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_set_agent_uri' : IDL.Func(
        [IDL.Nat64, IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'erc8004_set_metadata' : IDL.Func(
        [IDL.Nat64, IDL.Text, IDL.Vec(IDL.Nat8)],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'execute_command' : IDL.Func(
        [IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'execute_jupiter_swap' : IDL.Func(
        [IDL.Text, IDL.Text, IDL.Text, IDL.Nat64, IDL.Opt(IDL.Nat64)],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'execute_lifi_bridge' : IDL.Func(
        [IDL.Nat64, IDL.Nat64, IDL.Text, IDL.Text, IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'execute_uniswap_swap' : IDL.Func(
        [IDL.Nat64, IDL.Text, IDL.Text, IDL.Text, IDL.Text, IDL.Opt(IDL.Nat32)],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'get_auto_post_config' : IDL.Func([], [IDL.Opt(AutoPostConfig)], ['query']),
    'get_cached_briefing' : IDL.Func(
        [],
        [IDL.Opt(D2ABriefingResponse)],
        ['query'],
      ),
    'get_character' : IDL.Func([], [IDL.Opt(Character)], ['query']),
    'get_config' : IDL.Func([], [IDL.Opt(Config)], ['query']),
    'get_configured_chains' : IDL.Func(
        [],
        [IDL.Vec(EvmChainConfig)],
        ['query'],
      ),
    'get_conversation_count' : IDL.Func([], [IDL.Nat64], ['query']),
    'get_conversation_history' : IDL.Func([], [IDL.Vec(Message)], ['query']),
    'get_cycles_balance' : IDL.Func([], [IDL.Nat], ['query']),
    'get_d2a_config' : IDL.Func([], [IDL.Opt(D2AConfig)], ['query']),
    'get_d2a_status' : IDL.Func([], [D2AStatus], ['query']),
    'get_erc20_balance' : IDL.Func(
        [IDL.Nat64, IDL.Text, IDL.Opt(IDL.Text)],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'get_erc8004_registries' : IDL.Func(
        [],
        [IDL.Vec(Erc8004RegistryConfig)],
        ['query'],
      ),
    'get_erc8004_status' : IDL.Func([], [Erc8004Status], ['query']),
    'get_evm_address' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'get_evm_balance' : IDL.Func(
        [IDL.Nat64],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'get_evm_transaction_history' : IDL.Func(
        [IDL.Opt(IDL.Nat32)],
        [IDL.Vec(EvmTransactionRecord)],
        ['query'],
      ),
    'get_evm_wallet_info' : IDL.Func(
        [IDL.Nat64],
        [IDL.Variant({ 'Ok' : EvmWalletInfo, 'Err' : IDL.Text })],
        [],
      ),
    'get_execution_history' : IDL.Func(
        [IDL.Opt(IDL.Nat32)],
        [IDL.Vec(ExecutionRecord)],
        ['query'],
      ),
    'get_incoming_messages' : IDL.Func(
        [IDL.Opt(IDL.Nat32)],
        [IDL.Vec(IncomingMessage)],
        ['query'],
      ),
    'get_jupiter_quote' : IDL.Func(
        [IDL.Text, IDL.Text, IDL.Nat64, IDL.Opt(IDL.Nat64)],
        [IDL.Variant({ 'Ok' : JupiterQuote, 'Err' : IDL.Text })],
        [],
      ),
    'get_lifi_quote' : IDL.Func(
        [IDL.Nat64, IDL.Nat64, IDL.Text, IDL.Text, IDL.Text],
        [IDL.Variant({ 'Ok' : LiFiBridgeQuote, 'Err' : IDL.Text })],
        [],
      ),
    'get_platform_status' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : PlatformStatus, 'Err' : IDL.Text })],
        [],
      ),
    'get_portfolio' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : Portfolio, 'Err' : IDL.Text })],
        [],
      ),
    'get_scheduled_posts' : IDL.Func([], [IDL.Vec(ScheduledPost)], ['query']),
    'get_social_status' : IDL.Func([], [SocialStatus], ['query']),
    'get_solana_address' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'get_solana_balance' : IDL.Func(
        [IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Nat64, 'Err' : IDL.Text })],
        [],
      ),
    'get_solana_networks' : IDL.Func(
        [],
        [IDL.Vec(SolanaNetworkConfig)],
        ['query'],
      ),
    'get_solana_transaction_history' : IDL.Func(
        [IDL.Opt(IDL.Nat32)],
        [IDL.Vec(SolanaTransactionRecord)],
        ['query'],
      ),
    'get_solana_wallet_info' : IDL.Func(
        [IDL.Text],
        [IDL.Variant({ 'Ok' : SolanaWalletInfo, 'Err' : IDL.Text })],
        [],
      ),
    'get_spl_token_balance' : IDL.Func(
        [IDL.Text, IDL.Text, IDL.Opt(IDL.Text)],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'get_transaction_history' : IDL.Func(
        [IDL.Opt(IDL.Nat32)],
        [IDL.Vec(TransactionRecord)],
        ['query'],
      ),
    'get_uniswap_quote' : IDL.Func(
        [IDL.Nat64, IDL.Text, IDL.Text, IDL.Text, IDL.Opt(IDL.Nat32)],
        [IDL.Variant({ 'Ok' : DexSwapQuote, 'Err' : IDL.Text })],
        [],
      ),
    'get_wallet_address' : IDL.Func([], [IDL.Text], ['query']),
    'get_wallet_addresses' : IDL.Func(
        [],
        [IDL.Vec(IDL.Tuple(IDL.Text, IDL.Text))],
        ['query'],
      ),
    'get_wallet_info' : IDL.Func([], [WalletInfo], ['query']),
    'get_wallet_status' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : WalletInfo, 'Err' : IDL.Text })],
        [],
      ),
    'get_whitelist' : IDL.Func([], [IDL.Vec(WhitelistEntry)], ['query']),
    'get_whitelist_status' : IDL.Func([], [IDL.Bool, IDL.Nat64], ['query']),
    'health' : IDL.Func([], [IDL.Text], ['query']),
    'init_solana_wallet' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'post_now' : IDL.Func(
        [SocialPlatform, IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'remove_whitelist_address' : IDL.Func(
        [IDL.Nat64],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'reset_solana_wallet' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'schedule_post' : IDL.Func(
        [SocialPlatform, IDL.Text, IDL.Nat64, IDL.Opt(PostMetadata)],
        [IDL.Variant({ 'Ok' : IDL.Nat64, 'Err' : IDL.Text })],
        [],
      ),
    'send_erc20' : IDL.Func(
        [IDL.Nat64, IDL.Text, IDL.Text, IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'send_evm_native' : IDL.Func(
        [IDL.Nat64, IDL.Text, IDL.Text],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'send_icp' : IDL.Func(
        [IDL.Text, IDL.Nat64, IDL.Opt(IDL.Nat64)],
        [IDL.Variant({ 'Ok' : IDL.Nat64, 'Err' : IDL.Text })],
        [],
      ),
    'send_solana' : IDL.Func(
        [IDL.Text, IDL.Text, IDL.Nat64],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'send_spl_token' : IDL.Func(
        [IDL.Text, IDL.Text, IDL.Text, IDL.Nat64],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'set_admin' : IDL.Func(
        [IDL.Principal],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'set_auto_reply' : IDL.Func(
        [IDL.Bool],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'set_enabled_platforms' : IDL.Func(
        [IDL.Vec(SocialPlatform)],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'set_llm_provider' : IDL.Func(
        [LlmProvider],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'set_whitelist_enabled' : IDL.Func(
        [IDL.Bool],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'start_auto_posting' : IDL.Func(
        [IDL.Nat64, IDL.Vec(IDL.Text)],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'start_d2a_polling' : IDL.Func(
        [IDL.Nat64],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'start_social_polling' : IDL.Func(
        [IDL.Nat64],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'stop_auto_posting' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'stop_d2a_polling' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'stop_social_polling' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'store_encrypted_api_key' : IDL.Func(
        [IDL.Vec(IDL.Nat8)],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'transform_d2a_response' : IDL.Func(
        [
          IDL.Record({
            'context' : IDL.Vec(IDL.Nat8),
            'response' : IDL.Record({
              'status' : IDL.Nat,
              'body' : IDL.Vec(IDL.Nat8),
              'headers' : IDL.Vec(
                IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
              ),
            }),
          }),
        ],
        [
          IDL.Record({
            'status' : IDL.Nat,
            'body' : IDL.Vec(IDL.Nat8),
            'headers' : IDL.Vec(
              IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
            ),
          }),
        ],
        ['query'],
      ),
    'transform_evm_response' : IDL.Func(
        [
          IDL.Record({
            'context' : IDL.Vec(IDL.Nat8),
            'response' : IDL.Record({
              'status' : IDL.Nat,
              'body' : IDL.Vec(IDL.Nat8),
              'headers' : IDL.Vec(
                IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
              ),
            }),
          }),
        ],
        [
          IDL.Record({
            'status' : IDL.Nat,
            'body' : IDL.Vec(IDL.Nat8),
            'headers' : IDL.Vec(
              IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
            ),
          }),
        ],
        ['query'],
      ),
    'transform_openai_response' : IDL.Func(
        [
          IDL.Record({
            'context' : IDL.Vec(IDL.Nat8),
            'response' : IDL.Record({
              'status' : IDL.Nat,
              'body' : IDL.Vec(IDL.Nat8),
              'headers' : IDL.Vec(
                IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
              ),
            }),
          }),
        ],
        [
          IDL.Record({
            'status' : IDL.Nat,
            'body' : IDL.Vec(IDL.Nat8),
            'headers' : IDL.Vec(
              IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
            ),
          }),
        ],
        ['query'],
      ),
    'transform_social_response' : IDL.Func(
        [
          IDL.Record({
            'context' : IDL.Vec(IDL.Nat8),
            'response' : IDL.Record({
              'status' : IDL.Nat,
              'body' : IDL.Vec(IDL.Nat8),
              'headers' : IDL.Vec(
                IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
              ),
            }),
          }),
        ],
        [
          IDL.Record({
            'status' : IDL.Nat,
            'body' : IDL.Vec(IDL.Nat8),
            'headers' : IDL.Vec(
              IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
            ),
          }),
        ],
        ['query'],
      ),
    'transform_solana_response' : IDL.Func(
        [
          IDL.Record({
            'context' : IDL.Vec(IDL.Nat8),
            'response' : IDL.Record({
              'status' : IDL.Nat,
              'body' : IDL.Vec(IDL.Nat8),
              'headers' : IDL.Vec(
                IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
              ),
            }),
          }),
        ],
        [
          IDL.Record({
            'status' : IDL.Nat,
            'body' : IDL.Vec(IDL.Nat8),
            'headers' : IDL.Vec(
              IDL.Record({ 'value' : IDL.Text, 'name' : IDL.Text })
            ),
          }),
        ],
        ['query'],
      ),
    'trigger_auto_post' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'trigger_d2a_fetch' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Text, 'Err' : IDL.Text })],
        [],
      ),
    'trigger_poll' : IDL.Func(
        [],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'update_character' : IDL.Func(
        [Character],
        [IDL.Variant({ 'Ok' : IDL.Null, 'Err' : IDL.Text })],
        [],
      ),
    'version' : IDL.Func([], [IDL.Text], ['query']),
  });
};
export const init = ({ IDL }) => { return []; };
