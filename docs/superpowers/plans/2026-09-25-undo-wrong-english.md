# 英語の誤判定を戻せるようにする 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 英語と誤判定された語を、変換中は Tab と Space の送り切りで全部日本語に戻せるようにする。戻して確定したら、学習語の取り消しや組み込み語のブロックとして覚える。あわせて `autoputto` → 「あうとputと」になるバグを直す。

**Architecture:** engine（OS 非依存）では3つを変える。日本語区切りの結合時にかなを作り直す。短い語の促音またぎを日本語扱いにする。`Lexicon`（学習語とブロック語）を導入し、`Prefetcher::disown` で取り消しとブロックを判断する。client（overlay）では、`Composition.plain_override` で「利用者が英語混じりを退けた」状態を持つ。Tab と Space の送り切りで plain feed に切り替え、確定時に `mizuyokan::unlearn` がファイルに記録する。

**Tech Stack:** Rust（engine: `mizuyokan-engine`、client: azooKey-Windows fork の `azookey-windows` crate）、TSF、azooKey IPC

**Spec:** `docs/superpowers/specs/2026-09-25-undo-wrong-english-design.md`

## Global Constraints

- 編集するのは `engine/` と `overlay/` だけ。fork（`azookey-windows-mizuyokan/`）は `./scripts/bootstrap-fork.ps1` で作り直す。fork を直接編集した場合は、終わる前に overlay/engine へ戻す。
- `cargo fmt` はかけない（既存コードは rustfmt 整形されていない）。周囲のスタイルに合わせる。
- overlay の upstream 由来ファイルは差分を最小に保つ（`client_action.rs` と `composition.rs` は既に overlay 済み）。
- Jev の不調は、必ず plain azooKey への退避で扱う。入力を壊す `Err` を返さない。ファイル I/O の失敗も入力を止めない（`append_lines` と同じく握りつぶす）。
- 新しいファイル: `%APPDATA%\Azookey\mizuyokan_words_blocked.txt`（ブロック語、1行1語）と `mizuyokan_words_unwanted.txt`（逃げた回数、1回1行）。
- 組み込み語は `BLOCK_AFTER_ESCAPES = 2` 回逃げたらブロックする。学習語は1回で取り消す。
- `learn_words: false` のときは、取り消しもブロックもファイルに書かない。
- eval の下限（`engine/tests/eval.rs`）: offline ≥ 59、options ≥ 88、japanese_prefixes ≥ 33、japanese_kept は取りこぼし1件まで。どれも下げない。改善した場合は下限を上げる。
- コミットメッセージは英語で、既存の書き方（完全な文、末尾ピリオド）に合わせる。末尾に `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` を付ける。

## Review Focus

- **英語混じりの候補が1つしかないときの最初の Space:** 全部日本語に飛ばず、英語混じりの先頭候補を出す。送り切りの判定は、既に Previewing のときだけにする（Task 6 で state 条件を入れる。手動確認項目にも入れる）。
- **英語が混ざっていない入力での Tab:** 今までどおり Space と同じ動き（次候補）をする（Task 6 の match guard。手動確認項目）。
- **別プロセスやテキストエディタで words / blocked ファイルから行が消えたとき:** 次のキー入力でメモリ上からも消える。`set_learned` / `set_blocked` で集合を置き換える（Task 4 のテスト `set_learned_and_set_blocked_replace`）。
- **途中確定（ShrinkText）で確定した部分:** 確定した範囲に入っている英単語だけを取り消しやブロックの対象にする。まだ確定していない後半の語は対象外（Task 4 のテスト `english_words_within_counts_only_the_committed_part`）。
- **一度ブロックした語を大文字で打ったとき（`Pullshitara`）:** Jev の選択肢に英語として残る（Task 3 のテスト `blocked_words_are_not_english`）。

---

## ファイル構成

| ファイル | 変更内容 |
|---|---|
| `engine/src/convert.rs` | `rekana` と `merge_adjacent` の修正、促音またぎの規則、`Lexicon`、`english_words_within`。`extra_en` を `&Lexicon` に置き換える |
| `engine/src/readings.rs` | `extra_en` を `&Lexicon` に置き換え、ブロック語を除外 |
| `engine/src/prefetch.rs` | `blocked` / `unwanted` を追加。`lexicon`・`set_learned`・`set_blocked`・`note_unwanted`・`disown`・`forget_judgements`、`Disowned`、`BLOCK_AFTER_ESCAPES` |
| `engine/src/lib.rs` | `Lexicon`、`as_japanese`、`english_words_within`、`Disowned`、`BLOCK_AFTER_ESCAPES` を re-export |
| `engine/tests/eval_cases.txt` | `autoputto` 系のケース |
| `overlay/crates/client/src/engine/mizuyokan.rs` | ファイルの読み直し（置き換え方式）、`unlearn`・`record_disowned`・`remove_lines`・`refeed`・`has_english`・`hiragana` |
| `overlay/crates/client/src/engine/client_action.rs` | `ClientAction::TogglePlain` |
| `overlay/crates/client/src/engine/composition.rs` | `Composition.plain_override`、Tab、送り切り、F6〜F8、学習の分岐 |
| `README.md`、`AGENTS.md` | 使い方と Learning の説明 |

---

### Task 1: 日本語区切りの結合で、かなを作り直す

**Files:**
- Modify: `engine/src/convert.rs:388-396`（`segment_chunk` の末尾）、`engine/src/convert.rs:493-508`（`merge_adjacent`）
- Test: `engine/src/convert.rs` の `mod tests`

**Interfaces:**
- Produces: `fn rekana(s: &mut Segment)`（crate 内部）。`merge_adjacent` は `Ja` 同士を結合したあと `rekana` を呼ぶ

- [ ] **Step 1: 失敗するテストを書く**（`mod tests` の `sokuon_is_rendered_as_small_tsu` の後に追加）

```rust
    #[test]
    fn merged_japanese_is_read_again_across_the_seam() {
        // "put" flipped to Japanese is "ぷt"; joined with "to" it must read "ぷっと".
        assert_eq!(render_offline(&as_japanese("autoputto")), "あうとぷっと");
        let alts: Vec<String> = alternatives("autoputto", 8).iter().map(|a| render_offline(a)).collect();
        assert!(alts.contains(&"あうとぷっと".to_string()), "{alts:?}");
    }
```

