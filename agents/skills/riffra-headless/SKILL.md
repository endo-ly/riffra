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

`riffra`(`apps/cli`)は GUI を使わずに制作状態を照会・編集・運用するヘッドレス用 CLI である。Desktop アプリと同じ正準状態(`riffra-core`)を共有し、本書は help から読み取れない契約をまとめる。各コマンドの引数・値域・排他規則は `riffra <command> --help` が正本である。

実行ファイルは `riffra` と表記する(リポジトリでは `cargo run -p riffra-cli --`、インストール済み環境ではデスクトップ実行ファイルと同じディレクトリの `riffra.exe`(既定 `%LOCALAPPDATA%\Riffra`)に読み替える)。

## 目次

1. **実行のしかた** — 実行モード / Host の起動と接続 / Safe Mode の範囲
2. **要求と応答** — 要求の形式 / 応答の読み方 / エラーコード
3. **楽曲を組み立てる** — Session の照会と Undo / 一括適用(session apply) / Track・Clip・Region / 楽曲の入力 / 和声・フレーズ・リズム
4. **音とミックス** — Device・Plugin / Instrument / ミックス / Automation
5. **音声と素材** — 録音 / 書き出しと測定 / Library・解析・Preview
6. **共通の注意** — 引数と入出力

## 1. 実行のしかた

### 実行モード

| 形態                    | 起動                                            | 用途                                      |
| ----------------------- | ----------------------------------------------- | ----------------------------------------- |
| Standalone ワンショット | `riffra --data-root <path> <command> ...`       | 1 操作を実行し JSON 応答を出す            |
| Standalone 対話         | `riffra --data-root <path> --interactive`       | stdin へ JSON Lines 要求で連続操作        |
| Live Host               | `riffra --data-root <path> serve [--safe-mode]` | フォアグラウンド常駐し Audio Runtime 提供 |
| Attached                | `riffra --attach <command>`                     | current-user registryから選んだHostへ接続 |

正準状態は操作ごとに DataRoot へ永続化されるため、編集はプロセスを跨いで引き継がれる。Undo / Redo 履歴と `expectedSequence` はプロセス(Host)の寿命に紐づく。

そのため、音声を伴わない編集は Standalone で行い、連続操作や Undo / Redo には `--interactive` を使う。Audio Runtime を伴う操作は `serve` + `--attach` で行う。

Standaloneと`serve`では `--data-root` が必須である(慣例は `./riffra-data`)。同じDataRootを同時に所有できるプロセスは1つだけである。

### Host の起動と接続

```powershell
riffra --data-root ./riffra-data serve             # 通常モード(Native audio engine を使用)
riffra --data-root ./riffra-data serve --safe-mode # Safe Mode(音声・MIDI・外部プラグインをオフライン化し Host だけを起動)
```

`serve` は終了シグナル(SIGINT / SIGTERM)を受けるまでフォアグラウンドで動作する。エージェントからはバックグラウンドで起動し、`riffra --attach host status` で稼働を確認してから使う。停止は `riffra --attach host shutdown` かプロセスへのシグナルで行う。

通常モードは Native audio engine のサイドカー(`riffra-audio`)を必要とし、無い場合は `native/audio-engine` 配下のbuild scriptでビルドする。Safe Modeではサイドカー不要である。

Linuxの通常モードはALSA入出力デバイスを必要とする。デバイスを開けない場合は、Hostの起動後もRuntimeがReadyにならない(`riffra --attach audio status` で確認)。

Attachedは current-user registry から稼働中のHostを発見し、DataRootは開かない。候補が1件なら自動選択、複数件なら `--host <instance-id>` が必要である。Host一覧は `riffra host list` で確認する。

### Safe Mode の範囲

正準状態の編集はSafe Modeでもそのまま使える。transport、live MIDI送信、`audio probe` 系、`audio driver set`、`plugin scan`、`record start`、`asset preview` は `runtimeUnavailable` になる一方で、`render` とjob管理、`library`、`analysis`、`plugin catalog list`、`missing list`、`record list` などの管理系は使える。

## 2. 要求と応答

### 要求の形式

