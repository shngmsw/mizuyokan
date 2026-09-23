//! Ask Jev in the background while the user is still typing, so the answer is
//! usually ready by the time Space is pressed (Jev itself takes ~0.5 s).
//!
//! Options are rendered offline (kana, no kanji): the kana-kanji converter holds
//! the live composition and must not be touched from another thread, and
//! telling English from romaji does not need kanji.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Condvar, LazyLock, Mutex, RwLock},
    time::{Duration, Instant},
};

use crate::{
    convert::{alternatives_with, as_japanese, concat, render_offline, segment_with, Segment, SegmentKind},
    jev::{JevClient, JevConfig},
};

const MAX_ALTERNATIVES: usize = 8;
const MAX_ENTRIES: usize = 64;
const MAX_LEARNED: usize = 5000;

/// Outcome of judging one raw buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum Judgement {
    /// Jev (or the lack of any ambiguity) settled on this segmentation.
    Chosen(Vec<Segment>),
    /// Jev failed or timed out; keep the converter's own candidates.
    Failed,
}

#[derive(Default)]
struct State {
    done: HashMap<String, Judgement>,
    order: VecDeque<String>,
    in_flight: HashMap<String, Instant>,
}

impl State {
    fn finish_in_flight(&mut self, raw: &str) {
        self.in_flight.remove(raw);
    }

    /// Remember a successful judgement. Failures are not cached so a later
    /// keystroke can retry once the API is healthy again.
    fn store_chosen(&mut self, raw: String, segments: Vec<Segment>) {
        self.in_flight.remove(&raw);
        if self
            .done
            .insert(raw.clone(), Judgement::Chosen(segments))
            .is_none()
        {
            self.order.push_back(raw);
        }
        while self.order.len() > MAX_ENTRIES {
            if let Some(old) = self.order.pop_front() {
                self.done.remove(&old);
            }
        }
    }
}

pub struct Prefetcher {
    state: Mutex<State>,
    ready: Condvar,
    /// English words confirmed by past commits; treated like the built-in lexicon.
    learned: RwLock<HashSet<String>>,
}

/// Picks one of `options` (rendered texts); `None` when no decision could be made.
pub type Judge = dyn Fn(&str, &[String]) -> Option<usize> + Send + Sync;

pub fn judge_segments(raw: &str, judge: &Judge) -> Judgement {
    judge_segments_with(raw, &HashSet::new(), judge)
}

/// [`judge_segments`] with extra known English words.
pub fn judge_segments_with(raw: &str, extra_en: &HashSet<String>, judge: &Judge) -> Judgement {
    let options = alternatives_with(raw, MAX_ALTERNATIVES, extra_en);
    if options.len() < 2 {
        return Judgement::Chosen(options.into_iter().next().unwrap_or_default());
    }
    let texts: Vec<String> = options.iter().map(|o| render_offline(o)).collect();
    match judge(raw, &texts).and_then(|i| options.get(i).cloned()) {
        Some(chosen) => Judgement::Chosen(chosen),
        None => Judgement::Failed,
    }
}

pub fn jev_judge(config: JevConfig) -> Arc<Judge> {
    Arc::new(move |raw: &str, options: &[String]| match JevClient::new(config.clone())
        .choose_reading(raw, options)
    {
        Ok(probs) => probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i),
        Err(_) => None,
    })
}