- [ ] **Step 2: 失敗を確認する**

Run: `cd engine; cargo test merged_japanese_is_read_again_across_the_seam`
Expected: FAIL（`left: "あうとぷtと"`）

- [ ] **Step 3: 実装する**

`segment_chunk` 末尾の後処理を関数にくくり出し、`merge_adjacent` でも使う。

```rust
    // Piecewise kana can leave a sokuon letter behind ("あtt", "ざsし"); the
    // options Jev sees are these surfaces, and garbled Japanese loses to English.
    for s in segments.iter_mut().filter(|s| s.kind == SegmentKind::Ja) {
        rekana(s);
    }
    segments
}

/// Read a Japanese segment's raw again as a whole when that leaves fewer
/// letters than its piecewise kana ("ぷt" + "と" → "ぷっと").
fn rekana(s: &mut Segment) {
    let latin = |t: &str| t.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let whole = to_ime_kana(&s.raw, false);
    if latin(&whole) < latin(&s.surface) {
        s.surface = whole;
    }
}
```

`merge_adjacent`:

```rust
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
                if s.kind == SegmentKind::Ja {
                    rekana(last);
                }
            }
            _ => out.push(s),
        }
    }
    out
}
```

- [ ] **Step 4: 通ることを確認する**

Run: `cd engine; cargo test`
Expected: 全件 PASS

- [ ] **Step 5: eval の数字を記録する**

Run: `cd engine; cargo test --test eval -- --nocapture`
Expected: PASS。offline・options・japanese の数字をメモしておく（基準値: offline 59/93、options 88/93、japanese kept 37/38、prefixes 33/38）。

- [ ] **Step 6: コミット**

```bash
git add engine/src/convert.rs
git commit -m "Read merged Japanese segments again so sokuon across a seam is not left as a letter."
```

---

### Task 2: 短い語の最後の子音が促音になるときは日本語にする

**Files:**
- Modify: `engine/src/convert.rs` の `segment_chunk` の英語候補ループ（`let acronym = ...` の直後、`if PARTICLE_PREFERRED.contains(...)` の前）
- Modify: `engine/tests/eval_cases.txt`
- Test: `engine/src/convert.rs` の `mod tests`

**Interfaces:**
- Consumes: 既存の `cuts_syllable(left, right, count_n)`

- [ ] **Step 1: 失敗するテストを書く**

```rust
    #[test]
    fn short_word_before_its_own_sokuon_is_japanese() {
        for raw in ["autoputto", "autoputtoshita", "puttodasu"] {
            assert!(
                segment(raw).iter().all(|s| s.kind != SegmentKind::En),
                "{raw}: {:?}",
                segment(raw)
            );
        }
        assert_eq!(live_convert("autoputto").surface, "あうとぷっと");
        // Longer words and capitals keep English.
        assert_eq!(shape("gitpullshitara"), vec![en("gitpull"), ja("shitara")]);
        assert_eq!(shape("datatte"), vec![en("data"), ja("tte")]);
        assert_eq!(shape("PRwokittekudasai")[0], en("PR"));
    }
```

- [ ] **Step 2: 失敗を確認する**

Run: `cd engine; cargo test short_word_before_its_own_sokuon_is_japanese`
Expected: FAIL（`autoputto: [.. En "put" ..]`）

- [ ] **Step 3: 実装する**（`segment_chunk` の `let acronym = typed.chars().all(|c| c.is_ascii_uppercase());` の直後に追加）

```rust
            // "auto|put|to": a short word whose closing consonant starts the next
            // kana (っと) is romaji, not English. A capital keeps it ("Putto").
            if len <= 3
                && !typed.starts_with(|c: char| c.is_ascii_uppercase())
                && cuts_syllable(&word, &chars[i + len..].iter().collect::<String>(), false)
            {
                continue;
            }
```

- [ ] **Step 4: eval ケースを足す**（`engine/tests/eval_cases.txt` の末尾に追加）

```
# a short English word whose closing consonant is a small tsu: Japanese
autoputto
autoputtoshitai
puttodasu
```

- [ ] **Step 5: 通ることを確認する**

Run: `cd engine; cargo test; cargo test --test eval -- --nocapture`
Expected: 全件 PASS。offline と options が Task 1 の数字から下がっていないこと。追加した3件は `japanese kept japanese (offline)` の miss に出ないこと。数字が上がっていれば、`eval.rs` の下限（`offline.hit >= 59` など）を新しい値に上げる。下がった場合は、miss に出たケースを見て条件を絞る（例: `EN_WORDS` の語に限る）。下限は下げない。

- [ ] **Step 6: コミット**

```bash
git add engine/src/convert.rs engine/tests/eval_cases.txt engine/tests/eval.rs
git commit -m "Read a short word as romaji when its closing consonant starts the next kana (autoputto)."
```

---

### Task 3: `Lexicon` を導入し、ブロック語を英語扱いしない

**Files:**
- Modify: `engine/src/convert.rs`（`Lexicon` の定義。`segment_chunk`・`segment_with`・`segment`・`plausible`・`alternatives_with`・`alternatives` の引数）
- Modify: `engine/src/readings.rs`（`Chunk`・`chunk_readings`・`readings`）
- Modify: `engine/src/prefetch.rs`（`judge_segments`・`judge_segments_with`、`Prefetcher::lexicon` と呼び出し元）
- Modify: `engine/src/lib.rs`
- Test: `engine/src/convert.rs` の `mod tests`、既存テストの呼び出し修正

**Interfaces:**
- Produces:
  - `pub struct Lexicon { pub learned: HashSet<String>, pub blocked: HashSet<String> }`（`Debug, Clone, Default, PartialEq`）
  - `Lexicon::is_en(&self, word: &str) -> bool`（word は小文字）
  - `Lexicon::is_learned(&self, word: &str) -> bool`
  - `pub fn segment_with(raw: &str, lexicon: &Lexicon) -> Vec<Segment>`
  - `pub fn alternatives_with(raw: &str, limit: usize, lexicon: &Lexicon) -> Vec<Vec<Segment>>`
  - `pub fn judge_segments_with(raw: &str, lexicon: &Lexicon, judge: &Judge) -> Judgement`
  - `Prefetcher::lexicon(&self) -> Lexicon`（この Task では `blocked` は空。Task 4 で埋める）
  - lib.rs から `Lexicon` と `as_japanese` を re-export

