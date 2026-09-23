use crate::dict::{best_japanese, EN_WORDS, PROPER};
use crate::romaji::{map_commit_punct, particle_from_romaji, to_ime_kana};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct ConvertResult {
    pub surface: String,
    pub used_jev: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    /// Kept as typed English (proper-cased when known).
    En,
    /// Romaji meant as Japanese; `raw` is what the kana-kanji converter should see.
    Ja,
    /// Spaces, digits and punctuation typed by the user.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub kind: SegmentKind,
    pub raw: String,
    /// Offline rendering, used when no kana-kanji converter is available.
    pub surface: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    En,
    Ja,
    Particle,
    Kana,
    Raw,
}

#[derive(Clone)]
struct Choice {
    start: usize,
    surface: String,
    kind: Kind,
}

const PARTICLE_PREFERRED: &[&str] = &[
    "no", "to", "wa", "ha", "ga", "ni", "de", "wo", "o", "mo", "kara", "made", "node", "kedo",
];

fn english_surface(word: &str, original: &str) -> String {
    if let Some(p) = PROPER.get(word) {
        return (*p).to_string();
    }
    if original.chars().any(|c| c.is_ascii_uppercase()) {
        return original.to_string();
    }
    word.to_string()
}

/// Best offline segmentation of one run of latin letters (no spaces or punctuation).
fn segment_chunk(chunk: &str, extra_en: &HashSet<String>) -> Vec<Segment> {
    if chunk.is_empty() {
        return Vec::new();
    }
    let original: Vec<char> = chunk.chars().collect();
    let chars: Vec<char> = chunk.to_lowercase().chars().collect();
    let n = chars.len();
    let mut score = vec![i32::MIN; n + 1];
    let mut choice: Vec<Option<Choice>> = vec![None; n + 1];
    score[0] = 0;

    let is_en = |word: &str| -> bool {
        EN_WORDS.contains(word) || PROPER.contains_key(word) || extra_en.contains(word)
    };
    fn relax(score: &mut [i32], choice: &mut [Option<Choice>], end: usize, total: i32, c: Choice) {
        if total > score[end] {
            score[end] = total;
            choice[end] = Some(c);
        }
    }

    for i in 0..n {
        if score[i] == i32::MIN {
            continue;
        }
        let prev_kind = choice[i].as_ref().map(|c| c.kind);
        let rest: String = chars[i..].iter().collect();

        for len in (2..=(n - i).min(24)).rev() {
            let word: String = chars[i..i + len].iter().collect();
            if !word.chars().all(|c| c.is_ascii_lowercase()) || !is_en(&word) {
                continue;
            }
            let typed: String = original[i..i + len].iter().collect();
            let acronym = typed.chars().all(|c| c.is_ascii_uppercase());
            if PARTICLE_PREFERRED.contains(&word.as_str()) && !acronym {
                continue;
            }
            if len <= 2 && !acronym && !matches!(prev_kind, Some(Kind::En)) {
                continue;
            }
            let mut sc = 40 + (len as i32) * 12;
            if PROPER.contains_key(word.as_str()) || extra_en.contains(&word) {
                sc += 55;
            }
            if typed.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                sc += 35;
            }
            if len >= 4 {
                sc += 30;
            }
            let c = Choice {
                start: i,
                surface: english_surface(&word, &typed),
                kind: Kind::En,
            };
            let total = score[i].saturating_add(sc);
            relax(&mut score, &mut choice, i + len, total, c);
        }

        for len in [4usize, 3, 2, 1] {
            if i + len > n {
                continue;
            }
            let slice: String = chars[i..i + len].iter().collect();
            let Some(particle) = particle_from_romaji(&slice) else {
                continue;
            };
            if len == 1 {
                let whole = i == 0 && n == 1;
                let after_end =
                    i > 0 && i + 1 == n && matches!(prev_kind, Some(Kind::En) | Some(Kind::Ja));
                if !whole && !after_end {
                    continue;
                }
            }
            let longer_en = (len + 1..=(n - i).min(24)).any(|l| {
                let w: String = chars[i..i + l].iter().collect();
                is_en(&w)
            });
            if longer_en && !(matches!(prev_kind, Some(Kind::En)) && i + len == n) {
                continue;
            }
            let mut sc = 72 + (len as i32) * 10;
            if matches!(prev_kind, Some(Kind::En)) {
                sc += 50;
            }
            if matches!(slice.as_str(), "no" | "to" | "wo") {
                sc += 30;
            }
            if matches!(prev_kind, Some(Kind::En)) && i + len == n {
                sc += 40;
            }
            let c = Choice {
                start: i,
                surface: particle.to_string(),
                kind: Kind::Particle,
            };
            let total = score[i].saturating_add(sc);
            relax(&mut score, &mut choice, i + len, total, c);
        }

        for len in (2..=(n - i).min(24)).rev() {
            let romaji: String = chars[i..i + len].iter().collect();
            let kana = to_ime_kana(&romaji, false);
            if kana.chars().any(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            if let Some((surface, jp_score)) = best_japanese(&kana) {
                let c = Choice {
                    start: i,
                    surface: surface.to_string(),
                    kind: Kind::Ja,
                };
                let total = score[i].saturating_add(jp_score + (len as i32) * 2);
                relax(&mut score, &mut choice, i + len, total, c);
            }
        }

        let mut matched = false;
        for len in [3usize, 2, 1] {
            if i + len > n {
                continue;
            }
            let slice: String = chars[i..i + len].iter().collect();
            let kana = to_ime_kana(&slice, false);
            if kana.chars().any(|c| c.is_ascii_alphabetic()) || kana.is_empty() {
                continue;
            }
            let en_progress = rest.len() >= 2
                && (EN_WORDS.iter().any(|w| w.starts_with(&rest))
                    || extra_en.iter().any(|w| w.starts_with(&rest)))
                && !is_en(&rest)
                && particle_from_romaji(&rest).is_none();
            let sc = if en_progress {
                9 + len as i32
            } else {
                14 + len as i32
            };
            let c = Choice {
                start: i,
                surface: kana,
                kind: Kind::Kana,
            };
            let total = score[i].saturating_add(sc);
            relax(&mut score, &mut choice, i + len, total, c);
            matched = true;
            break;
        }
        if !matched {
            let c = Choice {
                start: i,
                surface: original[i].to_string(),
                kind: Kind::Raw,
            };
            let total = score[i].saturating_add(1);
            relax(&mut score, &mut choice, i + 1, total, c);
        }
    }

    let mut idx = n;
    while idx > 0 && score[idx] == i32::MIN {
        idx -= 1;
    }
    let mut pieces: Vec<(Kind, String, String)> = Vec::new();
    let tail: String = original[idx..].iter().collect();
    if !tail.is_empty() {
        pieces.push((Kind::Raw, tail.clone(), tail));
    }
    while idx > 0 {
        match choice[idx].clone() {
            Some(c) => {
                let raw: String = original[c.start..idx].iter().collect();
                pieces.push((c.kind, raw, c.surface));
                idx = c.start;
            }
            None => {
                let ch = original[idx - 1].to_string();
                pieces.push((Kind::Raw, ch.clone(), ch));
                idx -= 1;
            }
        }
    }
    pieces.reverse();

    let mut segments: Vec<Segment> = Vec::new();
    for (kind, raw, surface) in pieces {
        let kind = if kind == Kind::En {
            SegmentKind::En
        } else {
            SegmentKind::Ja
        };
        match segments.last_mut() {
            Some(last) if last.kind == kind => {
                if kind == SegmentKind::En {
                    last.surface.push(' ');
                }
                last.raw.push_str(&raw);
                last.surface.push_str(&surface);
            }
            _ => segments.push(Segment { kind, raw, surface }),
        }
    }
    segments
}

