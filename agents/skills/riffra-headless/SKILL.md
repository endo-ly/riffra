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

`riffra`(`apps/cli`)は GUI を使わずに制作状態を照会・編集・運用するヘッドレス用 CLI であり、Desktop アプリと同じ正準状態(`riffra-core`)を共有する。本書は全操作に共通する基盤(実行形態、制作フロー、入力契約、プロトコル、エラーコード)を扱う。

各コマンドの引数・値域・排他規則は `riffra <command> --help` が正本である。コマンドの選び方、構造入力の書式、応答の読み方は [references/commands.md](references/commands.md) を参照。

以下、実行ファイルは `riffra` と表記する(リポジトリでは `cargo run -p riffra-cli --`、インストール済み環境ではデスクトップ実行ファイルと同じディレクトリの `riffra.exe`(既定 `%LOCALAPPDATA%\Riffra`)に読み替える)。

## 実行モード

| 形態                    | 起動                                            | 用途                                      |
| ----------------------- | ----------------------------------------------- | ----------------------------------------- |
| Standalone ワンショット | `riffra --data-root <path> <command> ...`       | 1 操作を実行し JSON 応答を出す            |
| Standalone 対話         | `riffra --data-root <path> --interactive`       | stdin へ JSON Lines 要求で連続操作        |
| Live Host               | `riffra --data-root <path> serve [--safe-mode]` | フォアグラウンド常駐し Audio Runtime 提供 |
| Attached                | `riffra --attach <command>`                     | current-user registryから選んだHostへ接続 |

正準状態は操作ごとに DataRoot へ永続化されるため、編集自体はプロセスを跨いで引き継がれる。履歴(Undo / Redo)と `expectedSequence` のRevision tokenはプロセスまたはHostの寿命に紐づくため、音声を伴わない編集は Standalone(単発はワンショット、連続操作や Undo / Redo は `--interactive`)、Audio Runtime を伴う操作は `serve` で起動した Host へ `--attach` する。

## コマンドの 2 系統

コマンドは「正準状態の編集」と「Runtime サービス」の 2 系統しかなく、4 つの実行形態は同じコマンドへの要求経路の違いである。

- **正準状態の編集**はすべての実行形態で同じ引数が使える
- **Runtime サービス**は Live Host + `--attach` が必要で、Standalone では `runtimeUnavailable`

## 制作の基本手順

既存のSessionを編集するときは、全体を確認してから、音楽上のまとまりが大きい順に組み立てる。

1. `session inspect` で現在の構造と `sequence` を確認する
2. トラック、音源、テンポ、リージョンを必要な範囲で設定する。Effect が必要なら `plugin catalog list` で検証済み Effect を選び、`effect add` で末尾へ追加し、応答の `createdEntityIds.devices` から Device ID を得る
3. `music harmony`、リズムパターン、`music phrase` で和声や反復パターンを配置する
4. Noteは `music note list` で必要な範囲だけ取得し、一括調整は `music note transform`、個別の更新・削除だけ `--include-ids` で取得したIDを使う。raw tickやMIDI値が必要なときだけ `--raw` を付ける
5. `session inspect` または `track list` で結果を確認する
6. Live Host で `device inspect` と `device parameter list` を読み、Parameter の意味と表示値を確認して `device parameter set` で音を調整する。離散値は `choices` の表示から選び、その `value` を渡す。設定後は応答の`parameter`で対象の読み戻し値、`changedParameters`でほかのParameterへの影響を確認する。順序・Bypass・公開 Preset は `effect reorder`、`device bypass`、`plugin preset` を使う。ミックスのバランスを `track update`、`audio-clip update`、`automation set`、`session settings update --master-db` で整える
7. `render start` で音声を書き出し、完了を `job wait`(ワンショット)または `job get`(interactive)で確認する
8. 書き出し結果の`measurements.sections`で区間ごとのラウドネス・ピーク位置・帯域バランスを確認する。素材の解析には`analysis start`、実行状態の確認には`audio diagnostics`を使い、必要なら編集へ戻る

Sonalloy Bundleから制作を始める場合は`project import-sonalloy <BUNDLE_DIRECTORY>`を使う。新しいProjectへ切り替わったら通常のTrack、MIDI Clip、音源制御イベントを編集する。インポートはProject単位の操作であり、`session apply`へ含めない。

### まとめて構築する場合

生成したJSONLでSessionを作り直すときは `--replace` を指定する。楽曲の初期構築や、事前に決めた複数の正準編集は `session apply` を第一候補にする。全operationが成功したときだけ1回のcommitとして適用されるため、途中で失敗してもそれまでのoperationは残らない。途中結果を確認して次の編集を決める場合や、1操作ずつUndoしたい場合は `session apply` ではなく `--interactive` を使う。

### 音源選択

