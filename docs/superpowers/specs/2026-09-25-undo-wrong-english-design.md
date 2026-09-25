# 英語の誤判定をその場で戻せるようにする

## 背景

英語と誤判定された語を、変換中にも後からも直す手段がない。

- 分割が英語混じりのとき、azooKey には英語混じりの feed しか渡らない。候補はすべて英語混じりで、全部日本語の候補が出ない。F6 も `raw_hiragana`（全角英字混じり）を使うので、ひらがなに戻らない。
- 学習した語（例: `rebiew`）は `mizuyokan_words.txt` を手で編集しないと消えない。学習語は分割のスコアでも Jev の選択肢でも有利になるので、同じ誤りが繰り返される。
- 組み込み辞書の語（`EN_WORDS` の `put` など）は利用者側で無効にできない。

実例: `autoputto`（アウトプット）が `auto|put|to` と分割され、「あうとputと」になる。原因は2つある。

1. `merge_adjacent`（`engine/src/convert.rs`）が日本語の区切りを結合するとき、各区切りのかなをそのまま連結している。全部日本語の選択肢が「あうとぷっと」ではなく「あうとぷtと」になり、Jev はまともな日本語の選択肢を受け取れない。
2. オフラインの分割が、区切りをまたぐ促音（`put|to` の `tt`）を見ていない。

## 目標

- 誤判定の出どころ（組み込み辞書・学習語・Jev）に関係なく、変換中にその場で全部日本語の読みへ戻せる。
- 日本語へ戻して確定したら、それを覚える。ファイルを手で編集しなくても、使っているうちに直る。
- `autoputto` は何もしなくても「アウトプット」になる。

対象外: 入力単位の好みを覚えること（「`autoputto` だけは日本語」のような学習）。

## 1. put 系のバグ修正（engine）

- `merge_adjacent`: `Ja` 同士を結合するときは、結合後の `raw` から romaji → かなを作り直して `surface` にする（`flip` と同じ変換を使う）。`En` 同士の結合は今のまま（空白で連結）。
- 区切りをまたぐ促音: 3文字以下の英単語の最後の子音（n 以外）が直後で重なり、その後に母音か y が続く場合（`put|to`、`get|ta` など。っ になる）は、`segment_chunk` の最良分割で英語として読まない。`cuts_syllable` 全般に広げると `for|your`（りょ）や `has|expired`（せ）まで壊れるので、重なった子音に限る。組み込み語であっても日本語側を優先する。ただし大文字で始まる語（`Put`）は対象外。4文字以上の語（`commit|to` など）は今までどおり英語のまま。この規則を外した分割（`git|toshita` など）は、オフラインの最良と同じく `plausible` を通さずに Jev の選択肢に必ず入れる。確定時に Jev を待つかどうかの判定（`looks_mixed` → `may_be_english`）でもこの分割を見るので、`gitとした` のような入力は Jev が選べる。
- eval（`engine/tests/eval_cases.txt`）に `autoputto`、`autoputtoshita`、`sutoppu` 系などを追加する。既存ケースの正答率が下がらないことを確認する。

## 2. 変換中の逃げ道（client）

`Composition` に `plain_override: Option<Vec<Segment>>` を追加する。全部日本語に切り替えたときの、切り替え前の英語混じり分割を入れておく。`Some` の間は plain feed を維持し、Jev の結果で英語混じりに戻さない。

- **A. Space 送りの先:** Previewing で英語混じり（`mixed` が英語の区切りを含む）のとき、候補の最後でさらに Down / Space を押すと `restore_plain` で plain feed に切り替える。`plain_override = mixed`、`mixed = None`、`selection_index = 0` にする。plain 側の最後では今までどおり止まる（循環しない）。
- **B. Tab:** 英語混じりのとき、Composing / Previewing のどちらでも Tab で plain feed に切り替え、Previewing に入る（selection 0）。`plain_override` が `Some` の状態でもう一度 Tab を押すと、`plain_override` の分割で feed し直して英語混じりに戻す（トグル）。英語が混ざっていないときの Tab は今までどおり Space と同じ動きにする。
  - `UserAction::Space | UserAction::Tab` の arm を分け、Tab 用に `ClientAction::TogglePlain` を追加する。`client_action.rs` と `composition.rs` は overlay 済みなので、upstream への差分は増えない。
