//! Minimal romaji → hiragana (IME-style leftover latin kept).

use once_cell::sync::Lazy;
use std::collections::HashMap;

static MAP: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let pairs: &[(&str, &str)] = &[
        ("kya", "きゃ"),
        ("kyu", "きゅ"),
        ("kyo", "きょ"),
        ("sha", "しゃ"),
        ("shu", "しゅ"),
        ("sho", "しょ"),
        ("cha", "ちゃ"),
        ("chu", "ちゅ"),
        ("cho", "ちょ"),
        ("nya", "にゃ"),
        ("nyu", "にゅ"),
        ("nyo", "にょ"),
        ("hya", "ひゃ"),
        ("hyu", "ひゅ"),
        ("hyo", "ひょ"),
        ("mya", "みゃ"),
        ("myu", "みゅ"),
        ("myo", "みょ"),
        ("rya", "りゃ"),
        ("ryu", "りゅ"),
        ("ryo", "りょ"),
        ("gya", "ぎゃ"),
        ("gyu", "ぎゅ"),
        ("gyo", "ぎょ"),
        ("ja", "じゃ"),
        ("ju", "じゅ"),
        ("jo", "じょ"),
        ("bya", "びゃ"),
        ("byu", "びゅ"),
        ("byo", "びょ"),
        ("pya", "ぴゃ"),
        ("pyu", "ぴゅ"),
        ("pyo", "ぴょ"),
        ("kk", "っk"),
        ("ss", "っs"),
        ("tt", "っt"),
        ("pp", "っp"),
        ("cc", "っc"),
        ("gg", "っg"),
        ("zz", "っz"),
        ("dd", "っd"),
        ("bb", "っb"),
        ("hh", "っh"),
        ("ff", "っf"),
        ("jj", "っj"),
        // Not rr/yy/ww/vv/mm: those make English (review, pull…) look like Japanese.
        ("tc", "っc"),
        ("fa", "ふぁ"),
        ("fi", "ふぃ"),
        ("fe", "ふぇ"),
        ("fo", "ふぉ"),
        ("va", "ゔぁ"),
        ("vi", "ゔぃ"),
        ("vu", "ゔ"),
        ("ve", "ゔぇ"),
        ("vo", "ゔぉ"),
        ("che", "ちぇ"),
        ("she", "しぇ"),
        ("je", "じぇ"),
        ("thi", "てぃ"),
        ("dhi", "でぃ"),
        ("dyu", "でゅ"),
        ("wi", "うぃ"),
        ("we", "うぇ"),
        ("ye", "いぇ"),
        ("tsa", "つぁ"),
        ("xtu", "っ"),
        ("xa", "ぁ"),
        ("xi", "ぃ"),
        ("xu", "ぅ"),
        ("xe", "ぇ"),
        ("xo", "ぉ"),
        ("xya", "ゃ"),
        ("xyu", "ゅ"),
        ("xyo", "ょ"),
        ("sya", "しゃ"),
        ("syu", "しゅ"),
        ("syo", "しょ"),
        ("tya", "ちゃ"),
        ("tyu", "ちゅ"),
        ("tyo", "ちょ"),
        ("jya", "じゃ"),
        ("jyu", "じゅ"),
        ("jyo", "じょ"),
        ("zya", "じゃ"),
        ("zyu", "じゅ"),
        ("zyo", "じょ"),
        ("nyi", "にぃ"),
        ("ka", "か"),
        ("ki", "き"),
        ("ku", "く"),
        ("ke", "け"),
        ("ko", "こ"),
        ("sa", "さ"),
        ("si", "し"),
        ("shi", "し"),
        ("su", "す"),
        ("se", "せ"),
        ("so", "そ"),
        ("ta", "た"),
        ("ti", "ち"),
        ("chi", "ち"),
        ("tu", "つ"),
        ("tsu", "つ"),
        ("te", "て"),
        ("to", "と"),
        ("na", "な"),
        ("ni", "に"),
        ("nu", "ぬ"),
        ("ne", "ね"),
        ("no", "の"),
        ("ha", "は"),
        ("hi", "ひ"),
        ("hu", "ふ"),
        ("fu", "ふ"),
        ("he", "へ"),
        ("ho", "ほ"),
        ("ma", "ま"),
        ("mi", "み"),
        ("mu", "む"),
        ("me", "め"),
        ("mo", "も"),
        ("ya", "や"),
        ("yu", "ゆ"),
        ("yo", "よ"),
        ("ra", "ら"),
        ("ri", "り"),
        ("ru", "る"),
        ("re", "れ"),
        ("ro", "ろ"),
        ("wa", "わ"),
        ("wo", "を"),
        ("nn", "ん"),
        ("n'", "ん"),
        ("ga", "が"),
        ("gi", "ぎ"),
        ("gu", "ぐ"),
        ("ge", "げ"),
        ("go", "ご"),
        ("za", "ざ"),
        ("zi", "じ"),
        ("ji", "じ"),
        ("zu", "ず"),
        ("ze", "ぜ"),
        ("zo", "ぞ"),
        ("da", "だ"),
        ("di", "ぢ"),
        ("du", "づ"),
        ("de", "で"),
        ("do", "ど"),
        ("ba", "ば"),
        ("bi", "び"),
        ("bu", "ぶ"),
        ("be", "べ"),
        ("bo", "ぼ"),
        ("pa", "ぱ"),
        ("pi", "ぴ"),
        ("pu", "ぷ"),
        ("pe", "ぺ"),
        ("po", "ぽ"),
        ("a", "あ"),
        ("i", "い"),
        ("u", "う"),
        ("e", "え"),
        ("o", "お"),
        ("-", "ー"),
    ];
    pairs.iter().copied().collect()
});