impl Prefetcher {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            learned: RwLock::new(HashSet::new()),
        }
    }

    pub fn learned(&self) -> HashSet<String> {
        self.learned.read().map(|l| l.clone()).unwrap_or_default()
    }

    /// Add English words (lowercase); returns the ones that were new.
    pub fn remember(&self, words: impl IntoIterator<Item = String>) -> Vec<String> {
        let Ok(mut learned) = self.learned.write() else {
            return Vec::new();
        };
        words
            .into_iter()
            .filter(|w| learned.len() < MAX_LEARNED && learned.insert(w.clone()))
            .collect()
    }

    /// Offline segmentation that also knows the learned words.
    pub fn segment(&self, raw: &str) -> Vec<Segment> {
        segment_with(raw, &self.learned())
    }

    pub fn global() -> &'static Prefetcher {
        static GLOBAL: LazyLock<Prefetcher> = LazyLock::new(Prefetcher::new);
        &GLOBAL
    }

    /// Start judging `raw` on a background thread unless it is known or in flight.
    pub fn request(&'static self, raw: &str, judge: Arc<Judge>) {
        {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            if state.done.contains_key(raw) || state.in_flight.contains_key(raw) {
                return;
            }
            state.in_flight.insert(raw.to_string(), Instant::now());
        }
        let raw = raw.to_string();
        std::thread::spawn(move || {
            let judgement = judge_segments_with(&raw, &self.learned(), judge.as_ref());
            if let Ok(mut state) = self.state.lock() {
                match judgement {
                    Judgement::Chosen(segments) => state.store_chosen(raw, segments),
                    Judgement::Failed => state.finish_in_flight(&raw),
                }
            }
            self.ready.notify_all();
        });
    }

    /// Result for `raw`, waiting up to `timeout` if a request is in flight.
    /// `None` = never requested (or a failed attempt finished without caching).
    /// `Some(Failed)` = still in flight when the timeout expired.
    pub fn wait(&self, raw: &str, timeout: Duration) -> Option<Judgement> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().ok()?;
        loop {
            if let Some(judgement) = state.done.get(raw) {
                return Some(judgement.clone());
            }
            if !state.in_flight.contains_key(raw) {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return Some(Judgement::Failed);
            }
            state = self.ready.wait_timeout(state, deadline - now).ok()?.0;
        }
    }

    /// The longest judged prefix of `raw`: its byte length and segmentation.
    fn judged_prefix(&self, raw: &str) -> Option<(usize, Vec<Segment>)> {
        let state = self.state.lock().ok()?;
        let boundaries: Vec<usize> = raw
            .char_indices()
            .map(|(i, _)| i)
            .skip(1)
            .chain(std::iter::once(raw.len()))
            .collect();
        boundaries.iter().rev().find_map(|&end| match state.done.get(&raw[..end]) {
            Some(Judgement::Chosen(segments)) => Some((end, segments.clone())),
            _ => None,
        })
    }

    /// Segmentation to show while typing, without waiting: the longest prefix of
    /// `raw` that Jev judged to contain English, with the rest read as Japanese.
    /// `None` when no judged prefix contains English (plain kana-kanji conversion).
    pub fn confirmed(&self, raw: &str) -> Option<Vec<Segment>> {
        let (end, segments) = self.judged_prefix(raw)?;
        if !segments.iter().any(|s| s.kind == SegmentKind::En) {
            return None;
        }
        Some(concat(segments, as_japanese(&raw[end..])))
    }

    /// Like [`Self::confirmed`], but while Jev has not answered for the
    /// English part yet, show the offline guess instead of plain kana: without
    /// it English is always garbled until the answer arrives. A judged prefix
    /// still wins: offline English is used only where it starts after it.
    pub fn live(&self, raw: &str) -> Option<Vec<Segment>> {
        let judged = self.judged_prefix(raw);
        if let Some((_, segments)) = &judged {
            if segments.iter().any(|s| s.kind == SegmentKind::En) {
                return self.confirmed(raw);
            }
        }
        let judged_end = judged.map_or(0, |(end, _)| raw[..end].chars().count());
        let offline = self.segment(raw);
        let mut pos = 0;
        let mut has_en = false;
        for s in &offline {
            if s.kind == SegmentKind::En {
                if pos < judged_end {
                    return None;
                }
                has_en = true;
            }
            pos += s.raw.chars().count();
        }
        has_en.then_some(offline)
    }

    /// Judge synchronously (used when nothing was prefetched). Success is cached;
    /// failure is not, so the next call can retry.
    pub fn judge_now(&self, raw: &str, judge: &Judge) -> Judgement {
        let judgement = judge_segments_with(raw, &self.learned(), judge);
        if let Ok(mut state) = self.state.lock() {
            match &judgement {
                Judgement::Chosen(segments) => {
                    state.store_chosen(raw.to_string(), segments.clone());
                }
                Judgement::Failed => state.finish_in_flight(raw),
            }
        }
        judgement
    }
}

