use crate::dict::{best_japanese, EN_WORDS, PROPER};
use crate::romaji::{hard_leftovers, map_commit_punct, particle_from_romaji, to_ime_kana};
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
    "no", "to", "wa", "ha", "ga", "ni", "de", "wo", "o", "mo", "tte", "kara", "made", "node",
    "kedo",
];

pub(crate) fn english_surface(word: &str, original: &str) -> String {
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
    // Letters that cannot be read as romaji mark words outside the lexicon
    // ("kubernetes", "deploy") as likely English. A guess covers one leftover
    // cluster (leftovers with only a few letters between them), extended back
    // to the start of the letter run — not the whole chunk, so "nosettei" stays Japanese.
    let hard = hard_leftovers(chunk);
    let guess_runs: Vec<(usize, usize)> = {
        let mut runs = Vec::new();
        let mut cluster: Vec<usize> = Vec::new();
        let particle_between = |from: usize, to: usize| -> bool {
            // Only a particle that starts immediately after the previous leftover
            // (as in "…form wo apply"), not a letter inside the English word.
            const SPLITTERS: &[&str] = &[
                "wo", "no", "to", "wa", "ga", "ni", "de", "mo", "tte", "kara", "made", "node",
                "kedo",
            ];
            for &p in SPLITTERS {
                let plen = p.chars().count();
                if from + plen > to {
                    continue;
                }
                let slice: String = chars[from..from + plen].iter().collect();
                if slice == p {
                    return true;
                }
            }
            false
        };
        let flush = |cluster: &mut Vec<usize>, runs: &mut Vec<(usize, usize)>| {
            let Some(&first) = cluster.first() else {
                return;
            };
            let last = *cluster.last().unwrap();
            let min_start = runs.last().map(|&(_, e)| e).unwrap_or(0);
            let mut start = first;
            while start > min_start && chars[start - 1].is_ascii_lowercase() {
                start -= 1;
            }
            // Don't absorb a particle that sits between the previous run and this word.
            for &p in PARTICLE_PREFERRED {
                let plen = p.chars().count();
                if start + plen <= first {
                    let slice: String = chars[start..start + plen].iter().collect();
                    if slice == p {
                        start += plen;
                        break;
                    }
                }
            }
            // Include a trailing pending syllable ("…ply") so the full English
            // word is guessed, not a stump like "app" + "ly".
            let mut end = last + 1;
            while end < n
                && chars[end].is_ascii_lowercase()
                && !"aiueo".contains(chars[end])
                && end - last <= 2
            {
                end += 1;
            }
            if end - start >= 3 {
                runs.push((start, end));
            }
            cluster.clear();
        };
        for &p in &hard {
            if let Some(&prev) = cluster.last() {
                // A large readable gap, or a particle (wo/no/to…), means a new word.
                if p > prev + 6 || particle_between(prev + 1, p) {
                    flush(&mut cluster, &mut runs);
                }
            }
            cluster.push(p);
        }
        flush(&mut cluster, &mut runs);
        runs
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
        let known_start = (2..=(n - i).min(24)).any(|len| {
            let word: String = chars[i..i + len].iter().collect();
            is_en(&word)
        });

        for len in (2..=(n - i).min(24)).rev() {
            let word: String = chars[i..i + len].iter().collect();
            if !word.chars().all(|c| c.is_ascii_lowercase()) {
                continue;
            }
            let known = is_en(&word);
            if !known {
                // Only the full leftover-containing run, never a substring or a
                // neighbor of a known English word (which would insert a space).
                let guessable = len >= 3
                    && guess_runs.contains(&(i, i + len))
                    && !matches!(prev_kind, Some(Kind::En))
                    && !known_start;
                if !guessable {
                    continue;
                }
            }
            let typed: String = original[i..i + len].iter().collect();
            let acronym = typed.chars().all(|c| c.is_ascii_uppercase());
            if PARTICLE_PREFERRED.contains(&word.as_str()) && !acronym {
                continue;
            }
            if len <= 2 && !acronym && !matches!(prev_kind, Some(Kind::En)) {
                continue;
            }
            let word_hard = hard_leftovers(&word);
            // Short stems like "set"/"get" are valid romaji too. Prefer Japanese unless
            // more English or a particle follows ("getの", "setを"). Longer known words
            // ("data", "your") stay English.
            let ambiguous_stem = known
                && !acronym
                && !PROPER.contains_key(word.as_str())
                && !extra_en.contains(&word)
                && !matches!(prev_kind, Some(Kind::En))
                && word_hard.iter().all(|&p| p + 2 >= len);
            // Fully readable after Japanese ("sakkiitta|you|ni" = ように): Japanese.
            if ambiguous_stem && len <= 3 && i > 0 && crate::romaji::scan(&word).1.is_empty() {
                continue;
            }
            if ambiguous_stem && len <= 3 {
                let after = i + len;
                let continues_en = (2..=(n - after).min(24)).any(|l| {
                    let w: String = chars[after..after + l].iter().collect();
                    is_en(&w) && !PARTICLE_PREFERRED.contains(&w.as_str())
                });
                let continues_particle = (1..=(n - after).min(4)).any(|l| {
                    let w: String = chars[after..after + l].iter().collect();
                    particle_from_romaji(&w).is_some()
                });
                if !continues_en && !continues_particle {
                    continue;
                }
            }
            let mut sc = if known {
                40 + (len as i32) * 12
            } else {
                // Prefer the full run over any Japanese reading of the same letters.
                80 + (len as i32) * 20
            };
            if PROPER.contains_key(word.as_str()) || extra_en.contains(&word) {
                sc += 55;
            }
            if typed.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                sc += 35;
            }
            if known && len >= 4 {
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
            if matches!(slice.as_str(), "no" | "to" | "wo" | "tte") {
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
    // Piecewise kana can leave a sokuon letter behind ("あtt", "ざsし"); the
    // options Jev sees are these surfaces, and garbled Japanese loses to English.
    for s in segments.iter_mut().filter(|s| s.kind == SegmentKind::Ja) {
        let latin = |t: &str| t.chars().filter(|c| c.is_ascii_alphabetic()).count();
        let whole = to_ime_kana(&s.raw, false);
        if latin(&whole) < latin(&s.surface) {
            s.surface = whole;
        }
    }
    segments
}

pub(crate) fn push_other(segments: &mut Vec<Segment>, raw: char, surface: char) {
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

pub(crate) fn merge_adjacent(segments: Vec<Segment>) -> Vec<Segment> {
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

/// Which characters of the buffer a segmentation reads as English.
pub fn english_mask(segments: &[Segment]) -> Vec<bool> {
    segments
        .iter()
        .flat_map(|s| std::iter::repeat(s.kind == SegmentKind::En).take(s.raw.chars().count()))
        .collect()
}

/// Whether a person could have meant this reading. Jev is easily swayed by
/// options no one would type ("てstがとおらない", "issueをたてmasu"), so
/// those are not offered.
fn plausible(cand: &[Segment], extra_en: &HashSet<String>) -> bool {
    let known = |w: &str| EN_WORDS.contains(w) || PROPER.contains_key(w) || extra_en.contains(w);
    // Fine as romaji; a final n is ん still being typed ("hen").
    let readable = |w: &str| {
        let left = crate::romaji::scan(w).1;
        left.is_empty() || (w.len() > 1 && w.ends_with('n') && left == [w.len() - 1])
    };
    for (i, seg) in cand.iter().enumerate() {
        let lower = seg.raw.to_ascii_lowercase();
        let prev_en = i > 0 && cand[i - 1].kind == SegmentKind::En;
        if i > 0 && splits_known_word(&cand[i - 1], seg, &known) {
            return false;
        }
        match seg.kind {
            SegmentKind::Ja => {
                let last = i + 1 == cand.len();
                let leftovers = if last {
                    hard_leftovers(&lower)
                } else {
                    crate::romaji::scan(&lower).1
                };
                // "mm"/"rr" are not in the romaji table on purpose, but typed
                // doubled they still mean っ ("purogurammingu").
                let chars: Vec<char> = lower.chars().collect();
                let doubled = |&p: &usize| chars.get(p + 1) == Some(&chars[p]);
                if leftovers.iter().any(|p| !doubled(p)) {
                    return false;
                }
                if prev_en {
                    // "Than|k": a lone consonant after English belongs to the word.
                    if lower.chars().all(|c| c.is_ascii_alphabetic() && !"aiueo".contains(c)) {
                        return false;
                    }
                    // "kubernetesn|osettei": a syllable cut in half.
                    let prev = cand[i - 1].raw.to_ascii_lowercase();
                    let cut = lower.starts_with(|c: char| "aiueo".contains(c))
                        && prev.ends_with(|c: char| c.is_ascii_alphabetic() && !"aiueo".contains(c));
                    let last_word = cand[i - 1].surface.rsplit(' ').next().unwrap_or("").to_ascii_lowercase();
                    if cut && !known(&last_word) {
                        return false;
                    }
                    // "at|ta", "kit|te", "mecc|ha": the English side's closing
                    // consonant belongs to the next kana (った, って, っちゃ).
                    // Short known words are cut this way too ("set|tei").
                    let typed_upper = cand[i - 1]
                        .surface
                        .rsplit(' ')
                        .next()
                        .is_some_and(|w| w.starts_with(|c: char| c.is_ascii_uppercase()));
                    if !typed_upper
                        && (!known(&last_word) || last_word.len() <= 3)
                        && cuts_syllable(&prev, &lower, false)
                    {
                        return false;
                    }
                }
            }
            SegmentKind::En => {
                // "at", "att", "kit": romaji still being typed (あっt), not an
                // English word. A known or capitalized word is still offered.
                if i + 1 == cand.len() {
                    if let Some(word) = seg.surface.rsplit(' ').next().filter(|w| w.is_ascii()) {
                        let w = word.to_ascii_lowercase();
                        if !known(&w)
                            && !word.starts_with(|c: char| c.is_ascii_uppercase())
                            && hard_leftovers(&w).is_empty()
                        {
                            return false;
                        }
                    }
                }
                for word in seg.surface.split(' ').filter(|w| w.is_ascii()) {
                    let w = word.to_ascii_lowercase();
                    if !known(&w) && swallows_particle(word, &known) {
                        return false;
                    }
                    // Lowercase and fine as romaji: unknown ("masu", "kuda") or a
                    // short word that is also Japanese ("no", "are").
                    if (!known(&w) || w.len() <= 3)
                        && !word.starts_with(|c: char| c.is_ascii_uppercase())
                        && readable(&w)
                    {
                        return false;
                    }
                }
            }
            SegmentKind::Other => {}
        }
    }
    true
}

/// "at|ta", "mecc|ha": the consonants closing `left` are read together with
/// the start of `right` as one kana syllable (った, っちゃ), so the boundary
/// runs through a syllable. A closing "n" (ん) is only counted with
/// `count_n`: English often ends in n before a particle ("kotlin|de").
pub(crate) fn cuts_syllable(left: &str, right: &str, count_n: bool) -> bool {
    let tail: Vec<char> = left
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphabetic() && !"aiueoAIUEO".contains(*c))
        .collect();
    let Some(&first) = tail.last() else {
        return false;
    };
    if !right.starts_with(|c: char| c.is_ascii_alphabetic()) || (!count_n && first.eq_ignore_ascii_case(&'n')) {
        return false;
    }
    let joined: String = tail.iter().rev().chain(right.chars().collect::<Vec<_>>().iter()).collect();
    !crate::romaji::scan(&joined).1.contains(&0)
}

/// "terraformwoa|pply", "sampleco|de": the seam between English and Japanese
/// runs through a known word.
fn splits_known_word(left: &Segment, right: &Segment, known: &dyn Fn(&str) -> bool) -> bool {
    if left.kind == right.kind || left.kind == SegmentKind::Other || right.kind == SegmentKind::Other {
        return false;
    }
    let l: Vec<char> = left.raw.to_ascii_lowercase().chars().collect();
    let r: Vec<char> = right.raw.to_ascii_lowercase().chars().collect();
    (1..=l.len().min(12)).any(|a| {
        (1..=r.len().min(12)).any(|b| {
            if a + b < 4 {
                return false;
            }
            let word: String = l[l.len() - a..].iter().chain(&r[..b]).collect();
            known(&word)
        })
    })
}

/// "branchwo", "servergaochiteru", "AWSnoconsole": an English-looking start
/// followed by a particle, all read as one unknown word. Jev tends to pick
/// these, but English words almost never contain a particle at such a seam.
fn swallows_particle(word: &str, known: &dyn Fn(&str) -> bool) -> bool {
    const PARTICLES: &[&str] = &["no", "ni", "wo", "ga", "de", "to", "wa", "mo", "ha", "shi"];
    let typed: Vec<char> = word.chars().collect();
    let lower: Vec<char> = word.to_ascii_lowercase().chars().collect();
    (2..lower.len()).any(|k| {
        let rest: String = lower[k..].iter().collect();
        if !PARTICLES.iter().any(|p| rest.starts_with(p)) {
            return false;
        }
        // Part of a known word at the end ("sample|code"), not a particle.
        let inside_known_tail = (0..=k).any(|j| {
            let tail: String = lower[j..].iter().collect();
            tail.len() >= 3 && known(&tail)
        });
        if inside_known_tail {
            return false;
        }
        let head: String = lower[..k].iter().collect();
        known(&head)
            || !crate::romaji::scan(&head).1.is_empty()
            || typed[..k].iter().all(|c| c.is_ascii_uppercase())
    })
}

/// Plausible readings of the buffer for a judge (Jev) to choose from.
/// The offline best is always first, the all-Japanese reading second.
pub fn alternatives(raw: &str, limit: usize) -> Vec<Vec<Segment>> {
    alternatives_with(raw, limit, &HashSet::new())
}

/// [`alternatives`] with extra known English words (learned from past commits).
pub fn alternatives_with(raw: &str, limit: usize, extra_en: &HashSet<String>) -> Vec<Vec<Segment>> {
    let best = segment_with(raw, extra_en);
    let mut out: Vec<Vec<Segment>> = Vec::new();
    let mut seen: HashSet<Vec<bool>> = HashSet::new();
    let push = |cand: Vec<Segment>, out: &mut Vec<Vec<Segment>>, seen: &mut HashSet<Vec<bool>>| {
        let cand = merge_adjacent(cand);
        // The offline best is always offered, even when it looks odd.
        if !out.is_empty() && !plausible(&cand, extra_en) {
            return;
        }
        if out.len() < limit && seen.insert(english_mask(&cand)) {
            out.push(cand);
        }
    };
    push(best.clone(), &mut out, &mut seen);

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
    push(all_ja, &mut out, &mut seen);

    for cand in crate::readings::readings(raw, extra_en, limit) {
        push(cand, &mut out, &mut seen);
    }

    for i in 0..best.len() {
        if let Some(flipped) = flip(&best[i]) {
            let mut cand = best.clone();
            cand[i] = flipped;
            push(cand, &mut out, &mut seen);
        }
    }

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
    push(cased, &mut out, &mut seen);

    out
}

/// English words worth remembering from a commit: shown in `committed`, not in
/// the built-in lexicon, and not readable as romaji ("make" could be まけ, so
/// learning it would turn Japanese into English later).
pub fn words_to_learn(committed: &str, segments: &[Segment]) -> Vec<String> {
    let mut out = Vec::new();
    for s in segments.iter().filter(|s| s.kind == SegmentKind::En) {
        for word in s.surface.split(' ') {
            let lower = word.to_ascii_lowercase();
            if !lower.chars().all(|c| c.is_ascii_lowercase())
                || !committed.contains(word)
                || EN_WORDS.contains(lower.as_str())
                || PROPER.contains_key(lower.as_str())
                || !worth_learning(word)
                || out.contains(&lower)
            {
                continue;
            }
            out.push(lower);
        }
    }
    out
}

/// Whether a committed English word (as typed) looks like a real word rather
/// than romaji or a typo. Only decides what gets learned from now on; words
/// already in the learned list are loaded as they are.
pub fn worth_learning(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    if lower.len() < 2 || !lower.chars().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    // Readable as romaji: Japanese ("ltu" = っ).
    if crate::romaji::scan(&lower).1.is_empty() {
        return false;
    }
    let chars: Vec<char> = lower.chars().collect();
    if chars.iter().all(|&c| c == chars[0]) {
        return false; // "kk", "aaa": key mashing
    }
    let is_vowel = |c: char| "aeiouy".contains(c);
    if !chars.iter().any(|&c| is_vowel(c)) {
        // Acronyms (ssh, pc, npm, https) have no vowels; longer runs are mashing.
        return chars.len() <= 5;
    }
    // Romaji still being typed ("att" of "atta", "on" of "onaji"): a letter or
    // two more would make it Japanese. Real words of that shape ("bot") are
    // left out too; learning them would pull "botan" towards English.
    if pending_romaji(&lower) {
        return false;
    }
    let runs: Vec<usize> = lower.split(is_vowel).map(str::len).collect();
    // English starts with at most three consonants (str, spl) and rarely
    // stacks five inside a word; "dstry", "yoiunsmsrfr" do.
    runs[0] <= 3 && runs.iter().all(|&n| n <= 4)
}

/// `word` is not romaji yet, but becomes romaji with one or two more letters.
fn pending_romaji(word: &str) -> bool {
    const LETTERS: &str = "abcdefghijklmnopqrstuvwxyz";
    LETTERS.chars().any(|a| {
        crate::romaji::scan(&format!("{word}{a}")).1.is_empty()
            || LETTERS
                .chars()
                .any(|b| crate::romaji::scan(&format!("{word}{a}{b}")).1.is_empty())
    })
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
        assert_eq!(shape("datatte"), vec![en("data"), ja("tte")]);
        assert_eq!(live_convert("datatte").surface, "dataって");
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
    fn words_outside_the_lexicon_are_found_by_unreadable_romaji() {
        assert_eq!(
            shape("kubernetesnosettei"),
            vec![en("kubernetes"), ja("nosettei")]
        );
        assert_eq!(
            shape("terraformwoapply"),
            vec![en("terraform"), ja("wo"), en("apply")]
        );
    }

    #[test]
    fn readable_japanese_stays_japanese() {
        for raw in [
            "fairuwohiraku",
            "matchawonomu",
            "chekkushite",
            "konnnichiha",
            "sakkaa",
            "thi-shatsu",
            "kyouhaiitenk",
            "xtukoshi",
        ] {
            assert!(
                segment(raw).iter().all(|s| s.kind != SegmentKind::En),
                "{raw}: {:?}",
                segment(raw)
            );
        }
    }

    /// Small tsu, ん and syllables still being typed: every keystroke of these
    /// is Japanese, offline and in the options Jev chooses from ("att" must
    /// not be offered as English, or "atta" shows as "attあ").
    const SOKUON_AND_N: &[&str] = &[
        "atta", "motto", "kitte", "zutto", "chotto", "matte", "yappari", "kekkou", "sakki",
        "ippai", "konnnichiha", "kan'i", "tsukau", "xtukoshi", "ltukoshi", "ltsu", "hon",
        "honwoyomu", "kitto", "gakkou", "zasshi", "mecchakucha", "kotchi", "tokkyo",
    ];

    #[test]
    fn sokuon_prefixes_are_never_english() {
        for word in SOKUON_AND_N {
            let chars: Vec<char> = word.chars().collect();
            for k in 1..=chars.len() {
                let prefix: String = chars[..k].iter().collect();
                assert!(
                    segment(&prefix).iter().all(|s| s.kind != SegmentKind::En),
                    "offline {prefix}: {:?}",
                    segment(&prefix)
                );
                let alts = alternatives(&prefix, 8);
                assert!(
                    alts.iter().all(|a| a.iter().all(|s| s.kind != SegmentKind::En)),
                    "options {prefix}: {:?}",
                    alts.iter().map(|a| render_offline(a)).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn sokuon_is_rendered_as_small_tsu() {
        assert_eq!(live_convert("att").surface, "あっt");
        assert_eq!(live_convert("zasshi").surface, "ざっし");
        assert_eq!(live_convert("kotchi").surface, "こっち");
        assert_eq!(live_convert("mecchakucha").surface, "めっちゃくちゃ");
        assert_eq!(live_convert("xtukoshi").surface, "っこし");
    }

    #[test]
    fn english_is_still_offered_next_to_sokuon() {
        // Known, capitalized or unreadable English stays on offer.
        let has = |raw: &str, want: &str| {
            alternatives(raw, 8).iter().any(|a| render_offline(a) == want)
        };
        assert!(has("git", "git"));
        assert!(has("Att", "Att"));
        assert!(has("Slackdezuttomatteta", "Slackでずっとまってた"));
        assert!(has("reviewshitemitakedoyappari", "reviewしてみたけどやっぱり"));
        assert_eq!(shape("PRwokittekudasai")[0], en("PR"));
        // "you" after Japanese is よう, at the start it is English.
        assert!(segment("sakkiittayouni").iter().all(|s| s.kind != SegmentKind::En));
        assert_eq!(shape("yoursessionhasexpiredto")[0], en("yoursessionhasexpired"));
    }

    #[test]
    fn syllable_cut_at_the_seam() {
        assert!(cuts_syllable("at", "ta", false));
        assert!(cuts_syllable("att", "a", false));
        assert!(cuts_syllable("mecc", "ha", false));
        assert!(!cuts_syllable("git", "pull", false));
        assert!(!cuts_syllable("Zoom", "de", false));
        assert!(!cuts_syllable("data", "tte", false));
        assert!(!cuts_syllable("kotlin", "de", false));
        assert!(cuts_syllable("kotlin", "de", true));
    }

    #[test]
    fn long_vowel_is_fullwidth() {
        assert_eq!(live_convert("ko-hi-").surface, "こーひー");
    }

    #[test]
    fn alternatives_offer_flips() {
        let alts = alternatives("henshiwaThank", 8);
        assert_eq!(alts[0], segment("henshiwaThank"));
        assert!(alts.len() <= 8);
        // Japanese stays on offer when it reads as Japanese…
        assert!(alternatives("datatte", 8)
            .iter()
            .any(|a| a.iter().all(|s| s.kind == SegmentKind::Ja)));
        // …but not as garbled kana ("てぁnk").
        assert!(!alts
            .iter()
            .any(|a| a.iter().all(|s| s.kind == SegmentKind::Ja)));
        let unique: HashSet<String> = alts.iter().map(|a| format!("{a:?}")).collect();
        assert_eq!(unique.len(), alts.len());
    }

    #[test]
    fn particles_glued_to_english_are_not_offered() {
        let known = |w: &str| EN_WORDS.contains(w);
        for w in ["branchwo", "serverga", "bugwo", "AWSnoconsole", "Figmanodezain"] {
            assert!(swallows_particle(w, &known), "{w}");
        }
        for w in ["Gemini", "Notion", "kubernetes", "production", "samplecode"] {
            assert!(!swallows_particle(w, &known), "{w}");
        }
    }

    #[test]
    fn junk_readings_are_not_offered() {
        let none = HashSet::new();
        let en = |raw: &str, surface: &str| Segment { kind: SegmentKind::En, raw: raw.into(), surface: surface.into() };
        let ja = |raw: &str| Segment { kind: SegmentKind::Ja, raw: raw.into(), surface: to_ime_kana(raw, false) };
        assert!(!plausible(&[en("Meetno", "Meet no")], &none));
        assert!(!plausible(&[en("hen", "hen"), ja("shiwa")], &none));
        assert!(!plausible(&[en("terraform", "terraform"), ja("woa"), en("pply", "pply"), ja("shita")], &none));
        assert!(!plausible(&[en("closeshimasu", "closeshimasu")], &none));
        assert!(plausible(&[en("terraform", "terraform"), ja("wo"), en("apply", "apply"), ja("shita")], &none));
        assert!(plausible(&[en("data", "data"), ja("tte")], &none));
    }

    #[test]
    fn learns_only_unreadable_words_that_were_committed() {
        let segments = vec![
            Segment { kind: SegmentKind::En, raw: "Figma".into(), surface: "Figma".into() },
            Segment { kind: SegmentKind::Ja, raw: "no".into(), surface: "の".into() },
            Segment { kind: SegmentKind::En, raw: "makegit".into(), surface: "make git".into() },
        ];
        assert_eq!(words_to_learn("Figmaのmake git", &segments), vec!["figma"]);
        assert!(words_to_learn("ふぃgmaの", &segments).is_empty());
    }

    /// Words that were actually learned before the rules were tightened.
    #[test]
    fn learning_rejects_romaji_fragments_and_typos() {
        let table = [
            ("att", false),         // "atta" still being typed
            ("ut", false),          // "uta"…
            ("altultu", false),     // あっっ
            ("ltu", false),         // っ
            ("ssh", true),
            ("yoiunsmsrfr", false), // mashing
            ("dstry", false),       // typo
            ("on", false),          // "onaji"…
            ("pc", true),
        ];
        for (word, learn) in table {
            let segments = [Segment { kind: SegmentKind::En, raw: word.into(), surface: word.into() }];
            let got = words_to_learn(word, &segments);
            assert_eq!(!got.is_empty(), learn, "{word}: {got:?}");
        }
        for word in ["figma", "docker", "slack", "github", "kubectl", "npm", "https", "strength"] {
            assert!(worth_learning(word), "{word}");
        }
        for word in ["kk", "sdfghjk", "ky", "tt"] {
            assert!(!worth_learning(word), "{word}");
        }
        assert!(!worth_learning("Bot"), "could be ぼt of ぼたん");
    }

    #[test]
    fn commit_split() {
        let (s, rest) = take_committed("sukoshimattekudasai.").unwrap();
        assert!(s.ends_with('。'), "{s}");
        assert!(rest.is_empty());
    }
}
