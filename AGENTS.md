# AGENTS.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

mizuyokan is a Windows Japanese IME built on top of [azooKey-Windows](https://github.com/fkunn1326/azooKey-Windows). It lets users type mixed English/Japanese while staying in hiragana mode (`gitpullshitara` → `git pullしたら`). azooKey still does all kana-kanji conversion; mizuyokan only decides which spans of the raw romaji buffer are English vs. Japanese. User-facing docs (README) are in Japanese.

## Repository layout (not a normal Cargo workspace)

This repo does **not** contain a buildable IME on its own. It holds:

- `engine/` — standalone, OS-independent crate `mizuyokan-engine` (segmentation, Jev client, prefetcher). Builds and tests on its own.
- `overlay/` — **whole-file copies** of azooKey-Windows files (same paths as upstream) with mizuyokan changes applied. `overlay/crates/client/src/engine/mizuyokan.rs` and `tsf/compartment_sink.rs` are new files; the rest are modified upstream files.
- `scripts/bootstrap-fork.ps1` — clones upstream into `./azookey-windows-mizuyokan/` (gitignored), force-checks-out the pinned commit `65835aa1afd9ae7fafd7c58a86ea017877ebc58f`, copies `engine/` to `<fork>/mizuyokan-engine/`, then copies `overlay/` over the fork.

**Always edit `engine/` and `overlay/`, never the fork directly** — re-running bootstrap discards fork edits. If you edit in the fork to iterate, copy changes back into `overlay/` (or `engine/`) before finishing. When changing an overlaid upstream file, keep the diff against the pinned upstream minimal (it is MIT-derived code; see `THIRD_PARTY_NOTICES.md`).

## Commands

Requires Rust MSVC toolchain plus `i686-pc-windows-msvc` target, VS Build Tools, and `protoc` (set `$env:PROTOC` if not on PATH).

```powershell
# Engine only
cd engine; cargo test
cargo test <test_name>                 # single test, e.g. cargo test splits_english_and_japanese
cargo test --test eval -- --nocapture  # split accuracy on tests/eval_cases.txt ([English] marked)
$env:JEV_API_KEY="..."; cargo test --test eval jev -- --ignored --nocapture   # real Jev, costs API calls

# Full client (after bootstrap)
./scripts/bootstrap-fork.ps1
cd azookey-windows-mizuyokan
cargo build -p azookey-windows
cargo test -p azookey-windows
cargo test -p azookey-windows live_ -- --ignored --nocapture   # needs running azooKey server + real Jev key

# Install into a real azooKey-Windows v0.1.0-alpha1 install (%APPDATA%\Azookey), builds x64 + x86 release
./scripts/install-dev-dll.ps1          # -Restore to revert, -NoBuild to skip building
./scripts/set-jev-key.ps1              # store API key (DPAPI); -Test pings Jev; -Remove
```

Don't run `cargo fmt` over the crate: the existing code is not rustfmt-formatted (e.g. `dict.rs` word lists) and it would reflow unrelated files.

Only the client DLL is swapped; the installed azooKey server/launcher/UI are reused. Apps must be restarted to load the new DLL.

## Architecture

Pipeline per keystroke in Kana mode (`overlay/.../engine/composition.rs` calls into `engine/mizuyokan.rs`):

1. **Offline segmentation** (`engine/src/convert.rs`, `dict.rs`, `romaji.rs`): the raw Latin buffer is re-segmented statelessly on every keystroke into `Segment { kind: En | Ja | Other, raw, surface }` using an English word list (`EN_WORDS`, `PROPER` casing) and "hard leftovers" (letters that can't be read as romaji). `alternatives()` produces up to N candidate segmentations.
   `alternatives_with()` builds the options Jev chooses from: offline best, all-Japanese, then a scored search over every English/romaji cut (`engine/src/readings.rs`). Option coverage matters more than top-1 here: Jev cannot pick a split that is not offered. Measure changes with the eval test.
2. **Prefetch** (`engine/src/prefetch.rs`): `mizuyokan::prefetch()` sends each buffer (≥2 letters) to a global `Prefetcher` that asks Jev in a background thread which alternative is intended. Options are rendered offline as kana (no kanji) because the kana-kanji converter owns the live composition and must not be touched from another thread. Results are cached as `Judgement::Chosen(segments)` or `Judgement::Failed`.
3. **Jev** (`engine/src/jev.rs`): HTTP (ureq) "choice" question to the AI gateway; returns probabilities per option.
4. **Feeding azooKey** (`mizuyokan::sync` / `feed_for` / `display`): English spans are sent to azooKey as **full-width letters** (azooKey leaves those unconverted), keeping one element per keystroke so Backspace and partial commits stay aligned with the raw buffer. Returned candidates have those full-width spans replaced back with the half-width English surface. `Composition.mixed` holds the segmentation in use (`None` = plain azooKey feed).
   While typing, `Prefetcher::live()` shows the Jev-judged prefix, or the offline guess where Jev has not answered yet (a Japanese-only judgement of a prefix wins over it).
5. **Settle** (Space/Enter): if `looks_mixed(raw)`, wait for the prefetched judgement (up to `jev_timeout_ms`) or judge synchronously; Japanese-only input never waits.

**Learning:** on commit (`EndComposition` not preceded by `RemoveText`, and `ShrinkText`), `mizuyokan::learn` takes English words from `Composition.mixed` that pass `convert::worth_learning` (not romaji, not romaji still being typed like `att` of `atta`, English-like spelling; vowel-less acronyms such as `ssh`/`pc` up to 5 letters pass). Each commit is counted in `mizuyokan_word_candidates.txt` (one line per commit); after `LEARN_AFTER_COMMITS` (2) the word is due and Jev is asked once, on a background thread (`Prefetcher::vet_in_background`, `JevClient::real_word_probability`), whether it is a real English word / established tech term. At or above `learn_word_threshold` (mizuyokan.json, default 0.8) it goes to `%APPDATA%\Azookey\mizuyokan_words.txt` and feeds `extra_en` via `Prefetcher::remember`; below it, to `mizuyokan_words_rejected.txt` (never counted or asked again). No usable Jev (no key, disabled, error, timeout, circuit open) means not learned and nothing recorded: `Prefetcher::postpone` makes the word due again on its next commit. Words already in `mizuyokan_words.txt` are loaded as-is (the rules only gate new learning).

**Fallback invariant:** any Jev problem (no key, `enable: false`, API error, timeout, IPC error on the mixed feed) must degrade to plain azooKey, never an `Err` that breaks typing. Consecutive failures trip a circuit breaker (`jev_fail_threshold` / `jev_cooldown_ms`).

Settings live in `%APPDATA%\Azookey\mizuyokan.json`, deliberately separate from azooKey's `settings.json` (the launcher rewrites that file and drops unknown keys). The API key is stored DPAPI-encrypted (`jev_api_key_dpapi`) and decrypted in `mizuyokan.rs`. `debug_log: true` writes typed text to `mizuyokan.log` — debug only.

Input modes are just azooKey's two (`A` / `あ`); toggling via 半角/全角, ``Alt+` ``, IME open/close compartment (`compartment_sink.rs`), `VK_IME_ON`/`VK_IME_OFF` keys (`key_event_sink.rs`; the compartment change does not reach us in Chromium apps), or the taskbar icon.

## Distribution constraints

Never commit or ship upstream binaries/installers (`*.dll`, `*.exe` are gitignored along with the fork directory). Users install official azooKey-Windows and swap the DLL themselves.