対話モードでは標準入力へ 1 行 1 要求を書き、標準出力から 1 行 1 応答を読む。空行は無視される。

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

`session inspect`、Mutation、`render start`、`undo`、`redo` は `expectedSequence` を検証する。interactiveで操作を連鎖させるときは、応答を受け取ってから次の要求を組み立て、`expectedSequence` に直前応答の `sequence` を渡す。

### 応答の読み方

成功応答は次の形で、`sequence` がその結果の正準シーケンスであり、次の操作の `expectedSequence` に使える:

```json
{
  "requestId": "42",
  "ok": true,
  "sequence": 19,
  "result": { "type": "mutation", "value": { "...": "..." } }
}
```

Mutationでは `result.value.createdEntityIds` に、そのMutationで生成されたIDが種類別のキー(`tracks`、`midiClips`、`devices` など)で入る。キー名は応答から確認する。Session全体は `session inspect` / `track list` で確認する。

失敗応答は `ok: false` とともに `error.code` と `error.details` を持つ:

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

### エラーコード

分岐は必ず `error.code` で判定する。

| code                 | 意味                                          | 対処                                           |
| -------------------- | --------------------------------------------- | ---------------------------------------------- |
| `invalidRequest`     | 要求形式・params・未知のコマンドが不正        | params のキー名(camelCase)と型を見直す         |
| `commandFailed`      | Core / Host / 保存処理の失敗(ID 不存在など)   | message の内容に対処する                       |
| `conflict`           | `expectedSequence` が現在のシーケンスと不一致 | Inspectし直して編集内容を決め直す              |
| `hostUnavailable`    | Attached が Host へ接続できない               | `riffra host list` で登録とhandshake状態を確認 |
| `runtimeUnavailable` | Runtime を利用できない(Safe Mode、Standalone) | `serve` + `--attach` に切り替える              |

## 3. 楽曲を組み立てる

### Session の照会と Undo

現在の構造と `sequence` は `session inspect` で照会する。`session get` はNote全件・録音詳細を含むフルスナップショットなので、Inspectにない詳細が必要なときだけ使う。

編集には直前の `sequence` を `expectedSequence` に渡し、編集後は同じ範囲をInspectして確認する。取り消しは変更後の `sequence` を指定して `undo` する。

```powershell
# 応答の sequence を次の --expected-sequence に渡し、生成IDは createdEntityIds から取る
riffra --attach session inspect
riffra --attach --expected-sequence 0 track add --name Drums --kind audio
riffra --attach --expected-sequence 1 music midi-clip create --track-id track:01j... --start 5:1 --end 13:1 --name Piano
riffra --attach --expected-sequence 2 music note insert --clip-id midi-clip:01j... --notes-json '[{"pitch":"C4","position":"5:1","duration":"1/8"}]'
```

### 一括適用(session apply)

複数の正準編集を事前に決めているなら、CLI側でIDを受け渡すスクリプトを書かずに `session apply` へJSONLを渡す。全operationが成功したときだけ1回のcommitとして適用される。途中結果を確認しながら進めたい場合や、1操作ずつUndoしたい場合は `--interactive` を使う。

JSONLでは `trackName` / `clipName` / `busName` / `returnBusName`、ownerの `owner.name` で名前解決でき、同じBatch内で追加したTrack・Bus・Clipを参照できる。`--replace` はSessionを初期状態に戻してからJSONLを適用する。Projectの同一性は保たれ、録音素材ファイルも残る。

```powershell
riffra --data-root ./riffra-data --expected-sequence 42 session apply --file ./song.jsonl
riffra --attach session apply --file ./song.jsonl --include-created-ids
```

```jsonl
{"command":"track.add","params":{"name":"Lead","kind":"instrument"}}
{"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"9:1"}}
{"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"C4","position":"1:1","duration":"1/8"}]}}
```

既存ClipのNoteだけをまとめて置き換えるには、同じBatchに `midi-note.clear` → `music.note.insert` の順で含める。raw MIDI値でよければ `midi-clip.update` の `patch.notes` へNote配列を渡す。

