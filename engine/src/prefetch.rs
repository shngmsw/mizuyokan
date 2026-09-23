//! Ask Jev in the background while the user is still typing, so the answer is
//! usually ready by the time Space is pressed (Jev itself takes ~0.5 s).
//!
//! Options are rendered offline (kana, no kanji): the kana-kanji converter holds
//! the live composition and must not be touched from another thread, and
//! telling English from romaji does not need kanji.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Condvar, LazyLock, Mutex},
    time::{Duration, Instant},
};

use crate::{
    convert::{alternatives, as_japanese, concat, render_offline, Segment, SegmentKind},
    jev::{JevClient, JevConfig},
};

const MAX_ALTERNATIVES: usize = 8;
const MAX_ENTRIES: usize = 64;

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
    fn store(&mut self, raw: String, judgement: Judgement) {
        self.in_flight.remove(&raw);
        if self.done.insert(raw.clone(), judgement).is_none() {
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
}

/// Picks one of `options` (rendered texts); `None` when no decision could be made.
pub type Judge = dyn Fn(&str, &[String]) -> Option<usize> + Send + Sync;

pub fn judge_segments(raw: &str, judge: &Judge) -> Judgement {
    let options = alternatives(raw, MAX_ALTERNATIVES);
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
    Arc::new(move |raw: &str, options: &[String]| {
        let probs = JevClient::new(config.clone())
            .choose_reading(raw, options)
            .ok()?;
        probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
    })
}

impl Prefetcher {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
        }
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
            let judgement = judge_segments(&raw, judge.as_ref());
            if let Ok(mut state) = self.state.lock() {
                state.store(raw, judgement);
            }
            self.ready.notify_all();
        });
    }

    /// Result for `raw`, waiting up to `timeout` if a request is in flight.
    /// Returns `None` when `raw` was never requested.
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

    /// Segmentation to show while typing, without waiting: the longest prefix of
    /// `raw` that Jev judged to contain English, with the rest read as Japanese.
    /// `None` when no judged prefix contains English (plain kana-kanji conversion).
    pub fn confirmed(&self, raw: &str) -> Option<Vec<Segment>> {
        let state = self.state.lock().ok()?;
        let boundaries: Vec<usize> = raw
            .char_indices()
            .map(|(i, _)| i)
            .skip(1)
            .chain(std::iter::once(raw.len()))
            .collect();
        for &end in boundaries.iter().rev() {
            let Some(Judgement::Chosen(segments)) = state.done.get(&raw[..end]) else {
                continue;
            };
            if !segments.iter().any(|s| s.kind == SegmentKind::En) {
                return None;
            }
            return Some(concat(segments.clone(), as_japanese(&raw[end..])));
        }
        None
    }

    /// Judge synchronously (used when nothing was prefetched) and remember the result.
    pub fn judge_now(&self, raw: &str, judge: &Judge) -> Judgement {
        let judgement = judge_segments(raw, judge);
        if let Ok(mut state) = self.state.lock() {
            state.store(raw.to_string(), judgement.clone());
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
    fn failed_judge_is_remembered() {
        let p = leak();
        let none: Arc<Judge> = Arc::new(|_: &str, _: &[String]| None);
        assert_eq!(
            p.judge_now("gitpullshitara", none.as_ref()),
            Judgement::Failed
        );
        assert_eq!(
            p.wait("gitpullshitara", Duration::ZERO),
            Some(Judgement::Failed)
        );
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

        let japanese_only: Arc<Judge> = Arc::new(|_: &str, options: &[String]| {
            options
                .iter()
                .position(|o| o.is_ascii() == false && !o.contains("git"))
        });
        p.judge_now("gitpullshitar", japanese_only.as_ref());
        assert_eq!(p.confirmed("gitpullshitara"), None);
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
    fn cache_is_bounded() {
        let p = leak();
        let none: Arc<Judge> = Arc::new(|_: &str, _: &[String]| None);
        for i in 0..(MAX_ENTRIES + 10) {
            p.judge_now(&format!("x{i}"), none.as_ref());
        }
        let state = p.state.lock().unwrap();
        assert_eq!(state.done.len(), MAX_ENTRIES);
        assert!(!state.done.contains_key("x0"));
    }
}
