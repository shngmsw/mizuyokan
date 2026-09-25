//! mizuyokan engine: language-mode-free JP/EN live conversion.
//!
//! Designed to sit on top of azooKey-Windows composition:
//! keep a raw Latin keystroke buffer, re-segment the whole string into
//! English / Japanese spans on every keystroke (stateless), and let the
//! kana-kanji converter handle the Japanese spans. On demand (Space),
//! Jev picks the most plausible segmentation among alternatives.

mod convert;
mod dict;
mod jev;
mod prefetch;
mod readings;
mod romaji;

pub use convert::{
    alternatives, alternatives_with, as_japanese, english_mask, english_words_within, live_convert,
    may_be_english, render_offline, segment, segment_with, take_committed, words_to_learn,
    worth_learning, ConvertResult, Lexicon, Segment, SegmentKind,
};
pub use jev::{JevClient, JevConfig, JevError};
pub use prefetch::{
    jev_judge, jev_word_check, judge_segments, judge_segments_with, Disowned, Judge, Judgement, Prefetcher,
    Vetted, WordCheck, BLOCK_AFTER_ESCAPES, LEARN_AFTER_COMMITS, LEARN_THRESHOLD_DEFAULT,
};
pub use romaji::is_commit_punct;