static KATA: Lazy<HashMap<char, char>> = Lazy::new(|| {
    let hira = "ぁあぃいぅうぇえぉおかがきぎくぐけげこごさざしじすずせぜそぞただちぢっつづてでとどなにぬねのはばぱひびぴふぶぷへべぺほぼぽまみむめもゃやゅゆょよらりるれろゎわゐゑをん゛゜ー";
    let kata = "ァアィイゥウェエォオカガキギクグケゲコゴサザシジスズセゼソゾタダチヂッツヅテデトドナニヌネノハバパヒビピフブプヘベペホボポマミムメモャヤュユョヨラリルレロヮワヰヱヲン゛゜ー";
    hira.chars().zip(kata.chars()).collect()
});

fn to_katakana_str(s: &str) -> String {
    s.chars()
        .map(|c| KATA.get(&c).copied().unwrap_or(c))
        .collect()
}

/// Romaji → kana, reporting which input positions could not be read as romaji.
/// Non-ASCII input is passed through untouched.
fn scan(romaji: &str) -> (String, Vec<usize>) {
    let lower: Vec<char> = romaji.to_lowercase().chars().collect();
    let mut i = 0;
    let mut out = String::new();
    let mut leftovers = Vec::new();
    'outer: while i < lower.len() {
        for len in (1..=3).rev() {
            if i + len > lower.len() {
                continue;
            }
            let slice: String = lower[i..i + len].iter().collect();
            if let Some(kana) = MAP.get(slice.as_str()) {
                // "kk" style: emit っ and keep the consonant for the next syllable
                if kana.starts_with('っ') && kana.chars().count() > 1 {
                    out.push('っ');
                    i += 1;
                } else {
                    out.push_str(kana);
                    i += len;
                }
                continue 'outer;
            }
        }
        // lone n before a consonant → ん
        if lower[i] == 'n' && i + 1 < lower.len() && !"aiueoy".contains(lower[i + 1]) {
            out.push('ん');
            i += 1;
            continue;
        }
        if lower[i].is_ascii_alphabetic() {
            leftovers.push(i);
        }
        out.push(lower[i]);
        i += 1;
    }
    (out, leftovers)
}

/// Convert romaji with IME leftover (incomplete trailing latin kept).
pub fn to_ime_kana(romaji: &str, prefer_katakana: bool) -> String {
    let (out, _) = scan(romaji);
    if prefer_katakana {
        to_katakana_str(&out)
    } else {
        out
    }
}

/// Positions of letters that cannot be read as romaji, ignoring a trailing
/// consonant run of up to two letters (a syllable still being typed, "…tenk").
/// Japanese typed in romaji almost never has any; English words usually do
/// ("pull", "review", "kubernetes").
pub fn hard_leftovers(romaji: &str) -> Vec<usize> {
    let chars: Vec<char> = romaji.chars().collect();
    let pending = chars
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_alphabetic() && !"aiueo".contains(c.to_ascii_lowercase()))
        .count()
        .min(2);
    let pending_start = chars.len() - pending;
    scan(romaji)
        .1
        .into_iter()
        .filter(|&i| i < pending_start)
        .collect()
}

pub fn particle_from_romaji(romaji: &str) -> Option<&'static str> {
    match romaji.to_lowercase().as_str() {
        "no" => Some("の"),
        "wo" | "o" => Some("を"),
        "wa" | "ha" => Some("は"),
        "ga" => Some("が"),
        "ni" => Some("に"),
        "de" => Some("で"),
        "to" => Some("と"),
        "mo" => Some("も"),
        "e" | "he" => Some("へ"),
        "ya" => Some("や"),
        "ka" => Some("か"),
        "ne" => Some("ね"),
        "yo" => Some("よ"),
        "kara" => Some("から"),
        "made" => Some("まで"),
        "node" => Some("ので"),
        "kedo" => Some("けど"),
        "tte" => Some("って"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_leftovers_mark_unreadable_letters() {
        assert_eq!(hard_leftovers("kubernetesno"), vec![4, 9]);
        assert_eq!(
            hard_leftovers("kubernetes"),
            vec![4],
            "trailing s may still be typed"
        );
        assert!(!hard_leftovers("pullshitara").is_empty());
        assert!(!hard_leftovers("reviewwo").is_empty());
    }

    #[test]
    fn pending_syllable_is_not_unreadable() {
        assert!(hard_leftovers("kyouhaiitenk").is_empty());
        assert!(hard_leftovers("tenky").is_empty());
        assert!(hard_leftovers("fairu").is_empty());
        assert!(hard_leftovers("matcha").is_empty());
    }

    #[test]
    fn extended_romaji() {
        assert_eq!(to_ime_kana("fairu", false), "ふぁいる");
        assert_eq!(to_ime_kana("matcha", false), "まっちゃ");
        assert_eq!(to_ime_kana("thi-shatsu", false), "てぃーしゃつ");
        assert_eq!(to_ime_kana("baggu", false), "ばっぐ");
    }
}

pub fn is_commit_punct(ch: char) -> bool {
    matches!(ch, '.' | '?' | '。' | '？' | '!' | '！')
}

pub fn map_commit_punct(ch: char) -> char {
    match ch {
        '.' => '。',
        '?' => '？',
        '!' => '！',
        other => other,
    }
}