impl Default for Prefetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leak() -> &'static Prefetcher {
        Box::leak(Box::new(Prefetcher::new()))
    }

    fn prefer_english() -> Arc<Judge> {
        Arc::new(|_: &str, options: &[String]| options.iter().position(|o| o.starts_with("git")))
    }

    #[test]
    fn prefetched_result_is_ready_on_wait() {
        let p = leak();
        p.request("gitpullshitara", prefer_english());
        let Some(Judgement::Chosen(segments)) = p.wait("gitpullshitara", Duration::from_secs(5))
        else {
            panic!("no judgement");
        };
        assert_eq!(segments[0].kind, SegmentKind::En);
        assert_eq!(render_offline(&segments), "git pullしたら");
    }

    #[test]
    fn unknown_buffer_is_none() {
        assert_eq!(leak().wait("never", Duration::from_millis(10)), None);
    }

    #[test]
    fn slow_judge_times_out_as_failed() {
        let p = leak();
        let slow: Arc<Judge> = Arc::new(|_: &str, _: &[String]| {
            std::thread::sleep(Duration::from_millis(300));
            Some(0)
        });
        p.request("gitpullshitara", slow);
        assert_eq!(
            p.wait("gitpullshitara", Duration::from_millis(20)),
            Some(Judgement::Failed)
        );
    }

    #[test]
    fn failed_judge_is_not_cached() {
        let p = leak();
        let none: Arc<Judge> = Arc::new(|_: &str, _: &[String]| None);
        assert_eq!(
            p.judge_now("gitpullshitara", none.as_ref()),
            Judgement::Failed
        );
        // A failure must not stick: the next wait reports "never requested"
        // so the IME can fall back to plain azooKey and retry later.
        assert_eq!(p.wait("gitpullshitara", Duration::ZERO), None);
    }

    #[test]
    fn confirmed_extends_the_longest_judged_prefix() {
        let p = leak();
        let english = prefer_english();
        assert_eq!(p.confirmed("gitpullshi"), None);
        p.judge_now("gitpullshi", english.as_ref());

        let exact = p.confirmed("gitpullshi").unwrap();
        assert_eq!(render_offline(&exact), "git pullし");

        let longer = p.confirmed("gitpullshitara").unwrap();
        assert_eq!(render_offline(&longer), "git pullしたら");
        assert_eq!(longer.last().unwrap().raw, "shitara");

        judged_japanese(p, "gitpullshitar");
        assert_eq!(p.confirmed("gitpullshitara"), None);
    }

    /// As if Jev had picked the all-Japanese reading of `raw`.
    fn judged_japanese(p: &Prefetcher, raw: &str) {
        p.state
            .lock()
            .unwrap()
            .store_chosen(raw.to_string(), as_japanese(raw));
    }

    #[test]
    fn live_shows_offline_english_until_jev_answers() {
        let p = leak();
        // Nothing judged yet: the offline guess instead of garbled kana.
        assert_eq!(render_offline(&p.live("gitpullshitara").unwrap()), "git pullしたら");
        // Offline Japanese stays plain azooKey.
        assert_eq!(p.live("sukoshimatte"), None);

        // Jev said the start is Japanese: offline English inside it is not shown…
        judged_japanese(p, "gitpu");
        assert_eq!(p.live("gitpullshitara"), None);
        // …but English typed after the judged part is.
        judged_japanese(p, "kyouha");
        let shown = p.live("kyouhagitpull").unwrap();
        assert_eq!(shown.last().unwrap().kind, SegmentKind::En);

        // A judgement with English is used as before.
        p.judge_now("gitpullshi", prefer_english().as_ref());
        assert_eq!(p.live("gitpullshitara"), p.confirmed("gitpullshitara"));
    }

    #[test]
    fn suffix_is_never_guessed_as_english() {
        let p = leak();
        p.judge_now("PRno", prefer_english_acronym().as_ref());
        let segments = p.confirmed("PRnoareba").unwrap();
        assert_eq!(render_offline(&segments), "PRのあれば");
    }

    fn prefer_english_acronym() -> Arc<Judge> {
        Arc::new(|_: &str, options: &[String]| options.iter().position(|o| o.starts_with("PR")))
    }

    #[test]
    fn learned_words_shape_the_options() {
        let p = leak();
        assert_eq!(p.remember(["figma".to_string()]), vec!["figma".to_string()]);
        assert!(p.remember(["figma".to_string()]).is_empty());
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let record = seen.clone();
        let first: Arc<Judge> = Arc::new(move |_: &str, options: &[String]| {
            *record.lock().unwrap() = options.to_vec();
            Some(0)
        });
        let Judgement::Chosen(segments) = p.judge_now("Figmanodezain", first.as_ref()) else {
            panic!("no judgement");
        };
        assert_eq!(segments[0].raw, "Figma");
        assert_eq!(p.segment("figmanodezain")[0].kind, SegmentKind::En);
        // Often nothing else is plausible and Jev is not even asked.
        let options = seen.lock().unwrap().clone();
        if let Some(first) = options.first() {
            assert!(first.starts_with("Figmaの"));
        }
    }

    #[test]
    fn cache_is_bounded() {
        let p = leak();
        let first: Arc<Judge> = Arc::new(|_: &str, _: &[String]| Some(0));
        for i in 0..(MAX_ENTRIES + 10) {
            p.judge_now(&format!("x{i}"), first.as_ref());
        }
        let state = p.state.lock().unwrap();
        assert_eq!(state.done.len(), MAX_ENTRIES);
        assert!(!state.done.contains_key("x0"));
    }
}
