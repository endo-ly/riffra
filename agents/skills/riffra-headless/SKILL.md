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

`riffra`(`apps/cli`)は GUI を使わずに制作状態を照会・編集・運用するヘッドレス用 CLI であり、Desktop アプリと同じ正準状態(`riffra-core`)を共有する。本書は全操作に共通する基盤(実行形態、制作フロー、入力契約、プロトコル、エラーコード)を扱う。コマンド別の引数・例文・値域は [references/commands.md](references/commands.md) を参照。

以下、実行ファイルは `riffra` と表記する(`cargo run -p riffra-cli --` またはビルド済み `target/debug/riffra` に読み替える)。

## 実行モード

| 形態                    | 起動                                            | 用途                                      |
| ----------------------- | ----------------------------------------------- | ----------------------------------------- |
| Standalone ワンショット | `riffra --data-root <path> <command> ...`       | 1 操作を実行し JSON 応答を出す            |
| Standalone 対話         | `riffra --data-root <path> --interactive`       | stdin へ JSON Lines 要求で連続操作        |
| Live Host               | `riffra --data-root <path> serve [--safe-mode]` | フォアグラウンド常駐し Audio Runtime 提供 |
| Attached                | `riffra --attach <command>`                     | current-user registryから選んだHostへ接続 |

正準状態は操作ごとに DataRoot へ永続化されるため、編集自体はプロセスを跨いで引き継がれる。履歴(Undo / Redo)と `expectedSequence` のRevision tokenはプロセスまたはHostの寿命に紐づくため、音声を伴わない編集は Standalone(単発はワンショット、連続操作や Undo / Redo は `--interactive`)、Audio Runtime を伴う操作は `serve` で起動した Host へ `--attach` する。

- `--interactive` はワンショットコマンドと併用できない
- `serve` は `--attach` / `--interactive` / `--expected-sequence` と併用できない
- `--attach` は `--data-root` と併用しない

## コマンドの 2 系統

コマンドは「正準状態の編集」と「Runtime サービス」の 2 系統しかなく、4 つの実行形態は同じコマンドへの要求経路の違いである。

- **正準状態の編集**はすべての実行形態で同じ引数が使える
- **Runtime サービス**は Live Host + `--attach` が必要で、Standalone では `runtimeUnavailable`

各系統に含まれるコマンドの一覧と引数は `commands.md` を参照。

## 制作の基本手順

既存のSessionを編集するときは、全体を確認してから、音楽上のまとまりが大きい順に組み立てる。

1. `session inspect` で現在の構造と `sequence` を確認する
2. トラック、音源、テンポ、リージョンを必要な範囲で設定する
3. `music harmony`、リズムパターン、`music phrase` で和声や反復パターンを配置する
4. Noteは `music note list` で必要な範囲だけ取得し、一括調整は `music note transform`、個別の更新・削除だけ `--include-ids` で取得したIDを使う。raw tickやMIDI値が必要なときだけ `--raw` を付ける
5. `session inspect` または `track list` で結果を確認する
6. ミックスのバランスを `track update`、`audio-clip update`、`automation set`、`session settings update --master-db` で整える
7. `render start` で音声を書き出し、完了を `job wait`(ワンショット)または `job get`(interactive)で確認する
8. `analysis start` または `audio diagnostics` で結果を確認し、必要なら編集へ戻る

### まとめて構築する場合

楽曲の初期構築や、事前に決めた複数の正準編集は `session apply` を第一候補にする。Control Commandを1行ずつJSONLへ書き、全operationが成功したときだけ1回のcommitとして適用されるため、途中で失敗してもそれまでのoperationは残らない。

途中結果を確認して次の編集を決める場合や、1操作ずつUndoしたい場合は `session apply` ではなく `--interactive` を使う。JSONLの書式、名前解決、失敗時のdetailsは `commands.md` の「複数操作の一括適用」を参照。

### 音源選択

楽曲内の役割(リード、ベースライン、コードなど)とプリセットの `category` は直接対応しない。`category` はライブラリ上の主分類であり、候補は `instrument list` が返す全プリセットから `description`、`tags`、`recommended_range` を見て選ぶ。たとえば主旋律には `Lead` だけでなく `Pluck`、`Mallet`、`Keys` も候補になる。

## 楽曲制作の入力契約

通常の作曲では `music.*` コマンドを優先する。位置・音価・音高は音楽座標のままCLIへ渡し、Coreがプロジェクトの拍子と正準TimelineTickへ変換する。

```text
位置: 5:1、5:3+1/2
音価: 1/4、1/8、3/8、1/12
音高: C4、F#4、Bb3
```

通常のNoteの参照・作成・更新・削除・配置には、音楽座標を扱う `music note` と `music midi-clip` を使う。`midi-note` は、音楽座標に相当する操作がない量子化・変形・複製など、既存Noteをraw tickやMIDI値で直接編集する操作に使う。`midi-*` はCC、Pitch Bendなど音楽上の基本操作に含まれないMIDIイベントを直接編集するときにも使う。