- [ ] **Step 1: 失敗するテストを書く**

```rust
    fn words_of(segments: &[Segment]) -> Vec<String> {
        segments
            .iter()
            .filter(|s| s.kind == SegmentKind::En)
            .flat_map(|s| s.surface.split(' ').map(str::to_string).collect::<Vec<_>>())
            .collect()
    }

    #[test]
    fn blocked_words_are_not_english() {
        let lexicon = Lexicon { blocked: ["pull".to_string()].into(), ..Default::default() };
        assert!(!lexicon.is_en("pull"));
        assert!(lexicon.is_en("git"));
        assert!(!words_of(&segment_with("gitpullshitara", &lexicon)).contains(&"pull".to_string()));
        for alt in alternatives_with("gitpullshitara", 8, &lexicon) {
            assert!(!words_of(&alt).contains(&"pull".to_string()), "{}", render_offline(&alt));
        }
        // Typed with a capital, it is still on offer.
        assert!(alternatives_with("Pullshitara", 8, &lexicon)
            .iter()
            .any(|a| a[0].kind == SegmentKind::En && a[0].raw == "Pull"));
        // Learned and blocked: blocked wins.
        let both = Lexicon { learned: ["rebiew".to_string()].into(), blocked: ["rebiew".to_string()].into() };
        assert!(!both.is_en("rebiew") && !both.is_learned("rebiew"));
    }
```

- [ ] **Step 2: 失敗を確認する**

Run: `cd engine; cargo test blocked_words_are_not_english`
Expected: FAIL（コンパイルエラー: `Lexicon` が未定義）

- [ ] **Step 3: `Lexicon` を定義する**（`convert.rs` の `Segment` の定義の後）

```rust
/// Words the segmenter knows beyond the built-in lexicon, and words the user
/// turned down (built in or learned).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lexicon {
    /// Learned from commits (mizuyokan_words.txt); weighted above the built-in lexicon.
    pub learned: HashSet<String>,
    /// Never English when typed in lowercase (mizuyokan_words_blocked.txt).
    pub blocked: HashSet<String>,
}

impl Lexicon {
    /// `word` (lowercase) is an English word here.
    pub fn is_en(&self, word: &str) -> bool {
        !self.blocked.contains(word)
            && (EN_WORDS.contains(word) || PROPER.contains_key(word) || self.learned.contains(word))
    }

    pub fn is_learned(&self, word: &str) -> bool {
        self.learned.contains(word) && !self.blocked.contains(word)
    }
}
```

- [ ] **Step 4: `convert.rs` の呼び出しを置き換える**
  - `segment_chunk(chunk: &str, lexicon: &Lexicon)`: `is_en` クロージャを `|word: &str| -> bool { lexicon.is_en(word) }` に置き換える。
  - 英語候補ループの `let acronym = ...` の直後（Task 2 の規則の前）に、次のコードを入れる。

    ```rust
                // Turned down by the user; a capital still makes it English ("Pull").
                if lexicon.blocked.contains(&word) && !typed.starts_with(|c: char| c.is_ascii_uppercase()) {
                    continue;
                }
    ```

  - `ambiguous_stem` の `!extra_en.contains(&word)` → `!lexicon.is_learned(&word)`
  - `if PROPER.contains_key(word.as_str()) || extra_en.contains(&word) {`（+55）→ `if lexicon.is_learned(&word) || (known && PROPER.contains_key(word.as_str())) {`
  - `en_progress` の `extra_en.iter().any(...)` → `lexicon.learned.iter().any(...)`
  - `segment_with(raw: &str, lexicon: &Lexicon)`。`segment(raw)` は `segment_with(raw, &Lexicon::default())` にする。
  - `plausible(cand, lexicon: &Lexicon)`: `let known = |w: &str| lexicon.is_en(w);`。En 分岐の `for word in seg.surface.split(' ')` ループの先頭に、次のコードを入れる。

    ```rust
                    if lexicon.blocked.contains(&w) && !word.starts_with(|c: char| c.is_ascii_uppercase()) {
                        return false;
                    }
    ```

  - `alternatives_with(raw, limit, lexicon: &Lexicon)`。内部の `segment_with` / `plausible` / `readings` へ `lexicon` を渡す。`alternatives` は `&Lexicon::default()` を渡す。
  - テスト `junk_readings_are_not_offered` の `let none = HashSet::new();` → `let none = Lexicon::default();`

- [ ] **Step 5: `readings.rs` を置き換える**
  - `use crate::convert::{..., Lexicon, ...}` を追加する。`dict::{EN_WORDS, PROPER}` の import は、使わなくなったら消す。
  - `Chunk` の `extra_en: &'a HashSet<String>` → `lexicon: &'a Lexicon`
  - `known(&self, word)` → `self.lexicon.is_en(word)`
  - `english()` の `let typed = ...` の直後に、次のコードを入れる。

    ```rust
        if self.lexicon.blocked.contains(&word) && !typed[0].is_ascii_uppercase() {
            return None;
        }
    ```

    また `if self.extra_en.contains(&word)` → `if self.lexicon.is_learned(&word)` にする。
  - `chunk_readings(chunk, lexicon: &Lexicon, k)`、`readings(raw, lexicon: &Lexicon, limit)`
  - テスト: `readings(raw, &HashSet::new(), 8)` → `readings(raw, &Lexicon::default(), 8)`。`learned_words_win` は `let learned = Lexicon { learned: ["figma".to_string()].into(), ..Default::default() };` にする。

- [ ] **Step 6: `prefetch.rs` を置き換える**
  - `use crate::convert::{..., Lexicon, ...}`
  - `judge_segments` → `judge_segments_with(raw, &Lexicon::default(), judge)`
  - `judge_segments_with(raw: &str, lexicon: &Lexicon, judge: &Judge)` → `alternatives_with(raw, MAX_ALTERNATIVES, lexicon)`
  - `Prefetcher` に次を追加する。

    ```rust
    /// Learned and blocked words as the segmenter sees them.
    pub fn lexicon(&self) -> Lexicon {
        Lexicon { learned: self.learned(), ..Default::default() }
    }
    ```

  - `segment`・`request`・`judge_now` の `&self.learned()` → `&self.lexicon()`
  - `outgrown` の `known` の計算 → `let known = self.lexicon().is_en(&lower);`。`dict::{EN_WORDS, PROPER}` の import は、使わなくなったら消す。

