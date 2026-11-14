//! The conversations: what they hold, how many bytes they count for, and how room is made when the budget is full.
//! Nothing here touches the IC API, so all of it runs in unit tests.

use crate::limits::{
    ENTRY_OVERHEAD_BYTES, EVICTION_TARGET_PERCENT, MAX_ENTRIES, MAX_STORE_BYTES, MAX_TURNS, TURN_OVERHEAD_BYTES,
};
use candid::Principal;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub role: Role,
    pub content: String,
    /// On a user turn sent with `send_message`: its request id, by which a repeated request is recognised.
    pub request_id: Option<String>,
}

impl Turn {
    fn bytes(&self) -> usize {
        TURN_OVERHEAD_BYTES + self.content.len() + self.request_id.as_ref().map_or(0, String::len)
    }
}

/// One caller's conversation. A cleared conversation is an entry without turns: it only keeps the revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub turns: Vec<Turn>,
    /// Bumped by every saved exchange and every clear.
    pub revision: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Entry {
    pub fn bytes(&self) -> usize {
        ENTRY_OVERHEAD_BYTES + self.turns.iter().map(Turn::bytes).sum::<usize>()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StoreLimits {
    pub max_bytes: usize,
    pub max_entries: usize,
}

impl Default for StoreLimits {
    fn default() -> Self {
        StoreLimits { max_bytes: MAX_STORE_BYTES, max_entries: MAX_ENTRIES }
    }
}

#[derive(Default)]
pub struct Store {
    entries: HashMap<Principal, Entry>,
    /// Sum of `Entry::bytes` over all entries.
    bytes: usize,
    /// The revision of every caller that has no entry. Removing an entry raises it above that entry's revision,
    /// so no caller's revision ever goes back, and a request made for an earlier revision can never match again.
    revision_floor: u64,
    limits: StoreLimits,
}

impl Store {
    pub fn from_parts(entries: HashMap<Principal, Entry>, revision_floor: u64) -> Self {
        let bytes = entries.values().map(Entry::bytes).sum();
        Store { entries, bytes, revision_floor, limits: StoreLimits::default() }
    }

    #[cfg(test)]
    pub fn with_limits(limits: StoreLimits) -> Self {
        Store { limits, ..Store::default() }
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn entries(&self) -> usize {
        self.entries.len()
    }

    /// Conversations with at least one saved exchange.
    pub fn conversations(&self) -> usize {
        self.entries.values().filter(|e| !e.turns.is_empty()).count()
    }

    pub fn revision_floor(&self) -> u64 {
        self.revision_floor
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Principal, &Entry)> {
        self.entries.iter()
    }

    pub fn contains(&self, who: &Principal) -> bool {
        self.entries.contains_key(who)
    }

    pub fn revision_of(&self, who: &Principal) -> u64 {
        self.entries.get(who).map_or(self.revision_floor, |e| e.revision)
    }

    pub fn turns(&self, who: &Principal) -> &[Turn] {
        self.entries.get(who).map_or(&[], |e| &e.turns)
    }

    /// The saved user message with this request id and the reply right after it. (Trimming never leaves a user
    /// turn without the reply that follows it.)
    pub fn saved_exchange(&self, who: &Principal, request_id: &str) -> Option<(&str, &str)> {
        let turns = self.turns(who);
        let i = turns.iter().position(|t| t.role == Role::User && t.request_id.as_deref() == Some(request_id))?;
        let reply = turns.get(i + 1).filter(|t| t.role == Role::Assistant)?;
        Some((&turns[i].content, &reply.content))
    }

    /// Gives `who` an entry at `revision` if it has none. The caller reads the revision before making room (which
    /// may raise the floor) and pins it here, so the revision its request was checked against stays the one that
    /// is saved.
    pub fn pin(&mut self, who: Principal, revision: u64, now: u64) {
        if !self.entries.contains_key(&who) {
            self.bytes += ENTRY_OVERHEAD_BYTES;
            self.entries.insert(who, Entry { turns: Vec::new(), revision, created_at: now, updated_at: now });
        }
    }

    /// Saves an exchange and returns the new revision.
    pub fn append_exchange(
        &mut self,
        who: Principal,
        user: String,
        reply: String,
        request_id: String,
        now: u64,
    ) -> u64 {
        let floor = self.revision_floor;
        let entry = self.entries.entry(who).or_insert_with(|| {
            self.bytes += ENTRY_OVERHEAD_BYTES;
            Entry { turns: Vec::new(), revision: floor, created_at: now, updated_at: now }
        });
        let before = entry.bytes();
        entry.turns.push(Turn { role: Role::User, content: user, request_id: Some(request_id) });
        entry.turns.push(Turn { role: Role::Assistant, content: reply, request_id: None });
        trim(&mut entry.turns);
        entry.revision += 1;
        entry.updated_at = now;
        self.bytes = self.bytes - before + entry.bytes();
        entry.revision
    }

    /// Replaces the conversation by an empty one with the next revision (also when there was none, so that a
    /// request sent before the clear cannot run after it) and returns that revision.
    pub fn clear(&mut self, who: Principal, now: u64) -> u64 {
        let revision = self.revision_of(&who) + 1;
        let fresh = Entry { turns: Vec::new(), revision, created_at: now, updated_at: now };
        self.bytes += fresh.bytes();
        if let Some(old) = self.entries.insert(who, fresh) {
            self.bytes -= old.bytes();
        }
        revision
    }

    /// Makes room for `extra_bytes` more and, with `new_entry`, for one more entry, by removing the least recently
    /// updated entries that `protected` does not name. Returns false if that is not enough.
    /// One pass over all entries, and only when a limit is reached; it then frees down to the eviction target, so
    /// the next pass is a tenth of the budget away.
    pub fn make_room(&mut self, extra_bytes: usize, new_entry: bool, protected: impl Fn(&Principal) -> bool) -> bool {
        let StoreLimits { max_bytes, max_entries } = self.limits;
        let fits = |store: &Store| {
            store.bytes + extra_bytes <= max_bytes && !(new_entry && store.entries.len() >= max_entries)
        };
        if fits(self) {
            return true;
        }
        let bytes_target = if self.bytes + extra_bytes > max_bytes {
            (max_bytes / 100 * EVICTION_TARGET_PERCENT).saturating_sub(extra_bytes)
        } else {
            usize::MAX
        };
        let entries_target = if new_entry && self.entries.len() >= max_entries {
            (max_entries * EVICTION_TARGET_PERCENT / 100).min(max_entries.saturating_sub(1))
        } else {
            usize::MAX
        };
        let mut candidates: Vec<(u64, Principal)> = self
            .entries
            .iter()
            .filter(|(who, _)| !protected(who))
            .map(|(who, entry)| (entry.updated_at, *who))
            .collect();
        candidates.sort_unstable();
        for (_, who) in candidates {
            if self.bytes <= bytes_target && self.entries.len() <= entries_target {
                break;
            }
            self.remove(&who);
        }
        fits(self)
    }

    fn remove(&mut self, who: &Principal) {
        if let Some(entry) = self.entries.remove(who) {
            self.bytes -= entry.bytes();
            self.revision_floor = self.revision_floor.max(entry.revision + 1);
        }
    }

    #[cfg(test)]
    fn recount(&self) -> usize {
        self.entries.values().map(Entry::bytes).sum()
    }
}

/// Keeps the newest `MAX_TURNS` turns, and never starts with a reply whose question is gone.
pub fn trim(turns: &mut Vec<Turn>) {
    let excess = turns.len().saturating_sub(MAX_TURNS);
    let orphans = turns[excess..].iter().take_while(|t| t.role == Role::Assistant).count();
    turns.drain(..excess + orphans);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(n: u8) -> Principal {
        Principal::from_slice(&[n; 29])
    }

    fn user(i: usize) -> Turn {
        Turn { role: Role::User, content: format!("q{i}"), request_id: Some(format!("r{i}")) }
    }

    fn assistant(i: usize) -> Turn {
        Turn { role: Role::Assistant, content: format!("a{i}"), request_id: None }
    }

    fn add(store: &mut Store, n: u8, i: usize, now: u64) -> u64 {
        store.append_exchange(who(n), format!("q{i}"), format!("a{i}"), format!("r{i}"), now)
    }

    #[test]
    fn trim_keeps_the_newest_turns_and_starts_with_a_question() {
        let mut turns: Vec<Turn> = (0..30).flat_map(|i| [user(i), assistant(i)]).collect();
        trim(&mut turns);
        assert_eq!(turns.len(), MAX_TURNS);
        assert_eq!((turns[0].content.as_str(), turns[49].content.as_str()), ("q5", "a29"));

        let mut short: Vec<Turn> = (0..3).flat_map(|i| [user(i), assistant(i)]).collect();
        trim(&mut short);
        assert_eq!(short.len(), 6);

        // State from before 0.7.0 can start with a reply (its question was trimmed away) and be one turn longer.
        let mut legacy: Vec<Turn> = vec![assistant(0)];
        legacy.extend((1..=25).flat_map(|i| [user(i), assistant(i)]));
        assert_eq!(legacy.len(), 51);
        trim(&mut legacy);
        assert_eq!((legacy.len(), legacy[0].content.as_str()), (50, "q1"));

        let mut only_replies = vec![assistant(0), assistant(1)];
        trim(&mut only_replies);
        assert!(only_replies.is_empty());
    }

    #[test]
    fn exchanges_replay_until_trimmed_away() {
        let mut store = Store::default();
        for i in 0..30 {
            assert_eq!(add(&mut store, 7, i, 0), i as u64 + 1);
        }
        assert_eq!(store.revision_of(&who(7)), 30);
        assert_eq!(store.turns(&who(7)).len(), 50);
        assert_eq!(store.saved_exchange(&who(7), "r29"), Some(("q29", "a29")));
        assert_eq!(store.saved_exchange(&who(7), "r5"), Some(("q5", "a5")));
        assert_eq!(store.saved_exchange(&who(7), "r4"), None);
        assert_eq!(store.saved_exchange(&who(8), "r29"), None);
        assert_eq!(store.revision_of(&who(8)), 0);
        assert_eq!((store.entries(), store.conversations()), (1, 1));
    }

    #[test]
    fn a_question_without_its_reply_does_not_replay() {
        // Not written by any version, but must not be mistaken for a saved exchange.
        let mut entries = HashMap::new();
        entries.insert(
            who(1),
            Entry { turns: vec![user(0), user(1), assistant(1)], revision: 2, created_at: 0, updated_at: 0 },
        );
        let store = Store::from_parts(entries, 0);
        assert_eq!(store.saved_exchange(&who(1), "r0"), None);
        assert_eq!(store.saved_exchange(&who(1), "r1"), Some(("q1", "a1")));
    }

    #[test]
    fn clearing_moves_the_revision_on_even_without_a_conversation() {
        let mut store = Store::default();
        add(&mut store, 1, 0, 0);
        add(&mut store, 1, 1, 0);
        assert_eq!(store.clear(who(1), 5), 3);
        assert!(store.turns(&who(1)).is_empty());
        assert_eq!(store.saved_exchange(&who(1), "r0"), None);
        assert_eq!(store.clear(who(2), 5), 1);
        assert_eq!(store.revision_of(&who(2)), 1);
        assert_eq!((store.entries(), store.conversations()), (2, 0));
        assert_eq!(store.bytes(), 2 * ENTRY_OVERHEAD_BYTES);
        assert_eq!(add(&mut store, 1, 2, 6), 4);
    }

    #[test]
    fn pinning_keeps_the_revision_a_request_was_checked_against() {
        let mut store = Store::with_limits(StoreLimits { max_bytes: usize::MAX, max_entries: 2 });
        add(&mut store, 1, 0, 1);
        add(&mut store, 2, 0, 2);
        // A newcomer is checked against the floor (0), then room is made, which raises the floor...
        let revision = store.revision_of(&who(3));
        assert!(store.make_room(0, true, |p| *p == who(3)));
        assert_eq!(store.revision_floor(), 2);
        // ...but its own request still continues revision 0.
        store.pin(who(3), revision, 3);
        assert_eq!(store.revision_of(&who(3)), 0);
        assert_eq!(add(&mut store, 3, 0, 3), 1);
        // Pinning an existing entry changes nothing.
        store.pin(who(3), 99, 4);
        assert_eq!(store.revision_of(&who(3)), 1);
        assert_eq!(store.bytes(), store.recount());
    }

    #[test]
    fn a_removed_callers_revision_never_goes_back() {
        let mut store = Store::with_limits(StoreLimits { max_bytes: usize::MAX, max_entries: 3 });
        for i in 0..5 {
            add(&mut store, 1, i, 10);
        }
        add(&mut store, 2, 0, 20);
        assert_eq!(store.clear(who(3), 30), 1);
        assert_eq!(store.revision_of(&who(1)), 5);

        // A fourth caller needs an entry: the least recently updated one goes (down to 90 % of 3 = 2 entries).
        assert!(store.make_room(0, true, |p| *p == who(4)));
        assert!(!store.contains(&who(1)));
        assert!(store.contains(&who(2)) && store.contains(&who(3)));
        // Whoever was removed, or never had an entry, is now past every removed revision,
        assert_eq!(store.revision_floor(), 6);
        assert_eq!(store.revision_of(&who(1)), 6);
        assert_eq!(store.revision_of(&who(9)), 6);
        // so nothing sent for one of its earlier revisions (0..=5) can match, and its saved ids are gone.
        assert_eq!(store.saved_exchange(&who(1), "r4"), None);
        // Entries that were kept are untouched.
        assert_eq!((store.revision_of(&who(2)), store.revision_of(&who(3))), (1, 1));
        // A clear, then removal, then an old base: still ahead.
        assert_eq!(store.clear(who(1), 40), 7);
        assert!(store.make_room(0, true, |p| *p != who(1)));
        assert!(!store.contains(&who(1)));
        assert_eq!(store.revision_of(&who(1)), 8);
    }

    #[test]
    fn room_is_made_by_removing_the_least_recently_used_but_never_protected_entries() {
        let exchange = 2 * TURN_OVERHEAD_BYTES + "q0".len() + "a0".len() + "r0".len();
        let one = ENTRY_OVERHEAD_BYTES + exchange;
        let mut store = Store::with_limits(StoreLimits { max_bytes: 10 * one, max_entries: 100 });
        for n in 1..=10u8 {
            add(&mut store, n, 0, n as u64);
        }
        assert_eq!(store.bytes(), 10 * one);
        assert!(store.make_room(0, false, |_| false), "exactly full still fits");
        assert_eq!(store.entries(), 10);

        // One more exchange for caller 1 (the oldest entry, but protected as the caller) and caller 3 in flight.
        assert!(store.make_room(exchange, false, |p| *p == who(1) || *p == who(3)));
        // Freed down to 90 % of the budget minus what is about to be added: callers 2 and 4 went.
        assert!(store.contains(&who(1)) && store.contains(&who(3)));
        assert!(!store.contains(&who(2)) && !store.contains(&who(4)));
        assert_eq!(store.entries(), 8);
        assert!(store.bytes() + exchange <= 9 * one);
        assert_eq!(store.revision_floor(), 2);
        assert_eq!(store.bytes(), store.recount());
    }

    #[test]
    fn making_room_ends_and_reports_failure_when_nothing_helps() {
        // Only empty entries: each frees a little, and the loop ends when they run out.
        let mut store = Store::with_limits(StoreLimits { max_bytes: 4 * ENTRY_OVERHEAD_BYTES, max_entries: 100 });
        for n in 1..=4u8 {
            store.clear(who(n), n as u64);
        }
        assert!(!store.make_room(10 * ENTRY_OVERHEAD_BYTES, false, |_| false));
        assert_eq!(store.entries(), 0);

        // Everything protected: nothing is removed.
        let mut store = Store::with_limits(StoreLimits { max_bytes: usize::MAX, max_entries: 2 });
        add(&mut store, 1, 0, 0);
        add(&mut store, 2, 0, 0);
        assert!(!store.make_room(0, true, |_| true));
        assert_eq!((store.entries(), store.revision_floor()), (2, 0));
        // An existing caller does not need a new entry.
        assert!(store.make_room(0, false, |_| true));
    }

    #[test]
    fn the_byte_count_always_matches_the_content() {
        let mut store = Store::with_limits(StoreLimits { max_bytes: 40_000, max_entries: 12 });
        let mut seed = 12345u64;
        let mut next = |bound: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % bound
        };
        for step in 0..5_000u64 {
            let caller = who(next(20) as u8);
            match next(10) {
                0 => {
                    store.clear(caller, step);
                }
                1 => {
                    let new_entry = !store.contains(&caller);
                    store.make_room(next(3_000) as usize, new_entry, |p| *p == caller);
                }
                2 => store.pin(caller, store.revision_of(&caller), step),
                _ => {
                    let before = store.revision_of(&caller);
                    let text = "x".repeat(next(300) as usize);
                    let after = store.append_exchange(caller, text.clone(), text, format!("r{step}"), step);
                    assert_eq!(after, before + 1);
                }
            }
            assert_eq!(store.bytes(), store.recount(), "after step {step}");
            assert!(store.turns(&caller).len() <= MAX_TURNS);
        }
        assert!(store.revision_floor() > 0, "the run did evict");
    }
}
