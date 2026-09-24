//! Accuracy of the English / Japanese split on a fixed set of typed buffers.
//!
//!   cargo test --test eval -- --nocapture                  # offline + option coverage
//!   $env:JEV_API_KEY = "..."; cargo test --test eval jev -- --ignored --nocapture
//!
//! "offline" is the segmentation shown without Jev; "options" counts cases where
//! the intended split is among the alternatives Jev chooses from (Jev's ceiling).

use std::time::{Duration, Instant};

use mizuyokan_engine::{
    alternatives, render_offline, segment, JevClient, JevConfig, Segment, SegmentKind,
};

struct Case {
    raw: String,
    english: Vec<bool>,
}

fn cases() -> Vec<Case> {
    include_str!("eval_cases.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let mut raw = String::new();
            let mut english = Vec::new();
            let mut inside = false;
            for ch in line.chars() {
                match ch {
                    '[' => inside = true,
                    ']' => inside = false,
                    _ => {
                        raw.push(ch);
                        english.push(inside);
                    }
                }
            }
            Case { raw, english }
        })
        .collect()
}

fn mask(segments: &[Segment]) -> Vec<bool> {
    segments
        .iter()
        .flat_map(|s| std::iter::repeat(s.kind == SegmentKind::En).take(s.raw.chars().count()))
        .collect()
}

fn show(raw: &str, english: &[bool]) -> String {
    let mut out = String::new();
    let mut inside = false;
    for (ch, &en) in raw.chars().zip(english) {
        if en != inside {
            out.push(if en { '[' } else { ']' });
            inside = en;
        }
        out.push(ch);
    }
    if inside {
        out.push(']');
    }
    out
}

struct Score {
    hit: usize,
    total: usize,
    misses: Vec<String>,
}

impl Score {
    fn new() -> Self {
        Self {
            hit: 0,
            total: 0,
            misses: Vec::new(),
        }
    }
    fn add(&mut self, ok: bool, miss: impl FnOnce() -> String) {
        self.total += 1;
        if ok {
            self.hit += 1;
        } else {
            self.misses.push(miss());
        }
    }
    fn report(&self, name: &str) {
        println!("{name}: {}/{}", self.hit, self.total);
        for m in &self.misses {
            println!("  {m}");
        }
    }
}

#[test]
fn offline_and_options() {
    let mut offline = Score::new();
    let mut options = Score::new();
    let mut japanese_kept = Score::new();
    let mut japanese_prefixes = Score::new();
    for case in cases() {
        let got = mask(&segment(&case.raw));
        offline.add(got == case.english, || {
            format!(
                "{}  want {}",
                show(&case.raw, &got),
                show(&case.raw, &case.english)
            )
        });
        let alts = alternatives(&case.raw, 8);
        options.add(alts.iter().any(|a| mask(a) == case.english), || {
            show(&case.raw, &case.english)
        });
        if case.english.iter().all(|&e| !e) {
            japanese_kept.add(got.iter().all(|&e| !e), || show(&case.raw, &got));
            // Every keystroke on the way: Jev must not even be offered English
            // for a Japanese word being typed ("att" before "atta").
            let chars: Vec<char> = case.raw.chars().collect();
            let offered = (2..=chars.len()).find_map(|k| {
                let prefix: String = chars[..k].iter().collect();
                alternatives(&prefix, 8)
                    .into_iter()
                    .find(|a| a.iter().any(|s| s.kind == SegmentKind::En))
                    .map(|a| format!("{prefix} -> {}", render_offline(&a)))
            });
            japanese_prefixes.add(offered.is_none(), || offered.unwrap());
        }
    }
    offline.report("offline");
    options.report("options");
    japanese_kept.report("japanese kept japanese (offline)");
    japanese_prefixes.report("japanese never offered as english while typing");

    // Regression floors: raise them when the numbers improve.
    assert!(offline.hit >= 59, "offline accuracy dropped");
    assert!(options.hit >= 88, "option coverage dropped");
    // Misses left: get/set/ok (known short words, still Jev's call) and "purogurammi".
    assert!(japanese_prefixes.hit >= 33, "English offered for Japanese being typed");
    // "purogurammingu": mm is not read as っ on purpose (see romaji.rs), so
    // this one stays a known miss.
    assert!(japanese_kept.hit + 1 >= japanese_kept.total, "Japanese read as English offline");
}

#[test]
#[ignore]
fn jev() {
    let api_key = std::env::var("JEV_API_KEY").expect("set JEV_API_KEY");
    let client = JevClient::new(JevConfig {
        api_key,
        endpoint: std::env::var("JEV_ENDPOINT")
            .unwrap_or_else(|_| "https://ai-gateway.lolipop.jp/v1/systemone".into()),
        model: std::env::var("JEV_MODEL").unwrap_or_else(|_| "typesafe/jev-latest".into()),
        timeout: Duration::from_secs(5),
    });
    // (want, option masks, probabilities) per case; policies are replayed below.
    let mut runs: Vec<(Case, Vec<Vec<bool>>, Vec<f64>)> = Vec::new();
    let mut latencies = Vec::new();
    let mut failed = 0;
    for case in cases() {
        let options = alternatives(&case.raw, 8);
        let masks: Vec<Vec<bool>> = options.iter().map(|o| mask(o)).collect();
        let texts: Vec<String> = options.iter().map(|o| render_offline(o)).collect();
        let probs = if options.len() < 2 {
            vec![1.0]
        } else {
            let started = Instant::now();
            let result = client.choose_reading(&case.raw, &texts);
            latencies.push(started.elapsed());
            match result {
                Ok(p) => p,
                Err(e) => {
                    println!("{}  FAILED {e}", case.raw);
                    failed += 1;
                    continue;
                }
            }
        };
        runs.push((case, masks, probs));
    }

    let argmax = |p: &[f64]| {
        p.iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    // Follow Jev only when it is at least this sure; otherwise keep the offline best (option 0).
    for threshold in [0.0, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9] {
        let hit = runs
            .iter()
            .filter(|(case, masks, probs)| {
                let i = argmax(probs);
                let pick = if probs[i] >= threshold { i } else { 0 };
                masks[pick] == case.english
            })
            .count();
        println!("jev (follow when p >= {threshold:.1}): {hit}/{}", runs.len() + failed);
    }
    println!("misses when always following Jev:");
    for (case, masks, probs) in &runs {
        let i = argmax(probs);
        if masks[i] != case.english {
            let want = masks.iter().position(|m| *m == case.english);
            println!(
                "  {}  want {}  p={:.2} p(want)={}",
                show(&case.raw, &masks[i]),
                show(&case.raw, &case.english),
                probs[i],
                want.map_or("not offered".to_string(), |w| format!("{:.2}", probs[w]))
            );
        }
    }
    latencies.sort();
    if !latencies.is_empty() {
        let pct = |p: usize| latencies[(latencies.len() - 1) * p / 100];
        println!("latency p50={:?} p90={:?} max={:?}", pct(50), pct(90), pct(100));
    }
}