- [ ] **Step 7: `lib.rs` の re-export**

```rust
pub use convert::{
    alternatives, alternatives_with, as_japanese, english_mask, live_convert, render_offline, segment,
    segment_with, take_committed, words_to_learn, worth_learning, ConvertResult, Lexicon, Segment,
    SegmentKind,
};
```

- [ ] **Step 8: 通ることを確認する**

Run: `cd engine; cargo test; cargo test --test eval -- --nocapture`
Expected: 全件 PASS。eval の数字は Task 2 と同じ（ブロック語が空なので挙動は変わらない）。`Pullshitara` の assert だけが落ちる場合は、`alternatives_with` の上限 8 を cased の候補が押し出されていないか確かめる。押し出されていたら、テストの上限ではなく cased の push を readings より前に移す。

- [ ] **Step 9: コミット**

```bash
git add engine/src
git commit -m "Pass a Lexicon of learned and blocked words; blocked words are never English in lowercase."
```

---

### Task 4: `Prefetcher` にブロック語・取り消し・置き換え読み込みを入れる

**Files:**
- Modify: `engine/src/prefetch.rs`
- Modify: `engine/src/convert.rs`（`english_words_within`）
- Modify: `engine/src/lib.rs`
- Test: `engine/src/prefetch.rs` と `engine/src/convert.rs` の `mod tests`

**Interfaces:**
- Consumes: Task 3 の `Lexicon`
- Produces:
  - `pub const BLOCK_AFTER_ESCAPES: usize = 2;`
  - `pub struct Disowned { pub forgotten: Vec<String>, pub blocked: Vec<String>, pub noted: Vec<String> }`（`Debug, Default, Clone, PartialEq`）
  - `Prefetcher::blocked(&self) -> HashSet<String>`
  - `Prefetcher::set_learned(&self, words: impl IntoIterator<Item = String>)`
  - `Prefetcher::set_blocked(&self, words: impl IntoIterator<Item = String>)`
  - `Prefetcher::note_unwanted(&self, counts: impl IntoIterator<Item = (String, usize)>)`
  - `Prefetcher::disown(&self, words: impl IntoIterator<Item = String>) -> Disowned`
  - `pub fn english_words_within(segments: &[Segment], chars: usize) -> Vec<String>`（convert.rs）
  - lib.rs から `Disowned`、`BLOCK_AFTER_ESCAPES`、`english_words_within` を re-export

- [ ] **Step 1: 失敗するテストを書く**（prefetch.rs の `mod tests`）

```rust
    #[test]
    fn escaping_unlearns_learned_words_at_once() {
        let p = leak();
        p.remember(["rebiew".to_string()]);
        let disowned = p.disown(["rebiew".to_string()]);
        assert_eq!(disowned.forgotten, vec!["rebiew".to_string()]);
        assert!(!p.learned().contains("rebiew"));
        assert!(p.rejected().contains("rebiew"));
        assert!(p.undecided(["rebiew".to_string()]).is_empty());
    }

    #[test]
    fn escaping_twice_blocks_other_words() {
        let p = leak();
        assert_eq!(p.disown(["put".to_string()]).noted, vec!["put".to_string()]);
        assert!(p.lexicon().is_en("put"));
        assert_eq!(p.disown(["put".to_string()]).blocked, vec!["put".to_string()]);
        assert!(!p.lexicon().is_en("put"));
        assert!(p.undecided(["put".to_string()]).is_empty());
        // Already blocked: nothing more to record.
        assert_eq!(p.disown(["put".to_string()]), Disowned::default());
    }

    #[test]
    fn escapes_recorded_elsewhere_count() {
        let p = leak();
        p.note_unwanted([("put".to_string(), 1)]);
        assert_eq!(p.disown(["put".to_string()]).blocked, vec!["put".to_string()]);
    }

    #[test]
    fn set_learned_and_set_blocked_replace() {
        let p = leak();
        p.remember(["figma".to_string(), "docker".to_string()]);
        p.set_learned(["figma".to_string()]);
        assert!(p.learned().contains("figma") && !p.learned().contains("docker"));
        p.set_blocked(["put".to_string()]);
        p.set_blocked(Vec::<String>::new());
        assert!(p.lexicon().is_en("put"));
    }

    #[test]
    fn changing_words_drops_cached_judgements() {
        let p = leak();
        // Always answers, whatever the options: a failed judgement is not cached.
        let first: Arc<Judge> = Arc::new(|_: &str, _: &[String]| Some(0));
        p.judge_now("gitpullshi", first.as_ref());
        assert!(p.wait("gitpullshi", Duration::ZERO).is_some());
        p.set_blocked(["pull".to_string()]);
        assert_eq!(p.wait("gitpullshi", Duration::ZERO), None);
        p.judge_now("gitpullshi", first.as_ref());
        p.set_blocked(["pull".to_string()]); // unchanged: the cache stays
        assert!(p.wait("gitpullshi", Duration::ZERO).is_some());
    }
```

convert.rs の `mod tests`:

```rust
    #[test]
    fn english_words_within_counts_only_the_committed_part() {
        let segments = segment("gitpullshitara"); // [En "git pull"][Ja "shitara"]
        assert_eq!(english_words_within(&segments, usize::MAX), vec!["git", "pull"]);
        assert_eq!(english_words_within(&segments, 7), vec!["git", "pull"]);
        assert!(english_words_within(&segments, 6).is_empty());
    }
```

- [ ] **Step 2: 失敗を確認する**

Run: `cd engine; cargo test`
Expected: FAIL（コンパイルエラー: `disown` / `Disowned` / `english_words_within` が未定義）

- [ ] **Step 3: `english_words_within` を実装する**（convert.rs の `words_to_learn` の前）

```rust
/// Lowercase English words of `segments` that lie wholly within the first
/// `chars` characters of the buffer (the part being committed).
pub fn english_words_within(segments: &[Segment], chars: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut end = 0;
    for s in segments {
        end += s.raw.chars().count();
        if s.kind != SegmentKind::En || end > chars {
            continue;
        }
        for word in s.surface.split(' ') {
            let lower = word.to_ascii_lowercase();
            if !lower.is_empty() && lower.chars().all(|c| c.is_ascii_lowercase()) && !out.contains(&lower) {
                out.push(lower);
            }
        }
    }
    out
}
```

