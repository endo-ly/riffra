---
name: riffra-headless
description: >-
  Use when operating Riffra without the desktop GUI: editing sessions,
  tracks, clips, and MIDI notes, or running playback, recording, rendering,
  and plugin operations through the riffra CLI. Also trigger on:
  riffra CLI, ヘッドレス操作, CLIでDAWを操作.
---

# Riffra CLI ヘッドレス操作

`riffra`(`apps/cli`)は GUI を使わずに制作状態を照会・編集・運用するヘッドレス用 CLI であり、Desktop アプリと同じ正準状態(`riffra-core`)を共有する。本書は全操作に共通する基盤を扱う。コマンド引数と運用手順は [references/commands.md](references/commands.md) を参照。

以下、実行ファイルは `riffra` と表記する(`cargo run -p riffra-cli --` またはビルド済み `target/debug/riffra` に読み替える)。

## 実行モード

| 形態                    | 起動                                            | 用途                                      |
| ----------------------- | ----------------------------------------------- | ----------------------------------------- |
| Standalone ワンショット | `riffra --data-root <path> <command> ...`       | 1 操作を実行し JSON 応答を出す            |
| Standalone 対話         | `riffra --data-root <path> --interactive`       | stdin へ JSON Lines 要求で連続操作        |
| Live Host               | `riffra --data-root <path> serve [--safe-mode]` | フォアグラウンド常駐し Audio Runtime 提供 |
| Attached                | `riffra --data-root <path> --attach <command>`  | 稼働中の Host へ接続                      |

正準状態は操作ごとに DataRoot へ永続化されるため、編集自体はどの形態でもプロセスを跨いで引き継がれる。プロセス寿命に紐づくのは履歴(Undo / Redo)と Audio Runtime のみであり、前者が `--interactive` を、後者が `serve` を存在させている。

- 音声を伴わない編集なら Standalone。単発はワンショット、Undo / Redo や連続操作は `--interactive`
- 再生・録音・レンダリングなど Runtime を伴う操作は `serve` + `--attach`
- `--interactive` はワンショットコマンドと併用できない。`serve` は `--attach` / `--interactive` / `--expected-sequence` と併用できない

## コマンドの 2 系統

コマンドは「正準状態の編集」と「Runtime サービス」の 2 系統しかなく、4 つの実行形態は同じコマンドへの要求経路の違いである。

- **正準状態の編集**はすべての実行形態で同じ引数が使える
- **Runtime サービス**は Live Host + `--attach` が必要で、Standalone では `runtimeUnavailable`

各系統に含まれるコマンドの一覧と引数は [references/commands.md](references/commands.md) を参照。

## DataRoot

CLI には既定の場所はなく `--data-root` が必須である。位置は自由(慣例は `./riffra-data`)で、作った場所は呼び出し側が引き回す。同じ DataRoot を同時に所有できるプロセスは 1 つだけである。

```text
<data_root>/
├─ scratch/current.json     # 正準セッション(世代履歴は generations/ 配下)
├─ library/riffra.db        # ライブラリ索引(SQLite)
├─ assets/                  # Audio / MIDI Asset 本体
├─ recordings/              # 録音キャプチャ
├─ exports/                 # レンダリング出力と Project package
└─ control/host.json        # 接続情報(稼働中の Host のみ出力)

<user-runtime-root>/riffra/hosts/
└─ <instance-id>.json       # 同一OSユーザーの稼働Host一覧
```

`--attach` の相手は DataRoot ではなく稼働中の Host プロセスであり、`control/host.json`(instanceId・pid・エンドポイント)を読んで接続する。エンドポイントの Named Pipe / Unix Domain Socket の差異は吸収される。ファイルの有無で「稼働中か」「誰も所有していないか」を判断できない。

Host一覧を得る場合は current-user registry のentryを読み、各entryへcommand handshakeと`host.status`を行う。接続できないentry、instanceIdまたはPIDが一致しないentryはstaleとして削除する。Hostのcommand connectionとevents connectionは別々のローカル接続を使う。

### Desktop アプリの DataRoot

Desktop アプリのみ Tauri の app data directory(`identifier` 固定)を使い、位置は常に一定である。

| OS      | DataRoot                                             |
| ------- | ---------------------------------------------------- |
| Windows | `%APPDATA%\com.riffra.workbench`                     |
| Linux   | `~/.local/share/com.riffra.workbench`                |
| macOS   | `~/Library/Application Support/com.riffra.workbench` |

```powershell
cargo run -p riffra-cli -- --data-root "$env:APPDATA\com.riffra.workbench" --attach host status
```

## 制御プロトコル(JSON Lines)

階層化された CLI 引数は内部的に次の要求へ変換される。対話モードでは標準入力へ 1 行 1 要求を書き、標準出力から 1 行 1 応答を読む。空行は無視される。

```json
{
  "requestId": "42",
  "command": "track.add",
  "expectedSequence": 18,
  "params": { "name": "Bass", "kind": "instrument" }
}
```

- `requestId`: 任意の文字列。応答へそのまま返る
- `command`: 操作名
- `expectedSequence`: 任意。指定すると正準シーケンスが一致するときだけ実行する(楽観制御)。ワンショットでは `--expected-sequence <n>` フラグ
- `params`: コマンドごとの引数。キー名は camelCase

Named Pipe / Unix Domain Socket のフレームは、最初に次のHelloを送る。

```json
{ "type": "hello", "role": "command" }
```

Event connectionでは`role`を`events`にする。応答の`instanceId`と`pid`がdescriptorと一致した後に、command requestまたはHost event frameを送る。

Event frameはRuntime型を直接持たない。

```json
{ "event": "canonical-state-changed", "payload": { "sequence": 19 } }
```

初期同期はcommand connectionで`host.bootstrap`を要求し、event connectionの確立後に取得する。bootstrap取得中に受け取ったeventは順番どおりに適用する。

成功応答:

```json
{
  "requestId": "42",
  "ok": true,
  "sequence": 19,
  "result": { "type": "session", "value": { "...": "..." } }
}
```

- `sequence`: その結果が対応する正準シーケンス。次の変換操作の `expectedSequence` に使える
- `result.type`: 正準セッションを含む変更は `session`、Routing・Instrument・Effect・Device・Undo / Redo・Missing 復旧は `arrangementMutation`(`value.canonical` に CanonicalState)、その他は固有の型(`tracks` / `audioClips` / `history` など)

失敗応答:

```json
{
  "requestId": "42",
  "ok": false,
  "error": {
    "code": "conflict",
    "message": "canonical state changed",
    "details": { "expectedSequence": 18, "currentSequence": 20 }
  }
}
```

## エラーコードと対処

分岐は必ず `error.code` で判定する。message 文字列の解析はしない。

| code                 | 意味                                          | 対処                                                 |
| -------------------- | --------------------------------------------- | ---------------------------------------------------- |
| `invalidRequest`     | 要求形式・params・未知のコマンドが不正        | params のキー名(camelCase)と型を見直す               |
| `commandFailed`      | Core / Host / 保存処理の失敗(ID 不存在など)   | message の内容に対処する                             |
| `conflict`           | `expectedSequence` が現在のシーケンスと不一致 | `details.currentSequence` を取得して再試行           |
| `hostUnavailable`    | Attached が Host へ接続できない               | `control/host.json` の有無と Host プロセスの生存確認 |
| `runtimeUnavailable` | Runtime を利用できない(Safe Mode、Standalone) | `serve` + `--attach` に切り替える                    |