fn push_other(segments: &mut Vec<Segment>, raw: char, surface: char) {
    match segments.last_mut() {
        Some(last) if last.kind == SegmentKind::Other => {
            last.raw.push(raw);
            last.surface.push(surface);
        }
        _ => segments.push(Segment {
            kind: SegmentKind::Other,
            raw: raw.to_string(),
            surface: surface.to_string(),
        }),
    }
}

/// Split a raw keystroke buffer into English / Japanese / literal segments.
pub fn segment_with(raw: &str, extra_en: &HashSet<String>) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut buf = String::new();

    for ch in raw.chars() {
        if ch.is_ascii_alphabetic() || ch == '\'' || ch == '-' {
            buf.push(ch);
            continue;
        }
        segments.extend(segment_chunk(&buf, extra_en));
        buf.clear();
        let surface = match ch {
            ',' => '、',
            c if crate::romaji::is_commit_punct(c) => map_commit_punct(c),
            c => c,
        };
        push_other(&mut segments, ch, surface);
    }
    segments.extend(segment_chunk(&buf, extra_en));
    segments
}

pub fn segment(raw: &str) -> Vec<Segment> {
    segment_with(raw, &HashSet::new())
}

pub fn render_offline(segments: &[Segment]) -> String {
    segments.iter().map(|s| s.surface.as_str()).collect()
}

