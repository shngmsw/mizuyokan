//! mizuyokan: Jev-assisted mixed Japanese / English conversion on top of azooKey.
//!
//! Typing behaves like azooKey's live conversion. Jev judges the raw buffer in
//! the background; spans it considers English are fed to azooKey as full-width
//! letters, which azooKey leaves unconverted, and shown back as typed. So the
//! live preview, Space candidates and commits all keep English words intact.
//! Without an API key nothing here runs and the IME is plain azooKey.

use std::{
    path::PathBuf,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant, SystemTime},
};

use anyhow::Result;
use mizuyokan_engine::{
    jev_judge, jev_word_check, words_to_learn, JevConfig, Judgement, Prefetcher, Segment,
    SegmentKind, Vetted, LEARN_THRESHOLD_DEFAULT,
};
use serde::{Deserialize, Serialize};

use super::{
    full_width::to_fullwidth,
    ipc_service::{Candidates, IPCService},
};

const SETTINGS_FILENAME: &str = "mizuyokan.json";
/// English words learned from commits, one per line. Shared by every app
/// that loads the IME; each process appends what it learns.
const WORDS_FILENAME: &str = "mizuyokan_words.txt";
/// Words committed but not learned yet, one line per commit. A word moves to
/// WORDS_FILENAME once it has LEARN_AFTER_COMMITS lines here.
const CANDIDATES_FILENAME: &str = "mizuyokan_word_candidates.txt";
/// Words Jev judged not to be real English, one per line: never learned and
/// never asked about again. Checks that failed are not written here.
const REJECTED_FILENAME: &str = "mizuyokan_words_rejected.txt";
/// Past this many lines the candidates file is started over.
const MAX_CANDIDATE_LINES: usize = 2000;
/// Defaults match karukan's idea of adaptive degrade: a few bad calls and we
/// stop waiting on the enhancement layer so typing stays on plain azooKey.
const JEV_FAIL_THRESHOLD_DEFAULT: u32 = 3;
const JEV_COOLDOWN_DEFAULT: Duration = Duration::from_secs(60);

/// Kept apart from azooKey's settings.json, which the launcher rewrites
/// without the keys it does not know.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub enable: bool,
    /// API key encrypted with DPAPI (CurrentUser), base64. Written by scripts/set-jev-key.ps1.
    pub jev_api_key_dpapi: String,
    pub jev_model: String,
    pub jev_endpoint: String,
    pub jev_timeout_ms: u64,
    /// Consecutive API / timeout failures before Jev is paused (karukan-style degrade).
    pub jev_fail_threshold: u32,
    /// How long to stay on plain azooKey after the threshold is hit (ms).
    pub jev_cooldown_ms: u64,
    /// Append decisions to mizuyokan.log. Off by default: the log contains typed text.
    pub debug_log: bool,
    /// Remember committed English words (mizuyokan_words.txt) so they are
    /// recognised offline next time.
    pub learn_words: bool,
    /// A word is learned only when Jev puts the probability that it is a real
    /// English word / technical term at or above this (0.0 - 1.0).
    pub learn_word_threshold: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enable: true,
            jev_api_key_dpapi: String::new(),
            jev_model: "typesafe/jev-latest".to_string(),
            jev_endpoint: "https://ai-gateway.lolipop.jp/v1/systemone".to_string(),
            jev_timeout_ms: 1500,
            jev_fail_threshold: JEV_FAIL_THRESHOLD_DEFAULT,
            jev_cooldown_ms: JEV_COOLDOWN_DEFAULT.as_millis() as u64,
            debug_log: false,
            learn_words: true,
            learn_word_threshold: LEARN_THRESHOLD_DEFAULT,
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        let appdata = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(appdata).join("Azookey").join(SETTINGS_FILENAME))
    }

    fn parse(text: &str) -> Settings {
        serde_json::from_str(text).unwrap_or_default()
    }

    fn jev_config(&self) -> Option<JevConfig> {
        if !self.enable || self.jev_api_key_dpapi.is_empty() {
            return None;
        }
        let api_key = dpapi::decrypt_base64(&self.jev_api_key_dpapi)?;
        Some(JevConfig {
            api_key,
            endpoint: self.jev_endpoint.clone(),
            model: self.jev_model.clone(),
            timeout: Duration::from_millis(self.jev_timeout_ms),
        })
    }
}

