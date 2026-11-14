//! The canister's state: one cell, borrowed for the length of a closure.

use crate::api::{Character, LlmProvider};
use crate::flight::Flights;
use crate::store::Store;
use std::cell::RefCell;

pub struct State {
    pub store: Store,
    pub character: Character,
    pub provider: LlmProvider,
    /// Not saved across upgrades (see flight.rs).
    pub flights: Flights,
}

impl Default for State {
    fn default() -> Self {
        State {
            store: Store::default(),
            character: default_character(),
            provider: LlmProvider::OnChain,
            flights: Flights::default(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn read<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(&s.borrow()))
}

pub fn mutate<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Like `mutate`, for code that may run while the state is borrowed or the thread-local is being torn down (a
/// destructor): it then does nothing.
pub fn try_mutate(f: impl FnOnce(&mut State)) {
    let _ = STATE.try_with(|s| {
        if let Ok(mut state) = s.try_borrow_mut() {
            f(&mut state);
        }
    });
}

pub fn replace(state: State) -> State {
    STATE.with(|s| s.replace(state))
}

pub fn default_character() -> Character {
    Character {
        name: "Coo".to_string(),
        system_prompt: "You are Coo, a helpful AI assistant that runs as a canister on the Internet Computer.

You are:
- Friendly and approachable
- Knowledgeable about blockchain, Web3, and the Internet Computer
- Able to follow the recent conversation

Your responses should be:
- Concise but helpful
- Engaging and conversational
- Accurate; say so when you are not sure"
            .to_string(),
        bio: vec![
            "An AI agent running as a canister smart contract on the Internet Computer".to_string(),
            "Your answers come from the Llama 3.1 8B model served by the Internet Computer's LLM canister".to_string(),
            "An independent Rust implementation; your character format is inspired by elizaOS".to_string(),
        ],
        style: vec!["Friendly".to_string(), "Helpful".to_string(), "Knowledgeable".to_string()],
    }
}
