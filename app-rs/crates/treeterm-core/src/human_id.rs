//! Readable random names like `brave-otters-run`, in the shape of the `human-id` package
//! the Electron app uses for quick forks (adjective, plural noun, verb; lowercase, `-`).

const ADJECTIVES: &[&str] = &[
    "brave", "calm", "clever", "cool", "curly", "dull", "eager", "early", "fair", "fancy",
    "fast", "fluffy", "free", "fresh", "gentle", "giant", "good", "grumpy", "happy", "heavy",
    "hip", "honest", "huge", "jolly", "kind", "late", "lazy", "light", "little", "loud",
    "lucky", "mean", "merry", "mighty", "modern", "neat", "nice", "odd", "old", "plain",
    "polite", "proud", "quick", "quiet", "rare", "ready", "rich", "rotten", "rude", "salty",
    "shaggy", "shiny", "short", "shy", "silly", "slick", "slimy", "slow", "small", "smart",
    "smooth", "soft", "solid", "spicy", "strong", "sweet", "swift", "tall", "tame", "tasty",
    "tender", "thick", "tidy", "tiny", "tough", "twenty", "ugly", "upset", "warm", "weak",
    "wet", "whole", "wicked", "wide", "wild", "wise", "witty", "young", "yummy", "zany",
];

const NOUNS: &[&str] = &[
    "apes", "ants", "bats", "bears", "bees", "birds", "boats", "books", "bottles", "buses",
    "cameras", "cars", "cats", "chairs", "cobras", "coins", "cooks", "cows", "crabs", "cups",
    "deer", "dodos", "dogs", "donkeys", "doors", "dots", "doves", "dragons", "drinks", "ducks",
    "eagles", "ears", "eels", "eggs", "eyes", "falcons", "fans", "files", "fish", "flies",
    "foxes", "frogs", "geese", "ghosts", "goats", "grapes", "hairs", "hats", "hornets", "horses",
    "houses", "islands", "jars", "jobs", "keys", "kids", "kings", "kiwis", "lamps", "lands",
    "laws", "lemons", "lions", "lizards", "llamas", "maps", "mice", "moles", "moons", "moths",
    "mugs", "news", "nails", "nights", "olives", "otters", "owls", "pans", "parrots", "pens",
    "pigs", "plants", "plums", "pumas", "rabbits", "rats", "ravens", "rice", "rings", "roses",
];

const VERBS: &[&str] = &[
    "act", "add", "agree", "allow", "appear", "argue", "arrive", "ask", "attack", "battle",
    "beam", "beg", "behave", "bet", "bow", "brake", "breathe", "bring", "build", "burn",
    "buy", "call", "camp", "care", "carry", "cheat", "check", "cheer", "chew", "clap",
    "clean", "cough", "count", "cover", "crash", "cross", "cry", "cut", "dance", "decide",
    "deny", "design", "dig", "divide", "doubt", "draw", "dream", "dress", "drive", "drop",
    "drum", "eat", "end", "enjoy", "exist", "explain", "fail", "fall", "feel", "fetch",
    "fix", "float", "fly", "fold", "follow", "forgive", "give", "glow", "go", "grab",
    "grin", "grow", "guess", "hang", "happen", "hear", "help", "hide", "hope", "hug",
    "hunt", "invent", "itch", "jam", "joke", "judge", "juggle", "jump", "kick", "kiss",
];

/// `adjective-nouns-verb` from 16 bytes of randomness.
pub fn generate_from(random: [u8; 16]) -> String {
    let pick = |words: &[&'static str], bytes: [u8; 4]| words[u32::from_le_bytes(bytes) as usize % words.len()];
    let word = |i: usize| [random[i], random[i + 1], random[i + 2], random[i + 3]];
    format!("{}-{}-{}", pick(ADJECTIVES, word(0)), pick(NOUNS, word(4)), pick(VERBS, word(8)))
}

pub fn generate() -> String {
    generate_from(*uuid::Uuid::new_v4().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_three_lowercase_words() {
        let name = generate_from([7; 16]);
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 3, "{name}");
        assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '-'));
    }

    #[test]
    fn different_randomness_gives_different_names() {
        let names: std::collections::HashSet<String> = (0..50).map(|_| generate()).collect();
        assert!(names.len() > 40);
    }
}