#[derive(Clone)]
struct Loaded {
    config: Option<JevConfig>,
    fail_threshold: u32,
    cooldown: Duration,
    debug_log: bool,
    learn_words: bool,
    learn_word_threshold: f64,
}

impl Default for Loaded {
    fn default() -> Self {
        Self {
            config: None,
            fail_threshold: JEV_FAIL_THRESHOLD_DEFAULT,
            cooldown: JEV_COOLDOWN_DEFAULT,
            debug_log: false,
            learn_words: true,
            learn_word_threshold: LEARN_THRESHOLD_DEFAULT,
        }
    }
}

/// mizuyokan.json as seen by this process. Consulted on every keystroke, so
/// the file is only re-read (and the key only decrypted) when it changes.
/// A missing or broken file must never take down the host application.
fn loaded() -> Loaded {
    static CACHE: LazyLock<Mutex<Option<(SystemTime, Loaded)>>> =
        LazyLock::new(|| Mutex::new(None));

    let Some(path) = Settings::path() else {
        return Loaded::default();
    };
    let Ok(modified) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
        return Loaded::default();
    };
    let Ok(mut cache) = CACHE.lock() else {
        return Loaded::default();
    };
    if let Some((cached_at, loaded)) = cache.as_ref() {
        if *cached_at == modified {
            return loaded.clone();
        }
    }
    let settings = std::fs::read_to_string(&path)
        .map(|text| Settings::parse(&text))
        .unwrap_or_default();
    let loaded = Loaded {
        config: settings.jev_config(),
        fail_threshold: settings.jev_fail_threshold.max(1),
        cooldown: Duration::from_millis(settings.jev_cooldown_ms.max(1)),
        debug_log: settings.debug_log,
        learn_words: settings.learn_words,
        learn_word_threshold: settings.learn_word_threshold,
    };
    *cache = Some((modified, loaded.clone()));
    drop(cache);
    if settings.debug_log {
        log(&format!(
            "settings reloaded: enable={} key_stored={} key_decrypted={} endpoint={} model={}",
            settings.enable,
            !settings.jev_api_key_dpapi.is_empty(),
            loaded.config.is_some(),
            settings.jev_endpoint,
            settings.jev_model,
        ));
    }
    loaded
}

/// Jev configuration, or `None` for plain azooKey.
fn jev_config() -> Option<JevConfig> {
    loaded().config
}

fn log(message: &str) {
    use std::io::Write as _;
    let Some(path) = Settings::path().map(|p| p.with_file_name("mizuyokan.log")) else {
        return;
    };
    let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let _ = writeln!(
        file,
        "{} [{} {}] {message}",
        chrono::Local::now().format("%H:%M:%S%.3f"),
        exe,
        std::process::id()
    );
}

macro_rules! debug_log {
    ($($arg:tt)*) => {
        if loaded().debug_log {
            log(&format!($($arg)*));
        }
    };
}

struct Circuit {
    failures: u32,
    open_until: Option<Instant>,
}

impl Circuit {
    fn usable(&mut self) -> bool {
        if let Some(until) = self.open_until {
            if Instant::now() < until {
                return false;
            }
            // Cooldown over: allow a probe call.
            self.open_until = None;
            self.failures = 0;
        }
        true
    }

    fn success(&mut self) {
        self.failures = 0;
        self.open_until = None;
    }

    fn failure(&mut self, threshold: u32, cooldown: Duration) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= threshold {
            self.open_until = Some(Instant::now() + cooldown);
            self.failures = 0;
        }
    }
}

fn circuit() -> std::sync::MutexGuard<'static, Circuit> {
    static CIRCUIT: LazyLock<Mutex<Circuit>> = LazyLock::new(|| {
        Mutex::new(Circuit {
            failures: 0,
            open_until: None,
        })
    });
    CIRCUIT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Active Jev config when the circuit is closed. `None` means plain azooKey.
fn jev_ready() -> Option<JevConfig> {
    let config = jev_config()?;
    if !circuit().usable() {
        debug_log!("jev circuit open: plain azooKey");
        return None;
    }
    Some(config)
}

fn note_jev_success() {
    circuit().success();
}

fn note_jev_failure(reason: &str) {
    let loaded = loaded();
    circuit().failure(loaded.fail_threshold, loaded.cooldown);
    debug_log!("jev failure ({reason}): falling back to plain azooKey");
}