paramsキーはCLIフラグ名と対応する(`--notes-json` → `notes`、`--file` → `operations`)。`music phrase insert` だけは `--phrase-json` の内容が `pattern` と `placements` の2つのparamsへ分かれる。

失敗時は `error.details.operationIndex` と `command` でどのoperationか特定でき、JSONL入力では物理行番号が `inputLine` に入る。paramsの検証エラーには、JSON Pointer形式の `path` と配列要素の `index`、問題の値が付く。

### Track・Clip・Region

Trackは `track add`、MIDI Clipは `music midi-clip create`(「楽曲の入力」)で作成し、`track list` / `session inspect` で確認する。取り込んだMIDI Asset(`asset import-midi`)の配置は `audio-clip add-asset` / `midi-clip add-asset` で行う。Regionは `Intro` や `Verse` などの自由な名前を持つ時間範囲で、重複・入れ子・同名を許可する。

### 楽曲の入力

通常の作曲では `music.*` を優先する。位置・音価・音高は音楽座標のまま渡し、Coreがテンポ・拍子に基づいて正準TimelineTickへ変換する。テンポ・拍子の変更は `timebase update`、変更点全体は `timebase get-map` / `set-map` で扱う。

```text
位置: 5:1、5:3+1/2
音価: 1/4、1/8、3/8、1/12
音高: C4、F#4、Bb3
```

コマンドの使い分け:

- `music note` / `music midi-clip`: 音楽座標によるNote・MIDI Clipの参照・作成・更新・削除・配置。通常のNote操作はこちら
- `midi-note`: 量子化・変形・複製など、既知のNote IDをraw tick・MIDI値で直接編集する操作
- `midi-clip`・`instrument-event` のraw tick系操作: CCやPitch Bendなど、音楽上の基本操作に含まれないMIDIイベントのtickによる直接編集

tickを直接扱う操作での換算基準は `session inspect.project.ppq` である(Instrument previewの `ticksPerBeat` はpreview固有の値)。

Noteの取得は `music note list` で範囲を絞って行う。一括調整は `music note transform`、個別の更新・削除は `--include-ids` で取得したIDに対して行い、raw tickやMIDI値で扱うときだけ `--raw` を付ける。`position` はClip内部の相対位置ではなくArrangement全体の絶対位置である。複数Noteは1回の `music note insert` で渡す。

各Noteの `lyric` は `music note update --lyric` / `--clear-lyric` で変更・削除し、一括変更は `midi-note update-many` のpatchの `lyric`(`null` で削除)で扱う。

```powershell
riffra --attach --expected-sequence 20 music note transform `
  --track-id track:drums --start 5:1 --end 13:1 `
  --pitch D2 --timing-offset +1/48 --velocity-offset 4
```

`music export-midi --clip-id <id> --output <path>` はClip内のNoteとMIDIイベントをClip先頭から書き出す。`--track-id` を指定するとTrack内の全MIDI Clipをプロジェクト上の位置で書き出す。歌詞とプロジェクトのテンポ・拍子もSMFに含まれる。

`instrument-event` はMIDI Clip内の音源制御(`sustainPedal`、`pitchBend`、`modWheel`、`aftertouch`、`parameterChange`)を編集する。Parameter Changeは `parameter` と `nativeValue` を保持する。

### 和声・フレーズ・リズム

和声は一般的なChord Symbolを `music harmony insert` へ渡す。parserで表現できない音集合は `chord` の代わりに `pitches`、任意の `root` / `bass`、`label` をイベントJSONへ渡す。解釈の確認は `music harmony resolve`、コードヒットの反復は `music harmony realize` の `RhythmPattern` で指定する。

反復する旋律やモチーフは、半音差で表す `PhrasePattern` と複数の `placements` を `music phrase insert` へ渡す。同じ音高を繰り返すKick・SnareなどのドラムパターンもPhraseで表現できるため、Noteを大量に列挙する前に利用を検討する。挿入前の展開規模は `music phrase preview` で確認できる。

Patternは操作入力であり、正準セッションへは展開結果だけが保存される。`position` / `placements[].position` は `bar:beat`(または `bar:beat+fraction-of-beat`)、`length` / `offset` / `duration` は全音符を1とする音価の分数(`1/8` は8分音符)である。