楽曲内の役割(リード、ベースライン、コードなど)とプリセットの `category` は直接対応しない。`category` はライブラリ上の主分類であり、候補は `instrument list` が返す全プリセットから `description`、`tags`、`recommended_range` を見て選ぶ。たとえば主旋律には `Lead` だけでなく `Pluck`、`Mallet`、`Keys` も候補になる。

## 楽曲制作の入力契約

通常の作曲では `music.*` コマンドを優先する。位置・音価・音高は音楽座標のままCLIへ渡し、Coreがプロジェクトの拍子と正準TimelineTickへ変換する。

```text
位置: 5:1、5:3+1/2
音価: 1/4、1/8、3/8、1/12
音高: C4、F#4、Bb3
```

コマンドの使い分けは次のとおり。

- `music note` / `music midi-clip`: 音楽座標によるNoteとMIDI Clipの参照・作成・更新・削除・配置。通常のNote操作はこちらを使う
- `midi-note`: 量子化・変形・複製など、既知のNote IDをraw tick・MIDI値で直接編集する操作
- `midi-clip`・`instrument-event` のraw tick系操作: CCやPitch Bendなど音楽上の基本操作に含まれないMIDIイベントの、tickによる直接編集

既存Noteの通常の調整に `session get` で全Sessionを取得したりNote IDを列挙して再挿入したりする必要はない。和声の解決、Phrase / Rhythmの展開、bar・beatからtickへの変換、Clip相対位置はエージェント側で計算せず、Coreが解決・展開して正準セッションへ保存する。TimelineのPPQは `session inspect.project.ppq` を正とし、固定値を再定義しない。Instrument previewの `ticksPerBeat` はpreview固有の値であり、Timelineのtick計算には使わない。

小さな構造入力はinline JSON、大きな構造入力は `--*-file`、連続操作はinteractive JSONLを使う。

### 和声・フレーズ・リズム

和声は一般的なChord Symbolをそのまま `music harmony insert` へ渡す。解釈を確認したいときは `music harmony resolve` を使う。

反復する旋律やモチーフは、半音差で表す `PhrasePattern` と複数の `placements` を `music phrase insert` へ渡す。コードヒットの反復は `music harmony realize` の `RhythmPattern` で指定する。同じ音高を繰り返すKickやSnareなどのドラムパターンもPhraseで表現できるため、Noteを大量に列挙する前に利用を検討する。挿入前に展開規模を確認するときは `music phrase preview` を使う。

## ミックス

Track・Clipの音量や定位に加え、バスへの接続やセンドを使ってミックスを組み立てる。Group・Return・Send・Sidechainは `mixer` コマンド群で編集する。ミックス状態は `mixer get`、`track list`、`session inspect` で確認し、聞こえと測定値の確認はRenderで行う。

## 書き出しと評価

楽曲を編集したら `render start`、各パートを比較するなら `render stems` でWAVを作成する。返されたJob IDを `job wait` へ渡し、完了結果を確認する。区間や比較対象を変えて測定するだけなら、`render list` のRender IDまたは保存済みWAVを `analysis measure` へ渡す。測定は楽曲やMarkerを編集せず、再レンダリングも行わない。音源・ノート・ミックスを変更した音を評価するときは、新しいWAVを書き出す。

## Host discoveryとDataRoot

Standaloneと`serve`では `--data-root` が必須である(慣例は `./riffra-data`、作った場所は呼び出し側が引き回す)。同じDataRootを同時に所有できるプロセスは1つだけである。

AttachedはDataRootを開かず、current-user registryから稼働中のHostプロセスを発見する。候補が1件なら自動選択し、複数件なら `--host <instance-id>` を要求する。Host一覧は `riffra host list` で確認する。

## 制御プロトコル(JSON Lines)

対話モードでは標準入力へ 1 行 1 要求を書き、標準出力から 1 行 1 応答を読む。空行は無視される。ワンショットも内部的には同じ要求へ変換される。

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

`session inspect`、Mutation、`render start`、`undo`、`redo` は `expectedSequence` を検証する。interactive JSONLで操作を連鎖させる場合は、1つの要求を送り、応答を受け取ってから次の要求を組み立て、`expectedSequence` には直前の応答の `sequence` を渡す。

成功応答:

```json
{
  "requestId": "42",
  "ok": true,
  "sequence": 19,
  "result": { "type": "mutation", "value": { "...": "..." } }
}
```

`sequence` はその結果が対応する正準シーケンスであり、直前の応答から取って次の操作の `expectedSequence` に使える。正準Mutationの成功応答は `result.type: "mutation"` となるreceiptであり、`result.value.createdEntityIds` にはそのMutationで新しく生成されたIDだけが種類ごとのキー(`tracks`、`midiClips`、`devices` など)で含まれる。後続の操作でIDが必要な場合はこの応答から取り出して渡す。現在のSession全体は後続の `session inspect` または `track list` で確認する。

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