/// Jev judge that trips the circuit on API errors so background prefetch
/// failures also switch the IME to plain azooKey after a few tries.
fn monitored_judge(config: JevConfig) -> std::sync::Arc<mizuyokan_engine::Judge> {
    let inner = jev_judge(config);
    std::sync::Arc::new(move |raw: &str, options: &[String]| match inner(raw, options) {
        Some(i) => {
            note_jev_success();
            Some(i)
        }
        None => {
            note_jev_failure("api");
            None
        }
    })
}

/// Jev "is this a real word?" check for learning; API errors count towards
/// the circuit breaker like any other Jev call.
fn monitored_word_check(config: JevConfig) -> std::sync::Arc<mizuyokan_engine::WordCheck> {
    let inner = jev_word_check(config);
    std::sync::Arc::new(move |word: &str| match inner(word) {
        Some(p) => {
            note_jev_success();
            Some(p)
        }
        None => {
            note_jev_failure("word check");
            None
        }
    })
}

mod dpapi {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB},
    };

    pub fn decrypt_base64(encoded: &str) -> Option<String> {
        let mut data = STANDARD.decode(encoded.trim()).ok()?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(&input, None, None, None, None, 0, &mut output).ok()?;
            let plain = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            let _ = LocalFree(HLOCAL(output.pbData as _));
            String::from_utf8(plain).ok()
        }
    }
}


fn words_path() -> Option<PathBuf> {
    Settings::path().map(|p| p.with_file_name(WORDS_FILENAME))
}

fn candidates_path() -> Option<PathBuf> {
    Settings::path().map(|p| p.with_file_name(CANDIDATES_FILENAME))
}

fn rejected_path() -> Option<PathBuf> {
    Settings::path().map(|p| p.with_file_name(REJECTED_FILENAME))
}

/// The file's text when it changed since the last call for the same `seen` (by mtime).
fn read_if_changed(path: Option<PathBuf>, seen: &Mutex<Option<SystemTime>>) -> Option<String> {
    let path = path?;
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
    let mut seen = seen.lock().ok()?;
    if *seen == Some(modified) {
        return None;
    }
    *seen = Some(modified);
    std::fs::read_to_string(&path).ok()
}

fn file_words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|w| !w.is_empty())
}

/// Pull in words (and commit counts) other processes recorded since the last look.
/// Learned words are taken as they are, without today's `words_to_learn` rules.
fn load_learned() {
    static WORDS_SEEN: LazyLock<Mutex<Option<SystemTime>>> = LazyLock::new(|| Mutex::new(None));
    static CANDIDATES_SEEN: LazyLock<Mutex<Option<SystemTime>>> =
        LazyLock::new(|| Mutex::new(None));
    static REJECTED_SEEN: LazyLock<Mutex<Option<SystemTime>>> = LazyLock::new(|| Mutex::new(None));
    if let Some(text) = read_if_changed(words_path(), &WORDS_SEEN) {
        Prefetcher::global().remember(file_words(&text));
    }
    if let Some(text) = read_if_changed(rejected_path(), &REJECTED_SEEN) {
        Prefetcher::global().note_rejected(file_words(&text));
    }
    if let Some(text) = read_if_changed(candidates_path(), &CANDIDATES_SEEN) {
        let mut counts = std::collections::HashMap::<String, usize>::new();
        for word in file_words(&text) {
            *counts.entry(word).or_default() += 1;
        }
        Prefetcher::global().note_sightings(counts);
    }
}