- [ ] **Step 4: `Prefetcher` を実装する**

定数と型（`LEARN_THRESHOLD_DEFAULT` の後、`Vetted` の後）:

```rust
/// Escapes to the all-Japanese reading before a word that was not learned
/// (built in, or picked by Jev) is blocked: one could be a slip.
pub const BLOCK_AFTER_ESCAPES: usize = 2;

/// Result of [`Prefetcher::disown`]: what to record in files.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Disowned {
    /// Were learned: take them out of the learned file and record them as rejected.
    pub forgotten: Vec<String>,
    /// Escaped often enough: record them as blocked.
    pub blocked: Vec<String>,
    /// Escaped, not blocked yet: record one more escape each.
    pub noted: Vec<String>,
}
```

フィールド（`vetting` の後）と `new()` の初期化:

```rust
    /// Words the user keeps turning down; never English in lowercase.
    blocked: RwLock<HashSet<String>>,
    /// Escapes so far of words not blocked yet.
    unwanted: Mutex<HashMap<String, usize>>,
```

```rust
            blocked: RwLock::new(HashSet::new()),
            unwanted: Mutex::new(HashMap::new()),
```

メソッド。`lexicon` は Task 3 の実装を置き換える。

```rust
    pub fn blocked(&self) -> HashSet<String> {
        self.blocked.read().map(|b| b.clone()).unwrap_or_default()
    }

    /// Learned and blocked words as the segmenter sees them.
    pub fn lexicon(&self) -> Lexicon {
        Lexicon { learned: self.learned(), blocked: self.blocked() }
    }

    /// Replace the learned words with a file's: words removed there (by hand
    /// or by another process) are forgotten here too.
    pub fn set_learned(&self, words: impl IntoIterator<Item = String>) {
        if replace(&self.learned, words) {
            self.forget_judgements();
        }
    }

    /// Replace the blocked words with a file's.
    pub fn set_blocked(&self, words: impl IntoIterator<Item = String>) {
        if replace(&self.blocked, words) {
            self.forget_judgements();
        }
    }

    /// Merge escape counts recorded elsewhere; keeps the larger count.
    pub fn note_unwanted(&self, counts: impl IntoIterator<Item = (String, usize)>) {
        let Ok(mut unwanted) = self.unwanted.lock() else {
            return;
        };
        for (word, count) in counts {
            if unwanted.len() >= MAX_LEARNED {
                break;
            }
            let seen = unwanted.entry(word).or_insert(0);
            *seen = (*seen).max(count);
        }
    }

    /// The user went back to the all-Japanese reading of a commit whose
    /// English `words` they turned down: a learned word is unlearned at once
    /// (and rejected, so it is not learned again); any other word is blocked
    /// after [`BLOCK_AFTER_ESCAPES`] escapes.
    pub fn disown(&self, words: impl IntoIterator<Item = String>) -> Disowned {
        let blocked = self.blocked();
        let mut out = Disowned::default();
        let mut seen = HashSet::new();
        for word in words {
            if blocked.contains(&word) || !seen.insert(word.clone()) {
                continue;
            }
            if self.learned.write().map(|mut l| l.remove(&word)).unwrap_or(false) {
                out.forgotten.push(word);
                continue;
            }
            let Ok(mut unwanted) = self.unwanted.lock() else {
                continue;
            };
            let count = unwanted.entry(word.clone()).or_insert(0);
            *count += 1;
            if *count >= BLOCK_AFTER_ESCAPES {
                unwanted.remove(&word);
                out.blocked.push(word);
            } else {
                out.noted.push(word);
            }
        }
        self.note_rejected(out.forgotten.clone());
        if let Ok(mut b) = self.blocked.write() {
            b.extend(out.blocked.iter().cloned());
        }
        if !out.forgotten.is_empty() || !out.blocked.is_empty() {
            self.forget_judgements();
        }
        out
    }

    /// Cached judgements were made with other words; judge again.
    fn forget_judgements(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.done.clear();
            state.order.clear();
        }
    }
```

モジュール末尾の関数（`impl Default for Prefetcher` の前）:

```rust
/// Set `set` to `words` (at most MAX_LEARNED); whether it changed.
fn replace(set: &RwLock<HashSet<String>>, words: impl IntoIterator<Item = String>) -> bool {
    let new: HashSet<String> = words.into_iter().take(MAX_LEARNED).collect();
    let Ok(mut set) = set.write() else {
        return false;
    };
    let changed = *set != new;
    *set = new;
    changed
}
```

`undecided` でブロック語を除く:

```rust
        let blocked = self.blocked();
        ...
            .filter(|w| !learned.contains(w) && !rejected.contains(w) && !blocked.contains(w) && !vetting.contains(w))
```

lib.rs:

```rust
pub use prefetch::{
    jev_judge, jev_word_check, judge_segments, judge_segments_with, Disowned, Judge, Judgement,
    Prefetcher, Vetted, WordCheck, BLOCK_AFTER_ESCAPES, LEARN_AFTER_COMMITS, LEARN_THRESHOLD_DEFAULT,
};
```

`convert` の re-export にも `english_words_within` を追加する。

- [ ] **Step 5: 通ることを確認する**

Run: `cd engine; cargo test; cargo test --test eval -- --nocapture`
Expected: 全件 PASS。eval の数字は Task 2 と同じ。

- [ ] **Step 6: コミット**

```bash
git add engine/src
git commit -m "Let the prefetcher unlearn or block words the user turned down, and reload word lists by replacing them."
```

---

### Task 5: client にファイルの記録と切り替え用の関数を足す

**Files:**
- Modify: `overlay/crates/client/src/engine/mizuyokan.rs`
- Test: 同ファイルの `mod tests`

**Interfaces:**
- Consumes: `Prefetcher::{set_learned, set_blocked, note_unwanted, disown}`、`Disowned`、`english_words_within`、`as_japanese`、`render_offline`
- Produces（composition.rs から使う）:
  - `pub fn unlearn(turned_down: &[Segment], committed_chars: usize)`
  - `pub fn refeed(raw: &str, segments: Option<&[Segment]>, ipc: &mut IPCService) -> Option<Candidates>`
  - `pub fn has_english(segments: Option<&[Segment]>) -> bool`
  - `pub fn hiragana(raw: &str) -> String`

