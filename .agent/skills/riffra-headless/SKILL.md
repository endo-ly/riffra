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

正準状態は操作ごとに DataRoot へ永続化されるため、編集自体はどの形態でもプロセスを跨いで引き継がれる。履歴(Undo / Redo)と `expectedSequence` のRevision tokenはプロセスまたはHostの寿命に紐づくため、Standaloneで連続利用する場合は `--interactive` を使う。Audio Runtimeを利用する場合は `serve` を使う。

- 音声を伴わない編集なら Standalone。単発はワンショット、Undo / Redo や連続操作は `--interactive`
- 再生・録音・レンダリングなど Runtime を伴う操作は `serve` + `--attach`
- `--interactive` はワンショットコマンドと併用できない。`serve` は `--attach` / `--interactive` / `--expected-sequence` と併用できない

## コマンドの 2 系統

コマンドは「正準状態の編集」と「Runtime サービス」の 2 系統しかなく、4 つの実行形態は同じコマンドへの要求経路の違いである。

- **正準状態の編集**はすべての実行形態で同じ引数が使える
- **Runtime サービス**は Live Host + `--attach` が必要で、Standalone では `runtimeUnavailable`

各系統に含まれるコマンドの一覧と引数は [references/commands.md](references/commands.md) を参照。

## Agentの制作ループ

既存Sessionを編集する場合は、次の順序で構造と音を確認する。

1. `session inspect` で現在の構造と `sequence` を取得する
2. 必要なら `--start` / `--end` または `--track-id` で対象を絞る
3. 取得した `sequence` を `--expected-sequence` としてMutationへ渡す
4. 編集後に同じ範囲をもう一度 `session inspect` し、新しい `sequence` を取得する
5. 音を確認するときは、その `sequence` を `--expected-sequence` として同じ範囲を `render start` し、`job get` で完了を確認する
6. 必要なら既存の `analysis start` で音声の観測値を取得する
7. 採用しない変更は、その変更後の `sequence` を `--expected-sequence` として `undo` し、再度Inspectする

`session.inspect`、Mutation、`render.start`、`undo`、`redo` はすべて `expectedSequence` を検証する。同じ `sequence` で確認した状態を操作する必要があるため、Inspect後に人間の編集が入った場合はConflictとして処理される。Conflictになった要求は自動再送せず、最新状態をInspectして編集内容やRender対象を決め直す。

`sequence` は同じ `AppCore` の有効期間でだけRevision tokenとして機能する。Standaloneのワンショットはプロセス終了時に状態を破棄するため、Inspectと後続操作を別プロセスで行う場合の共同編集保護には使わない。GUIと共同編集する場合はLive Hostを起動して `--attach` で接続し、Standaloneで連続操作する場合は `--interactive` を使う。InspectとRenderの音楽座標はそのまま渡し、tick、MIDI pitch番号、Harmony toneはエージェント側で計算しない。

## 楽曲制作の入力契約

通常の作曲では `music.*` コマンドを優先する。位置・音価・音高は次の表記で渡し、Coreがプロジェクトの拍子と正準TimelineTickへ変換する。

```text
位置: 5:1、5:3+1/2
音価: 1/4、1/8、3/8、1/12
音高: C4、F#4、Bb3
```

通常の楽曲制作で、次の計算や補助スクリプトは行わない。

- PPQを使った手計算
- 小節・拍からtickへの手計算
- 音名からMIDI pitch番号への手計算
- クリップ開始位置を使った相対tickの計算
- Node.js / Python / PowerShellでのMIDI note JSON生成

通常の作曲では、対応する `music.*` 操作がある場合はそれを優先する。`midi-note` は、既存NoteのIDを指定した更新・削除・量子化・変形・複製など、MIDI Noteを直接編集する必要がある操作で使う。`midi-*` はCC、Pitch Bendなど音楽上の基本操作に含まれないMIDIイベントを直接編集するときにも使う。tickやMIDI pitch番号を自分で計算して新しいNoteを組み立てる用途には `music.*` を使う。

`music.*` はStandalone、serve、Attachedで同じControl契約を使える。

### 和声・フレーズ・リズム

和声は一般的なChord Symbolをそのまま `music harmony insert` へ渡す。解釈を確認したいときは `music harmony resolve` を使う。parserで表現できない特殊な音集合は、`pitches`、任意の `root` / `bass`、`label` を持つexplicit tonesで指定する。

反復する旋律やモチーフは、半音差で表す `PhrasePattern` と複数の `placements` を `music phrase insert` へ渡す。コードヒットの反復は `music harmony realize` の `RhythmPattern` で指定する。独立した `music rhythm` 操作はない。

和声のTone、MIDI pitch番号、Phrase / Rhythmの反復、bar・beatからtickへの変換、Clip相対位置はエージェント側で計算しない。Coreが解決・展開し、HarmonyEventを正準セッションへ保存する。

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

`--attach`の接続先はDataRootではなく、稼働中のHostプロセスである。`control/host.json`（instanceId・pid・エンドポイント）を読んで接続する。ファイルの有無だけでは「稼働中か」「誰も所有していないか」を判断できない。

Host一覧は、registryに登録された各Hostへ接続して`host.status`を確認する。登録を削除するのは、そのプロセスが存在しないか、接続先が登録内容と異なるHostであると確定したときだけである。一時的に接続できないだけなら、一覧から外すのみで登録は残す。

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

- `sequence`: その結果が対応する正準シーケンス。同じAppCoreへの次の操作の `expectedSequence` に使える
- Agent向けCLIの正準Mutation成功応答は `result.type: "mutation"` となり、Canonical Session全体を含めない。`result.value` にはprojection状態、`sequence`、Track/Clip/Region/Harmony/Marker/Automation Lane/Deviceの構造Entity IDが含まれ、一部の直接Note操作では生成されたMIDI Note IDも含まれる。新しい正準状態は後続の `session inspect` で確認する
- DesktopとHost間の共有Control接続では、Desktop同期のため従来のCanonical結果とCanonical eventを維持する

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
| `conflict`           | `expectedSequence` が現在のシーケンスと不一致 | 最新状態を `session inspect` して編集内容を決め直す  |
| `hostUnavailable`    | Attached が Host へ接続できない               | `control/host.json` の有無と Host プロセスの生存確認 |
| `runtimeUnavailable` | Runtime を利用できない(Safe Mode、Standalone) | `serve` + `--attach` に切り替える                    |