- `plain_override` が `Some` の間は、Settle と first_conversion の `sync(settle=true)` を行わない。
- 解除: AppendText / RemoveText（入力を編集し始めた）、EndComposition、SetIMEMode、ShrinkText で `None` に戻す。ShrinkText では確定部分の学習（下記 3）を先に行う。
- F6〜F8: `raw_hiragana` ではなく `raw_input` から romaji → ひらがなを作る（英語混じりでも全部ひらがなになる）。plain feed のときは今までどおり `raw_hiragana` を使う。

## 3. 逃げ道で確定したときの学習

`learn` の呼び出し元（EndComposition と ShrinkText）で `plain_override` が `Some` なら、通常の学習の代わりに `mizuyokan::unlearn(overridden: &[Segment])` を呼ぶ。対象は `overridden` の `En` 区切りの語（小文字）。ShrinkText では、確定した範囲に含まれる語だけを対象にする。

- **学習語**（`mizuyokan_words.txt` にある）: すぐに学習を取り消す。
  - ファイルからその行を削除し、`mizuyokan_words_rejected.txt` に追記する（再学習させないため）。
  - `Prefetcher::forget(words)` で学習語の集合から外す。
- **それ以外**（組み込み語、Jev が選んだ未知語）: 1回目は `mizuyokan_words_unwanted.txt` に1行追記して数える（候補ファイルと同じく1行1回）。2回目で `mizuyokan_words_blocked.txt` に移し、以後は英語扱いしない。
  - ブロックした語は `EN_WORDS`、`PROPER`、学習語より優先して「英語ではない」とする。対象は `segment_chunk`、`plausible`、`readings`。
  - 大文字で始まる語は、ブロックした語でも Jev の選択肢に残す（逃げ道として残す）。大文字の区切りを英語にする既存の選択肢（`alternatives_with` の cased）がそのまま使われる。
  - 学習の対象（`words_to_learn` → `undecided`）からも外す。
- 学習語の集合かブロックした語の集合が変わったら、`Prefetcher` の判定キャッシュ（`State.done`）を消す（却下した語は分割に影響しないので対象外）。

### engine 側のインターフェース

- `extra_en: &HashSet<String>` を引数に取っている関数（`segment_with`、`alternatives_with`、`readings`、`judge_segments_with`）は、`&Lexicon` を取るように変える。
  - `Lexicon { learned: HashSet<String>, blocked: HashSet<String> }`
  - `Lexicon::is_en(word) -> bool`: `blocked` を先に見て、次に組み込みの辞書と `learned` を見る。
- `Prefetcher` にブロックした語を持たせ、次を追加する。
  - `block(words)`
  - `forget(words)`
  - `set_learned(words)` / `set_blocked(words)`: ファイルの内容で集合を置き換える。今の `remember` は追加しかしないので、削除が他のプロセスに伝わらない。
  - `lexicon() -> Lexicon`
- client の `load_learned` は、ファイルが変わったら（mtime で判定）words / blocked を読み直し、`set_*` で集合を置き換える。

## 失敗時の扱い

- Jev を使わずにできる操作（逃げ道、取り消し、ブロック）だけで完結させる。Jev が使えなくても動く。
- ファイルの読み書きに失敗しても、入力は止めない。その回の取り消しとブロックが記録されないだけにする（今の `append_lines` と同じ扱い）。
- plain への切り替えで IPC に失敗した場合は、今の候補のまま何もしない（`restore_plain` が `None` を返す場合と同じ）。

## テスト

- engine:
  - `merge_adjacent` の再かな化（`autoputto` の全部日本語の選択肢が「あうとぷっと」になる）
  - 区切りをまたぐ促音
  - `Lexicon` のブロック優先と、大文字の例外
  - `Prefetcher::forget` / `block` / `set_*` とキャッシュの消去
  - eval ケースの追加。`cargo test --test eval -- --nocapture` で既存の正答率が下がらないこと
- client（`mizuyokan.rs` の test モジュール）:
  - `unlearn` による、学習語の削除と却下ファイルへの追記、2回目でのブロック（一時ディレクトリで）
  - F6 用のひらがな化
- 手動: `install-dev-dll.ps1` で入れて、次を確認する。
  - `autoputto` → アウトプット
  - 英語混じりの候補で、Space の送り切りと Tab で日本語に戻り、Tab で英語混じりに戻る
  - `rebiew` を Tab で戻して確定すると学習ファイルから消える
- README（日本語）に Tab、Space の送り切り、ブロックファイルの説明を追加し、AGENTS.md の Learning の段落も更新する。
