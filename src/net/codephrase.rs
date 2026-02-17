//! Code phrase generation and validation for Croc-like file transfers.
//!
//! Generates human-readable code phrases in the format `NNNN-word-word-word-word`
//! where NNNN is a random 4-digit number and words come from a curated 256-word
//! list. This gives ~45 bits of entropy (9000 * 256^4 = ~3.8 * 10^13 combinations).
//!
//! The code phrase is hashed with BLAKE3 to produce a short hash that is advertised
//! via mDNS TXT records, allowing the receiver to find the correct sender without
//! revealing the code phrase over the network.

use rand::Rng;

/// 256 short, common, easy-to-type English words.
const WORD_LIST: [&str; 256] = [
    "ace", "add", "age", "ago", "aid", "aim", "air", "all", "and", "ant",
    "any", "ape", "arc", "arm", "art", "ash", "ask", "ate", "awe", "axe",
    "bad", "bag", "ban", "bar", "bat", "bay", "bed", "bee", "bet", "bid",
    "big", "bit", "bow", "box", "boy", "bud", "bug", "bus", "but", "buy",
    "cab", "can", "cap", "car", "cat", "cop", "cow", "cry", "cub", "cup",
    "cut", "dad", "dam", "day", "den", "dew", "did", "dig", "dim", "dip",
    "dog", "dot", "dry", "dub", "dug", "dye", "ear", "eat", "eel", "egg",
    "elk", "elm", "emu", "end", "era", "eve", "ewe", "eye", "fan", "far",
    "fat", "fax", "fed", "few", "fig", "fin", "fir", "fit", "fix", "fly",
    "fog", "for", "fox", "fry", "fun", "fur", "gag", "gap", "gas", "gel",
    "gem", "get", "gin", "got", "gum", "gun", "gut", "guy", "gym", "had",
    "ham", "has", "hat", "hay", "hen", "her", "hid", "him", "hip", "his",
    "hit", "hog", "hop", "hot", "how", "hub", "hue", "hug", "hum", "hut",
    "ice", "icy", "ill", "imp", "ink", "inn", "ion", "ire", "irk", "ivy",
    "jab", "jag", "jam", "jar", "jaw", "jay", "jet", "jig", "job", "jog",
    "joy", "jug", "jut", "keg", "ken", "key", "kid", "kin", "kit", "lab",
    "lad", "lag", "lap", "law", "lay", "led", "leg", "let", "lid", "lie",
    "lip", "lit", "log", "lot", "low", "lug", "mad", "man", "map", "mat",
    "may", "men", "met", "mix", "mob", "mod", "mom", "mop", "mud", "mug",
    "nab", "nag", "nap", "net", "new", "nil", "nip", "nit", "nod", "nor",
    "not", "now", "nun", "nut", "oak", "oar", "oat", "odd", "ode", "off",
    "oft", "oil", "old", "one", "opt", "orb", "ore", "our", "out", "owe",
    "owl", "own", "pad", "pal", "pan", "pat", "paw", "pay", "pea", "peg",
    "pen", "per", "pet", "pie", "pig", "pin", "pit", "ply", "pod", "pop",
    "pot", "pry", "pub", "pug", "pun", "pup", "put", "ram", "ran", "rap",
    "rat", "raw", "ray", "red", "rib", "rid",
];

/// Generate a random code phrase in the format `NNNN-word-word-word-word`.
///
/// - NNNN: random 4-digit number (1000-9999)
/// - Four words chosen randomly from the 256-word list
///
/// Total entropy: ~45 bits (9000 * 256^4 ≈ 3.8 * 10^13)
pub fn generate() -> String {
    let mut rng = rand::rng();
    let number: u16 = rng.random_range(1000..=9999);
    let w1 = WORD_LIST[rng.random_range(0..256)];
    let w2 = WORD_LIST[rng.random_range(0..256)];
    let w3 = WORD_LIST[rng.random_range(0..256)];
    let w4 = WORD_LIST[rng.random_range(0..256)];
    format!("{}-{}-{}-{}-{}", number, w1, w2, w3, w4)
}

