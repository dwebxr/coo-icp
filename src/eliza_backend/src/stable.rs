//! What survives an upgrade: the conversations, the revision floor, the character and the provider, as one candid
//! value in stable memory behind an 8-byte little-endian length. Reads what 0.6.0 wrote (schema version 2), and
//! cuts it to this version's limits, which 0.6.0 did not have.
//!
//! There is no way back by an upgrade. 0.6.0 cannot read schema 3: it requires a copy of the character in every
//! conversation, so with any entry saved its `post_upgrade` traps and the downgrade is refused (and with none it
//! would drop the revision floor). Going back to 0.6.0 means restoring the canister snapshot taken before the
//! upgrade, which loses everything saved after that snapshot: every exchange, clear and eviction since.

use crate::api::{Character, LlmProvider};
use crate::limits::{MAX_READ_STATE_BYTES, MAX_WRITTEN_STATE_BYTES};
use crate::state::{default_character, State};
use crate::store::{Dropped, Entry, Role, Store, Turn};
use candid::{CandidType, Deserialize, Principal};
use std::collections::HashMap;

/// Version 3 (0.7.0): no system message and no character copy per conversation, `revision_floor` added, no API key.
pub const SCHEMA_VERSION: u32 = 3;
pub const LENGTH_PREFIX_BYTES: usize = 8;

// ----- what is read: the fields of versions 2 and 3 that are still used. Candid skips the others (0.6.0's
// `encrypted_api_key`, the character copy in each conversation, `admin` and `max_conversation_length`).

#[derive(CandidType, Deserialize)]
struct Persisted {
    version: u32,
    conversations: Vec<(Principal, PersistedConversation)>,
    character: Option<Character>,
    config: Option<PersistedConfig>,
    /// Absent before version 3.
    revision_floor: Option<u64>,
}

#[derive(CandidType, Deserialize)]
struct PersistedConversation {
    messages: Vec<PersistedMessage>,
    created_at: u64,
    updated_at: u64,
    revision: Option<u64>,
}

#[derive(CandidType, Deserialize)]
struct PersistedMessage {
    role: String,
    content: String,
    request_id: Option<String>,
}

#[derive(CandidType, Deserialize)]
struct PersistedConfig {
    llm_provider: PersistedProvider,
}

#[derive(CandidType, Deserialize, Clone, Copy)]
enum PersistedProvider {
    OnChain,
    /// Removed in 0.7.0; read as OnChain.
    OpenAI,
    Fallback,
}

// ----- what is written: the same candid types, borrowing from the state so nothing is copied before encoding.

#[derive(CandidType)]
struct PersistedOut<'a> {
    version: u32,
    conversations: Vec<(&'a Principal, ConversationOut<'a>)>,
    character: Option<&'a Character>,
    config: Option<PersistedConfig>,
    revision_floor: Option<u64>,
}

#[derive(CandidType)]
struct ConversationOut<'a> {
    messages: Vec<MessageOut<'a>>,
    created_at: u64,
    updated_at: u64,
    revision: Option<u64>,
}

#[derive(CandidType)]
struct MessageOut<'a> {
    role: &'static str,
    content: &'a str,
    request_id: Option<&'a str>,
}

/// The candid payload of the state (without the length prefix).
pub fn encode(state: &State) -> Result<Vec<u8>, String> {
    let conversations = state
        .store
        .iter()
        .map(|(who, entry)| {
            let messages = entry
                .turns
                .iter()
                .map(|t| MessageOut { role: t.role.as_str(), content: &t.content, request_id: t.request_id.as_deref() })
                .collect();
            let conversation = ConversationOut {
                messages,
                created_at: entry.created_at,
                updated_at: entry.updated_at,
                revision: Some(entry.revision),
            };
            (who, conversation)
        })
        .collect();
    let llm_provider = match state.provider {
        LlmProvider::OnChain => PersistedProvider::OnChain,
        LlmProvider::Fallback => PersistedProvider::Fallback,
    };
    let out = PersistedOut {
        version: SCHEMA_VERSION,
        conversations,
        character: Some(&state.character),
        config: Some(PersistedConfig { llm_provider }),
        revision_floor: Some(state.store.revision_floor()),
    };
    let payload = candid::encode_one(&out).map_err(|e| e.to_string())?;
    // Cannot happen while the store keeps its budget (see limits.rs); refused here rather than written.
    if payload.len() > MAX_WRITTEN_STATE_BYTES {
        return Err(format!("state is {} bytes; the limit is {}", payload.len(), MAX_WRITTEN_STATE_BYTES));
    }
    Ok(payload)
}