fn append_lines(path: Option<PathBuf>, words: &[String]) {
    use std::io::Write as _;
    if let Some(mut file) =
        path.and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok())
    {
        let _ = file.write_all(format!("{}
", words.join("
")).as_bytes());
    }
}

/// Remember the English words of a commit. `segments` is the segmentation
/// azooKey was fed (`None` = plain azooKey: nothing to learn).
pub fn learn(committed: &str, segments: Option<&[Segment]>) {
    let Some(segments) = segments else {
        return;
    };
    if committed.is_empty() || !loaded().learn_words {
        return;
    }
    load_learned();
    let prefetcher = Prefetcher::global();
    // Learned, rejected by Jev, or being checked right now: nothing to count.
    let candidates = prefetcher.undecided(words_to_learn(committed, segments));
    if candidates.is_empty() {
        return;
    }
    let due = prefetcher.sight(candidates.clone());
    let pending: Vec<String> = candidates.into_iter().filter(|w| !due.contains(w)).collect();
    if !pending.is_empty() {
        debug_log!("seen once {pending:?}");
        let path = candidates_path();
        let too_long = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .is_some_and(|t| t.lines().count() >= MAX_CANDIDATE_LINES);
        if too_long {
            if let Some(p) = &path {
                let _ = std::fs::write(p, "");
            }
        }
        append_lines(path, &pending);
    }
    if due.is_empty() {
        return;
    }
    // Only words Jev calls real are learned. The check runs on a background
    // thread so a commit never waits for it; without a usable Jev nothing is
    // learned and the words are checked again on their next commit.
    match jev_ready() {
        Some(config) => {
            let threshold = loaded().learn_word_threshold;
            prefetcher.vet_in_background(due, monitored_word_check(config), threshold, record_vetted);
        }
        None => {
            debug_log!("word check skipped (no usable Jev): not learning {due:?} yet");
            prefetcher.postpone(due);
        }
    }
}

/// Store the outcome of a word check (runs on the check's thread).
fn record_vetted(vetted: Vetted) {
    if !vetted.learned.is_empty() {
        debug_log!("learned {:?}", vetted.learned);
        append_lines(words_path(), &vetted.learned);
    }
    if !vetted.rejected.is_empty() {
        debug_log!("not learned (Jev: not a real word) {:?}", vetted.rejected);
        append_lines(rejected_path(), &vetted.rejected);
    }
    if !vetted.retry.is_empty() {
        debug_log!("word check failed: not learning {:?} yet", vetted.retry);
    }
}

/// Jev is only worth waiting for when the buffer may contain English.
pub fn looks_mixed(raw: &str) -> bool {
    raw.chars().any(|c| c.is_ascii_uppercase())
        || Prefetcher::global().segment(raw).iter().any(|s| s.kind == SegmentKind::En)
}

/// Called after every keystroke in Kana mode: start judging in the background.
/// Any buffer with a couple of letters is sent, so English words missing from
/// the offline lexicon can still be found; nothing ever waits for these.
pub fn prefetch(raw: &str) {
    if raw.chars().filter(|c| c.is_ascii_alphabetic()).count() < 2 {
        return;
    }
    match jev_ready() {
        Some(config) => {
            load_learned();
            debug_log!("prefetch {raw:?}");
            Prefetcher::global().request(raw, monitored_judge(config));
        }
        None => debug_log!("prefetch {raw:?} skipped: no usable Jev"),
    }
}

/// For call sites outside this module that want to explain a decision.
pub fn trace(message: &str) {
    debug_log!("{message}");
}

fn fullwidth_ascii(c: char) -> char {
    if c.is_ascii_graphic() {
        char::from_u32(c as u32 + 0xFEE0).unwrap_or(c)
    } else {
        c
    }
}

fn halfwidth_letters(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'Ａ'..='Ｚ' | 'ａ'..='ｚ' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect()
}

/// What azooKey should see. azooKey leaves full-width letters unconverted, so
/// English spans go in full-width and come back untouched; the input length
/// stays one element per keystroke, which keeps Backspace and partial commits
/// consistent with the raw buffer.
pub fn feed_for(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|s| match s.kind {
            SegmentKind::En => s.raw.chars().map(fullwidth_ascii).collect(),
            _ => to_fullwidth(&s.raw, false),
        })
        .collect()
}

/// Show English spans as typed (half-width, with word spaces) in azooKey's output.
/// Only letters are narrowed: full-width Japanese punctuation such as "！" stays.
pub fn display(mut candidates: Candidates, segments: &[Segment]) -> Candidates {
    let mut spans: Vec<(String, &str)> = segments
        .iter()
        .filter(|s| s.kind == SegmentKind::En)
        .map(|s| (s.raw.chars().map(fullwidth_ascii).collect(), s.surface.as_str()))
        .collect();
    spans.sort_by_key(|(fw, _)| std::cmp::Reverse(fw.chars().count()));
    for text in candidates.texts.iter_mut().chain(candidates.sub_texts.iter_mut()) {
        for (fw, surface) in &spans {
            *text = text.replace(fw.as_str(), surface);
        }
        *text = halfwidth_letters(text);
    }
    candidates
}