pub fn live_convert(raw: &str) -> ConvertResult {
    ConvertResult {
        surface: render_offline(&segment(raw)),
        used_jev: false,
    }
}

fn flip(segment: &Segment) -> Option<Segment> {
    match segment.kind {
        SegmentKind::En => Some(Segment {
            kind: SegmentKind::Ja,
            raw: segment.raw.clone(),
            surface: to_ime_kana(&segment.raw, false),
        }),
        SegmentKind::Ja
            if segment.raw.len() >= 2 && segment.raw.chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            Some(Segment {
                kind: SegmentKind::En,
                raw: segment.raw.clone(),
                surface: segment.raw.clone(),
            })
        }
        _ => None,
    }
}

/// `raw` read as Japanese only (English guesses from the tiny lexicon are not trusted).
pub fn as_japanese(raw: &str) -> Vec<Segment> {
    let flipped = segment(raw)
        .iter()
        .map(|s| {
            if s.kind == SegmentKind::En {
                flip(s).unwrap()
            } else {
                s.clone()
            }
        })
        .collect();
    merge_adjacent(flipped)
}

/// Concatenate two segmentations, merging the seam when the kinds match.
pub fn concat(mut head: Vec<Segment>, tail: Vec<Segment>) -> Vec<Segment> {
    head.extend(tail);
    merge_adjacent(head)
}

fn merge_adjacent(segments: Vec<Segment>) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for s in segments {
        match out.last_mut() {
            Some(last) if last.kind == s.kind && s.kind != SegmentKind::Other => {
                if s.kind == SegmentKind::En {
                    last.surface.push(' ');
                }
                last.raw.push_str(&s.raw);
                last.surface.push_str(&s.surface);
            }
            _ => out.push(s),
        }
    }
    out
}

/// Plausible readings of the buffer for a judge (Jev) to choose from.
/// The offline best is always first.
pub fn alternatives(raw: &str, limit: usize) -> Vec<Vec<Segment>> {
    let best = segment(raw);
    let mut out = vec![best.clone()];
    let push = |cand: Vec<Segment>, out: &mut Vec<Vec<Segment>>| {
        let cand = merge_adjacent(cand);
        if out.len() < limit && !out.contains(&cand) {
            out.push(cand);
        }
    };

    for i in 0..best.len() {
        if let Some(flipped) = flip(&best[i]) {
            let mut cand = best.clone();
            cand[i] = flipped;
            push(cand, &mut out);
        }
    }

    let all_ja: Vec<Segment> = best
        .iter()
        .map(|s| {
            if s.kind == SegmentKind::En {
                flip(s).unwrap()
            } else {
                s.clone()
            }
        })
        .collect();
    push(all_ja, &mut out);

    // Uppercase letters usually start an English word ("henshiwaThank").
    let mut cased = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, cased: &mut Vec<Segment>| {
        if buf.is_empty() {
            return;
        }
        let kind = if buf.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            SegmentKind::En
        } else {
            SegmentKind::Ja
        };
        let surface = if kind == SegmentKind::En {
            buf.clone()
        } else {
            to_ime_kana(buf, false)
        };
        cased.push(Segment {
            kind,
            raw: buf.clone(),
            surface,
        });
        buf.clear();
    };
    for s in &best {
        if s.kind == SegmentKind::Other {
            flush(&mut buf, &mut cased);
            cased.push(s.clone());
            continue;
        }
        for ch in s.raw.chars() {
            if ch.is_ascii_uppercase() && !buf.chars().all(|c| c.is_ascii_uppercase()) {
                flush(&mut buf, &mut cased);
            }
            buf.push(ch);
        }
    }
    flush(&mut buf, &mut cased);
    push(cased, &mut out);

    out
}

