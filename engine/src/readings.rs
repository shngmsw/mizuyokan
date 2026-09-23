//! Wider search for readings to offer a judge (Jev).
//!
//! The offline segmentation has to be conservative because it is shown as is.
//! The options Jev chooses from do not: if the intended split is missing there,
//! Jev cannot pick it. So this module scores every way of cutting a letter run
//! into English words and romaji pieces and returns the best few, including
//! boundaries the offline pass never proposes ("Zoom|dehanashimashou",
//! "Notion|nimatomemasu").

use std::collections::HashSet;

use crate::{
    convert::{english_surface, merge_adjacent, push_other, Segment, SegmentKind},
    dict::{EN_WORDS, PROPER},
    romaji::{hard_leftovers, map_commit_punct, scan, to_ime_kana},
};

/// Romaji that usually opens the Japanese right after an English word:
/// particles, the copula and forms of する.
const JA_AFTER_EN: &[&str] = &[
    "no", "ni", "wo", "ga", "de", "to", "wa", "ha", "mo", "tte", "kara", "made", "node", "kedo",
    "shi", "su", "sa", "se", "da", "ya", "ka", "ne", "yo", "e", "o",
];

const MAX_WORD: usize = 24;
/// Every piece costs something, so a run is not chopped into many tiny words.
const PIECE_COST: i32 = -12;
const BEAM: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Piece {
    En,
    Ja,
}

#[derive(Clone)]
struct Path {
    score: i32,
    /// (kind, start, end) over chars of the chunk.
    pieces: Vec<(Piece, usize, usize)>,
}

impl Path {
    fn last(&self) -> Option<Piece> {
        self.pieces.last().map(|p| p.0)
    }
    fn mask(&self) -> Vec<bool> {
        self.pieces
            .iter()
            .flat_map(|&(k, s, e)| std::iter::repeat(k == Piece::En).take(e - s))
            .collect()
    }
}

struct Chunk<'a> {
    original: Vec<char>,
    lower: Vec<char>,
    /// Letters that cannot be read as romaji in the context of the whole run.
    leftover: Vec<bool>,
    extra_en: &'a HashSet<String>,
}

impl Chunk<'_> {
    fn word(&self, i: usize, j: usize) -> String {
        self.lower[i..j].iter().collect()
    }

    fn known(&self, word: &str) -> bool {
        EN_WORDS.contains(word) || PROPER.contains_key(word) || self.extra_en.contains(word)
    }

    /// A letter in `i..j` that is not romaji, in the context of the whole run
    /// or inside the word itself (not just its cut-off last letter).
    fn evidence(&self, i: usize, j: usize) -> bool {
        self.leftover[i..j].iter().any(|&l| l)
            || scan(&self.word(i, j)).1.iter().any(|&p| p + 1 < j - i)
    }

    fn english(&self, i: usize, j: usize) -> Option<i32> {
        let len = (j - i) as i32;
        if len < 2 || !self.lower[i..j].iter().all(|c| c.is_ascii_lowercase()) {
            return None;
        }
        let word = self.word(i, j);
        let typed = &self.original[i..j];
        let evidence = self.evidence(i, j);
        let mut score = PIECE_COST;
        score += if self.extra_en.contains(&word) {
            20 + 3 * len
        } else if self.known(&word) && (len >= 4 || evidence) {
            12 + 3 * len
        } else if evidence {
            10 + len
        } else {
            // Reads fine as romaji ("make", "no"): possible, but never likely.
            -8
        };
        if typed[0].is_ascii_uppercase() {
            score += 15;
        }
        if typed.iter().all(|c| c.is_ascii_uppercase()) {
            score += 20;
        }
        Some(score)
    }

    fn japanese(&self, i: usize, j: usize, after: Option<(usize, usize)>) -> Option<i32> {
        let piece = self.word(i, j);
        let readable = if j == self.lower.len() {
            hard_leftovers(&piece).is_empty()
        } else {
            scan(&piece).1.is_empty()
        };
        if !readable {
            return None;
        }
        let mut score = PIECE_COST + 2 * (j - i) as i32;
        if let Some((s, e)) = after {
            let starts_vowel = "aiueo".contains(self.lower[i]);
            let en_ends_consonant = !"aiueon".contains(self.lower[e - 1]);
            // "Zoomd|ehana": a syllable cut in half is not a word boundary,
            // unless the English word is known ("review|onegai").
            if starts_vowel && en_ends_consonant && !self.known(&self.word(s, e)) {
                return None;
            }
            if JA_AFTER_EN.iter().any(|p| piece.starts_with(p)) {
                score += 12;
            }
        }
        Some(score)
    }

    fn best_paths(&self, k: usize) -> Vec<Path> {
        let n = self.lower.len();
        let mut beams: Vec<Vec<Path>> = vec![Vec::new(); n + 1];
        beams[0].push(Path {
            score: 0,
            pieces: Vec::new(),
        });
        for i in 0..n {
            let here = std::mem::take(&mut beams[i]);
            for path in &here {
                let last = path.last();
                for j in i + 1..=(i + MAX_WORD).min(n) {
                    let en_ok = match last {
                        Some(Piece::En) => {
                            // Two English words in a row only when both are known.
                            let prev = path.pieces.last().unwrap();
                            self.known(&self.word(prev.1, prev.2)) && self.known(&self.word(i, j))
                        }
                        _ => true,
                    };
                    if en_ok {
                        if let Some(sc) = self.english(i, j) {
                            push(&mut beams[j], path, Piece::En, i, j, sc);
                        }
                    }
                    if last != Some(Piece::Ja) {
                        let after = path
                            .pieces
                            .last()
                            .filter(|p| p.0 == Piece::En)
                            .map(|p| (p.1, p.2));
                        if let Some(sc) = self.japanese(i, j, after) {
                            push(&mut beams[j], path, Piece::Ja, i, j, sc);
                        }
                    }
                }
            }
            beams[i] = here;
        }
        let mut done = std::mem::take(&mut beams[n]);
        done.sort_by(|a, b| b.score.cmp(&a.score));
        let mut seen = HashSet::new();
        done.retain(|p| seen.insert(p.mask()));
        done.truncate(k);
        done
    }

    fn segments(&self, path: &Path) -> Vec<Segment> {
        let pieces = path
            .pieces
            .iter()
            .map(|&(kind, i, j)| {
                let raw: String = self.original[i..j].iter().collect();
                match kind {
                    Piece::En => Segment {
                        kind: SegmentKind::En,
                        surface: english_surface(&self.word(i, j), &raw),
                        raw,
                    },
                    Piece::Ja => Segment {
                        kind: SegmentKind::Ja,
                        surface: to_ime_kana(&raw, false),
                        raw,
                    },
                }
            })
            .collect();
        merge_adjacent(pieces)
    }
}