/// Segmentation to feed azooKey, or `None` for a plain azooKey feed.
/// While typing (`settle == false`) finished judgements are used, with the
/// offline guess filling in until Jev answers (see `Prefetcher::live`); on Space /
/// Enter the judgement of the whole buffer is awaited when it may hold English.
/// Any Jev API / timeout failure returns `None` so the IME stays on plain azooKey.
fn target_segments(raw: &str, settle: bool) -> Option<Vec<Segment>> {
    let config = jev_ready()?;
    let prefetcher = Prefetcher::global();
    if settle && looks_mixed(raw) {
        let started = std::time::Instant::now();
        let judgement = match prefetcher.wait(raw, config.timeout) {
            Some(Judgement::Chosen(segments)) => Judgement::Chosen(segments),
            Some(Judgement::Failed) => {
                // Prefetch still running past the deadline: do not block typing
                // on a second call; plain azooKey until the circuit cools down.
                note_jev_failure("timeout waiting for prefetch");
                return None;
            }
            None => prefetcher.judge_now(raw, monitored_judge(config).as_ref()),
        };
        debug_log!("settle {raw:?}: {judgement:?} after {:?}", started.elapsed());
        return match judgement {
            Judgement::Chosen(segments) if segments.iter().any(|s| s.kind == SegmentKind::En) => {
                Some(segments)
            }
            // Japanese-only, or API failed (already counted by monitored_judge).
            _ => None,
        };
    }
    prefetcher.live(raw)
}

fn feed_azookey(
    ipc: &mut IPCService,
    raw: &str,
    segments: Option<&[Segment]>,
) -> Result<Candidates> {
    ipc.clear_text()?;
    let feed = match segments {
        Some(segments) => feed_for(segments),
        None => to_fullwidth(raw, false),
    };
    let candidates = ipc.append_text(feed)?;
    Ok(match segments {
        Some(segments) => display(candidates, segments),
        None => candidates,
    })
}

/// Bring azooKey's composing text in line with the current judgement of `raw`.
/// Returns the new candidates and segmentation (`None` = plain feed), or `None`
/// when azooKey already holds a plain feed that needs no change.
///
/// Never returns `Err` for Jev problems: API errors fall back to plain azooKey
/// so a dead gateway cannot break typing.
pub fn sync(
    raw: &str,
    was_mixed: bool,
    ipc: &mut IPCService,
    settle: bool,
) -> Result<Option<(Candidates, Option<Vec<Segment>>)>> {
    match target_segments(raw, settle) {
        Some(segments) => match feed_azookey(ipc, raw, Some(&segments)) {
            Ok(candidates) => {
                debug_log!(
                    "sync {raw:?} settle={settle}: {:?}",
                    candidates
                        .texts
                        .first()
                        .map(|t| format!("{t}{}", candidates.sub_texts.first().cloned().unwrap_or_default()))
                );
                Ok(Some((candidates, Some(segments))))
            }
            Err(e) => {
                debug_log!("sync mixed feed failed ({e:?}), plain azooKey");
                Ok(restore_plain(ipc, raw, settle))
            }
        },
        None if was_mixed => Ok(restore_plain(ipc, raw, settle)),
        None => Ok(None),
    }
}