/// The payload length announced by the 8-byte prefix: anything up to what 0.6.0 itself could read back.
pub fn payload_len(prefix: [u8; LENGTH_PREFIX_BYTES]) -> Result<usize, String> {
    let len = u64::from_le_bytes(prefix);
    match usize::try_from(len) {
        Ok(len) if len > 0 && len <= MAX_READ_STATE_BYTES => Ok(len),
        _ => Err(format!("invalid saved state length {len}")),
    }
}

pub struct Loaded {
    pub state: State,
    /// The schema version the payload was written with.
    pub version: u32,
    /// For state written before schema 3, what was cut to bring it within the limits. State of schema 3 was
    /// written by this version within them and is taken as it is.
    pub dropped: Option<Dropped>,
}

/// The state saved in `payload`. Anything that cannot be read in full is an error: the caller traps, which rolls
/// the upgrade back, rather than starting with part of the state.
pub fn decode(payload: &[u8]) -> Result<Loaded, String> {
    let persisted: Persisted = candid::decode_one(payload).map_err(|e| e.to_string())?;
    let version = persisted.version;
    if version > SCHEMA_VERSION {
        return Err(format!("state was saved by a newer version (schema {version}; this is {SCHEMA_VERSION})"));
    }
    // Every version writes these. Candid decodes an `opt` it cannot read as null instead of failing, so a missing
    // value here means the state was not understood.
    let character = persisted.character.ok_or("saved state has no character")?;
    let config = persisted.config.ok_or("saved state has no config")?;
    let revision_floor = match persisted.revision_floor {
        Some(floor) => floor,
        None if version < 3 => 0,
        None => return Err("saved state has no revision floor".to_string()),
    };

    let mut entries = HashMap::with_capacity(persisted.conversations.len());
    for (who, conversation) in persisted.conversations {
        let mut turns = Vec::with_capacity(conversation.messages.len());
        for message in conversation.messages {
            let role = match message.role.as_str() {
                "user" => Role::User,
                "assistant" => Role::Assistant,
                // Versions before 3 saved the system prompt as each conversation's first message.
                "system" if version < 3 => continue,
                other => return Err(format!("saved message has role {other:?}")),
            };
            turns.push(Turn { role, content: message.content, request_id: message.request_id });
        }
        // A conversation trimmed by a version before 3 can start with a reply whose question is gone.
        let orphans = turns.iter().take_while(|t| t.role == Role::Assistant).count();
        turns.drain(..orphans);
        let entry = Entry {
            turns,
            revision: conversation.revision.unwrap_or(0),
            created_at: conversation.created_at,
            updated_at: conversation.updated_at,
        };
        entries.insert(who, entry);
    }

    let mut store = Store::from_parts(entries, revision_floor);
    let dropped = (version < 3).then(|| store.fit_to_limits());
    let state = State {
        store,
        // The default character of 0.6.0 and earlier claimed to be built on elizaOS. A character that was never
        // changed is replaced by the current default; a customised one is kept as it is.
        character: if character == default_character_before_v3() { default_character() } else { character },
        provider: match config.llm_provider {
            PersistedProvider::OnChain | PersistedProvider::OpenAI => LlmProvider::OnChain,
            PersistedProvider::Fallback => LlmProvider::Fallback,
        },
        flights: Default::default(),
    };
    Ok(Loaded { state, version, dropped })
}