fn push(beam: &mut Vec<Path>, from: &Path, kind: Piece, i: usize, j: usize, sc: i32) {
    let mut pieces = from.pieces.clone();
    pieces.push((kind, i, j));
    beam.push(Path {
        score: from.score + sc,
        pieces,
    });
    if beam.len() > BEAM * 2 {
        beam.sort_by(|a, b| b.score.cmp(&a.score));
        beam.truncate(BEAM);
    }
}

fn chunk_readings(chunk: &str, extra_en: &HashSet<String>, k: usize) -> Vec<(i32, Vec<Segment>)> {
    let original: Vec<char> = chunk.chars().collect();
    let lower: Vec<char> = chunk.to_lowercase().chars().collect();
    let mut leftover = vec![false; lower.len()];
    for p in hard_leftovers(chunk) {
        leftover[p] = true;
    }
    let c = Chunk {
        original,
        lower,
        leftover,
        extra_en,
    };
    c.best_paths(k)
        .iter()
        .map(|p| (p.score, c.segments(p)))
        .collect()
}

enum Item {
    Letters(Vec<(i32, Vec<Segment>)>),
    Other(char),
}

/// Up to `limit` readings of `raw`, best first. Letter runs that have no
/// reading at all (only possible for unreadable leftovers) are kept as typed.
pub(crate) fn readings(raw: &str, extra_en: &HashSet<String>, limit: usize) -> Vec<Vec<Segment>> {
    let mut items = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, items: &mut Vec<Item>| {
        if buf.is_empty() {
            return;
        }
        let mut found = chunk_readings(buf, extra_en, limit);
        if found.is_empty() {
            found.push((
                0,
                vec![Segment {
                    kind: SegmentKind::Ja,
                    raw: buf.clone(),
                    surface: to_ime_kana(buf, false),
                }],
            ));
        }
        items.push(Item::Letters(found));
        buf.clear();
    };
    for ch in raw.chars() {
        if ch.is_ascii_alphabetic() || ch == '\'' || ch == '-' {
            buf.push(ch);
        } else {
            flush(&mut buf, &mut items);
            items.push(Item::Other(ch));
        }
    }
    flush(&mut buf, &mut items);

    // Best reading of every run, then single-run variants by how little they lose.
    let mut picks: Vec<(i32, Vec<usize>)> = vec![(0, vec![0; items.len()])];
    for (idx, item) in items.iter().enumerate() {
        if let Item::Letters(found) = item {
            for (alt, (score, _)) in found.iter().enumerate().skip(1) {
                let mut choice = vec![0; items.len()];
                choice[idx] = alt;
                picks.push((found[0].0 - score, choice));
            }
        }
    }
    picks.sort_by_key(|(loss, _)| *loss);
    picks
        .into_iter()
        .take(limit)
        .map(|(_, choice)| {
            let mut segments = Vec::new();
            for (item, &alt) in items.iter().zip(&choice) {
                match item {
                    Item::Letters(found) => segments.extend(found[alt].1.clone()),
                    Item::Other(ch) => {
                        let surface = match *ch {
                            ',' => '、',
                            c if crate::romaji::is_commit_punct(c) => map_commit_punct(c),
                            c => c,
                        };
                        push_other(&mut segments, *ch, surface);
                    }
                }
            }
            merge_adjacent(segments)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::render_offline;

    fn rendered(raw: &str) -> Vec<String> {
        readings(raw, &HashSet::new(), 8)
            .iter()
            .map(|r| render_offline(r))
            .collect()
    }

    #[test]
    fn boundaries_the_offline_pass_misses() {
        assert!(rendered("Zoomdehanashimashou").contains(&"Zoomではなしましょう".to_string()));
        assert!(rendered("Notionnimatomemasu").contains(&"Notionにまとめます".to_string()));
        assert!(rendered("taskwocloseshimasu").contains(&"taskをcloseします".to_string()));
    }

    #[test]
    fn japanese_only_reading_is_offered() {
        assert!(rendered("getsuyoubi").contains(&"げつようび".to_string()));
        assert!(rendered("sakenonomitai").contains(&"さけののみたい".to_string()));
    }

    #[test]
    fn learned_words_win() {
        let learned: HashSet<String> = ["figma".to_string()].into();
        let top = readings("Figmanodezain", &learned, 8);
        assert!(render_offline(&top[0]).starts_with("Figmaのでざい"), "{top:?}");
    }
}

