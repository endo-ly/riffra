---
name: riffra-headless
description: >-
  Use when operating Riffra without the desktop GUI: editing sessions,
  tracks, clips, and MIDI notes, selecting built-in instruments, or running
  playback, recording, rendering, and plugin operations through the riffra CLI.
  Also trigger on:
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
| Attached                | `riffra --attach <command>`                     | current-user registryから選んだHostへ接続 |

正準状態は操作ごとに DataRoot へ永続化されるため、編集自体はどの形態でもプロセスを跨いで引き継がれる。履歴(Undo / Redo)と `expectedSequence` のRevision tokenはプロセスまたはHostの寿命に紐づくため、Standaloneで連続利用する場合は `--interactive` を使う。Audio Runtimeを利用する場合は `serve` を使う。

- 音声を伴わない編集なら Standalone。単発はワンショット、Undo / Redo や連続操作は `--interactive`
- 再生・録音・レンダリングなど Runtime を伴う操作は `serve` + `--attach`
- `--interactive` はワンショットコマンドと併用できない。`serve` は `--attach` / `--interactive` / `--expected-sequence` と併用できない。`--attach` は `--data-root` と併用しない

## コマンドの 2 系統

コマンドは「正準状態の編集」と「Runtime サービス」の 2 系統しかなく、4 つの実行形態は同じコマンドへの要求経路の違いである。

- **正準状態の編集**はすべての実行形態で同じ引数が使える
- **Runtime サービス**は Live Host + `--attach` が必要で、Standalone では `runtimeUnavailable`

各系統に含まれるコマンドの一覧と引数は [references/commands.md](references/commands.md) を参照。

## 制作の基本手順

既存のSessionを編集するときは、全体を確認してから、音楽上のまとまりが大きい順に組み立てる。基本の流れは次のとおりである。

1. `session inspect` で現在の構造と `sequence` を確認する
2. トラック、音源、テンポ、リージョンを必要な範囲で設定する
3. `music harmony`、リズムパターン、`music phrase` で和声や反復パターンを配置する
4. 既存Noteの確認は `music note list` で必要なClipまたはTrackと範囲だけ取得する
5. 範囲・音高・channelに一致するNoteの一括調整は `music note transform` で行う
6. 個別の更新・削除が必要な場合だけ `--include-ids` でIDを取得し、`music note update` / `remove` を使う
7. raw tickやMIDI値が必要な場合だけ `music note list --raw` を使う
8. `session inspect` または `track list` で結果を確認する
9. `render start` で音声を書き出し、Attachedでワンショット実行する場合は `job wait`、interactiveでは `job.get` の繰り返しで完了を確認する
10. `analysis start` または `audio diagnostics` で結果を確認し、必要なら編集へ戻る

音声を扱わない編集はStandaloneで行い、再生・録音・RenderなどRuntimeを使う操作はLive HostへAttachedして行う。大きなJSONは`--*-file`で渡し、連続した操作はinteractive JSONLで送る。意味のある進捗率を取得できない間、ジョブの`progress`は`null`になる。

`session inspect`、Mutation、`render start`、`undo`、`redo` は `expectedSequence` を検証する。確認後に別の編集が入ってConflictになった場合は、最新状態を確認してから操作を組み直す。

`sequence`が競合検出用の値として機能するのは、同じ `AppCore` が動作している間だけである。GUIと共同編集する場合はLive HostへAttachedし、Standaloneで連続操作する場合は`--interactive`を使う。

## 楽曲制作の入力契約

通常の作曲では `music.*` コマンドを優先する。位置・音価・音高は次の表記で渡し、Coreがプロジェクトの拍子と正準TimelineTickへ変換する。

```text
位置: 5:1、5:3+1/2
音価: 1/4、1/8、3/8、1/12
音高: C4、F#4、Bb3
```

通常の楽曲制作では、位置・音価・音名を音楽座標のままCLIへ渡す。拍子に応じたtick、音名に対応するMIDI pitch番号、Clipを基準にした相対位置への変換はCLIとCoreが行う。

通常のNoteの参照・作成・更新・削除・配置には、音楽座標を扱う `music note` と `music midi-clip` を使う。`midi-note` は、音楽座標に相当する操作がない量子化・変形・複製など、既存Noteをraw tickやMIDI値で直接編集する操作に使う。`midi-*` はCC、Pitch Bendなど音楽上の基本操作に含まれないMIDIイベントを直接編集するときにも使う。

既存Noteの通常の調整では、`session get` で全Sessionを取得したり、Note IDを列挙して再挿入したりしない。必要範囲を `music note list` で確認し、`music note transform` で一括編集する。`midi-note transform` は、既知のNote IDをraw tick / MIDI値で直接操作するときだけ使う。

TimelineのPPQは `session inspect.project.ppq` を正とする。Instrument previewの `ticksPerBeat` はpreview固有の値であり、Timelineのtick計算には使わない。現在の正準Timeline PPQは960だが、エージェントは固定値を再定義せずinspectionの値を使う。