既存Noteの通常の調整に `session get` で全Sessionを取得したりNote IDを列挙して再挿入したりする必要はない。`midi-note transform` は、既知のNote IDをraw tick / MIDI値で直接操作するときだけ使う。

TimelineのPPQは `session inspect.project.ppq` を正とし、固定値を再定義しない。Instrument previewの `ticksPerBeat` はpreview固有の値であり、Timelineのtick計算には使わない。

`marker add` やRange操作は `bar:beat` の音楽座標を受け取る。MIDI NoteやClipの低レベル編集ではtickを使い、MarkerとRangeはプロジェクトの拍子に応じて内部でTimeline tickへ変換する。

小さな構造入力はinline JSON、大きな構造入力は `--*-file`、連続操作はinteractive JSONLを使う。CLI入力とControl Protocolのparamsの対応は `commands.md` を参照する。

### 和声・フレーズ・リズム

和声は一般的なChord Symbolをそのまま `music harmony insert` へ渡す。解釈を確認したいときは `music harmony resolve` を使う。parserで表現できない音集合の指定とupdateのpatch契約は `commands.md` の Harmony 節を参照。

反復する旋律やモチーフは、半音差で表す `PhrasePattern` と複数の `placements` を `music phrase insert` へ渡す。コードヒットの反復は `music harmony realize` の `RhythmPattern` で指定する。同じ音高を繰り返すドラムパターンもPhraseで表現できるため、KickやSnareなどのNoteを大量に列挙する前に利用を検討する。

和声の解決、Phrase / Rhythmの展開、bar・beatからtickへの変換、Clip相対位置はエージェント側で計算せず、Coreが解決・展開して正準セッションへ保存する。

Patternの時間は2種類に分けて考える。`Position` は `bar:beat` または `bar:beat+fraction-of-beat` で、たとえば `5:1+1/2` は5小節1拍目から半拍後を表す。`length`、`offset`、`duration` は全音符を1とする音価の分数で、`1/8` は8分音符、`1/4` は4分音符、`1/2` は2分音符である。

Phraseを挿入する前に展開規模とClip内への収まりを確認する場合は `music phrase preview` を使う。書式と例は `commands.md` の Phrase 節を参照。

## ミックス

ミックスの正本は既存の正準状態であり、BusやSendのようなMixer専用の正準モデルは存在しない。TrackとClipのパラメータ、Master音量、Automationの組み合わせで表現する。Gain / VolumeはdB、Panは-1.0(左)..1.0(右)。編集コマンド、値域、Solo / MuteとAutomationの挙動は `commands.md` のミックスを参照。ミックス状態は `track list` と `session inspect` で確認し、聞こえの確認はRenderで行う。

## Host discoveryとDataRoot

Standaloneと`serve`では既定の場所はなく `--data-root` が必須である(慣例は `./riffra-data`、作った場所は呼び出し側が引き回す)。同じDataRootを同時に所有できるプロセスは1つだけである。AttachedはDataRootを開かず、current-user registryから稼働中のHostプロセスを発見する。候補が1件なら自動選択し、複数件なら `--host <instance-id>` を要求し、指定がなければエラーを返す。再試行はHostの検出と初回handshakeの段階に限り有限回数で、以降は要求が自動再送されることはない。

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

Host一覧は次で確認する。

```powershell
riffra host list
riffra --attach --host <instance-id> session inspect
```

`host list` はcurrent-user registryのローカル操作である。registryの各候補をhandshakeで検証し、稼働HostのDataRoot、PID、instance ID、起動時刻を表示する。登録を削除するのは、そのプロセスが存在しないか、接続先が登録内容と異なるHostであると確定したときだけである。一時的に接続できないだけなら、一覧から外すのみで登録は残す。

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
- `expectedSequence`: 任意。指定すると正準シーケンスが一致するときだけ実行する。ワンショットでは `--expected-sequence <n>` フラグ
- `params`: コマンドごとの引数。キー名は camelCase

`session inspect`、Mutation、`render start`、`undo`、`redo` は `expectedSequence` を検証する。

成功応答:

```json
{
  "requestId": "42",
  "ok": true,
  "sequence": 19,
  "result": { "type": "session", "value": { "...": "..." } }
}
```

- `sequence`: その結果が対応する正準シーケンス。直前の応答から取って次の操作の `expectedSequence` に使える
- 正準Mutationの成功応答は `result.type: "mutation"` となるreceiptである。`result.value.createdEntityIds` には、そのMutationで新しく生成されたIDだけが種類ごとに含まれ、生成IDがないMutationでは `{}` になる。現在のSession全体は後続の `session inspect` または `track list` で確認する

後続の操作でIDが必要な場合は、応答の `createdEntityIds` を使う。たとえば `track add` の応答からTrack IDを取り出して `instrument apply` や `track update` へ渡す。

interactive JSONLで操作を連鎖させる場合は、1つの要求を送り、応答を受け取ってから次の要求を組み立てる。`expectedSequence` には直前の応答の `sequence` を渡す。

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