fn restore_plain(
    ipc: &mut IPCService,
    raw: &str,
    log_settle: bool,
) -> Option<(Candidates, Option<Vec<Segment>>)> {
    match feed_azookey(ipc, raw, None) {
        Ok(candidates) => {
            debug_log!("sync {raw:?} settle={log_settle}: back to plain azooKey");
            Some((candidates, None))
        }
        Err(e) => {
            debug_log!("sync plain restore failed ({e:?}): leave azooKey as-is");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mizuyokan_engine::{render_offline, segment};

    fn candidates(texts: &[&str], subs: &[&str]) -> Candidates {
        Candidates {
            texts: texts.iter().map(|s| s.to_string()).collect(),
            sub_texts: subs.iter().map(|s| s.to_string()).collect(),
            hiragana: String::new(),
            corresponding_count: vec![0; texts.len()],
        }
    }

    #[test]
    fn english_spans_are_fed_full_width() {
        assert_eq!(feed_for(&segment("gitpullshitara")), "ｇｉｔｐｕｌｌshitara");
        assert_eq!(feed_for(&segment("PRno")), "ＰＲno");
        assert_eq!(feed_for(&segment("ra-men")), "raーmen");
        for raw in ["gitpullshitara", "Google Meetno", "PRnoreview.", "ra-men"] {
            assert_eq!(feed_for(&segment(raw)).chars().count(), raw.chars().count(), "{raw}");
        }
    }

    #[test]
    fn display_restores_english_as_typed() {
        let segments = segment("gitpullshitara");
        let shown = display(
            candidates(&["ｇｉｔｐｕｌｌしたら", "ｇｉｔｐｕｌｌ"], &["", "したら"]),
            &segments,
        );
        assert_eq!(shown.texts, vec!["git pullしたら", "git pull"]);
        assert_eq!(shown.sub_texts[1], "したら");
    }

    #[test]
    fn display_keeps_japanese_punctuation() {
        let segments = segment("PRno!");
        let shown = display(candidates(&["ＰＲの！"], &[""]), &segments);
        assert_eq!(shown.texts[0], "PRの！");
    }

    #[test]
    fn display_narrows_split_spans() {
        let segments = segment("gitpullshitara");
        let shown = display(candidates(&["ｇｉｔ"], &["ｐｕｌｌしたら"]), &segments);
        assert_eq!(shown.texts[0], "git");
        assert_eq!(shown.sub_texts[0], "pullしたら");
    }

    #[test]
    fn pure_japanese_is_not_awaited() {
        assert!(!looks_mixed("sukoshimattekudasai"));
        assert!(looks_mixed("Google Meetno"));
        assert!(looks_mixed("henshiwaThank"));
        assert!(looks_mixed("gitpullshitara"));
    }

    /// Talks to the running azooKey server: does it pass full-width letters through?
    /// cargo test -p azookey-windows live_azookey -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_azookey() {
        let mut ipc = IPCService::new().expect("azooKey server not running");
        for raw in ["gitpullshitara", "Google Meetno", "PRnoreviewwoonegaishimasu"] {
            let segments = segment(raw);
            ipc.clear_text().unwrap();
            let shown = display(ipc.append_text(feed_for(&segments)).unwrap(), &segments);
            let top: Vec<String> = shown
                .texts
                .iter()
                .zip(&shown.sub_texts)
                .zip(&shown.corresponding_count)
                .take(3)
                .map(|((t, s), n)| format!("{t}|{s}|{n}"))
                .collect();
            println!("{raw} -> {top:?}");
            assert!(shown.texts[0].is_ascii() == false);
            assert_eq!(shown.corresponding_count[0] as usize, raw.chars().count());
        }
        ipc.clear_text().unwrap();
    }

    /// Real key from %APPDATA%\Azookey\mizuyokan.json and a real Jev call.
    /// cargo test -p azookey-windows live_jev -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_jev() {
        let config = jev_config().expect("no key stored; run scripts/set-jev-key.ps1");
        let prefetcher = Prefetcher::global();
        for raw in ["gitpullshitara", "Google Meetno", "PRnoreview", "kubernetesnosettei"] {
            let started = std::time::Instant::now();
            prefetch(raw);
            let judgement = prefetcher.wait(raw, Duration::from_secs(5));
            let shown = match &judgement {
                Some(Judgement::Chosen(s)) => render_offline(s),
                other => format!("{other:?}"),
            };
            println!("{raw} -> {shown} ({:?})", started.elapsed());
        }
        let started = std::time::Instant::now();
        let confirmed = prefetcher.confirmed("gitpullshitaraa");
        println!("confirmed(gitpullshitaraa) -> {confirmed:?} ({:?})", started.elapsed());
        assert!(confirmed.is_some());
        drop(config);
    }

    #[test]
    fn settings_defaults_and_missing_key() {
        let parsed = Settings::parse(r#"{"jev_timeout_ms": 900}"#);
        assert!(parsed.enable);
        assert_eq!(parsed.jev_timeout_ms, 900);
        assert_eq!(parsed.jev_fail_threshold, 3);
        assert_eq!(parsed.jev_cooldown_ms, 60_000);
        assert_eq!(parsed.jev_model, "typesafe/jev-latest");
        assert!(parsed.learn_words);
        assert_eq!(parsed.learn_word_threshold, 0.8);
        assert_eq!(Settings::parse(r#"{"learn_word_threshold": 0.7}"#).learn_word_threshold, 0.7);
        assert_eq!(Settings::parse("{ not json"), Settings::default());
        assert!(Settings::default().jev_config().is_none());
        let disabled = Settings { enable: false, jev_api_key_dpapi: "x".into(), ..Default::default() };
        assert!(disabled.jev_config().is_none());
    }
}
