import type { Principal } from '@dfinity/principal';
import type { ActorMethod } from '@dfinity/agent';
import type { IDL } from '@dfinity/candid';

export interface AutoPostConfig {
  'platform' : SocialPlatform,
  'enabled' : boolean,
  'interval_seconds' : bigint,
  'topics' : Array<string>,
  'last_post_time' : bigint,
}
/**
 * D2A Briefing API Types
 */
export interface BriefingScores {
  'credibility' : number,
  'composite' : number,
  'l_slop' : number,
  'originality' : number,
  'v_signal' : number,
  'insight' : number,
  'c_context' : number,
}
export type ChainType = { 'EVM' : { 'chain_id' : bigint } } |
  { 'ICP' : null } |
  { 'Solana' : { 'network' : string } };
export interface Character {
  'bio' : Array<string>,
  'system_prompt' : string,
  'name' : string,
  'style' : Array<string>,
}
export type CommandType = { 'ExecuteJupiterSwap' : null } |
  { 'SendEvmNative' : null } |
  { 'SendIcp' : null } |
  { 'ExecuteUniswapSwap' : null } |
  { 'SendSplToken' : null } |
  { 'SendSolana' : null } |
  { 'SendErc20' : null } |
  { 'ExecuteLifiBridge' : null };
export interface Config {
  'admin' : Principal,
  'llm_provider' : LlmProvider,
  'max_conversation_length' : bigint,
}
export interface D2ABriefingItem {
  'title' : string,
  'content' : string,
  'source' : string,
  'scores' : BriefingScores,
  'briefing_score' : number,
  'verdict' : string,
  'source_url' : string,
  'topics' : Array<string>,
  'reason' : string,
}
export interface D2ABriefingResponse {
  'generated_at' : string,
  'version' : string,
  'summary' : D2ABriefingSummary,
  'items' : Array<D2ABriefingItem>,
  'serendipity_pick' : [] | [D2ABriefingItem],
}
export interface D2ABriefingSummary {
  'total_burned' : bigint,
  'total_evaluated' : bigint,
  'quality_rate' : number,
}
export interface D2AConfig {
  'enabled' : boolean,
  'x402_usdc_contract' : string,
  'aegis_url' : string,
  'fetch_interval_seconds' : bigint,
  'x402_network' : string,
  'cache_ttl_seconds' : bigint,
  'max_context_items' : bigint,
  'min_composite_score' : number,
  'x402_enabled' : boolean,
}
export interface D2AStatus {
  'last_error' : [] | [string],
  'enabled' : boolean,
  'polling_active' : boolean,
  'total_fetches' : bigint,
  'aegis_url' : string,
  'cache_age_seconds' : bigint,
  'cache_item_count' : bigint,
  'x402_enabled' : boolean,
  'last_fetch_time' : bigint,
}
/**
 * DEX Swap Types
 */
