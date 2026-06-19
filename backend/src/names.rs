//! Friendly random display names ("calm-otter", "bright-heron"). Used when a
//! principal is created; users rename themselves afterwards.

use crate::auth::random_bytes;

const ADJECTIVES: &[&str] = &[
    "calm", "bright", "swift", "quiet", "bold", "warm", "keen", "lucky", "brave", "clever",
    "gentle", "merry", "noble", "proud", "spry", "vivid",
];

const NOUNS: &[&str] = &[
    "otter", "heron", "fox", "lynx", "raven", "ibex", "wren", "marten", "stork", "tern", "vole",
    "finch", "hare", "newt", "shrew", "owl",
];

pub fn random_name() -> String {
    let b = random_bytes::<3>();
    let adj = ADJECTIVES[b[0] as usize % ADJECTIVES.len()];
    let noun = NOUNS[b[1] as usize % NOUNS.len()];
    // small numeric suffix to cut collisions
    format!("{adj}-{noun}-{:02}", b[2] % 100)
}