```powershell
riffra --data-root ./riffra-data music phrase insert --clip-id midi-clip:01j... `
  --phrase-json '{"pattern":{"length":"1/1","notes":[{"offset":"0/1","duration":"1/8","semitones":0},{"offset":"1/4","duration":"1/8","semitones":5}]},"placements":[{"position":"5:1","anchor":"C4","repeats":2}]}'

riffra --data-root ./riffra-data music harmony realize --clip-id midi-clip:01j... `
  --start 1:1 --end 5:1 --rhythm-json '{"length":"1/2","steps":[{"offset":"0/1","duration":"1/8"},{"offset":"1/4","duration":"1/16","velocity":112}]}'
```

## 4. 音とミックス

### Device・Plugin

EffectとDeviceのOwnerは `--track-id`、`--bus-id`、`--master` のいずれか1つで指定する。Master・BusにはEffectsだけを追加できる。

Effectの候補は `plugin catalog list` で確認し、`effect add` でEffect Chainの末尾へ追加する。新しいDevice IDは応答の `createdEntityIds.devices` に入る。Chain内の順序・Bypass・公開Presetは `effect reorder`、`device bypass`、`plugin preset` で扱う。

`--plugin-id` は現在のDataRootのCatalogにあるValidated・Role一致のPluginへ解決され、解決後のパスがProjectへ保存される。IDはインストールパスに依存するため、別PCへ持ち運べる識別子ではない。Standaloneではパスだけを登録し、実体のロードはRuntimeが行う。

`device inspect`、`device parameter list/get/set`、`plugin state save/load`、`plugin preset` は実体のPluginを扱うため Live Host 専用である。Built-in instrumentはparameter・state・preset・editorをサポートしない。

`device parameter list` の主要フィールド:

| フィールド     | 意味                                                                          |
| -------------- | ----------------------------------------------------------------------------- |
| `parameterId`  | Automationへ指定するStable ID。一意で非空のIDを公開できない場合は`null`       |
| `index`        | set/get へ渡すParameter index                                                 |
| `name`         | Pluginが返したParameter名                                                     |
| `value`        | normalized 0..1 の現在値                                                      |
| `defaultValue` | Pluginのnormalized既定値                                                      |
| `displayValue` | 現在値の表示文字列                                                            |
| `choices`      | 有限離散値が列挙できる場合の `{ value, displayValue }` 配列。それ以外は空配列 |
| `automatable`  | PluginのAutomation対応情報                                                    |

手動操作は `name` でParameterを特定し、離散値は `choices` の表示から選んでその `value` を渡す。Continuousの表示が空の場合は、normalized値(`value`)を用いる。Automationには `automatable: true` かつ `parameterId` が非nullのParameterを選び、既定値への復元には `defaultValue` を渡す。

`device parameter set` の応答は `mutation`(正準変更)、`parameter`(対象の読み戻し)、`changedParameters`(値が変わったほかのParameter、index順)を持つ。`--display` はPluginに文字列からの変換を依頼し、表示へ戻した結果が入力と等価な場合だけ適用される。`session apply` 内では正規化値だけを書き、`--display` は使えない。

### Instrument

役割(リード、ベース、コードなど)とプリセットの `category` は直接対応しない。`category` はライブラリ上の主分類であり、`instrument list` が返す全プリセットから `description`・`tags`・`recommended_range` で選ぶ。主旋律には `Lead` だけでなく `Pluck`・`Mallet`・`Keys` も候補になる。

割り当ては `instrument apply`(`builtin:<presetId>` または `user:<id>`)。Built-in instrumentはSafe Modeでも割り当てられる。Instrument毎のMIDI設定は `instrument midi set --settings-json` で置換し、`transposeSemitones`、`playableNotes`(`{"type":"range","start":36,"end":84}` / `{"type":"set","notes":[...]}` / 制限なしの`null`)、`keyswitches`(`[{"note":24,"name":"C0"}]`)を持つ。

