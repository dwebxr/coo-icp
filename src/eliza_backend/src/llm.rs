//! Turning a conversation into a model request, and asking the model.

use crate::api::{Character, LlmProvider};
use crate::limits::{prefix, MAX_REPLY_BYTES, MODEL_CALL_TIMEOUT_SECS};
use crate::store::{Role, Turn};
use candid::Principal;
use ic_cdk::call::Call;

/// The Internet Computer's LLM canister.
pub const LLM_CANISTER: &str = "w36hm-eqaaa-aaaal-qr76a-cai";
pub const LLM_MODEL: &str = "llama3.1:8b";

// The LLM canister accepts at most 10 messages and 10 KiB of prompt per request. The budget below leaves a margin,
// because how it counts the 10 KiB is not documented.
const MAX_PROMPT_MESSAGES: usize = 10;
const PROMPT_BUDGET_BYTES: usize = 9_000;
/// The system message is cut to this (a character set through `update_character` is far below it; one restored
/// from older state may not be).
const MAX_SYSTEM_BYTES: usize = 3_000;
/// Earlier turns are context, not the question: each is cut to this, so one long reply cannot push out the rest.
const MAX_CONTEXT_TURN_BYTES: usize = 1_200;
const CUT_MARK: &str = " […]";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptMessage {
    pub role: PromptRole,
    pub content: String,
}

/// Everything the character says about itself, as one system message.
pub fn system_text(character: &Character) -> String {
    let mut text = character.system_prompt.trim().to_string();
    for (title, lines) in [("About you:", &character.bio), ("Your style:", &character.style)] {
        let lines: Vec<&str> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() {
            text.push_str("\n\n");
            text.push_str(title);
            for line in lines {
                text.push_str("\n- ");
                text.push_str(line);
            }
        }
    }
    prefix(&text, MAX_SYSTEM_BYTES).to_string()
}

/// The request for `user_message` as the next turn after `turns`: the system message first, always; then as many
/// of the newest turns as fit the LLM canister's limits, starting with a user turn; then the message itself, whole.
pub fn build_prompt(character: &Character, turns: &[Turn], user_message: &str) -> Vec<PromptMessage> {
    let system = system_text(character);
    let mut budget = PROMPT_BUDGET_BYTES.saturating_sub(system.len() + user_message.len());
    let mut context: Vec<PromptMessage> = Vec::new();
    for turn in turns.iter().rev() {
        if context.len() == MAX_PROMPT_MESSAGES - 2 {
            break;
        }
        let content = if turn.content.len() > MAX_CONTEXT_TURN_BYTES {
            format!("{}{}", prefix(&turn.content, MAX_CONTEXT_TURN_BYTES - CUT_MARK.len()), CUT_MARK)
        } else {
            turn.content.clone()
        };
        if content.len() > budget {
            break;
        }
        budget -= content.len();
        let role = match turn.role {
            Role::User => PromptRole::User,
            Role::Assistant => PromptRole::Assistant,
        };
        context.push(PromptMessage { role, content });
    }
    // Oldest first, and not beginning with a reply to a question that is not included.
    while context.last().is_some_and(|m| m.role == PromptRole::Assistant) {
        context.pop();
    }
    context.reverse();

    let mut prompt = Vec::with_capacity(context.len() + 2);
    prompt.push(PromptMessage { role: PromptRole::System, content: system });
    prompt.append(&mut context);
    prompt.push(PromptMessage { role: PromptRole::User, content: user_message.to_string() });
    prompt
}

/// A model reply as it is saved: trimmed, cut to the limit, and never empty.
pub fn clean_reply(reply: &str) -> Option<String> {
    let reply = prefix(reply.trim(), MAX_REPLY_BYTES).trim_end();
    (!reply.is_empty()).then(|| reply.to_string())
}

/// The reply, or a text for the user saying that there is none. Details of a failure go to the canister log.
pub async fn ask(provider: LlmProvider, prompt: Vec<PromptMessage>) -> Result<String, String> {
    let reply = match provider {
        LlmProvider::OnChain => ask_llm_canister(prompt).await?,
        LlmProvider::Fallback => fallback_reply(&prompt),
    };
    clean_reply(&reply).ok_or_else(|| {
        ic_cdk::println!("model: empty reply");
        NO_REPLY.to_string()
    })
}

const NO_REPLY: &str = "The model did not answer.";