`music.*` はStandalone、serve、Attachedで同じControl契約を使える。

小さな構造入力はinline JSON、大きな構造入力は`--*-file`、連続操作はinteractive JSONLを使う。CLI入力とControl Protocolのparamsの詳細な対応は [references/commands.md](references/commands.md) を参照する。

`marker add`やRange操作は`bar:beat`の音楽座標を受け取る。MIDI NoteやClipの低レベル編集ではtickを使い、MarkerとRangeはプロジェクトの拍子に応じて内部でTimeline tickへ変換する。

### 和声・フレーズ・リズム

和声は一般的なChord Symbolをそのまま `music harmony insert` へ渡す。解釈を確認したいときは `music harmony resolve` を使う。parserで表現できない特殊な音集合は、`pitches`、任意の `root` / `bass`、`label` を持つexplicit tonesで指定する。

反復する旋律やモチーフは、半音差で表す `PhrasePattern` と複数の `placements` を `music phrase insert` へ渡す。コードヒットの反復は `music harmony realize` の `RhythmPattern` で指定する。同じ音高を繰り返すドラムパターンもPhraseで表現できるため、KickやSnareなどのNoteを大量に列挙する前に利用を検討する。

和声のTone、MIDI pitch番号、Phrase / Rhythmの反復、bar・beatからtickへの変換、Clip相対位置はエージェント側で計算しない。Coreが解決・展開し、HarmonyEventを正準セッションへ保存する。

## Host discoveryとDataRoot

Standaloneと`serve`では既定の場所はなく `--data-root` が必須である。AttachedはDataRootを開かず、current-user registryからHostを発見する。位置は自由(慣例は `./riffra-data`)で、作った場所は呼び出し側が引き回す。同じ DataRoot を同時に所有できるプロセスは 1 つだけである。

```text
<data_root>/
├─ workspace.json           # Active Projectの識別
├─ .riffra.lock             # DataRootの排他所有
├─ projects/<project-id>/   # canonical Project（session.json / generations/）
├─ library/riffra.db        # ライブラリ索引(SQLite)
├─ assets/imports/          # 外部Assetの取り込み先
├─ recordings/              # inbox / archive / library
├─ renders/                 # 音声Renderの出力
└─ control/host.json        # 接続情報(稼働中の Host のみ出力)

<user-runtime-root>/riffra/hosts/
└─ <instance-id>.json       # 同一OSユーザーの稼働Host一覧
```

`--attach`の接続先はDataRootではなく、稼働中のHostプロセスである。CLIはcurrent-user registryからHostを発見する。候補が1件なら自動選択し、複数件なら`--host <instance-id>`を要求する。

Host一覧は次で確認する。

```powershell
riffra host list
riffra --attach --host <instance-id> session inspect
```

`host list`はcurrent-user registryのローカル操作である。registryの各候補をhandshakeで検証し、稼働HostのDataRoot、PID、instance ID、起動時刻を表示する。登録を削除するのは、そのプロセスが存在しないか、接続先が登録内容と異なるHostであると確定したときだけである。一時的に接続できないだけなら、一覧から外すのみで登録は残す。

```powershell
cargo run -p riffra-cli -- --attach host status
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
- Agent向けCLIの正準Mutation成功応答は `result.type: "mutation"` となる軽量なreceiptである。`result.value.createdEntityIds` には、そのMutationで新しく生成されたIDだけが種類ごとに含まれ、生成IDがないMutationでは`{}`になる。現在のSession全体は後続の `session inspect` または `track list` で確認する
- DesktopとHost間の共有Control接続では、Desktop同期のため従来のCanonical結果とCanonical eventを維持する

後続の操作でIDが必要な場合は、応答の`createdEntityIds`を使う。たとえば`track add`の応答からTrack IDを取り出して`instrument apply`や`track update`へ渡す。

interactive JSONLで操作を連鎖させる場合は、1つの要求を送り、応答を受け取ってから次の要求を組み立てる。`expectedSequence`を使う場合は直前の応答の`sequence`を渡し、Conflict時は状態を確認して操作を組み直す。

`track list`のInternal Instrumentには`presetId`が含まれる。

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

| code                 | 意味                                          | 対処                                                |
| -------------------- | --------------------------------------------- | --------------------------------------------------- |
| `invalidRequest`     | 要求形式・params・未知のコマンドが不正        | params のキー名(camelCase)と型を見直す              |
| `commandFailed`      | Core / Host / 保存処理の失敗(ID 不存在など)   | message の内容に対処する                            |
| `conflict`           | `expectedSequence` が現在のシーケンスと不一致 | 最新状態を `session inspect` して編集内容を決め直す |
| `hostUnavailable`    | Attached が Host へ接続できない               | `riffra host list`でHostの登録とhandshake状態を確認 |
| `runtimeUnavailable` | Runtime を利用できない(Safe Mode、Standalone) | `serve` + `--attach` に切り替える                   |
