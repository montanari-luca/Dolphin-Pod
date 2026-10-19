//! Roger's instant acknowledgments: hard-coded and randomized, so they appear with no model call.

use rand::seq::IndexedRandom;

pub const ACKS: &[&str] = &[
    "Give me a sec, I'll find the right person for this.",
    "On it. Let me ask for help.",
    "Good one. Let me loop in the right specialist.",
    "One moment, I'm checking who's best for this.",
    "Hang tight, I'm routing this.",
    "Got it. Let me see who should take this one.",
    "Let me think about who can help most here.",
    "Sure thing. Finding the right brain for the job.",
    "Okay! Let me get someone on this.",
    "Noted. Pulling in help now.",
    "Just a moment while I sort this out.",
    "Let me check the roster for this one.",
    "Right away. Give me a second to route it.",
    "Interesting. Let me ask around.",
    "On it. I'll get you the best answer I can.",
    "Let me pass this to the right teammate.",
    "One sec, figuring out the best approach.",
    "Copy that. Getting help.",
    "Got your message. Routing it now.",
    "Alright, let me organize this.",
    "Let me see who's free and fits this best.",
    "Thinking about the best way to handle this.",
    "Give me a moment to get the team on it.",
    "Received. Coordinating now.",
    "Great question. Let me get the right expert.",
    "Okay, sorting out who takes this.",
    "Let me line this up for you.",
    "Hold on, I'm briefing the team.",
    "Sounds good. One moment.",
    "Let me figure out the right path for this.",
    "On my way to find help.",
    "Let me check what you need first.",
    "Working on it. Back in a sec.",
    "Let me ask the specialists.",
    "Understood. Getting this to the right place.",
    "Let me get the pod on it.",
    "Sure. Give me a beat to route this.",
    "Hmm, let me see who knows this best.",
    "Quick moment while I coordinate.",
    "Let me make sure this lands with the right agent.",
];

pub fn random_ack() -> &'static str {
    ACKS.choose(&mut rand::rng()).copied().unwrap_or(ACKS[0])
}

pub const CLEANED: &str = "🧹 I cleaned up this chat: messages older than {hours} hours were removed.";
pub const CLEANED_MANUAL: &str = "🧹 Done. This chat is clean.";