- [ ] **Step 1: 失敗するテストを書く**（`mod tests`）

```rust
    #[test]
    fn disowned_words_move_between_files() {
        let dir = std::env::temp_dir().join(format!("mizuyokan-disown-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(WORDS_FILENAME), "rebiew\nluck\n").unwrap();
        std::fs::write(dir.join(UNWANTED_FILENAME), "put\n").unwrap();
        record_disowned(
            &dir,
            &Disowned {
                forgotten: vec!["rebiew".into()],
                blocked: vec!["put".into()],
                noted: vec!["get".into()],
            },
        );
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_default();
        assert_eq!(read(WORDS_FILENAME), "luck\n");
        assert_eq!(read(REJECTED_FILENAME), "rebiew\n");
        assert_eq!(read(BLOCKED_FILENAME), "put\n");
        assert_eq!(read(UNWANTED_FILENAME), "get\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hiragana_reads_the_whole_buffer_as_japanese() {
        assert_eq!(hiragana("autoputto"), "あうとぷっと");
        assert!(!has_english(None));
        assert!(has_english(Some(segment("gitpullshitara").as_slice())));
        assert!(!has_english(Some(segment("sukoshimatte").as_slice())));
    }
```

- [ ] **Step 2: 失敗を確認する**

Run（repo ルートで）: `./scripts/bootstrap-fork.ps1; cd azookey-windows-mizuyokan; cargo test -p azookey-windows disowned_words_move_between_files`
Expected: FAIL（コンパイルエラー: `record_disowned` などが未定義）

- [ ] **Step 3: 実装する**

import を更新する:

```rust
use std::{
    path::{Path, PathBuf},
    ...
};
use mizuyokan_engine::{
    as_japanese, english_words_within, jev_judge, jev_word_check, render_offline, words_to_learn,
    Disowned, JevConfig, Judgement, Prefetcher, Segment, SegmentKind, Vetted, LEARN_THRESHOLD_DEFAULT,
};
```

（`mod tests` の `use mizuyokan_engine::{render_offline, segment};` は `use mizuyokan_engine::segment;` に変える。`render_offline` は `super::*` から入ってくる。）

定数（`REJECTED_FILENAME` の後）:

```rust
/// Words turned down by going back to the all-Japanese reading often
/// enough, one per line: never English when typed in lowercase.
const BLOCKED_FILENAME: &str = "mizuyokan_words_blocked.txt";
/// Such escapes of words not blocked yet, one line per escape.
const UNWANTED_FILENAME: &str = "mizuyokan_words_unwanted.txt";
```

パス（`rejected_path` の後）:

```rust
fn blocked_path() -> Option<PathBuf> {
    Settings::path().map(|p| p.with_file_name(BLOCKED_FILENAME))
}

fn unwanted_path() -> Option<PathBuf> {
    Settings::path().map(|p| p.with_file_name(UNWANTED_FILENAME))
}
```

`load_learned` は置き換え方式にする（doc コメントも更新する）:

```rust
/// Pull in word lists (and counts) other processes, or the user, changed
/// since the last look. Learned and blocked words replace what is in memory,
/// so lines deleted from those files are forgotten. Learned words are taken
/// as they are, without today's `words_to_learn` rules.
fn load_learned() {
    static WORDS_SEEN: LazyLock<Mutex<Option<SystemTime>>> = LazyLock::new(|| Mutex::new(None));
    static CANDIDATES_SEEN: LazyLock<Mutex<Option<SystemTime>>> =
        LazyLock::new(|| Mutex::new(None));
    static REJECTED_SEEN: LazyLock<Mutex<Option<SystemTime>>> = LazyLock::new(|| Mutex::new(None));
    static BLOCKED_SEEN: LazyLock<Mutex<Option<SystemTime>>> = LazyLock::new(|| Mutex::new(None));
    static UNWANTED_SEEN: LazyLock<Mutex<Option<SystemTime>>> = LazyLock::new(|| Mutex::new(None));
    if let Some(text) = read_if_changed(words_path(), &WORDS_SEEN) {
        Prefetcher::global().set_learned(file_words(&text));
    }
    if let Some(text) = read_if_changed(blocked_path(), &BLOCKED_SEEN) {
        Prefetcher::global().set_blocked(file_words(&text));
    }
    if let Some(text) = read_if_changed(rejected_path(), &REJECTED_SEEN) {
        Prefetcher::global().note_rejected(file_words(&text));
    }
    if let Some(text) = read_if_changed(candidates_path(), &CANDIDATES_SEEN) {
        Prefetcher::global().note_sightings(line_counts(&text));
    }
    if let Some(text) = read_if_changed(unwanted_path(), &UNWANTED_SEEN) {
        Prefetcher::global().note_unwanted(line_counts(&text));
    }
}

fn line_counts(text: &str) -> std::collections::HashMap<String, usize> {
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for word in file_words(text) {
        *counts.entry(word).or_default() += 1;
    }
    counts
}
```

注意: ファイルがまだない状態では `read_if_changed` が `None` を返すので、`record_vetted` の `remember` で覚えた語はメモリに残る。words ファイルができたあとは、そのファイルが正になる。

`unlearn` 系（`record_vetted` の後）:

```rust
/// The user went back to the all-Japanese reading and committed it: unlearn
/// or block the English words of the reading they turned down, within the
/// first `committed_chars` characters of the buffer.
pub fn unlearn(turned_down: &[Segment], committed_chars: usize) {
    if !loaded().learn_words {
        return;
    }
    let words = english_words_within(turned_down, committed_chars);
    if words.is_empty() {
        return;
    }
    load_learned();
    let disowned = Prefetcher::global().disown(words);
    debug_log!("turned down {disowned:?}");
    if let Some(dir) = Settings::path().as_deref().and_then(Path::parent) {
        record_disowned(dir, &disowned);
    }
}

/// Write the outcome of `Prefetcher::disown` into the word files in `dir`.
fn record_disowned(dir: &Path, disowned: &Disowned) {
    if !disowned.forgotten.is_empty() {
        remove_lines(&dir.join(WORDS_FILENAME), &disowned.forgotten);
        append_lines(Some(dir.join(REJECTED_FILENAME)), &disowned.forgotten);
    }
    if !disowned.blocked.is_empty() {
        append_lines(Some(dir.join(BLOCKED_FILENAME)), &disowned.blocked);
        remove_lines(&dir.join(UNWANTED_FILENAME), &disowned.blocked);
    }
    if !disowned.noted.is_empty() {
        append_lines(Some(dir.join(UNWANTED_FILENAME)), &disowned.noted);
    }
}

/// Drop the lines holding any of `words` (case-insensitive) from a word file.
fn remove_lines(path: &Path, words: &[String]) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let kept: String = text
        .lines()
        .filter(|l| !words.contains(&l.trim().to_ascii_lowercase()))
        .map(|l| format!("{l}\n"))
        .collect();
    let _ = std::fs::write(path, kept);
}
```

