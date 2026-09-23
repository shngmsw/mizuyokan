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
    time::{Duration, SystemTime},
};

use anyhow::Result;
use mizuyokan_engine::{jev_judge, segment, JevConfig, Judgement, Prefetcher, Segment, SegmentKind};
use serde::{Deserialize, Serialize};

use super::{
    full_width::to_fullwidth,
    ipc_service::{Candidates, IPCService},
};

const SETTINGS_FILENAME: &str = "mizuyokan.json";

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
    /// Append decisions to mizuyokan.log. Off by default: the log contains typed text.
    pub debug_log: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enable: true,
            jev_api_key_dpapi: String::new(),
            jev_model: "typesafe/jev-latest".to_string(),
            jev_endpoint: "https://ai-gateway.lolipop.jp/v1/systemone".to_string(),
            jev_timeout_ms: 1500,
            debug_log: false,
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

#[derive(Clone, Default)]
struct Loaded {
    config: Option<JevConfig>,
    debug_log: bool,
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
        debug_log: settings.debug_log,
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


/// Jev is only worth waiting for when the buffer may contain English.
pub fn looks_mixed(raw: &str) -> bool {
    raw.chars().any(|c| c.is_ascii_uppercase())
        || segment(raw).iter().any(|s| s.kind == SegmentKind::En)
}

/// Called after every keystroke in Kana mode: start judging in the background.
/// Any buffer with a couple of letters is sent, so English words missing from
/// the offline lexicon can still be found; nothing ever waits for these.
pub fn prefetch(raw: &str) {
    if raw.chars().filter(|c| c.is_ascii_alphabetic()).count() < 2 {
        return;
    }
    match jev_config() {
        Some(config) => {
            debug_log!("prefetch {raw:?}");
            Prefetcher::global().request(raw, jev_judge(config));
        }
        None => debug_log!("prefetch {raw:?} skipped: no usable key"),
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
/// While typing (`settle == false`) only finished judgements are used; on Space /
/// Enter the judgement of the whole buffer is awaited when it may hold English.
fn target_segments(raw: &str, settle: bool) -> Option<Vec<Segment>> {
    let config = jev_config()?;
    let prefetcher = Prefetcher::global();
    if settle && looks_mixed(raw) {
        let started = std::time::Instant::now();
        let judgement = match prefetcher.wait(raw, config.timeout) {
            Some(j) => j,
            None => prefetcher.judge_now(raw, jev_judge(config).as_ref()),
        };
        debug_log!("settle {raw:?}: {judgement:?} after {:?}", started.elapsed());
        if let Judgement::Chosen(segments) = judgement {
            return segments
                .iter()
                .any(|s| s.kind == SegmentKind::En)
                .then_some(segments);
        }
    }
    prefetcher.confirmed(raw)
}

/// Bring azooKey's composing text in line with the current judgement of `raw`.
/// Returns the new candidates and segmentation (`None` = plain feed), or `None`
/// when azooKey already holds a plain feed that needs no change.
pub fn sync(
    raw: &str,
    was_mixed: bool,
    ipc: &mut IPCService,
    settle: bool,
) -> Result<Option<(Candidates, Option<Vec<Segment>>)>> {
    match target_segments(raw, settle) {
        Some(segments) => {
            ipc.clear_text()?;
            let candidates = ipc.append_text(feed_for(&segments))?;
            let candidates = display(candidates, &segments);
            debug_log!(
                "sync {raw:?} settle={settle}: {:?}",
                candidates.texts.first().map(|t| format!("{t}{}", candidates.sub_texts[0]))
            );
            Ok(Some((candidates, Some(segments))))
        }
        None if was_mixed => {
            ipc.clear_text()?;
            let candidates = ipc.append_text(to_fullwidth(raw, false))?;
            debug_log!("sync {raw:?} settle={settle}: back to plain azooKey");
            Ok(Some((candidates, None)))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mizuyokan_engine::render_offline;

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
        assert_eq!(parsed.jev_model, "typesafe/jev-latest");
        assert_eq!(Settings::parse("{ not json"), Settings::default());
        assert!(Settings::default().jev_config().is_none());
        let disabled = Settings { enable: false, jev_api_key_dpapi: "x".into(), ..Default::default() };
        assert!(disabled.jev_config().is_none());
    }
}