User Instrumentは `instrument save` で登録し、応答のIDを割当へ渡す。定義JSONと相対パスの素材は同じパッケージ構成で置く。複数Trackへの割当は `session apply` の `instrument.apply` に含める。

### ミックス

Track・Clipの音量・定位は `track update` / `audio-clip update` で、Masterのゲインは `session settings update --master-db` で調整する。Gain / VolumeはdB、Panは-1.0(左)..1.0(右)で指定する。

Group・Return・Send・Sidechainは `mixer` コマンド群で編集し、状態は `mixer get` で確認する。RenderはTrackとClipのGain、Pan、Mute、Solo、Automation、Masterをすべて反映する。

Sidechainの送信先は別Track・Group・Return・Master上のEffect Deviceであり、Instrument Deviceと送信元自身のTrack上のEffectは対象外である。接続前にLive Hostの `device inspect` で `capabilities.sidechain` を確認する。Source TrackのMuteはSidechainも止まり、Soloで主出力が分離されてもSidechainは維持される。

### Automation

`automation set` は指定対象のLane全体を置き換える。Pointは `id`・`tick`・`value` を持ち、Tickは楽曲全体の絶対位置である。音楽座標からの変換は `project.ppq` と拍子で行う。

Deviceの `value` には正規化数値またはPluginの表示文字列(例:`"250 ms"`)を渡せる。文字列はLive Hostが変換できた場合だけ保存される。

更新はLane IDを維持し、新規LaneのIDは `createdEntityIds.automationLanes` に入る。空のPoint配列と `clear` はLaneを削除する。

`session apply` 内では対象をCLI引数ではなく `target` オブジェクトで指定する。Pointは `tick` と `position` のいずれか一方を持ち、`position` はそのoperation時点のテンポ・拍子でTickへ変換される。

| `target.kind`        | 必要なフィールド                          |
| -------------------- | ----------------------------------------- |
| `trackMix`           | `trackId` または `trackName`、`parameter` |
| `busMix`             | `busId` または `busName`、`parameter`     |
| `masterGain`         | -                                         |
| `sendLevel`          | `sendId`                                  |
| `sidechainSendLevel` | `sidechainSendId`                         |
| `deviceParameter`    | `deviceId`、`parameterId`                 |

```jsonl
{
  "command": "automation.set",
  "params": {
    "target": {
      "kind": "trackMix",
      "trackName": "Organ",
      "parameter": "volume"
    },
    "interpolation": "linear",
    "points": [
      {
        "id": "intro",
        "position": "9:1",
        "value": -6
      }
    ]
  }
}
```

## 5. 音声と素材

transport / audio / midi 送信 / record / render / job / library / plugin 実体の操作 / device parameter set は Live Host(`serve`)+ `--attach` が必要で、Standaloneでは引数の内容にかかわらず `runtimeUnavailable` になる。

### 録音

録音にはarm済み(`track update --armed true`)のTrackが少なくとも1つ必要である。キャプチャはDataRootの `recordings/` 配下に置かれ、`record promote` で正準セッションへ反映する。

Processed Variantに必要な制御信号を録音から再利用できない場合はRaw Takeが保持され、`record list` の `processedUnavailableReason` で理由を確認できる。

### 書き出しと測定

音声の書き出しは `render start` で行う。`--expected-sequence` はRender対象の正準スナップショットを固定し、Conflictになった場合はInspectし直してからRenderし直す。

応答は `type: "job"` のジョブIDである。ワンショットは `job wait`(無指定ならterminal stateまで待機)、interactiveは `job get` の繰り返しで追跡し、`job cancel` で停止する。`plugin scan-start` と `analysis measure` も同じ非同期ジョブである。`ok: true` は要求の受付成功を表すので、Job自体の成功は `state: "completed"` で判断する。出力はDataRootの `renders/` 配下へ書き出される。

各パートの比較は `render stems` でTrack別WAVを作る。全WAVで開始Tick、終了Tick、Sample Rate、フレーム数が揃い、個別の正規化とMasteringは行われない。共有する非線形Effectでは、Stemの合計と全体Mixが一致しない場合がある。StemとMasteringの設計上の扱いは[音声エンジンの設計](../../../docs/audio-engine.md)を参照。

