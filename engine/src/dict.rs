//! Compact JP readings + EN proper nouns (offline fallback only).

use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};

pub static PROPER: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("google", "Google"),
        ("meet", "Meet"),
        ("slack", "Slack"),
        ("url", "URL"),
        ("git", "git"),
        ("pull", "pull"),
        ("merge", "merge"),
        ("conflict", "conflict"),
        ("pr", "PR"),
        ("thank", "Thank"),
        ("login", "ログイン"),
        ("llm", "LLM"),
        ("api", "API"),
        ("ime", "IME"),
        ("azookey", "azooKey"),
        ("jev", "Jev"),
        ("github", "GitHub"),
    ]
    .into_iter()
    .collect()
});

pub static EN_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    let mut s: HashSet<&'static str> = [
        "your", "session", "has", "expired", "thank", "you", "for", "help", "merge", "pull",
        "conflict", "google", "meet", "slack", "url", "git", "login", "please", "hello", "world",
        "error", "request", "response", "server", "client", "window", "update", "branch", "commit",
        "review", "issue", "build", "test", "debug", "deploy", "status", "message", "file", "code",
        "and", "the", "with", "from", "this", "that", "have", "been", "will", "can", "not", "are",
        "was", "were", "but", "all", "any", "new", "old", "get", "set", "put", "run", "use", "try",
        "ok", "ng", "yes", "no",
    ]
    .into_iter()
    .collect();
    for k in PROPER.keys() {
        s.insert(k);
    }
    s
});

pub static JP: Lazy<Vec<(&'static str, &'static str, i32)>> = Lazy::new(|| {
    let mut v = vec![
        ("へんしわ", "返信は", 98),
        ("へんし", "返信", 96),
        ("へんしん", "返信", 96),
        ("おくってもらえますか", "送ってもらえますか", 98),
        ("おくってもらえま", "送ってもらえま", 95),
        ("ひょうじされて", "表示されて", 98),
        ("ひょうじ", "表示", 94),
        ("ログインしなおしても", "ログインし直しても", 98),
        ("しなおしても", "し直しても", 97),
        ("しなおして", "し直して", 96),
        ("さきにすすめません", "先に進めません", 98),
        ("すすめません", "進めません", 96),
        ("もうすこし", "もう少し", 97),
        ("ていねいに", "丁寧に", 97),
        ("ていねい", "丁寧", 94),
        ("でいいかな", "でいいかな", 94),
        ("いいかな", "いいかな", 90),
        ("したい", "したい", 88),
        ("したら", "したら", 92),
        ("でたので", "出たので", 96),
        ("でた", "出た", 90),
        ("まってください", "待ってください", 98),
        ("まって", "待って", 94),
        ("すこし", "少し", 94),
        ("この", "この", 88),
        ("その", "その", 88),
        ("から", "から", 92),
        ("まで", "まで", 92),
        ("ので", "ので", 95),
        ("ください", "ください", 94),
        ("ありがとう", "ありがとう", 90),
        ("おねがいします", "お願いします", 96),
        ("にほんご", "日本語", 96),
        ("にほん", "日本", 94),
        ("えいご", "英語", 94),
        ("かいしゃ", "会社", 92),
        ("しごと", "仕事", 92),
        ("もんだい", "問題", 92),
        ("かくにん", "確認", 94),
        ("れんらく", "連絡", 94),
        ("へんかん", "変換", 94),
        ("にゅうりょく", "入力", 94),
        ("じどう", "自動", 90),
    ];
    v.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.2.cmp(&a.2)));
    v
});

pub fn best_japanese(reading: &str) -> Option<(&'static str, i32)> {
    for (r, s, score) in JP.iter() {
        if *r == reading {
            return Some((*s, *score));
        }
    }
    None
}