export interface DexSwapQuote {
  'token_in' : string,
  'amount_out' : string,
  'chain_id' : bigint,
  'price_impact' : string,
  'amount_in' : string,
  'token_out' : string,
}
export interface DiscordConfig {
  'webhook_url' : [] | [string],
  'channel_ids' : Array<string>,
  'bot_token' : Uint8Array | number[],
}
export interface Erc8004AgentInfo {
  'owner' : string,
  'agent_uri' : string,
  'agent_id' : bigint,
  'chain_id' : bigint,
  'wallet' : [] | [string],
  'registry' : string,
}
export interface Erc8004Feedback {
  'value_decimals' : number,
  'feedback_index' : bigint,
  'value' : bigint,
  'tag1' : string,
  'tag2' : string,
  'client_address' : string,
  'is_revoked' : boolean,
}
export interface Erc8004MetadataEntry {
  'key' : string,
  'value' : Uint8Array | number[],
}
export interface Erc8004RegistryConfig {
  'reputation_registry' : string,
  'name' : string,
  'validation_registry' : string,
  'is_default' : boolean,
  'chain_id' : bigint,
  'registry_id' : string,
  'identity_registry' : string,
}
export interface Erc8004ReputationSummary {
  'count' : bigint,
  'tag1' : string,
  'tag2' : string,
  'summary_value' : bigint,
  'agent_id' : bigint,
  'summary_value_decimals' : number,
}
export interface Erc8004Status {
  'default_registry' : [] | [string],
  'agent_uri' : [] | [string],
  'agent_id' : [] | [bigint],
  'configured_registries' : number,
  'registered' : boolean,
}
export interface Erc8004ValidationStatus {
  'tag' : string,
  'request_hash' : string,
  'response_hash' : string,
  'validator_address' : string,
  'agent_id' : bigint,
  'response' : number,
  'last_update' : bigint,
}
export interface EvmChainBalance {
  'chain_id' : bigint,
  'address' : string,
  'balance_wei' : string,
  'chain_name' : string,
}
export interface EvmChainConfig {
  'decimals' : number,
  'native_symbol' : string,
  'chain_id' : bigint,
  'rpc_url' : string,
  'chain_name' : string,
}
export interface EvmTransactionRecord {
  'id' : bigint,
  'to' : string,
  'status' : EvmTransactionStatus,
  'data' : [] | [string],
  'chain_id' : bigint,
  'value_wei' : string,
  'timestamp' : bigint,
  'tx_hash' : [] | [string],
}
export type EvmTransactionStatus = { 'Failed' : string } |
  { 'Confirmed' : bigint } |
  { 'Submitted' : string } |
  { 'Pending' : null };
/**
 * EVM Wallet Types (Chain-Key ECDSA)
 */
export interface EvmWalletInfo {
  'chain_id' : bigint,
  'address' : string,
  'chain_name' : string,
}
export interface ExecutionRecord {
  'id' : bigint,
  'status' : ExecutionStatus,
  'result' : [] | [string],
  'params_json' : string,
  'command_type' : CommandType,
  'timestamp' : bigint,
  'caller' : Principal,
  'tx_hash' : [] | [string],
}
export type ExecutionStatus = { 'Failed' : string } |
  { 'Success' : null };
export interface IncomingMessage {
  'id' : string,
  'content' : string,
  'author_name' : string,
  'conversation_id' : [] | [string],
  'platform' : SocialPlatform,
  'timestamp' : bigint,
  'replied' : boolean,
  'author_id' : string,
  'processed' : boolean,
}
/**
 * Jupiter Swap Types
 */
export interface JupiterQuote {
  'price_impact_pct' : string,
  'in_amount' : string,
  'slippage_bps' : bigint,
  'output_mint' : string,
  'input_mint' : string,
  'out_amount' : string,
}
/**
 * LiFi Bridge Types
 */
export interface LiFiBridgeQuote {
  'to_token' : string,
  'from_amount' : string,
  'from_token' : string,
  'tool' : string,
  'to_chain_id' : bigint,
  'from_chain_id' : bigint,
  'to_amount' : string,
  'estimated_gas' : string,
}
export type LlmProvider = { 'Fallback' : null } |
  { 'OpenAI' : null } |
  { 'OnChain' : null };
export interface Message { 'content' : string, 'role' : string }
export interface PlatformStatus {
  'cycles_balance' : bigint,
  'solana_balances' : Array<SolanaNetworkBalance>,
  'evm_balances' : Array<EvmChainBalance>,
  'icp_balance' : bigint,
}
export interface Portfolio {
  'icp' : PortfolioAsset,
  'total_chains' : number,
  'solana_assets' : Array<PortfolioAsset>,
  'evm_assets' : Array<PortfolioAsset>,
  'last_updated' : bigint,
}
/**
 * Portfolio Types
 */
export interface PortfolioAsset {
  'balance' : string,
  'chain' : string,
  'token_address' : [] | [string],
  'address' : string,
  'symbol' : string,
}
export interface PostMetadata {
  'reply_to_id' : [] | [string],
  'result_id' : [] | [string],
  'discord_channel_id' : [] | [string],
}
export type PostStatus = { 'Failed' : string } |
  { 'Processing' : null } |
  { 'Completed' : null } |
  { 'Pending' : null };
