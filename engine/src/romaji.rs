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

/// Convert romaji with IME leftover (incomplete trailing latin kept).
pub fn to_ime_kana(romaji: &str, prefer_katakana: bool) -> String {
    let lower = romaji.to_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    let mut out = String::new();
    while i < bytes.len() {
        let mut matched = false;
        for len in (1..=3).rev() {
            if i + len > bytes.len() {
                continue;
            }
            let slice = std::str::from_utf8(&bytes[i..i + len]).unwrap_or("");
            if let Some(kana) = MAP.get(slice) {
                // special: っk style — emit っ and keep consonant for next
                if kana.starts_with('っ') && kana.len() > 3 {
                    out.push('っ');
                    // leave the repeated consonant for next iteration by only consuming 1
                    i += 1;
                } else {
                    out.push_str(kana);
                    i += len;
                }
                matched = true;
                break;
            }
        }
        if !matched {
            // lone n before consonant → ん
            if bytes[i] == b'n' && i + 1 < bytes.len() {
                let next = bytes[i + 1] as char;
                if !"aiueoy".contains(next) {
                    out.push('ん');
                    i += 1;
                    continue;
                }
            }
            // leftover latin
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    if prefer_katakana {
        to_katakana_str(&out)
    } else {
        out
    }
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
        _ => None,
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