/// Validate a code phrase string.
///
/// Checks:
/// 1. Format is `NNNN-word-word-word-word` (exactly 5 parts separated by hyphens)
/// 2. First part is a 4-digit number (1000-9999)
/// 3. All four words are in the word list
pub fn validate(code: &str) -> Result<(), String> {
    let parts: Vec<&str> = code.split('-').collect();
    if parts.len() != 5 {
        return Err(format!(
            "Invalid code phrase format: expected NNNN-word-word-word-word, got {} parts",
            parts.len()
        ));
    }

    // Validate number part
    let num: u16 = parts[0]
        .parse()
        .map_err(|_| format!("Invalid code phrase: '{}' is not a valid number", parts[0]))?;
    if !(1000..=9999).contains(&num) {
        return Err(format!(
            "Invalid code phrase: number must be 1000-9999, got {}",
            num
        ));
    }

    // Validate words
    for &word in &parts[1..] {
        if !WORD_LIST.contains(&word) {
            return Err(format!(
                "Invalid code phrase: '{}' is not a recognized word",
                word
            ));
        }
    }

    Ok(())
}

/// Compute a BLAKE3 hash prefix of a code phrase for mDNS matching.
///
/// Returns the first 16 hex characters of the BLAKE3 hash. This is used as
/// a TXT property in mDNS to allow receivers to find senders by code phrase
/// without revealing the actual code phrase on the network.
pub fn code_hash(code: &str) -> String {
    let hash = blake3::hash(code.as_bytes());
    hash.to_hex()[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_list_has_256_entries() {
        assert_eq!(WORD_LIST.len(), 256);
    }

    #[test]
    fn word_list_entries_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for word in &WORD_LIST {
            assert!(seen.insert(word), "Duplicate word in list: {}", word);
        }
    }

    #[test]
    fn word_list_entries_are_lowercase_ascii() {
        for word in &WORD_LIST {
            assert!(
                word.chars().all(|c| c.is_ascii_lowercase()),
                "Word '{}' contains non-lowercase-ASCII characters",
                word
            );
            assert!(
                word.len() >= 2 && word.len() <= 5,
                "Word '{}' has unexpected length {}",
                word,
                word.len()
            );
        }
    }

    #[test]
    fn generate_produces_valid_format() {
        for _ in 0..100 {
            let code = generate();
            assert!(
                validate(&code).is_ok(),
                "Generated code '{}' failed validation: {:?}",
                code,
                validate(&code)
            );
        }
    }

    #[test]
    fn generate_produces_five_parts() {
        let code = generate();
        let parts: Vec<&str> = code.split('-').collect();
        assert_eq!(parts.len(), 5);
    }

    #[test]
    fn generate_number_in_range() {
        for _ in 0..100 {
            let code = generate();
            let num: u16 = code.split('-').next().unwrap().parse().unwrap();
            assert!((1000..=9999).contains(&num));
        }
    }

    #[test]
    fn validate_rejects_too_few_parts() {
        // Three-word (old) format must be rejected.
        assert!(validate("1234-ocean-brave-echo").is_err());
    }

    #[test]
    fn validate_rejects_too_many_parts() {
        // Six-part phrase must be rejected.
        assert!(validate("1234-ace-bad-car-dog-elk").is_err());
    }

    #[test]
    fn validate_rejects_invalid_number() {
        assert!(validate("abcd-ace-bad-car-dog").is_err());
        assert!(validate("999-ace-bad-car-dog").is_err());
        assert!(validate("10000-ace-bad-car-dog").is_err());
    }

    #[test]
    fn validate_rejects_unknown_words() {
        // "ocean" and "brave" are not in the word list.
        assert!(validate("1234-ocean-brave-echo-fox").is_err());
    }

    #[test]
    fn validate_accepts_valid_code() {
        assert!(validate("1234-ace-bad-car-dog").is_ok());
        assert!(validate("9999-owl-red-hub-ant").is_ok());
    }

    #[test]
    fn code_hash_is_deterministic() {
        let code = "1234-ace-bad-car";
        let h1 = code_hash(code);
        let h2 = code_hash(code);
        assert_eq!(h1, h2);
    }

    #[test]
    fn code_hash_is_16_hex_chars() {
        let hash = code_hash("1234-ace-bad-car");
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn code_hash_differs_for_different_codes() {
        let h1 = code_hash("1234-ace-bad-car");
        let h2 = code_hash("5678-dog-elk-fig");
        assert_ne!(h1, h2);
    }

    #[test]
    fn generate_produces_unique_codes() {
        let codes: std::collections::HashSet<String> =
            (0..100).map(|_| generate()).collect();
        // With ~45 bits of entropy and 100 samples, collisions are astronomically unlikely
        assert!(codes.len() >= 99);
    }
}