export interface ScheduledPost {
  'id' : bigint,
  'scheduled_time' : bigint,
  'status' : PostStatus,
  'content' : string,
  'retry_count' : number,
  'metadata' : [] | [PostMetadata],
  'platform' : SocialPlatform,
  'created_at' : bigint,
}
/**
 * Social Integration Types
 */
export type SocialPlatform = { 'Discord' : null } |
  { 'Twitter' : null };
export interface SocialStatus {
  'unprocessed_messages' : number,
  'discord_configured' : boolean,
  'last_discord_poll' : bigint,
  'pending_posts' : number,
  'polling_active' : boolean,
  'last_twitter_poll' : bigint,
  'twitter_configured' : boolean,
  'enabled_platforms' : Array<SocialPlatform>,
}
export interface SolanaNetworkBalance {
  'network_name' : string,
  'address' : string,
  'balance_lamports' : bigint,
}
export interface SolanaNetworkConfig {
  'network_name' : string,
  'rpc_url' : string,
}
export interface SolanaTransactionRecord {
  'id' : bigint,
  'to' : string,
  'status' : SolanaTransactionStatus,
  'signature' : [] | [string],
  'timestamp' : bigint,
  'amount_lamports' : bigint,
}
export type SolanaTransactionStatus = { 'Failed' : string } |
  { 'Confirmed' : bigint } |
  { 'Submitted' : string } |
  { 'Pending' : null };
/**
 * Solana Wallet Types (Ed25519)
 */
export interface SolanaWalletInfo { 'network' : string, 'address' : string }
export interface TransactionRecord {
  'id' : bigint,
  'to' : [] | [string],
  'status' : TransactionStatus,
  'from' : [] | [string],
  'memo' : bigint,
  'timestamp' : bigint,
  'tx_type' : TransactionType,
  'amount' : bigint,
  'block_height' : [] | [bigint],
}
export type TransactionStatus = { 'Failed' : string } |
  { 'Completed' : null } |
  { 'Pending' : null };
export type TransactionType = { 'Send' : null } |
  { 'Receive' : null };
export interface TwitterCredentials {
  'access_token_secret' : Uint8Array | number[],
  'api_key' : Uint8Array | number[],
  'api_secret' : Uint8Array | number[],
  'user_id' : [] | [string],
  'access_token' : Uint8Array | number[],
}
/**
 * Wallet Types
 */