切り替え用（`sync` の後）:

```rust
/// Feed `raw` again by hand: with `segments` (English spans) or plain. For
/// switching between the mixed and the all-Japanese reading; `None` when the
/// IPC call failed and azooKey was left as it was.
pub fn refeed(raw: &str, segments: Option<&[Segment]>, ipc: &mut IPCService) -> Option<Candidates> {
    match feed_azookey(ipc, raw, segments) {
        Ok(candidates) => Some(candidates),
        Err(e) => {
            debug_log!("refeed {raw:?} failed ({e:?})");
            None
        }
    }
}

pub fn has_english(segments: Option<&[Segment]>) -> bool {
    segments.is_some_and(|s| s.iter().any(|s| s.kind == SegmentKind::En))
}

/// The whole buffer as hiragana (F6 while English spans are fed as letters,
/// where azooKey's reading still holds them).
pub fn hiragana(raw: &str) -> String {
    render_offline(&as_japanese(raw))
}
```

- [ ] **Step 4: 通ることを確認する**

Run: `./scripts/bootstrap-fork.ps1; cd azookey-windows-mizuyokan; cargo build -p azookey-windows; cargo test -p azookey-windows`
Expected: build 成功、全件 PASS（`#[ignore]` のものを除く）。この時点では `unlearn` / `refeed` / `has_english` / `hiragana` の呼び出し元がないので、dead_code の警告が出てもかまわない。

- [ ] **Step 5: コミット**

```bash
git add overlay/crates/client/src/engine/mizuyokan.rs
git commit -m "Record turned-down words in the word files and reload learned and blocked words by replacing them."
```

---

### Task 6: Tab と Space の送り切りで全部日本語に切り替え、確定時に学ぶ

**Files:**
- Modify: `overlay/crates/client/src/engine/client_action.rs`
- Modify: `overlay/crates/client/src/engine/composition.rs`

**Interfaces:**
- Consumes: Task 5 の `mizuyokan::{unlearn, refeed, has_english, hiragana}`
- Produces: `Composition.plain_override: Option<Vec<mizuyokan_engine::Segment>>`、`ClientAction::TogglePlain`

TSF と IPC に依存するためユニットテストはない。確認は build と Step 9 の手動確認で行う。

- [ ] **Step 1: `ClientAction::TogglePlain` を追加する**（`client_action.rs` の `Settle` の後）

```rust
    /// Switch between the mixed reading (English spans) and the all-Japanese one (Tab).
    TogglePlain,
```

- [ ] **Step 2: `Composition` にフィールドを追加する**（`mixed` の後）

```rust
    /// The mixed segmentation the user switched away from (Tab, or Space past
    /// the last candidate); azooKey holds the plain reading until the input
    /// is edited. `None` when there was no such switch.
    pub plain_override: Option<Vec<mizuyokan_engine::Segment>>,
```

- [ ] **Step 3: `process_key` の Composing と Previewing の両方で、`UserAction::Space | UserAction::Tab =>` の arm の直前に次を追加する**

```rust
                UserAction::Tab
                    if composition.plain_override.is_some()
                        || super::mizuyokan::has_english(composition.mixed.as_deref()) =>
                {
                    (CompositionState::Previewing, vec![ClientAction::TogglePlain])
                }
```

- [ ] **Step 4: `handle_action` の状態変数と書き戻し**

`let mut mixed = composition.mixed.clone();` の後に `let mut plain_override = composition.plain_override.clone();` を置く。末尾の書き戻しブロックに `composition.plain_override = plain_override;` を足す。

- [ ] **Step 5: 各 action での扱い**

EndComposition:

```rust
                ClientAction::EndComposition => {
                    if !cancelled {
                        match &plain_override {
                            Some(turned_down) => super::mizuyokan::unlearn(turned_down, usize::MAX),
                            None => super::mizuyokan::learn(&format!("{preview}{suffix}"), mixed.as_deref()),
                        }
                    }
                    ...（既存の処理）
                    mixed = None;
                    plain_override = None;
```

AppendText と RemoveText は、先頭に `plain_override = None;` を入れる（入力を編集し始めたら、英語混じりの判定に戻る）。SetIMEMode の `mixed = None;` の隣にも `plain_override = None;` を入れる。

Settle の条件を `if mode == InputMode::Kana && plain_override.is_none() {` にする。

SetSelection:
- `first_conversion` に `&& plain_override.is_none()` を足す。
- `if first_conversion { ... }` ブロックの直後に、次を入れる。

```rust
                    // Space past the last mixed candidate: the all-Japanese reading.
                    // Only once converting, so the first Space never skips the mixed one.
                    let past_end = matches!(selection, SetSelectionType::Down)
                        && composition.state == CompositionState::Previewing
                        && selection_index + 1 >= candidates.texts.len() as i32
                        && super::mizuyokan::has_english(mixed.as_deref());
                    if past_end {
                        if let Some(plain) = super::mizuyokan::refeed(&raw_input, None, &mut ipc_service) {
                            candidates = plain;
                            plain_override = mixed.take();
                            ipc_service.set_candidates(candidates.texts.clone())?;
                            // Down below lands on the first plain candidate.
                            selection_index = -1;
                        }
                    }
```

ShrinkText:

```rust
                ClientAction::ShrinkText(text) => {
                    // `preview` is the part being committed.
                    match &plain_override {
                        Some(turned_down) => {
                            super::mizuyokan::unlearn(turned_down, corresponding_count as usize)
                        }
                        None => super::mizuyokan::learn(&preview, mixed.as_deref()),
                    }
                    plain_override = None;
                    ...（既存の処理）
```

SetTextWithType: F6〜F8 で使う読みを切り替える。