/// The default character of every version up to 0.6.0, to recognise a character that was never customised.
fn default_character_before_v3() -> Character {
    Character {
        name: "Coo".to_string(),
        system_prompt: r#"You are Coo, a helpful AI assistant built on the elizaOS framework, running fully on-chain on the Internet Computer.

You are:
- Friendly and approachable
- Knowledgeable about blockchain, Web3, and the Internet Computer
- Running as a decentralized, censorship-resistant AI agent
- Built on elizaOS - the leading open-source AI agent framework
- Capable of maintaining context across conversations

Your responses should be:
- Concise but helpful
- Engaging and conversational
- Accurate and informative"#.to_string(),
        bio: vec![
            "On-chain AI agent powered by elizaOS and Internet Computer".to_string(),
            "Fully decentralized and censorship-resistant".to_string(),
            "Built on elizaOS framework for autonomous AI agents".to_string(),
        ],
        style: vec![
            "Friendly".to_string(),
            "Helpful".to_string(),
            "Knowledgeable".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{
        ENTRY_OVERHEAD_BYTES, MAX_CONVERSATION_BYTES, MAX_ENTRIES, MAX_MESSAGE_BYTES, MAX_REPLY_BYTES, MAX_STORE_BYTES,
        TURN_OVERHEAD_BYTES,
    };

    fn who(n: u8) -> Principal {
        Principal::from_slice(&[n; 29])
    }

    /// The state types exactly as 0.6.0 (schema version 2) encodes them.
    mod v2 {
        use crate::api::Character;
        use candid::{CandidType, Principal};
        use std::collections::HashMap;

        #[derive(CandidType, Clone)]
        pub struct Message {
            pub role: String,
            pub content: String,
            pub request_id: Option<String>,
        }

        #[derive(CandidType)]
        pub struct ConversationState {
            pub messages: Vec<Message>,
            pub character: Character,
            pub created_at: u64,
            pub updated_at: u64,
            pub revision: Option<u64>,
        }

        #[derive(CandidType)]
        pub enum LlmProvider {
            OnChain,
            OpenAI,
            Fallback,
        }

        #[derive(CandidType)]
        pub struct Config {
            pub llm_provider: LlmProvider,
            pub max_conversation_length: usize,
            pub admin: Principal,
        }

        #[derive(CandidType)]
        pub struct StableState {
            pub version: u32,
            pub conversations: HashMap<Principal, ConversationState>,
            pub encrypted_api_key: Option<Vec<u8>>,
            pub character: Option<Character>,
            pub config: Option<Config>,
        }

        pub fn message(role: &str, content: &str, request_id: Option<&str>) -> Message {
            Message { role: role.into(), content: content.into(), request_id: request_id.map(Into::into) }
        }
    }

    fn v2_state(provider: v2::LlmProvider, character: Character) -> v2::StableState {
        let conversation = |messages: Vec<v2::Message>, revision: Option<u64>| v2::ConversationState {
            messages,
            character: default_character_before_v3(),
            created_at: 1,
            updated_at: 2,
            revision,
        };
        let conversations = [
            (
                who(1),
                conversation(
                    vec![
                        v2::message("system", "prompt", None),
                        v2::message("user", "hi", Some("r1")),
                        v2::message("assistant", "hello", None),
                    ],
                    Some(1),
                ),
            ),
            // Trimmed by 0.6.0: starts with a reply. From before 0.6.0: no revision, no request ids.
            (
                who(2),
                conversation(
                    vec![
                        v2::message("system", "prompt", None),
                        v2::message("assistant", "orphan", None),
                        v2::message("user", "q", None),
                        v2::message("assistant", "a", None),
                    ],
                    None,
                ),
            ),
            // Cleared.
            (who(3), conversation(vec![v2::message("system", "prompt", None)], Some(4))),
        ];
        v2::StableState {
            version: 2,
            conversations: conversations.into_iter().collect(),
            encrypted_api_key: Some(b"sk-secret".to_vec()),
            character: Some(character),
            config: Some(v2::Config { llm_provider: provider, max_conversation_length: 50, admin: who(9) }),
        }
    }

    fn contents(turns: &[Turn]) -> Vec<&str> {
        turns.iter().map(|t| t.content.as_str()).collect()
    }

    #[test]
    fn reads_what_the_deployed_version_wrote() {
        let old = v2_state(v2::LlmProvider::Fallback, default_character_before_v3());
        let loaded = decode(&candid::encode_one(&old).unwrap()).unwrap();
        assert_eq!(loaded.version, 2);
        // Within every limit already: nothing is cut.
        assert_eq!(loaded.dropped, Some(Dropped::default()));
        let store = &loaded.state.store;
        assert_eq!((store.entries(), store.conversations(), store.revision_floor()), (3, 2, 0));

        assert_eq!(contents(store.turns(&who(1))), ["hi", "hello"]);
        assert_eq!(store.revision_of(&who(1)), 1);
        assert_eq!(store.saved_exchange(&who(1), "r1"), Some(("hi", "hello")));
        let entry = store.iter().find(|(p, _)| **p == who(1)).unwrap().1;
        assert_eq!((entry.created_at, entry.updated_at), (1, 2));

        assert_eq!(contents(store.turns(&who(2))), ["q", "a"]);
        assert_eq!(store.revision_of(&who(2)), 0);
        assert!(store.turns(&who(3)).is_empty());
        assert_eq!(store.revision_of(&who(3)), 4);
        assert_eq!(store.bytes(), store.iter().map(|(_, e)| e.bytes()).sum::<usize>());

        assert_eq!(loaded.state.provider, LlmProvider::Fallback);
        // The untouched old default character is replaced; nothing of the key survives.
        assert_eq!(loaded.state.character, default_character());
        let again = encode(&loaded.state).unwrap();
        assert!(!again.windows(9).any(|w| w == b"sk-secret"));
    }

    #[test]
    fn keeps_a_customised_character_and_maps_the_removed_provider() {
        let custom = Character { name: "Zoe".into(), ..default_character_before_v3() };
        let old = v2_state(v2::LlmProvider::OpenAI, custom.clone());
        let loaded = decode(&candid::encode_one(&old).unwrap()).unwrap();
        assert_eq!(loaded.state.character, custom);
        assert_eq!(loaded.state.provider, LlmProvider::OnChain);
    }

    #[test]
    fn refuses_state_it_does_not_fully_understand() {
        let mut no_config = v2_state(v2::LlmProvider::OnChain, default_character());
        no_config.config = None;
        assert!(decode(&candid::encode_one(&no_config).unwrap()).is_err());
        let mut no_character = v2_state(v2::LlmProvider::OnChain, default_character());
        no_character.character = None;
        assert!(decode(&candid::encode_one(&no_character).unwrap()).is_err());
        let mut newer = v2_state(v2::LlmProvider::OnChain, default_character());
        newer.version = SCHEMA_VERSION + 1;
        assert!(decode(&candid::encode_one(&newer).unwrap()).err().unwrap().contains("newer version"));
        // A version-3 state always has a revision floor (this one, built from the old types, does not).
        let mut no_floor = v2_state(v2::LlmProvider::OnChain, default_character());
        no_floor.version = 3;
        assert!(decode(&candid::encode_one(&no_floor).unwrap()).is_err());
        // An unknown role is not silently dropped.
        let mut odd = v2_state(v2::LlmProvider::OnChain, default_character());
        odd.conversations.get_mut(&who(1)).unwrap().messages.push(v2::message("tool", "x", None));
        assert!(decode(&candid::encode_one(&odd).unwrap()).is_err());

        assert!(decode(&[]).is_err());
        assert!(decode(b"junk").is_err());
        let good = candid::encode_one(v2_state(v2::LlmProvider::OnChain, default_character())).unwrap();
        assert!(decode(&good[..good.len() - 1]).is_err());
    }

    #[test]
    fn the_length_prefix_is_checked() {
        assert_eq!(payload_len(100u64.to_le_bytes()), Ok(100));
        // Everything 0.6.0 could read back itself (its limit was 100 000 000 bytes), more than this version writes.
        assert_eq!(payload_len(100_000_000u64.to_le_bytes()), Ok(100_000_000));
        assert!(payload_len(0u64.to_le_bytes()).is_err());
        assert!(payload_len((MAX_READ_STATE_BYTES as u64 + 1).to_le_bytes()).is_err());
        // 2^32 + 100 must not wrap to 100 on a 32-bit target.
        assert!(payload_len(((1u64 << 32) + 100).to_le_bytes()).is_err());
    }

    fn v2_conversation(messages: Vec<v2::Message>, revision: u64, updated_at: u64) -> v2::ConversationState {
        // A small character, so that big states stay cheap to build; 0.6.0 kept a copy in every conversation.
        let character = Character { name: "C".into(), system_prompt: "p".into(), bio: vec![], style: vec![] };
        v2::ConversationState { messages, character, created_at: 0, updated_at, revision: Some(revision) }
    }

    fn v2_with(conversations: Vec<(Principal, v2::ConversationState)>) -> Vec<u8> {
        let mut state = v2_state(v2::LlmProvider::OnChain, default_character());
        state.conversations = conversations.into_iter().collect();
        candid::encode_one(&state).unwrap()
    }

    fn many(n: u32) -> Principal {
        let mut id = [7u8; 29];
        id[..4].copy_from_slice(&n.to_be_bytes());
        Principal::from_slice(&id)
    }

    #[test]
    fn an_oversized_conversation_from_the_deployed_version_is_cut_from_the_front() {
        // 0.6.0 had no message limit: 30 exchanges of 2 × 30 000 bytes, after its system message.
        let big = "x".repeat(30_000);
        let mut messages = vec![v2::message("system", "prompt", None)];
        for i in 0..30 {
            messages.push(v2::message("user", &big, Some(&format!("r{i:02}"))));
            messages.push(v2::message("assistant", &format!("{i:02}{big}"), None));
        }
        let small = vec![v2::message("user", "hi", Some("s1")), v2::message("assistant", "hello", None)];
        let loaded =
            decode(&v2_with(vec![(who(1), v2_conversation(messages, 30, 5)), (who(2), v2_conversation(small, 1, 1))]))
                .unwrap();
        let store = &loaded.state.store;

        let kept = store.turns(&who(1)).len() / 2;
        assert!(kept > 0 && kept < 25, "{kept}");
        let entry = |p: Principal| store.iter().find(|(w, _)| **w == p).unwrap().1.clone();
        assert!(entry(who(1)).bytes() <= MAX_CONVERSATION_BYTES);
        // The newest exchanges stay, and replay; the oldest are gone, and the revision is the same.
        let first_kept = 30 - kept;
        assert_eq!(store.turns(&who(1))[0].request_id, Some(format!("r{first_kept:02}")));
        assert_eq!(store.saved_exchange(&who(1), "r29").map(|(_, a)| &a[..2]), Some("29"));
        assert!(store.saved_exchange(&who(1), &format!("r{first_kept:02}")).is_some());
        assert_eq!(store.saved_exchange(&who(1), &format!("r{:02}", first_kept - 1)), None);
        assert_eq!(store.revision_of(&who(1)), 30);
        // An exchange more would not have fitted.
        assert!(entry(who(1)).bytes() + 2 * (TURN_OVERHEAD_BYTES + 30_000) + 5 > MAX_CONVERSATION_BYTES);
        // The small one is untouched.
        assert_eq!(store.saved_exchange(&who(2), "s1"), Some(("hi", "hello")));
        assert_eq!(loaded.dropped, Some(Dropped { turns: 60 - 2 * kept, entries: 0 }));
        assert_eq!(store.bytes(), store.iter().map(|(_, e)| e.bytes()).sum::<usize>());
    }

    #[test]
    fn a_deployed_state_over_the_store_budget_loses_its_least_recently_used_entries() {
        // Conversations of 8 exchanges of 2 × 30 000 bytes each (under the conversation limit), enough of them to be
        // over the store budget; and one cleared conversation, the most recently updated of all.
        let big = "x".repeat(30_000);
        let conversation = |n: u32| {
            let messages = (0..8)
                .flat_map(|i| {
                    [v2::message("user", &big, Some(&format!("r{n:05}-{i}"))), v2::message("assistant", &big, None)]
                })
                .collect();
            // Older conversations have lower revisions here, so the floor shows which went.
            v2_conversation(messages, 100 + n as u64, 10 + n as u64)
        };
        let one = ENTRY_OVERHEAD_BYTES + 8 * (2 * TURN_OVERHEAD_BYTES + 2 * 30_000 + "r00000-0".len());
        let count = (MAX_STORE_BYTES / one + 3) as u32;
        let mut conversations: Vec<_> = (0..count).map(|n| (many(n), conversation(n))).collect();
        conversations.push((who(1), v2_conversation(vec![v2::message("system", "prompt", None)], 7, 1_000)));

        let loaded = decode(&v2_with(conversations)).unwrap();
        let store = &loaded.state.store;
        assert!(store.bytes() <= MAX_STORE_BYTES);
        // The empty entry went first, then the least recently updated conversations until the rest fit.
        let kept = (MAX_STORE_BYTES / one) as u32;
        assert!(!store.contains(&who(1)));
        assert_eq!(store.entries(), kept as usize);
        let gone = count - kept;
        assert!((0..gone).all(|n| !store.contains(&many(n))));
        assert!((gone..count).all(|n| store.contains(&many(n)) && store.turns(&many(n)).len() == 16));
        // Whoever was removed is now past every revision it had.
        assert_eq!(store.revision_floor(), 100 + gone as u64);
        assert_eq!(store.revision_of(&many(0)), 100 + gone as u64);
        assert_eq!(loaded.dropped, Some(Dropped { turns: 0, entries: 1 + gone as usize }));
    }

    #[test]
    fn a_deployed_state_over_the_entry_limit_loses_its_least_recently_used_entries() {
        let empty = |n: u32| v2_conversation(vec![v2::message("system", "p", None)], 1, n as u64);
        let conversations = (0..MAX_ENTRIES as u32 + 5).map(|n| (many(n), empty(n))).collect();
        let loaded = decode(&v2_with(conversations)).unwrap();
        assert_eq!(loaded.state.store.entries(), MAX_ENTRIES);
        assert!((0..5).all(|n| !loaded.state.store.contains(&many(n))));
        assert_eq!(loaded.dropped, Some(Dropped { turns: 0, entries: 5 }));
    }

    #[test]
    fn state_of_this_version_is_taken_as_it_is() {
        // Schema 3 is written by this version within the limits, so it is not cut on reading, even where a
        // conversation is bigger than this version would let it grow.
        let mut state = sample_state();
        let turns: Vec<Turn> = (0..20)
            .flat_map(|i| {
                [
                    Turn { role: Role::User, content: "x".repeat(30_000), request_id: Some(format!("b{i}")) },
                    Turn { role: Role::Assistant, content: "y".repeat(30_000), request_id: None },
                ]
            })
            .collect();
        let mut entries: HashMap<_, _> = state.store.iter().map(|(p, e)| (*p, e.clone())).collect();
        entries.insert(who(5), Entry { turns, revision: 20, created_at: 0, updated_at: 0 });
        state.store = Store::from_parts(entries, state.store.revision_floor());
        let loaded = decode(&encode(&state).unwrap()).unwrap();
        assert_eq!(loaded.dropped, None);
        assert_eq!(loaded.state.store.turns(&who(5)).len(), 40);
        assert_eq!(loaded.state.store.bytes(), state.store.bytes());
    }

    fn sample_state() -> State {
        let mut store = Store::default();
        store.append_exchange(who(1), "q".into(), "a".into(), "r1".into(), 5);
        store.append_exchange(who(1), "q2".into(), "a2".into(), "r2".into(), 6);
        store.clear(who(2), 7);
        let entries = store.iter().map(|(p, e)| (*p, e.clone())).collect();
        State {
            // A floor above zero, as after an eviction.
            store: Store::from_parts(entries, 2),
            character: Character { name: "Zoe".into(), ..default_character() },
            provider: LlmProvider::Fallback,
            flights: Default::default(),
        }
    }

    #[test]
    fn the_current_state_round_trips() {
        let state = sample_state();
        let loaded = decode(&encode(&state).unwrap()).unwrap();
        assert_eq!(loaded.version, SCHEMA_VERSION);
        let (before, after) = (&state.store, &loaded.state.store);
        assert_eq!(after.revision_floor(), 2);
        assert_eq!((after.entries(), after.bytes()), (before.entries(), before.bytes()));
        for (who, entry) in before.iter() {
            assert_eq!(after.iter().find(|(p, _)| *p == who).map(|(_, e)| e), Some(entry));
        }
        assert_eq!(after.saved_exchange(&who(1), "r2"), Some(("q2", "a2")));
        assert_eq!(after.revision_of(&who(9)), 2);
        assert_eq!(loaded.state.character, state.character);
        assert_eq!(loaded.state.provider, LlmProvider::Fallback);
    }

    #[test]
    fn the_encoded_state_is_smaller_than_the_counted_bytes() {
        // The store budget counts ENTRY_OVERHEAD_BYTES per entry and TURN_OVERHEAD_BYTES per turn on top of the
        // text. The encoded state must stay below that count (plus the fixed part), so that keeping the budget
        // keeps the state far below MAX_STATE_BYTES. Checked for the shapes with the least text per entry and
        // turn, where overhead dominates, and for full-size turns.
        let fixed = encode(&State::default()).unwrap().len();
        let mut state = State::default();
        for n in 0..200u8 {
            let caller = Principal::from_slice(&[
                n, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
                28,
            ]);
            match n % 4 {
                0 => {
                    state.store.clear(caller, u64::MAX);
                }
                1 => {
                    for i in 0..25u64 {
                        state.store.append_exchange(caller, String::new(), "a".into(), "r".into(), u64::MAX - i);
                    }
                }
                2 => {
                    state.store.append_exchange(
                        caller,
                        "x".repeat(MAX_MESSAGE_BYTES),
                        "y".repeat(MAX_REPLY_BYTES),
                        "r".repeat(64),
                        u64::MAX,
                    );
                }
                _ => {
                    state.store.append_exchange(
                        caller,
                        "あ".into(),
                        "い".into(),
                        "0f8c2c1e-4b7a-4d2e-9a51-8f2b7c6d5e4a".into(),
                        1,
                    );
                }
            }
        }
        let encoded = encode(&state).unwrap().len();
        assert!(encoded <= fixed + state.store.bytes(), "encoded {encoded}, counted {}", state.store.bytes());
        // And the margin is real: an empty entry encodes to well under its counted overhead.
        let mut empties = State::default();
        for n in 0..100u8 {
            empties.store.clear(Principal::from_slice(&[n; 29]), u64::MAX);
        }
        let per_entry = (encode(&empties).unwrap().len() - fixed) / 100;
        assert!(per_entry < ENTRY_OVERHEAD_BYTES, "{per_entry}");
    }
}