export interface WalletInfo {
  'icp_address' : string,
  'last_balance_update' : bigint,
  'principal_id' : string,
  'icp_balance' : bigint,
}
export interface WhitelistEntry {
  'id' : bigint,
  'chain' : ChainType,
  'added_at' : bigint,
  'added_by' : Principal,
  'label' : [] | [string],
  'address' : string,
}
export interface _SERVICE {
  /**
   * Whitelist Management
   */
  'add_whitelist_address' : ActorMethod<
    [string, ChainType, [] | [string]],
    { 'Ok' : bigint } |
      { 'Err' : string }
  >,
  'cancel_scheduled_post' : ActorMethod<
    [bigint],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * Chat
   */
  'chat' : ActorMethod<[string], { 'Ok' : string } | { 'Err' : string }>,
  'check_icp_balance' : ActorMethod<[], { 'Ok' : bigint } | { 'Err' : string }>,
  'clear_conversation' : ActorMethod<[], undefined>,
  /**
   * D2A Briefing API
   */
  'configure_d2a' : ActorMethod<
    [D2AConfig],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * Discord Configuration
   */
  'configure_discord' : ActorMethod<
    [DiscordConfig],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * Registry Configuration
   */
  'configure_erc8004_registry' : ActorMethod<
    [Erc8004RegistryConfig],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'configure_evm_chain' : ActorMethod<
    [EvmChainConfig],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'configure_solana_network' : ActorMethod<
    [SolanaNetworkConfig],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * Twitter Configuration
   */
  'configure_twitter' : ActorMethod<
    [TwitterCredentials],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'erc8004_get_agent_info' : ActorMethod<
    [bigint],
    { 'Ok' : Erc8004AgentInfo } |
      { 'Err' : string }
  >,
  'erc8004_get_metadata' : ActorMethod<
    [bigint, string],
    { 'Ok' : Uint8Array | number[] } |
      { 'Err' : string }
  >,
  'erc8004_get_reputation_summary' : ActorMethod<
    [bigint, string, string],
    { 'Ok' : Erc8004ReputationSummary } |
      { 'Err' : string }
  >,
  'erc8004_get_validation_status' : ActorMethod<
    [string],
    { 'Ok' : Erc8004ValidationStatus } |
      { 'Err' : string }
  >,
  /**
   * Reputation Registry
   */
  'erc8004_give_feedback' : ActorMethod<
    [bigint, bigint, number, string, string, string, string],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'erc8004_read_feedback' : ActorMethod<
    [bigint, string, bigint],
    { 'Ok' : Erc8004Feedback } |
      { 'Err' : string }
  >,
  /**
   * Identity Registry
   */
  'erc8004_register_agent' : ActorMethod<
    [string, Array<Erc8004MetadataEntry>],
    { 'Ok' : bigint } |
      { 'Err' : string }
  >,
  /**
   * Validation Registry
   */
  'erc8004_request_validation' : ActorMethod<
    [string, bigint, string],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'erc8004_respond_validation' : ActorMethod<
    [string, number, string, string],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'erc8004_revoke_feedback' : ActorMethod<
    [bigint, bigint],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'erc8004_set_agent_uri' : ActorMethod<
    [bigint, string],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'erc8004_set_metadata' : ActorMethod<
    [bigint, string, Uint8Array | number[]],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * Unified Command Execution
   */
  'execute_command' : ActorMethod<
    [string],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'execute_jupiter_swap' : ActorMethod<
    [string, string, string, bigint, [] | [bigint]],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'execute_lifi_bridge' : ActorMethod<
    [bigint, bigint, string, string, string],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'execute_uniswap_swap' : ActorMethod<
    [bigint, string, string, string, string, [] | [number]],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'get_auto_post_config' : ActorMethod<[], [] | [AutoPostConfig]>,
  'get_cached_briefing' : ActorMethod<[], [] | [D2ABriefingResponse]>,
  'get_character' : ActorMethod<[], [] | [Character]>,
  'get_config' : ActorMethod<[], [] | [Config]>,
  'get_configured_chains' : ActorMethod<[], Array<EvmChainConfig>>,
  'get_conversation_count' : ActorMethod<[], bigint>,
  /**
   * Conversation management
   */
  'get_conversation_history' : ActorMethod<[], Array<Message>>,
  /**
   * Cycles Monitoring
   */
  'get_cycles_balance' : ActorMethod<[], bigint>,
  'get_d2a_config' : ActorMethod<[], [] | [D2AConfig]>,
  'get_d2a_status' : ActorMethod<[], D2AStatus>,
  'get_erc20_balance' : ActorMethod<
    [bigint, string, [] | [string]],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'get_erc8004_registries' : ActorMethod<[], Array<Erc8004RegistryConfig>>,
  'get_erc8004_status' : ActorMethod<[], Erc8004Status>,
  /**
   * ========== EVM Wallet (Chain-Key ECDSA) ==========
   */
  'get_evm_address' : ActorMethod<[], { 'Ok' : string } | { 'Err' : string }>,
  'get_evm_balance' : ActorMethod<
    [bigint],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'get_evm_transaction_history' : ActorMethod<
    [[] | [number]],
    Array<EvmTransactionRecord>
  >,
  'get_evm_wallet_info' : ActorMethod<
    [bigint],
    { 'Ok' : EvmWalletInfo } |
      { 'Err' : string }
  >,
  /**
   * Execution History
   */
  'get_execution_history' : ActorMethod<
    [[] | [number]],
    Array<ExecutionRecord>
  >,
  /**
   * Message Monitoring
   */
  'get_incoming_messages' : ActorMethod<
    [[] | [number]],
    Array<IncomingMessage>
  >,
  /**
   * Jupiter Swap Integration
   */
  'get_jupiter_quote' : ActorMethod<
    [string, string, bigint, [] | [bigint]],
    { 'Ok' : JupiterQuote } |
      { 'Err' : string }
  >,
  /**
   * LiFi Cross-Chain Bridge
   */
  'get_lifi_quote' : ActorMethod<
    [bigint, bigint, string, string, string],
    { 'Ok' : LiFiBridgeQuote } |
      { 'Err' : string }
  >,
  /**
   * Get platform status (ICP, Cycles, EVM balances)
   */
  'get_platform_status' : ActorMethod<
    [],
    { 'Ok' : PlatformStatus } |
      { 'Err' : string }
  >,
  /**
   * ========== Portfolio Analysis ==========
   */
  'get_portfolio' : ActorMethod<[], { 'Ok' : Portfolio } | { 'Err' : string }>,
  'get_scheduled_posts' : ActorMethod<[], Array<ScheduledPost>>,
  /**
   * Status
   */
  'get_social_status' : ActorMethod<[], SocialStatus>,
  'get_solana_address' : ActorMethod<
    [],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'get_solana_balance' : ActorMethod<
    [string],
    { 'Ok' : bigint } |
      { 'Err' : string }
  >,
  'get_solana_networks' : ActorMethod<[], Array<SolanaNetworkConfig>>,
  'get_solana_transaction_history' : ActorMethod<
    [[] | [number]],
    Array<SolanaTransactionRecord>
  >,
  'get_solana_wallet_info' : ActorMethod<
    [string],
    { 'Ok' : SolanaWalletInfo } |
      { 'Err' : string }
  >,
  'get_spl_token_balance' : ActorMethod<
    [string, string, [] | [string]],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'get_transaction_history' : ActorMethod<
    [[] | [number]],
    Array<TransactionRecord>
  >,
  /**
   * Uniswap/DEX Swap
   */
  'get_uniswap_quote' : ActorMethod<
    [bigint, string, string, string, [] | [number]],
    { 'Ok' : DexSwapQuote } |
      { 'Err' : string }
  >,
  /**
   * ========== ICP Wallet ==========
   */
  'get_wallet_address' : ActorMethod<[], string>,
  'get_wallet_addresses' : ActorMethod<[], Array<[string, string]>>,
  'get_wallet_info' : ActorMethod<[], WalletInfo>,
  'get_wallet_status' : ActorMethod<
    [],
    { 'Ok' : WalletInfo } |
      { 'Err' : string }
  >,
  'get_whitelist' : ActorMethod<[], Array<WhitelistEntry>>,
  'get_whitelist_status' : ActorMethod<[], [boolean, bigint]>,
  /**
   * Health
   */
  'health' : ActorMethod<[], string>,
  /**
   * ========== Solana Wallet (Ed25519) ==========
   */
  'init_solana_wallet' : ActorMethod<
    [],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  /**
   * Immediate Posting
   */
  'post_now' : ActorMethod<
    [SocialPlatform, string],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'remove_whitelist_address' : ActorMethod<
    [bigint],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'reset_solana_wallet' : ActorMethod<[], { 'Ok' : null } | { 'Err' : string }>,
  /**
   * Scheduled Posts
   */
  'schedule_post' : ActorMethod<
    [SocialPlatform, string, bigint, [] | [PostMetadata]],
    { 'Ok' : bigint } |
      { 'Err' : string }
  >,
  /**
   * ERC-20 Token Operations
   */
  'send_erc20' : ActorMethod<
    [bigint, string, string, string],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'send_evm_native' : ActorMethod<
    [bigint, string, string],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'send_icp' : ActorMethod<
    [string, bigint, [] | [bigint]],
    { 'Ok' : bigint } |
      { 'Err' : string }
  >,
  'send_solana' : ActorMethod<
    [string, string, bigint],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  /**
   * SPL Token Operations
   */
  'send_spl_token' : ActorMethod<
    [string, string, string, bigint],
    { 'Ok' : string } |
      { 'Err' : string }
  >,
  'set_admin' : ActorMethod<[Principal], { 'Ok' : null } | { 'Err' : string }>,
  'set_auto_reply' : ActorMethod<
    [boolean],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * Platform Management
   */
  'set_enabled_platforms' : ActorMethod<
    [Array<SocialPlatform>],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * Configuration
   */
  'set_llm_provider' : ActorMethod<
    [LlmProvider],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'set_whitelist_enabled' : ActorMethod<
    [boolean],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * ========== Autonomous Posting ==========
   */
  'start_auto_posting' : ActorMethod<
    [bigint, Array<string>],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'start_d2a_polling' : ActorMethod<
    [bigint],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  /**
   * Polling Control
   */
  'start_social_polling' : ActorMethod<
    [bigint],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'stop_auto_posting' : ActorMethod<[], { 'Ok' : null } | { 'Err' : string }>,
  'stop_d2a_polling' : ActorMethod<[], { 'Ok' : null } | { 'Err' : string }>,
  'stop_social_polling' : ActorMethod<[], { 'Ok' : null } | { 'Err' : string }>,
  /**
   * API Key management (vetKeys)
   */
  'store_encrypted_api_key' : ActorMethod<
    [Uint8Array | number[]],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'transform_d2a_response' : ActorMethod<
    [
      {
        'context' : Uint8Array | number[],
        'response' : {
          'status' : bigint,
          'body' : Uint8Array | number[],
          'headers' : Array<{ 'value' : string, 'name' : string }>,
        },
      },
    ],
    {
      'status' : bigint,
      'body' : Uint8Array | number[],
      'headers' : Array<{ 'value' : string, 'name' : string }>,
    }
  >,
  'transform_evm_response' : ActorMethod<
    [
      {
        'context' : Uint8Array | number[],
        'response' : {
          'status' : bigint,
          'body' : Uint8Array | number[],
          'headers' : Array<{ 'value' : string, 'name' : string }>,
        },
      },
    ],
    {
      'status' : bigint,
      'body' : Uint8Array | number[],
      'headers' : Array<{ 'value' : string, 'name' : string }>,
    }
  >,
  /**
   * Transform functions (internal)
   */
  'transform_openai_response' : ActorMethod<
    [
      {
        'context' : Uint8Array | number[],
        'response' : {
          'status' : bigint,
          'body' : Uint8Array | number[],
          'headers' : Array<{ 'value' : string, 'name' : string }>,
        },
      },
    ],
    {
      'status' : bigint,
      'body' : Uint8Array | number[],
      'headers' : Array<{ 'value' : string, 'name' : string }>,
    }
  >,
  'transform_social_response' : ActorMethod<
    [
      {
        'context' : Uint8Array | number[],
        'response' : {
          'status' : bigint,
          'body' : Uint8Array | number[],
          'headers' : Array<{ 'value' : string, 'name' : string }>,
        },
      },
    ],
    {
      'status' : bigint,
      'body' : Uint8Array | number[],
      'headers' : Array<{ 'value' : string, 'name' : string }>,
    }
  >,
  'transform_solana_response' : ActorMethod<
    [
      {
        'context' : Uint8Array | number[],
        'response' : {
          'status' : bigint,
          'body' : Uint8Array | number[],
          'headers' : Array<{ 'value' : string, 'name' : string }>,
        },
      },
    ],
    {
      'status' : bigint,
      'body' : Uint8Array | number[],
      'headers' : Array<{ 'value' : string, 'name' : string }>,
    }
  >,
  'trigger_auto_post' : ActorMethod<[], { 'Ok' : string } | { 'Err' : string }>,
  'trigger_d2a_fetch' : ActorMethod<[], { 'Ok' : string } | { 'Err' : string }>,
  'trigger_poll' : ActorMethod<[], { 'Ok' : null } | { 'Err' : string }>,
  /**
   * Character management
   */
  'update_character' : ActorMethod<
    [Character],
    { 'Ok' : null } |
      { 'Err' : string }
  >,
  'version' : ActorMethod<[], string>,
}
export declare const idlFactory: IDL.InterfaceFactory;
export declare const init: (args: { IDL: typeof IDL }) => IDL.Type[];
