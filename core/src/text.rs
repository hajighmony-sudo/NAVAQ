//! Text helpers for searching Persian, Arabic and Latin metadata.

/// Normalize text for case-insensitive, script-tolerant comparison:
/// lowercases, maps Arabic yeh/kaf to Persian forms, converts Persian and Arabic-Indic
/// digits to ASCII, and drops diacritics, tatweel and zero-width joiners/non-joiners.
pub fn normalize_for_search(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\u{064A}' | '\u{0649}' => out.push('\u{06CC}'), // ي ى -> ی
            '\u{0643}' => out.push('\u{06A9}'),              // ك -> ک
            '\u{06C0}' | '\u{0629}' => out.push('\u{0647}'), // ۀ ة -> ه
            '\u{0623}' | '\u{0625}' | '\u{0671}' => out.push('\u{0627}'), // أ إ ٱ -> ا
            '\u{06F0}'..='\u{06F9}' => out.push(char::from(b'0' + (ch as u32 - 0x06F0) as u8)),
            '\u{0660}'..='\u{0669}' => out.push(char::from(b'0' + (ch as u32 - 0x0660) as u8)),
            '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{0640}'
            | '\u{200C}'
            | '\u{200D}'
            | '\u{200E}'
            | '\u{200F}' => {}
            _ => out.extend(ch.to_lowercase()),
        }
    }
    out
}

/// fzf-style subsequence match. Returns a score (higher is better) when every character
/// of `query` appears in `text` in order; `None` otherwise or for an empty query.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let query: Vec<char> = normalize_for_search(query)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if query.is_empty() {
        return None;
    }
    let text: Vec<char> = normalize_for_search(text).chars().collect();

    let mut score = 0_i32;
    let mut cursor = 0;
    let mut previous: Option<usize> = None;
    for wanted in query {
        let position = (cursor..text.len()).find(|&i| text[i] == wanted)?;
        cursor = position + 1;
        score += 10;
        if position == 0 || !text[position - 1].is_alphanumeric() {
            score += 10; // start of a word
        }
        match previous {
            Some(p) if position == p + 1 => score += 15, // consecutive run
            Some(p) => score -= (position - p - 1).min(10) as i32,
            None => score -= position.min(10) as i32,
        }
        previous = Some(position);
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unifies_persian_and_arabic_letters_digits_and_marks() {
        assert_eq!(normalize_for_search("علي"), normalize_for_search("علی"));
        assert_eq!(normalize_for_search("كتاب"), normalize_for_search("کتاب"));
        assert_eq!(
            normalize_for_search("می‌خواهم"),
            normalize_for_search("میخواهم")
        );
        assert_eq!(
            normalize_for_search("آهنگ ۱۲۳"),
            normalize_for_search("آهنگ ١٢٣")
        );
        assert_eq!(normalize_for_search("آهنگ ۱۲۳"), "آهنگ 123");
        assert_eq!(normalize_for_search("مُحَمَّد"), normalize_for_search("محمد"));
        assert_eq!(normalize_for_search("HeLLo"), "hello");
    }

    #[test]
    fn fuzzy_requires_ordered_subsequence_and_prefers_tight_matches() {
        assert!(fuzzy_score("bhm", "Bohemian Rhapsody").is_some());
        assert!(fuzzy_score("zq", "Bohemian Rhapsody").is_none());
        assert!(fuzzy_score("  ", "x").is_none());
        let tight = fuzzy_score("rhap", "Bohemian Rhapsody").unwrap();
        let loose = fuzzy_score("rhap", "r h a p").unwrap();
        assert!(tight > loose);
        assert!(fuzzy_score("به", "به نام خدا").is_some());
    }
}
