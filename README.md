# mizuyokan

Windows 向けの日本語 IME です。[azooKey-Windows](https://github.com/fkunn1326/azooKey-Windows) を土台に、**ひらがなモードのまま英語と日本語を混ぜて打てる**ようにしたものです。

```
gitpullshitara              →  git pullしたら
Google Meetno               →  Google Meetの
PRnoreviewwoonegaishimasu   →  PRのreviewをお願いします
```

かな漢字変換・ライブ変換・候補選択は azooKey のものを使います。mizuyokan が担うのは、入力のうちどこが英語でどこが日本語かの判定です。

## 何ができるか

- **英数モードに切り替えなくても英語が打てる**  
  入力モードは azooKey と同じく英数（`A`）とひらがな（`あ`）の2つです。ひらがなのまま `git pullしたら` や `PRのreview` のような文を続けて入力できます。
- **azooKey と同じ操作**  
  打っている途中のライブ変換、Space での候補、Tab や矢印での選択、Enter での確定は、ふつうの azooKey と同じです。
- **判定はオフライン＋任意のクラウド**  
  まず端末内で区切りの候補を作ります。API キーを入れると [Jev](https://ai-gateway.lolipop.jp/docs/guides/features/probabilistic-decision)（判定用のモデル）が、候補のうちどれが自然かを選びます。キーが無いときや通信に失敗したときは、通常の azooKey として動きます。
- **使うほど英単語を覚える**  
  Jev を使って確定した英単語を覚え、次からは打っている途中から英語として扱います。

かな漢字の辞書は azooKey のものを使います。mizuyokan が覚えるのは、英日の区切りに使う英単語だけです。

## 使い方

ひらがな（`あ`）で普通に打ちます。英語を混ぜたいときもモードはそのままでかまいません。

Jev を使う場合は API キーを登録します（後述）。登録しなければ、インストールした azooKey と同じ動きになります。

### モード切替

| 操作 | 動作 |
|---|---|
| 半角/全角キー | `A` ⇔ `あ` |
| `` Alt+` `` | `A` ⇔ `あ`（US 配列向け） |
| IME オン / オフ（[alt-ime-ahk](https://github.com/karakaram/alt-ime-ahk) の右 Alt / 左 Alt など） | オン = `あ`、オフ = `A` |
| タスクバーのアイコンをクリック | `A` ⇔ `あ` |

## しくみ

1. ひらがなモードで打つたび、ローマ字の入力バッファから「英語／日本語の区切り方」の候補を端末内で作ります（英単語リスト、覚えた英単語、ローマ字として読めない箇所の推定）。`branchwoきる` や `てstがとおらない` のような、人が打つつもりのない区切りは候補に入れません。
2. API キーがある場合、裏で Jev に「どの候補が意図した文か」を尋ねます（目安 0.3〜0.5 秒）。打鍵のたびに先読みするので、Space を押すころには答えが揃っていることが多いです。候補が1つに絞れたときは Jev に尋ねません。
3. API キーがある場合、Jev の答えが届くまでは端末内の区切りで英語らしい部分を仮に表示し、答えが届いたらそちらに合わせます。
4. 英語と判断した部分は、azooKey には全角の英字として渡します。azooKey は全角英字をかな漢字にしないため英語が残り、画面には元の半角綴りで見せます。日本語部分だけがかな漢字変換されます。
5. Space や Enter のときは、英語が混ざっていそうな入力に限って判定の完了を待ってから候補表示・確定します。日本語だけのときは待ちません。
6. Jev への問い合わせが何度も失敗すると、しばらく Jev を使わず azooKey だけに切り替えます。
7. Jev を使って確定した文に含まれる英単語のうち、ローマ字として読めない語（`figma`、`docker` など）を覚えます。次からは打っている途中から英語として表示し、Jev に渡す候補でも優先します。`make` のようにローマ字としても読める語は、日本語を誤って英語にしないよう覚えません。Escape や Backspace で取り消した入力からは覚えません。

## 構成

| パス | 内容 |
|---|---|
| `engine/` | 英日の区切り候補の生成、Jev クライアント、先読み、覚えた英単語の管理（Rust、OS 非依存） |
| `overlay/` | azooKey-Windows に当てる変更（配置は upstream と同じ） |
| `scripts/bootstrap-fork.ps1` | upstream を clone し、オーバーレイを当てる |
| `scripts/install-dev-dll.ps1` | インストール済み azooKey の IME DLL を開発版に差し替える |
| `scripts/set-jev-key.ps1` | Jev の API キーを DPAPI で暗号化して保存する |

Upstream pin: `65835aa1afd9ae7fafd7c58a86ea017877ebc58f`（ライセンスは下記「ライセンスと配布の形」を参照）

## 開発

必要なもの: Rust（MSVC、`i686-pc-windows-msvc` ターゲットも）、Visual Studio Build Tools、[protoc](https://github.com/protocolbuffers/protobuf/releases)

```powershell
# エンジン単体
cd engine; cargo test; cd ..

# azooKey-Windows + オーバーレイ
./scripts/bootstrap-fork.ps1          # ./azookey-windows-mizuyokan に upstream + オーバーレイ
cd azookey-windows-mizuyokan
$env:PROTOC = "<protoc.exe のパス>"    # PATH に入っていれば不要
cargo test -p azookey-windows
cargo test -p azookey-windows live_ -- --ignored --nocapture   # 起動中の azooKey と実 Jev で確認
```

区切りの精度は `engine/tests/eval_cases.txt`（英語の部分を `[ ]` で囲んだ入力例 75 件）で測ります。2026-09-23 時点で、実 Jev の正解は 68/75、端末内だけの区切りは 43/75 です。

```powershell
cd engine
cargo test --test eval -- --nocapture                               # 端末内の区切り／Jev に渡す選択肢に正解が入っている割合
$env:JEV_API_KEY = "<key>"; cargo test --test eval jev -- --ignored --nocapture   # 実 Jev の正解率と待ち時間（件数分 API を呼びます）
```

`bootstrap-fork.ps1` を再実行すると、upstream を pin に戻してからオーバーレイを当て直します。フォーク側で直接いじった変更は消えるので、編集は `overlay/` に入れてください。

### 実機での確認

公式の azooKey-Windows を入れたうえで、IME の DLL だけ差し替えます（フルビルドや Swift は不要）。リリース v0.1.0-alpha1 と pin の差分はクライアント DLL とインストーラにとどまるため、変換サーバなどはインストール済みのものをそのまま使えます。

```powershell
# 1. https://github.com/fkunn1326/azooKey-Windows/releases の azookey-setup.exe をインストール
# 2. 開発版 DLL に差し替え（再ビルド込み）。戻すときは -Restore
./scripts/install-dev-dll.ps1
# 3. Jev の API キーを保存して疎通確認
./scripts/set-jev-key.ps1          # または -OpRef "op://..." で 1Password から
./scripts/set-jev-key.ps1 -Test
```

差し替え後は、試すアプリ（メモ帳など）を開き直してください。IME の不具合で Windows が固まることがある点には注意してください。

## 設定

`%APPDATA%\Azookey\mizuyokan.json`（azooKey 本体の `settings.json` とは別ファイルです）:

```json
{
  "enable": true,
  "jev_api_key_dpapi": "<set-jev-key.ps1 が書き込む>",
  "jev_endpoint": "https://ai-gateway.lolipop.jp/v1/systemone",
  "jev_model": "typesafe/jev-latest",
  "jev_timeout_ms": 1500,
  "jev_fail_threshold": 3,
  "jev_cooldown_ms": 60000,
  "debug_log": false,
  "learn_words": true
}
```

- API キーは DPAPI（その Windows ユーザー向け）で暗号化して保存します。平文では保存しません。
- `enable: false`、キー未設定、または API エラーが続くときは、通常の azooKey だけが動きます。
- `jev_fail_threshold` / `jev_cooldown_ms` で、連続失敗後に Jev を休止する条件を変えられます（既定: 3回 / 60秒）。
- `learn_words: false` にすると英単語の学習を止めます。覚えた語は `%APPDATA%\Azookey\mizuyokan_words.txt` に1行1語で残ります。消したい語はこのファイルから行を削除してください。
- `debug_log: true` にすると `%APPDATA%\Azookey\mizuyokan.log` に判定の経過を書きます。**打った文字列が残る**ので、調査のときだけ使ってください。
- Jev 利用時は、英日判定のために入力ローマ字がゲートウェイ（既定はロリポップ！AIゲートウェイ）へ送られます。キーが無いときは送りません。

## 既知の制限

- Jev の答えが届くまでは端末内の推定で表示するため、答えが届いたときに表示が入れ替わることがあります。
- 端末内の英単語リストに無い語は、ローマ字として読めない箇所からの推定に頼ります。区切りを誤ることがあり、Jev を使うほうが安定しやすいです。
- `make`、`home`、`token` のようにローマ字としても読める英単語は、小文字で打つと英語の候補に入らず日本語になります。先頭を大文字（`Make`）にすると候補に入ります。

## ライセンスと配布の形

- このリポジトリ（`engine/` とドキュメントなど）は [MIT License](LICENSE) です。
- `overlay/` は [azooKey-Windows](https://github.com/fkunn1326/azooKey-Windows)（MIT、Copyright (c) 2026 fkunn1326）を改変して当てる差分です。元の著作権表示は [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) にあります。
- **公式のインストーラやビルド済みバイナリは同梱しません。** 利用時は azooKey-Windows を自分で入れ、必要なら DLL だけ差し替えます（`scripts/install-dev-dll.ps1`）。
- 本家への取り込みとは無関係の、非公式な改変です。