```rust
                ClientAction::SetTextWithType(set_type) => {
                    // azooKey's reading holds English spans as letters; read them as kana too.
                    let hiragana = if super::mizuyokan::has_english(mixed.as_deref()) {
                        super::mizuyokan::hiragana(&raw_input)
                    } else {
                        raw_hiragana.clone()
                    };
                    let text = match set_type {
                        SetTextType::Hiragana => hiragana,
                        SetTextType::Katakana => to_katakana(&hiragana),
                        SetTextType::HalfKatakana => to_half_katakana(&hiragana),
                        SetTextType::FullLatin => to_fullwidth(&raw_input, true),
                        SetTextType::HalfLatin => to_halfwidth(&raw_input),
                    };
```

TogglePlain（`Settle` の arm の後）:

```rust
                ClientAction::TogglePlain => {
                    // Back to the mixed reading turned down earlier, or away from it.
                    let (feed, next_mixed, next_override) = match &plain_override {
                        Some(turned_down) => (Some(turned_down.clone()), Some(turned_down.clone()), None),
                        None => (None, None, mixed.clone()),
                    };
                    if let Some(fed) =
                        super::mizuyokan::refeed(&raw_input, feed.as_deref(), &mut ipc_service)
                    {
                        candidates = fed;
                        mixed = next_mixed;
                        plain_override = next_override;
                        selection_index = 0;
                        let first = |list: &Vec<String>| list.first().cloned().unwrap_or_default();
                        preview = first(&candidates.texts);
                        suffix = first(&candidates.sub_texts);
                        raw_hiragana = candidates.hiragana.clone();
                        corresponding_count = candidates.corresponding_count.first().cloned().unwrap_or(0);
                        self.set_text(&preview, &suffix)?;
                        ipc_service.set_candidates(candidates.texts.clone())?;
                        ipc_service.set_selection(selection_index)?;
                    }
                }
```

- [ ] **Step 6: build とテスト**

Run: `./scripts/bootstrap-fork.ps1; cd azookey-windows-mizuyokan; cargo build -p azookey-windows; cargo test -p azookey-windows`
Expected: build 成功（Task 5 の dead_code 警告は消えている）、全件 PASS

- [ ] **Step 7: upstream との差分が最小か確認する**

Run: `git -C azookey-windows-mizuyokan diff --stat`
Expected: 変わっているのは overlay 済みのファイルだけ（`client_action.rs`、`composition.rs`、`mizuyokan.rs` など）。upstream の他のファイルに差分がないこと。

- [ ] **Step 8: コミット**

```bash
git add overlay/crates/client/src/engine/client_action.rs overlay/crates/client/src/engine/composition.rs
git commit -m "Switch to the all-Japanese reading with Tab or Space past the last candidate, and learn from it on commit."
```

- [ ] **Step 9: 手動確認**（利用者の azooKey のインストールに DLL を入れるので、実行前に利用者に確認する）

Run: `./scripts/install-dev-dll.ps1` を実行し、メモ帳などアプリを再起動する。

確認項目:
1. `autoputto` + Space → 「アウトプット」系の候補が出る
2. `gitpullshitara` + Space → 「git pullしたら」。Tab → 全部日本語の候補。もう一度 Tab → 「git pullしたら」に戻る
3. `gitpullshitara` で Space を連打する → 英語混じりの候補を送り切ったら、全部日本語の候補に切り替わる
4. 英語混じりの候補が1つしかない入力で、最初の Space を押す → 全部日本語に飛ばない
5. `sukoshimatte` で Tab → 今までどおり次候補に進む
6. 英語混じりで F6 を押す → 全部ひらがなになる（全角英字が混ざらない）
7. `rebiew` を含む入力を Tab で日本語にして確定 → `mizuyokan_words.txt` から消え、`mizuyokan_words_rejected.txt` に入る
8. `put` が英語として出る入力を、2回 Tab で日本語にして確定 → `mizuyokan_words_blocked.txt` に `put` が入る
9. 最後に `./scripts/install-dev-dll.ps1 -Restore` で戻すかどうかを利用者に確認する

---

### Task 7: README と AGENTS.md の更新

**Files:**
- Modify: `README.md`（学習の説明の段落、`README.md:127-129` 付近）
- Modify: `AGENTS.md`（Architecture の **Learning:** 段落と、Input modes の段落）

- [ ] **Step 1: README に追記する**（学習の説明の後に、日本語で追加）

```markdown
### 英語にされたくないとき

- 変換中に英語混じりの候補しか出ないときは、**Tab** で全部日本語の候補に切り替わります。もう一度 Tab を押すと英語混じりに戻ります。英語混じりの候補を Space で最後まで送った場合も、全部日本語の候補に切り替わります。
- 全部日本語に切り替えて確定すると、退けた英単語を覚えます。
  - 学習した語（`mizuyokan_words.txt` にある語）は、その場で学習を取り消し、`mizuyokan_words_rejected.txt` に移します。以後は学習しません。
  - 組み込みの英単語や Jev が選んだ語は、2回退けると `%APPDATA%\Azookey\mizuyokan_words_blocked.txt` に入り、小文字で打ったときは英語として扱わなくなります。大文字で始めて打つ（`Put`）と、今までどおり英語の候補に出ます。ブロックを解除したいときは、このファイルから行を削除してください。
- 英語混じりの候補で F6 を押すと、入力全体をひらがなにします。
```

- [ ] **Step 2: AGENTS.md の Learning 段落の末尾に追記する**

```markdown
**Turning English down:** Tab (`ClientAction::TogglePlain`) or Space past the last mixed candidate re-feeds azooKey plain and keeps the turned-down segmentation in `Composition.plain_override` until the input is edited. Committing in that state calls `mizuyokan::unlearn` instead of `learn`: `Prefetcher::disown` unlearns learned words at once (removed from `mizuyokan_words.txt`, added to `mizuyokan_words_rejected.txt`) and blocks other words after `BLOCK_AFTER_ESCAPES` (2) escapes, counted in `mizuyokan_words_unwanted.txt`, into `mizuyokan_words_blocked.txt`. Blocked words are never English in lowercase (`Lexicon::is_en`), even when built in. Learned and blocked files are reloaded by replacing the in-memory sets, so deleting a line takes effect.
```

- [ ] **Step 3: コミット**

```bash
git add README.md AGENTS.md
git commit -m "Document Tab, Space past the last candidate and blocked words."
```