/// Returns (committed_surface, rest_raw) when composition contains sentence-end punct.
pub fn take_committed(raw: &str) -> Option<(String, String)> {
    for (i, ch) in raw.char_indices() {
        if crate::romaji::is_commit_punct(ch) {
            let end = i + ch.len_utf8();
            let surface = live_convert(&raw[..end]).surface;
            return Some((surface, raw[end..].to_string()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(raw: &str) -> Vec<(SegmentKind, String)> {
        segment(raw).into_iter().map(|s| (s.kind, s.raw)).collect()
    }

    fn en(s: &str) -> (SegmentKind, String) {
        (SegmentKind::En, s.to_string())
    }
    fn ja(s: &str) -> (SegmentKind, String) {
        (SegmentKind::Ja, s.to_string())
    }
    fn other(s: &str) -> (SegmentKind, String) {
        (SegmentKind::Other, s.to_string())
    }

    #[test]
    fn splits_english_and_japanese() {
        assert_eq!(
            shape("Google Meetno"),
            vec![en("Google"), other(" "), en("Meet"), ja("no")]
        );
        assert_eq!(shape("henshiwaThank"), vec![ja("henshiwa"), en("Thank")]);
        assert_eq!(shape("gitpullshitara"), vec![en("gitpull"), ja("shitara")]);
        assert_eq!(shape("PRno"), vec![en("PR"), ja("no")]);
        assert_eq!(
            shape("yoursessionhasexpiredto"),
            vec![en("yoursessionhasexpired"), ja("to")]
        );
        assert_eq!(
            shape("sukoshimattekudasai."),
            vec![ja("sukoshimattekudasai"), other(".")]
        );
    }

    #[test]
    fn english_words_get_spaces_but_japanese_boundaries_do_not() {
        assert_eq!(
            live_convert("thankyouforyourhelp").surface,
            "Thank you for your help"
        );
        assert_eq!(live_convert("gitpullshitara").surface, "git pullしたら");
        assert_eq!(live_convert("PRno").surface, "PRの");
    }

    #[test]
    fn pending_romaji_stays_japanese() {
        assert_eq!(shape("ra-men"), vec![ja("ra-men")]);
        assert_eq!(shape("nak"), vec![ja("nak")]);
        assert!(!live_convert("ra-men").surface.contains(' '));
    }

    #[test]
    fn long_vowel_is_fullwidth() {
        assert_eq!(live_convert("ko-hi-").surface, "こーひー");
    }

    #[test]
    fn alternatives_offer_flips() {
        let alts = alternatives("henshiwaThank", 8);
        assert_eq!(alts[0], segment("henshiwaThank"));
        assert!(alts
            .iter()
            .any(|a| a.iter().all(|s| s.kind == SegmentKind::Ja)));
        assert!(alts.len() <= 8);
        let unique: HashSet<String> = alts.iter().map(|a| format!("{a:?}")).collect();
        assert_eq!(unique.len(), alts.len());
    }

    #[test]
    fn commit_split() {
        let (s, rest) = take_committed("sukoshimattekudasai.").unwrap();
        assert!(s.ends_with('。'), "{s}");
        assert!(rest.is_empty());
    }
}