async fn ask_llm_canister(prompt: Vec<PromptMessage>) -> Result<String, String> {
    use wire::{AssistantMessage, ChatMessage, Request, Response};

    let messages = prompt
        .into_iter()
        .map(|m| match m.role {
            PromptRole::System => ChatMessage::System { content: m.content },
            PromptRole::User => ChatMessage::User { content: m.content },
            PromptRole::Assistant => {
                ChatMessage::Assistant(AssistantMessage { content: Some(m.content), tool_calls: vec![] })
            }
        })
        .collect();
    let request = Request { model: LLM_MODEL.to_string(), messages, tools: None };
    let llm = Principal::from_text(LLM_CANISTER).expect("LLM_CANISTER is a canister id");

    // Bounded wait: the call ends after the timeout whatever the LLM canister does, so a model that hangs cannot
    // keep this canister from stopping (and so from being upgraded). The price: the answer can get lost even
    // though the model ran. Either way the call is an error here, never a trap.
    let response =
        Call::bounded_wait(llm, "v1_chat").change_timeout(MODEL_CALL_TIMEOUT_SECS).with_arg(&request).await.map_err(
            |e| {
                ic_cdk::println!("model: call failed: {}", e);
                NO_REPLY.to_string()
            },
        )?;
    let response: Response = response.candid().map_err(|e| {
        ic_cdk::println!("model: reply not understood: {}", e);
        NO_REPLY.to_string()
    })?;
    Ok(response.message.content.unwrap_or_default())
}

/// For local replicas, where the LLM canister does not exist: says so and echoes the start of the message.
fn fallback_reply(prompt: &[PromptMessage]) -> String {
    let message = prompt.last().map_or("", |m| m.content.as_str());
    format!("[Local fallback: no model is called on a local replica.] You said: {}", prefix(message, 500))
}