Job結果と `render.json` の `measurements` には完成WAVのSample Peakとラウドネス測定が含まれる。区間ごとの音量・ピーク位置・帯域バランスは `measurements.sections` で読む。

| 項目                               | 読み方                                                                              |
| ---------------------------------- | ----------------------------------------------------------------------------------- |
| `name` / `start` / `end`           | Marker名と区間の音楽座標                                                            |
| `loudness.integratedLufs`          | 区間の統合ラウドネス。有効な400msブロックがない区間では`null`                       |
| `loudness.truePeak`                | 最大True Peakの`db`・`seconds`・`position`。無音なら`null`。位置は100msフレーム単位 |
| `bands.lowDb` / `midDb` / `highDb` | 250Hz未満・250Hz〜4kHz・4kHz超のRMSレベル(dBFS)。無音なら`null`。帯域間比較の目安   |

区間の切り分けは書き出し時点の正準Markerで決まる。`measurements.sectionLoudnessState.state` が `measured` なら無音・短区間の値が `null` でも測定は正常に完了しており、`unavailable` なら `reason` を確認する。

通常Renderで測定器を利用できない場合は、成功結果のラウドネス測定に `unavailable` と理由が返る。Masteringを要求したJobでの測定失敗はJob失敗になる。全体と区間のラウドネスは独立して測定され、区間の測定が失敗してもWAVと帯域値は保持される。

区間や比較対象だけを変えて測定し直すときは、`render list` のRender IDまたは保存済み・外部WAVを `analysis measure` へ渡す。測定対象は書き出し済みのWAVであるため、音源・Note・ミックスを変更した音の評価は、新しいWAVを書き出してから行う。

```json
{
  "renderId": "render-...",
  "ranges": [
    { "name": "Intro", "startSeconds": 0, "endSeconds": 8 },
    { "name": "Verse", "startSeconds": 8, "endSeconds": 24 }
  ]
}
```

外部WAVの比較では `renderId` を `"paths": ["C:/music/drums.wav", "C:/music/bass.wav"]` に置き換える。

```powershell
riffra --attach analysis measure --file measurements.json
riffra --attach job wait --id job:analysis-measure:0 --timeout-ms 120000
```

待機には開始応答のJob IDを使い、結果の `sources` のパス・Track情報を比較対象と照合する。区間True Peakの位置はWAV先頭からの秒数である。帯域値・測定状態の読み方はRenderと共通である。

Mixdown設定は `mixdown get` / `mixdown set --settings-json` で参照・置換する。Masteringは目標ラウドネスへMix全体を一定のゲインで合わせ、True Peak上限を超えるときは上限に合わせてゲインを下げる。

`mastering.deviation.integratedLufs` は最終ラウドネス − 目標で、負のときは目標に届いていない。`mastering.truePeakCorrectionDb` はTrue Peak上限のために下げたゲインで、0より大きいときは上限で止まったことを示す。

```powershell
riffra --attach mixdown set --settings-json '{"musicalEndTick":0,"tailSeconds":2,"fadeOutSeconds":0.5,"mastering":{"integratedLufs":-14,"truePeakDb":-1,"loudnessRangeLu":11},"sampleRate":48000,"blockSize":1024}'
```

### Library・解析・Preview

`library search` はAsset metadataへのfree-text検索である。短い語は全Assetがヒットして応答が膨らむため、具体語で絞る。

素材の解析は `analysis start`、試聴は `asset preview` / `stop-preview`、Runtime projectionの状態は `runtime projection get` / `retry` で扱う。

## 6. 共通の注意

構造入力は小さければinline JSON、大きければ `--*-file`、連続操作はinteractive JSONLを使う。複数IDはコンマ区切りとJSON形式のどちらでも渡せるが、recording-slot由来などWindowsパスを含むIDはJSON形式(`--*-ids-json`、またはfile)が安全である。

CLIが標準出力へ書くJSONはUTF-8である。PowerShell 5.1など親Shellがパイプ経由で別のコードページへ変換した後の表示はRiffraのProtocol保証に含まれない。
