# mizuyokan

言語モードを切り替えずに日本語と英語を混ぜて打てる、Windows 向け IME です。
[fkunn1326/azooKey-Windows](https://github.com/fkunn1326/azooKey-Windows) の「あ」モードに、[Jev](https://ai-gateway.lolipop.jp/docs/guides/features/probabilistic-decision)（TypeSafe AI の判定特化モデル）による英日の区切り判定を足しています。

```
gitpullshitara              →  git pullしたら
Google Meetno               →  Google Meetの
PRnoreviewwoonegaishimasu   →  PRのreviewをお願いします
```

## 使い方

- モードは azooKey と同じ `A` と `あ` の2つだけ。英語を打つために `A` に切り替える必要はありません。
- 「あ」のライブ変換のまま、英単語は英語のまま残ります。Space の候補、Tab / ↑↓ での選択、Enter での確定も azooKey と同じです。
- API キーを設定していなければ、普通の azooKey として動きます。

### モード切替

| 操作 | 動作 |
|---|---|
| 半角/全角キー | `A` ⇔ `あ` |
| `` Alt+` `` | `A` ⇔ `あ`（US 配列向け） |
| IME オン / オフ（[alt-ime-ahk](https://github.com/karakaram/alt-ime-ahk) の右 Alt / 左 Alt など） | オン = `あ`、オフ = `A` |
| タスクバーのアイコンをクリック | `A` ⇔ `あ` |

## しくみ

1. 「あ」で1文字打つたびに、打鍵バッファ（ローマ字のまま）の区切り方の候補を `engine/` がオフラインで作り、裏で Jev に「どれが意図した文か」を尋ねます（1回 0.3〜0.5 秒）。
2. Jev が英語と判定した部分は、azooKey に**全角英字**として渡します。azooKey は全角英字を変換せずに残すので、日本語部分だけが漢字になり、表示するときに元の綴りへ戻します。
3. 判定は判定済みの部分から順にライブ変換へ反映されます。Space / Enter のときは全体の判定を待ってから候補を出す / 確定します（英語らしさのない入力では待ちません）。

## 構成

| パス | 内容 |
|---|---|
| `engine/` | `mizuyokan-engine`（Rust、OS 非依存）。英日の区切り候補の生成、Jev クライアント、先読み |
| `overlay/` | azooKey-Windows に上書きするファイル群（ディレクトリ構成は upstream と同じ） |
| `scripts/bootstrap-fork.ps1` | upstream を clone してオーバーレイを当てる |
| `scripts/install-dev-dll.ps1` | インストール済み azooKey の IME DLL を開発版に差し替える |
| `scripts/set-jev-key.ps1` | Jev の API キーを DPAPI で暗号化して保存する |

Upstream pin: `65835aa1afd9ae7fafd7c58a86ea017877ebc58f`

## 開発

必要なもの: Rust (MSVC、`i686-pc-windows-msvc` ターゲットも)、Visual Studio Build Tools、[protoc](https://github.com/protocolbuffers/protobuf/releases)

```powershell
# エンジン単体
cd engine; cargo test; cd ..

# azooKey-Windows + オーバーレイ
./scripts/bootstrap-fork.ps1          # ./azookey-windows-mizuyokan に upstream + オーバーレイ
cd azookey-windows-mizuyokan
$env:PROTOC = "<protoc.exe のパス>"    # PATH に入っていれば不要
cargo test -p azookey-windows
cargo test -p azookey-windows live_ -- --ignored --nocapture   # 動いている azooKey と本物の Jev を使う確認
```

`bootstrap-fork.ps1` は再実行すると upstream を pin に戻してからオーバーレイを当て直します。フォーク内で直接編集した内容は消えるので、変更は必ず `overlay/` 側に入れてください。

### 実機での確認

フルビルド（Swift 等が必要）の代わりに、公式リリースの azooKey-Windows を入れて IME の DLL だけ差し替えます。リリース v0.1.0-alpha1 と pin の差分はクライアント DLL とインストーラだけなので、変換サーバ等はそのまま使えます。

```powershell
# 1. https://github.com/fkunn1326/azooKey-Windows/releases の azookey-setup.exe をインストール
# 2. 開発版 DLL に差し替え（再ビルド込み）。戻すときは -Restore
./scripts/install-dev-dll.ps1
# 3. Jev の API キーを保存して疎通確認
./scripts/set-jev-key.ps1          # または -OpRef "op://..." で 1Password から
./scripts/set-jev-key.ps1 -Test
```

差し替え後は、試すアプリ（メモ帳など）を開き直してください。IME の改変は Windows ごと固まることがあります。

## 設定

`%APPDATA%\Azookey\mizuyokan.json`（azooKey の `settings.json` とは別。launcher に上書きされないため）:

```json
{
  "enable": true,
  "jev_api_key_dpapi": "<set-jev-key.ps1 が書き込む>",
  "jev_endpoint": "https://ai-gateway.lolipop.jp/v1/systemone",
  "jev_model": "typesafe/jev-latest",
  "jev_timeout_ms": 1500,
  "debug_log": false
}
```

- API キーは DPAPI（現在の Windows ユーザー）で暗号化して保存します。平文では保存しません。
- `enable: false` で Jev を止め、普通の azooKey に戻します。
- `debug_log: true` で `%APPDATA%\Azookey\mizuyokan.log` に判定の経過を書きます。**打った文字列が記録される**ので、調査のときだけ使ってください。
- 入力した文字列（ローマ字）は、英日の判定のために Jev（既定ではロリポップ！AIゲートウェイ経由）へ送信されます。

## 既知の制限

- 英語への切り替えは Jev の応答を待つぶん、1〜2文字遅れて表示に反映されます。
- 区切り方の候補は小さな内蔵英単語リストを元に作るため、リストにない単語（`kubernetes` など）は正しく区切れないことがあります。

## 注意

azooKey-Windows の非公式フォーク向けパッチです。本家への upstream PR は別途相談が必要です。