/// The candid types of the LLM canister's `v1_chat`, with the names the `ic-llm` 1.1.0 crate gives them. (That
/// crate is not used: it is built on an older ic-cdk, and its `send()` traps when the call is rejected.)
mod wire {
    use candid::{CandidType, Deserialize};

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub enum ChatMessage {
        #[serde(rename = "user")]
        User { content: String },
        #[serde(rename = "system")]
        System { content: String },
        #[serde(rename = "assistant")]
        Assistant(AssistantMessage),
        #[serde(rename = "tool")]
        Tool { content: String, tool_call_id: String },
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct AssistantMessage {
        pub content: Option<String>,
        pub tool_calls: Vec<ToolCall>,
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct ToolCall {
        pub id: String,
        pub function: FunctionCall,
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct FunctionCall {
        pub name: String,
        pub arguments: Vec<ToolCallArgument>,
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct ToolCallArgument {
        pub name: String,
        pub value: String,
    }

    // Tools are never sent; the types exist so that `tools = null` has the type the LLM canister declares.
    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub enum Tool {
        #[serde(rename = "function")]
        Function(Function),
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct Function {
        pub name: String,
        pub description: Option<String>,
        pub parameters: Option<Parameters>,
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct Parameters {
        #[serde(rename = "type")]
        pub type_: String,
        pub properties: Option<Vec<Property>>,
        pub required: Option<Vec<String>>,
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct Property {
        #[serde(rename = "type")]
        pub type_: String,
        pub name: String,
        pub description: Option<String>,
        #[serde(rename = "enum")]
        pub enum_: Option<Vec<String>>,
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct Request {
        pub model: String,
        pub messages: Vec<ChatMessage>,
        pub tools: Option<Vec<Tool>>,
    }

    #[derive(CandidType, Deserialize, Clone, Debug)]
    pub struct Response {
        pub message: AssistantMessage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::MAX_MESSAGE_BYTES;

    fn character() -> Character {
        Character {
            name: "Coo".into(),
            system_prompt: "You are Coo.".into(),
            bio: vec!["Runs in a canister".into(), " ".into()],
            style: vec!["Friendly".into()],
        }
    }

    fn turn(role: Role, content: &str) -> Turn {
        Turn { role, content: content.to_string(), request_id: None }
    }

    fn exchanges(n: usize) -> Vec<Turn> {
        (0..n).flat_map(|i| [turn(Role::User, &format!("q{i}")), turn(Role::Assistant, &format!("a{i}"))]).collect()
    }

    fn roles(prompt: &[PromptMessage]) -> Vec<PromptRole> {
        prompt.iter().map(|m| m.role).collect()
    }

    fn total(prompt: &[PromptMessage]) -> usize {
        prompt.iter().map(|m| m.content.len()).sum()
    }

    #[test]
    fn the_system_message_carries_the_whole_character() {
        assert_eq!(
            system_text(&character()),
            "You are Coo.\n\nAbout you:\n- Runs in a canister\n\nYour style:\n- Friendly"
        );
        let bare = Character { bio: vec![], style: vec![], ..character() };
        assert_eq!(system_text(&bare), "You are Coo.");
        let huge = Character { system_prompt: "p".repeat(10_000), ..character() };
        assert_eq!(system_text(&huge).len(), MAX_SYSTEM_BYTES);
    }

    #[test]
    fn a_first_message_is_system_then_user() {
        let prompt = build_prompt(&character(), &[], "hello");
        assert_eq!(roles(&prompt), [PromptRole::System, PromptRole::User]);
        assert_eq!(prompt[1].content, "hello");
    }

    #[test]
    fn a_long_conversation_keeps_the_system_message_and_the_newest_turns() {
        let prompt = build_prompt(&character(), &exchanges(30), "now");
        assert_eq!(prompt.len(), MAX_PROMPT_MESSAGES);
        assert_eq!(prompt[0].role, PromptRole::System);
        assert!(prompt[0].content.starts_with("You are Coo."));
        // Eight turns of context = the last four exchanges, oldest first, then the new message.
        let texts: Vec<&str> = prompt[1..].iter().map(|m| m.content.as_str()).collect();
        assert_eq!(texts, ["q26", "a26", "q27", "a27", "q28", "a28", "q29", "a29", "now"]);
        assert_eq!(prompt[1].role, PromptRole::User);
    }

    #[test]
    fn context_never_starts_with_a_reply() {
        // History ending in two replies: the eight newest turns would start with a26, which is dropped.
        let mut turns = exchanges(30);
        turns.push(turn(Role::Assistant, "extra"));
        let prompt = build_prompt(&character(), &turns, "now");
        assert_eq!(prompt[1].role, PromptRole::User);
        assert_eq!(prompt[1].content, "q27");
        assert_eq!(prompt.len(), 9);
        // Only replies in the history (cannot be written by this version): no context at all.
        let prompt = build_prompt(&character(), &[turn(Role::Assistant, "a")], "now");
        assert_eq!(roles(&prompt), [PromptRole::System, PromptRole::User]);
    }

    #[test]
    fn the_prompt_stays_within_the_budget_and_the_message_is_never_cut() {
        let message = "m".repeat(MAX_MESSAGE_BYTES);
        let long = "あ".repeat(4_000); // 12 000 bytes each, as replies saved by older versions can be
        let turns: Vec<Turn> = (0..10).flat_map(|_| [turn(Role::User, &long), turn(Role::Assistant, &long)]).collect();
        let prompt = build_prompt(&character(), &turns, &message);
        assert!(total(&prompt) <= PROMPT_BUDGET_BYTES, "{}", total(&prompt));
        assert_eq!(prompt.last().unwrap().content, message);
        assert_eq!(prompt[0].role, PromptRole::System);
        // Earlier turns are cut, at a character boundary, and marked; some context still fits.
        assert!(prompt.len() > 2);
        for m in &prompt[1..prompt.len() - 1] {
            assert!(m.content.len() <= MAX_CONTEXT_TURN_BYTES && m.content.ends_with(CUT_MARK));
        }
        assert_eq!(prompt[1].role, PromptRole::User);

        // The largest character and the largest message together still leave the message whole.
        let big = Character { system_prompt: "p".repeat(10_000), ..character() };
        let prompt = build_prompt(&big, &turns, &message);
        assert!(total(&prompt) <= PROMPT_BUDGET_BYTES);
        assert_eq!(prompt.last().unwrap().content, message);
    }

    #[test]
    fn replies_are_trimmed_cut_and_never_empty() {
        assert_eq!(clean_reply("  hi\n").as_deref(), Some("hi"));
        assert_eq!(clean_reply(" \n "), None);
        assert_eq!(clean_reply(""), None);
        assert_eq!(clean_reply(&"x".repeat(MAX_REPLY_BYTES + 10)).unwrap().len(), MAX_REPLY_BYTES);
        let cut = clean_reply(&"あ".repeat(MAX_REPLY_BYTES)).unwrap();
        assert!(cut.len() <= MAX_REPLY_BYTES && cut.chars().all(|c| c == 'あ'));
    }

    #[test]
    fn the_fallback_says_what_it_is() {
        let reply = fallback_reply(&build_prompt(&character(), &[], &"x".repeat(2_000)));
        assert!(reply.starts_with("[Local fallback"));
        assert!(reply.len() < 700);
    }
}
