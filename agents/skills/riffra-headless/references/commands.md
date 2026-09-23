# riffra コマンドリファレンス

実行モード・DataRoot・プロトコルの基礎は [SKILL.md](../SKILL.md) を参照。

コマンド引数はロングフラグ(camelCase を kebab-case 化)で渡し、完全な一覧は `riffra <command> --help` で確認できる。各系統に含まれるコマンドと実行できる場所は次のとおり。

| 系統             | 主なコマンド                                                                           | 実行できる場所                 |
| ---------------- | -------------------------------------------------------------------------------------- | ------------------------------ |
| 正準状態の編集   | session / track / music / clip / midi-note / marker / automation / rack / missing 復旧 | すべての実行形態               |
| Runtime サービス | transport / audio / midi 送信 / record / render / job / library / plugin               | Live Host(`serve`)+ `--attach` |

## 正準状態の編集

### 基本サイクル

1. `session inspect` で現在の構造と `sequence` を把握する。必要なら `--start` / `--end` または `--track-id` で対象を絞る。`session get` は MIDI ノート全件や録音の詳細を含むフルスナップショットなので、Inspectにない詳細が必要なときだけ使う
2. 編集コマンドを実行する。Inspectまたは直前の応答の `sequence` を `--expected-sequence` に渡す
3. 編集後は同じ範囲を `session inspect` し、音を確認するときは確認後の `sequence` を指定した `render start` を行い、完了を `job wait`(ワンショット)または `job get`(interactive)で待つ。採用しない変更は変更後の `sequence` を指定した `undo` を実行して再Inspectする
4. `conflict` になったMutation・Render・Undo・Redoは自動再送せず、最新状態をInspectして内容を決め直す

```powershell
# 現在の構造と sequence を把握
riffra --attach session inspect

# 必要な範囲だけ確認
riffra --attach session inspect --start 9:1 --end 13:1 --track-id track:01j...

# Track追加の応答からIDを取得し、後続の要求に渡す
riffra --attach --expected-sequence 0 track add --name Drums --kind audio
riffra --attach session inspect

# MIDI Clip 作成 → 再Inspectで状態を確認して音楽上のNoteを積む
riffra --attach --expected-sequence 1 music midi-clip create --track-id track:01j... --start 5:1 --end 13:1 --name Piano
riffra --attach session inspect --track-id track:01j...
riffra --attach --expected-sequence 2 music note insert --clip-id midi-clip:01j... --notes-json '[{"pitch":"C4","position":"5:1","duration":"1/8"}]'
```

Timebaseのテンポ・拍子は `timebase update` で変更できる。MIDI channel は1〜16の範囲で指定する。

`createdEntityIds`で利用できるキーは次のとおりである。これ以外のキーは推測して使わない。

| キー              | 対象                        |
| ----------------- | --------------------------- |
| `tracks`          | Track                       |
| `audioClips`      | Audio Clip                  |
| `midiClips`       | MIDI Clip                   |
| `midiNotes`       | MIDI Note                   |
| `markers`         | Marker                      |
| `regions`         | Region                      |
| `harmonyEvents`   | Harmony Event               |
| `automationLanes` | Automation Lane             |
| `devices`         | Instrument or effect device |

### 複数操作の一括適用

複数の正準Mutationを事前に決めている場合は、CLI側でIDを受け渡すスクリプトを書かずに `session apply` へJSONLを渡す。空行は無視され、外側の要求だけが `expectedSequence` を持つ。

```powershell
riffra --data-root ./riffra-data --expected-sequence 42 session apply `
  --file ./song.jsonl
riffra --attach session apply --file ./song.jsonl --include-created-ids
```

```jsonl
{"command":"track.add","params":{"name":"Lead","kind":"instrument"}}
{"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"9:1"}}
{"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"C4","position":"1:1","duration":"1/8"}]}}
```

各行は `command` と `params` を持つ既存Control Commandであり、Batch専用の楽曲記法は使わない。`trackName`は候補Session上で一意に解決される。`clipName`は指定したTrack内のMIDI Clipから一意に解決されるため、`clipName`を使うときは `trackId` か `trackName` を併せる。`trackId`と`trackName`、`clipId`と`clipName`は同時に指定できない。同名が複数ある場合は曖昧さとして失敗する。解決結果はProtocolやCanonical identityには保存されない。

Batchは全operationを候補Sessionへ適用してから、成功時だけ1回commitする。`expectedSequence`の不一致はoperation開始前にBatch全体を拒否し、途中の失敗、非対応コマンド、名前の未解決・曖昧さでもCanonical Sessionは変更されない。`session.get`、`session.inspect`、`history`、Undo / Redo、Render、Transport、Recording、Asset importなどはBatchへ含めない。`instrument.apply` は組み込みinstrumentに限りBatchへ含められ、`user:` instrumentは単独のcommandとして実行する。

成功応答はCanonical Sessionやoperationごとの結果を含まない。

```json
{
  "type": "batchMutation",
  "value": {
    "appliedCommands": 34,
    "createdEntityCounts": {
      "tracks": 8,
      "midiClips": 8,
      "midiNotes": 642
    }
  }
}
```

生成IDが必要なときだけ `--include-created-ids` を指定する。失敗時は `error.details.operationIndex` と `command`でoperationを特定でき、CLIのJSONL入力では元ファイルの物理行が`inputLine`に入る。

interactive JSONLの構文エラーまたは検証エラーには、物理入力行が`error.details.inputLine`として付く。空行も行番号に含まれるため、エラー箇所は入力ファイルの実際の行番号で確認する。

paramsのdeserializeエラーには、可能な範囲でJSON Pointer形式の`error.details.path`、配列要素のゼロ始まり`index`、問題の値が付く。大きなObjectやArrayの値はレスポンスを膨らませないため省略される。`inputLine`とこれらのdetailsは同じエラーへ共存する。

```json
{
  "code": "invalidRequest",
  "message": "invalid command parameters: expected u8",
  "details": {
    "inputLine": 12,
    "path": "/notes/37/velocity",
    "index": 37,
    "value": null
  }
}
```

### Music Operations

#### MIDI ClipとNote

```powershell
riffra --data-root ./riffra-data music midi-clip create `
  --track-id track:01j... --start 5:1 --end 13:1 --name Piano

riffra --data-root ./riffra-data music note insert `
  --clip-id midi-clip:01j... `
  --notes-json '[{"pitch":"C4","position":"5:1","duration":"1/8"},{"pitch":"E4","position":"5:1+1/2","duration":"1/8"},{"pitch":"G4","position":"5:2","duration":"1/2","velocity":92}]'
```

`position` はArrangement全体の絶対位置で、Clip内部の相対位置ではない。`velocity` の既定値は100、`channel` の既定値は1である。複数Noteは1回の `music note insert` で渡す。

```powershell
riffra --attach music note list --clip-id midi-clip:01j... --start 5:1 --end 9:1
riffra --attach music note list --track-id track:drums --start 5:1 --end 13:1
riffra --attach music note list --clip-id midi-clip:01j... --raw --include-ids
riffra --attach --expected-sequence 20 music note transform `
  --track-id track:drums --start 5:1 --end 13:1 `
  --pitch D2 --timing-offset +1/48 --velocity-offset 4
riffra --attach music note get --clip-id midi-clip:01j... --note-id note:01j...
riffra --attach --expected-sequence 21 music note update `
  --clip-id midi-clip:01j... --note-id note:01j... `
  --position 6:1 --duration 1/4 --pitch F#4 --velocity 96
riffra --attach --expected-sequence 22 music note remove `
  --clip-id midi-clip:01j... --note-id note:01j...
riffra --attach --expected-sequence 23 music midi-clip resize `
  --clip-id midi-clip:01j... --end 17:1
```

`music note list` はClipまたはTrackを一つだけ指定する。Track指定では範囲も必須である。listの範囲は半開区間で、Noteと範囲が重なるものを返す。Track指定の結果はClipごとにgroup化され、各NoteへClip IDを繰り返さない。通常結果は音楽座標だけで、IDは`--include-ids`、raw tick / MIDI値は`--raw`を指定したときだけ返る。

`--raw` の結果ではClipの`startTick`がArrangement上の位置、Noteの`startTick`がClip相対位置になる。絶対tickが必要な場合は両者を加算する。raw結果には`timebase.ppq`と拍子も含まれる。

`music note transform` は、指定範囲内で開始するNoteだけを対象にする。範囲指定はClip開始位置に対する絶対的な音楽座標で、Track指定では必須である。`--pitch`と`--channel`は変形前の値に対して判定される。`--timing-offset`は符号付き音価（例：`+1/48`、`-1/48`）、`--velocity-offset`は0〜127へclampされ、`--transpose-semitones`がMIDI範囲外になる場合や移動後のNoteがClip外になる場合はMutation全体が失敗する。対象Noteが0件でも失敗する。

ClipのresizeはNote/EventのArrangement上の絶対位置を保ち、範囲外へ出る場合はNote/Eventを削除・cropせず失敗する。

Note入力はJSON配列を一度に渡す。inline、file、stdinは排他的で、file/stdinでも1回のMutationになる。

```powershell
riffra --attach music note insert --clip-id midi-clip:01j... --notes-file ./notes.json
Get-Content ./notes.json -Raw | riffra --attach music note insert --clip-id midi-clip:01j... --stdin
```

Noteのpitch、position、duration、velocity、channelの意味検証はCoreが行う。CLIは入力の読み込み、JSON parse、top-levelが配列であることだけを確認する。ClipのNoteがClip終端を超える追加・更新・複製は自動延長せず、Mutation全体が失敗する。

### CLI入力とControl Protocolのparams

構造化JSONはCLIが読み込み、Control Protocolのparamsへ渡す。

| CLI入力                                     | Protocol params                            |
| ------------------------------------------- | ------------------------------------------ |
| `--notes-json` / `--notes-file` / `--stdin` | `notes`                                    |
| `--events-json` / `--events-file`           | `events`                                   |
| `--rhythm-json` / `--rhythm-file`           | `rhythm`                                   |
| `--phrase-json` / `--phrase-file`           | `pattern` と `placements`                  |
| `session apply --file`                      | `operations`                               |
| `music note update` の個別項目              | `clipId`、`noteId`、変更項目をparams直下へ |
| `music harmony update --patch-json`         | patchの各項目をparams直下へ展開            |

低レベルAPIが独自に`patch`を持つ場合は、その契約を維持する。

```powershell
riffra --attach music harmony insert --events-file ./events.json
riffra --attach music harmony realize --clip-id midi-clip:01j... --rhythm-file ./rhythm.json
riffra --attach music phrase insert --clip-id midi-clip:01j... --phrase-file ./phrase.json
```

#### Region

Regionは自由な名前を持つ時間範囲である。`Intro`や`Verse`などの種類は固定されず、重複・入れ子・同名を許可する。

```powershell
riffra --data-root ./riffra-data music region add `
  --name "A'" --start 5:1 --end 13:1
riffra --data-root ./riffra-data music region list
riffra --data-root ./riffra-data music region update `
  --region-id region:01j... --name "A' variation" --start 5:1 --end 17:1
riffra --data-root ./riffra-data music region remove --region-id region:01j...
```

#### Harmony

Chord Symbolの解決結果は `HarmonyChord` として返り、和声イベントはArrangement全体の音楽座標で管理される。イベント同士の重複・入れ子・gapは許可される。

```powershell
riffra --data-root ./riffra-data music harmony resolve `
  --chord "G7(b9,#11)/F"

riffra --data-root ./riffra-data music harmony insert `
  --events-json '[{"start":"1:1","end":"2:1","chord":"Dm9"},{"start":"2:1","end":"3:1","chord":"G7(b9,#11)"}]'

riffra --data-root ./riffra-data music harmony list
riffra --data-root ./riffra-data music harmony update `
  --event-id harmony:01j... --patch-json '{"chord":"Cmaj9","start":"3:1","end":"4:1"}'
riffra --data-root ./riffra-data music harmony remove `
  --event-ids-json '["harmony:01j..."]'
```

`harmony update` の `patch-json` では、`chord` と `pitches` は完全なChord定義として排他的に扱う。`pitches` を使う場合に限り `root`、`bass`、`label` を指定でき、指定しない値を現在のChordから継承しない。`root`、`bass`、`label` だけの更新や、`chord` との併用は拒否される。

parserに適さない音集合は `chord` と同時に指定せず、`pitches`、任意の `root` / `bass`、`label` をイベントJSONへ渡す。

#### Harmony realization

指定したHarmonyEventをCoreが決定的なvoicingでMIDI Noteへ展開する。`--lowest-octave` の既定値は3、`--velocity` の既定値は100、`--channel` の既定値は1である。slash bassは最低音になる。

```powershell
riffra --data-root ./riffra-data music harmony realize `
  --clip-id midi-clip:01j... --start 1:1 --end 5:1 --lowest-octave 3 `
  --rhythm-json '{"length":"1/2","steps":[{"offset":"0/1","duration":"1/8"},{"offset":"1/4","duration":"1/16","velocity":112}]}'
```

#### Phrase

Phraseの相対音高はanchorからの半音差で指定する。Patternは複数配置へCoreが展開し、生成Noteを1回の編集として保存する。

```powershell
riffra --data-root ./riffra-data music phrase insert `
  --clip-id midi-clip:01j... `
  --phrase-json '{"pattern":{"length":"1/1","notes":[{"offset":"0/1","duration":"1/8","semitones":0},{"offset":"1/8","duration":"1/8","semitones":2},{"offset":"1/4","duration":"1/4","semitones":5},{"offset":"1/2","duration":"1/4","semitones":7}]},"placements":[{"position":"5:1","anchor":"C4","repeats":2},{"position":"9:1","anchor":"Eb4","repeats":1}]}'
```

`PhrasePattern` と `RhythmPattern` は操作入力であり、正準セッションへ二重保存しない。

入力の単位が不正なときは、単位を含む説明をエラーへ返す。

挿入前の検証と展開規模の確認には `music phrase preview` を使う。PreviewはPattern、配置、Clip境界、MIDI pitch、channel、Note上限を検証するがSessionを変更しない。既定ではsummaryだけを返し、`--include-notes`を付けた場合だけ展開Noteを返す。Previewと同じSession状態での `music phrase insert` は同じresolverを使う。

```powershell
riffra --attach music phrase preview `
  --clip-id midi-clip:01j... `
  --phrase-file ./phrase.json
riffra --attach music phrase preview `
  --clip-id midi-clip:01j... `
  --phrase-file ./phrase.json --include-notes
```

KickとSnareの反復なども、次のようなPatternでPhraseとして表現できる。

```json
{
  "pattern": {
    "length": "1/1",
    "notes": [
      { "offset": "0/1", "duration": "1/8", "semitones": 0 },
      { "offset": "1/4", "duration": "1/8", "semitones": 0 }
    ]
  },
  "placements": [{ "position": "1:1", "anchor": "C1", "repeats": 4 }]
}
```

### Undo / Redo

履歴はプロセス内にあるため、ワンショットではなく `--interactive` の連続要求として送る。

```powershell
riffra --data-root ./riffra-data --interactive
```

```json
{"requestId":"u1","command":"track.add","params":{"name":"Bass","kind":"instrument"}}
{"requestId":"u2","command":"undo","expectedSequence":1,"params":{}}
{"requestId":"u3","command":"redo","expectedSequence":0,"params":{}}
{"requestId":"u4","command":"history.get","params":{}}
```

### Session と Timebase

| コマンド                  | 主要引数                                                                                          | 備考                                                                                                                                                                                          |
| ------------------------- | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `session inspect`         | [`--start <bar:beat>` `--end <bar:beat>`] [`--track-id <id>`]                                     | 軽量な構造Projection。全Track/Clip/Region/Harmony/Markerを返し、Note/Event/Automation Point/Plugin stateは展開しない。`project.ppq`に正準Timeline PPQを返す。`--start`と`--end`は両方指定する |
| `session get`             | -                                                                                                 | CreativeSession 全体(MIDI ノート全件・録音詳細込み)とシーケンスを返す。応答は大きいため注意                                                                                                   |
| `session settings update` | `--project-name` `--master-db` `--loop-enabled` `--count-in-beats` `--metronome-enabled` `--note` | 指定した項目だけ更新                                                                                                                                                                          |
| `history get`             | -                                                                                                 | 履歴状態                                                                                                                                                                                      |
| `undo` / `redo`           | -                                                                                                 | `--interactive` 限定                                                                                                                                                                          |
| `timebase update`         | [`--bpm`] [`--time-signature-numerator`] [`--time-signature-denominator`]                         | 指定した項目だけ更新。PPQは固定値で外部から変更しない                                                                                                                                         |

### Track と入力 Routing

| コマンド                     | 主要引数                                                                                          |
| ---------------------------- | ------------------------------------------------------------------------------------------------- |
| `track list`                 | -                                                                                                 |
| `track add`                  | `--name` `--kind audio\|instrument`                                                               |
| `track update`               | `--track-id` + `--name` `--gain-db` `--pan` `--muted` `--solo` `--armed` `--monitoring` `--color` |
| `track remove` / `duplicate` | `--track-id`                                                                                      |
| `track reorder`              | `--track-id` `--target-index`                                                                     |
| `track audio-input set`      | `--track-id` `--channel-index`                                                                    |
| `track midi-input set`       | `--track-id` [`--device-id`] [`--channel`] (1〜16)                                                |

`audio-input clear` / `midi-input clear --track-id` で解除する。

### Clip

| コマンド                             | 主要引数                                                                                                                                         |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `audio-clip list` / `midi-clip list` | -                                                                                                                                                |
| `audio-clip add-asset`               | `--asset-id` `--name` [`--start-tick`] [`--track-id`]                                                                                            |
| `midi-clip create`                   | `--track-id` `--start-tick` `--duration-ticks` [`--name`]                                                                                        |
| `midi-clip add-asset`                | `--asset-id` `--name` [`--start-tick`] [`--track-id`]                                                                                            |
| `... update`                         | `--clip-id` + 個別フラグ、または `--patch '<JSON>'`                                                                                              |
| `... move`                           | `--clip-id` `--start-tick` `--track-id`                                                                                                          |
| `audio-clip trim`                    | `--clip-id` `--start-tick` `--source-start` `--source-end`                                                                                       |
| `midi-clip trim`                     | `--clip-id` `--start-tick` `--duration-ticks`                                                                                                    |
| `... split`                          | `--clip-id` `--split-tick`                                                                                                                       |
| `... duplicate`                      | `--clip-id`                                                                                                                                      |
| `audio-clip crossfade`               | `--first-clip-id` `--second-clip-id`                                                                                                             |
| `clip paste` / `clip remove`         | `--audio-clip-ids a,b` / `--audio-clip-ids-json '[...]'` `--midi-clip-ids c,d` / `--midi-clip-ids-json '[...]'` (`paste` は `--start-tick` 追加) |

Audio Clip の trim における `--source-start` / `--source-end` は Asset 内のフレーム位置である。

複数 ID はコンマ区切り形式と JSON 形式を選べる。recording-slot 由来など Windows パスを含む ID は、ファイルから JSON を渡すと安全である。

```powershell
$ids = Get-Content -Raw .\midi-clip-ids.json
riffra --data-root ./riffra-data clip remove --midi-clip-ids-json $ids
```

### ミックス

ミックスの編集コマンドと値の範囲は次のとおり。

| 項目                  | コマンドと主な引数                                       | 値の範囲                                 |
| --------------------- | -------------------------------------------------------- | ---------------------------------------- |
| Track Gain            | `track update --track-id <id> --gain-db <dB>`            | -90..24 dB。範囲外はclampされる          |
| Track Pan             | `track update --pan <-1.0..1.0>`                         | -1.0(左)..1.0(右)。範囲外はclampされる   |
| Track Mute / Solo     | `track update --muted <bool>` / `--solo <bool>`          | true / false                             |
| Master                | `session settings update --master-db <dB>`               | -90..0 dB。範囲外はclamp、非有限値は失敗 |
| Audio Clip Gain / Pan | `audio-clip update --clip-id <id> --gain-db` / `--pan`   | Trackと同じ範囲でclampされる             |
| Clip Mute             | `audio-clip update --muted` / `midi-clip update --muted` | true / false                             |
| Effect Bypass         | `device bypass --bypassed <bool>`(既定はfalse)           | true / false                             |

- `track update`は指定した項目だけを更新する
- 1つでもSoloが有効なTrackがあるとSolo以外のTrackは無音になり、MuteとSoloが両方有効なTrackも無音になる。Clip MuteはTrackのMute/Soloとは独立にそのClipだけを消す。Soloの解除は、Soloを立てた各Trackへ`--solo false`を渡して行う
- RenderはTrackとClipのGain、Pan、Mute、Solo、Automation、Masterをすべて反映する。`render start --track-id`のstem Renderは対象Track以外をmutedにした状態で書き出す。`--normalize true`はMaster Gain適用後のピーク正規化であり、ミックス内の相対バランスは保たれる

#### Automation(volume / pan)

TrackのvolumeとpanをTimeline上で制御する。`points`の配列で該当するTrackのレーン全体を置き換え、空配列を渡すとレーンが削除される。`automation clear`は空配列での置換と同じである。

```powershell
riffra --attach automation set `
  --track-id track:01j... --parameter volume `
  --points-json '[{"id":"v1","tick":0,"value":-6},{"id":"v2","tick":3840,"value":0}]'

riffra --attach automation set `
  --track-id track:01j... --parameter pan `
  --points-json '[{"id":"p1","tick":3840,"value":-0.5},{"id":"p2","tick":7680,"value":0.5}]'

riffra --attach automation clear --track-id track:01j... --parameter volume
```

- `value`の意味は`parameter`で決まる。`volume`はdB(-90..24へclamp)、`pan`は-1.0..1.0へclampされる。0..1の正規化値ではない
- 各点は`id`(レーン内で一意、空は不可)、`tick`(u64)、`value`を持つ。同一tickの重複は失敗し、点はtick順に正規化される。1レーンの上限は16,384点である
- `tick`はTimeline上の絶対位置である。`bar:beat`からの変換は`session inspect`で得た`ppq`と拍子を用いて行い、PPQを固定値として扱わない
- レーンIDは`automation:{trackId}:{volume|pan}`で採番され、新規作成時は`createdEntityIds.automationLanes`に入る
- 非空のレーンがある区間では、volumeとpanはTrackの静的な`--gain-db`/`--pan`を置き換える。点の範囲外では静的な値へ戻る。Mute/SoloはAutomationと独立にTrack出力の可否を決める

### 低レベル MIDI Note

| コマンド                           | 主要引数                                                                                                   |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `midi-note add`                    | `--clip-id` `--pitch` `--start-tick` `--duration-ticks` `--velocity` `--channel` (1〜16)                   |
| `midi-note insert`                 | `--clip-id` `--notes-json '[{"pitch":60,"startTick":0,"durationTicks":480,"velocity":100,"channel":1}]'`   |
| `midi-note update`                 | `--clip-id` `--note-id` `--patch '{"note":64,"velocity":90}'`                                              |
| `midi-note update-many`            | `--clip-id` `--updates-json '[{"noteId":"...","patch":{...}}]'`                                            |
| `midi-note remove` / `remove-many` | `--clip-id` + `--note-id` / `--note-ids a,b,c` / `--note-ids-json '[...]'`                                 |
| `midi-note clear`                  | `--clip-id`                                                                                                |
| `midi-note quantize`               | `--clip-id` `--note-ids a,b,c` / `--note-ids-json '[...]'` `--grid-ticks 240`                              |
| `midi-note transform`              | `--clip-id` `--note-ids a,b,c` / `--note-ids-json '[...]'` [`--transpose-semitones`] [`--velocity-offset`] |
| `midi-note duplicate`              | `--clip-id` `--note-ids a,b,c` / `--note-ids-json '[...]'` `--offset-ticks 3840`                           |

通常の音符の作成・配置には `music midi-clip` と `music note` を使い、この節は既知のNote IDやraw tick、CC・Pitch Bendなどの直接編集向けである。

### Marker と Range

| コマンド          | 主要引数                                                          |
| ----------------- | ----------------------------------------------------------------- |
| `marker add`      | `--name` `--position <bar:beat>`                                  |
| `marker update`   | `--marker-id` [`--name`] [`--position <bar:beat>`]                |
| `marker remove`   | `--marker-id`                                                     |
| `loop-range set`  | [`--enabled true\|false`] `--start <bar:beat>` `--end <bar:beat>` |
| `punch-range set` | [`--enabled true\|false`] `--start <bar:beat>` `--end <bar:beat>` |

```powershell
riffra --attach marker add --name Chorus --position 17:1
riffra --attach loop-range set --start 17:1 --end 25:1 --enabled true
riffra --attach punch-range set --start 9:1 --end 13:1 --enabled true
```

### Asset と Project

| コマンド            | 主要引数            | 備考                                               |
| ------------------- | ------------------- | -------------------------------------------------- |
| `asset import-midi` | `<path>` [`--name`] | SMF を正準 MIDI Asset へ取り込み、`assetId` を返す |
| `project export`    | -                   | DataRoot へ Project package を書き出す             |
| `project import`    | `<path>`            | Project package からセッションを置き換える         |

取り込んだ Asset の配置は `audio-clip add-asset` / `midi-clip add-asset` で行う。

### Rack 状態(Instrument / Effect / Device)

| コマンド                | 主要引数                                                                                       |
| ----------------------- | ---------------------------------------------------------------------------------------------- |
| `instrument list`       | -                                                                                              |
| `instrument apply`      | `--track-id` `--instrument-id`（catalogの`id`。Built-inは`builtin:<presetId>`）                |
| `plugin instrument`     | `--track-id` `--plugin-path`(VST3 パス)                                                        |
| `plugin effect`         | `--track-id` `--plugin-path`                                                                   |
| `instrument clear`      | `--track-id`                                                                                   |
| `effect remove`         | `--track-id` `--device-id`                                                                     |
| `effect reorder`        | `--track-id` `--device-ids a,b,c` または `--device-ids-json '[...]'`(チェーン順に全 ID を列挙) |
| `device bypass`         | `--track-id` `--device-id` [`--bypassed true\|false`]                                          |
| `device inspect`        | `--track-id` `--device-id`                                                                     |
| `device parameter list` | `--track-id` `--device-id`                                                                     |
| `device parameter get`  | `--track-id` `--device-id` `--parameter-index`                                                 |
| `device parameter set`  | `--track-id` `--device-id` `--parameter-index` `--value`                                       |

パスだけを登録し実体のロードは Runtime が行うため、VST3 が無い環境でも安全に実行できる。

device inspectはmetadataとcapabilityだけを返し、stateData本体や全parameter配列を返さない。Built-in instrumentはparameter、state、preset、editorをサポートしない。VST3のparameter list/getはHostから取得できる範囲のindex/valueを返し、parameter名が公開されない場合は推測しない。

```powershell
riffra --attach plugin state save --track-id track:01j... --device-id device:01j... --output ./piano-state.json
riffra --attach plugin state load --track-id track:01j... --device-id device:01j... --file ./piano-state.json
riffra --attach plugin preset list --track-id track:01j... --device-id device:01j...
riffra --attach plugin preset get --track-id track:01j... --device-id device:01j...
riffra --attach plugin preset set --track-id track:01j... --device-id device:01j... --preset-index 2
```

Plugin presetはHostへ公開されたprogramだけを対象とし、Plugin固有GUIのpreset browserは対象外である。Plugin state fileにはschema version、Plugin path、parameter values、opaque stateを含め、別VST3のstateは適用しない。

`instrument list`は同梱のBuilt-in instrumentとUser Instrumentのcatalogを返し、各エントリの`id`が`instrument apply`への入力になる。割り当て後の`track list`は`instrument`として`name`と`source`（`internal`）を返す。Built-in instrumentの割り当てはSafe Modeでも実行できる。

### Missing 復旧

| コマンド                 | 主要引数                   |
| ------------------------ | -------------------------- |
| `missing relink`         | `--asset-id` `--new-path`  |
| `missing disable-plugin` | `--device-id`              |
| `missing replace-plugin` | `--device-id` `--new-path` |

欠落の一覧表示(`missing list`)は Live Host 専用である。

## Runtime サービス(Live Host 必須)

### Host の起動

```powershell
# 通常モード(Native audio engine を使用)
cargo run -p riffra-cli -- --data-root ./riffra-data serve

# Safe Mode(音声・MIDI・外部プラグインをオフライン化し Host だけを起動)
cargo run -p riffra-cli -- --data-root ./riffra-data serve --safe-mode
```

- `serve` は終了シグナル(SIGINT / SIGTERM)を受けるまでフォアグラウンドで動作する。エージェントからはバックグラウンド起動し、`<data_root>/control/host.json` の出現を起動準備の目安にしたうえで、実際の利用可否はhandshakeで確認する。起動診断は標準エラーへ出る(`riffra serve ready: ...`)
- 通常モードは Native audio engine サイドカー(`riffra-audio`)を実行ファイルと同じ `target/debug/` か `target/release/` から自動解決する。無ければ `native/audio-engine/build.ps1`(Windows)/ `build.sh`(Linux・macOS)でビルドする。Safe Mode ではサイドカー不要
- Linux の通常モードは ALSA 入出力デバイスを必要とする。デバイスを開けない環境では Host は起動できても Runtime が Ready にならない(`--attach audio status` で確認)

### 接続と終了

```powershell
riffra --attach host status
riffra --attach session get
riffra --attach --interactive   # 1 接続で連続要求
riffra --attach host shutdown
```

- DataRootの排他所有はHostが持つため、`--attach` でDataRootを開き直すことはない
- Host の停止は `host shutdown`、またはプロセスへの SIGINT / SIGTERM

### Safe Mode の範囲

正準状態の編集は Safe Mode でもそのまま使える。Runtime 系コマンドのうち、次のグループが `runtimeUnavailable` になる。

| 不可                                                                          | 可能                                                          |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------- |
| transport 全般(play / stop / go-to-start / seek)                              | `audio status` / `audio diagnostics` / `audio driver get`     |
| `audio probe` / `channels-probe` / `recover` / `startup-retry` / `driver set` | `plugin catalog list` / `missing list`                        |
| `midi send` / `midi panic` / `asset preview`                                  | `render start` と job 管理                                    |
| `plugin scan` / `scan-start` / `record start`                                 | `record list` / `status` などの管理系、`library` / `analysis` |

### Transport 制御

| コマンド                | 主要引数   |
| ----------------------- | ---------- |
| `transport play`        | -          |
| `transport stop`        | -          |
| `transport go-to-start` | -          |
| `transport seek`        | `--tick N` |

### Audio デバイス

| コマンド                          | 主要引数                                                                                                           |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `audio status`                    | -                                                                                                                  |
| `audio diagnostics`               | [`--json`] [`--debug`]                                                                                             |
| `audio probe`                     | -                                                                                                                  |
| `audio channels-probe`            | `--driver` `--input-device` `--output-device`                                                                      |
| `audio driver get` / `driver set` | `set` は `--driver` [`--input-device`] [`--input-channel`] [`--output-device`] [`--sample-rate`] [`--buffer-size`] |
| `audio recover` / `startup-retry` | -                                                                                                                  |

デバイス異常からの回復は `status` で状態(faulted 等)を確認し、`recover`、改善しなければ `startup-retry` の順で試す。

`audio diagnostics` はデバイス、Safetyのミュート理由、Realtime負荷、出力状態、InstrumentごとのfaultとMIDI dropをまとめて返す読み取り専用コマンドである。通常は人間向けの表示になり、`--json` を付けるとControl responseの外側を除いた診断オブジェクトだけをJSONで出力する。`--debug` はASIO切替などの調査に必要なProjectionとGraphの内部情報を追加するが、安定した公開項目ではない。

```powershell
riffra --attach audio diagnostics
riffra --attach audio diagnostics --json
riffra --attach audio diagnostics --json --debug
```

### 録音

```powershell
riffra --attach record start
riffra --attach record status
riffra --attach record stop
riffra --attach record list
riffra --attach record promote --id rec:01j...
```

| コマンド                                | 主要引数                    |
| --------------------------------------- | --------------------------- |
| `record start` / `another-take`         | [`--recording-session-id`]  |
| `record stop` / `status` / `duplicates` | -                           |
| `record list`                           | [`--query`]                 |
| `record rename`                         | `--id` `--new-name`         |
| `record archive` / `promote` / `delete` | `--id`                      |
| `record tag`                            | `--id` [`--tag`] [`--note`] |

キャプチャは DataRoot の `recordings/` 配下に置かれ、promote により正準セッションへ反映される。録音対象は arm 済み(`track update --armed true`)の Track であり、1 つも無ければ開始は失敗する。

### レンダリング(非同期ジョブ)

```powershell
riffra --attach --expected-sequence 43 render start --range loop-range --normalize true
riffra --attach --expected-sequence 43 render start --start 9:1 --end 13:1 --track-id track:01j...
riffra --attach job get --id job:01j...
riffra --attach job wait --id job:01j...
riffra --attach job wait --id job:01j... --timeout-ms 30000
riffra --attach job cancel --id job:01j...
```

- `render start` は `--range entire-arrangement` (既定) または `--range loop-range` を指定できる。音楽座標の部分Renderは `--start <bar:beat> --end <bar:beat>` を両方指定し、`--track-id` と併用できる。[`--normalize true|false`] も指定できる。`--range loop-range` と `--start` / `--end` は併用しない
- `render start` の `--expected-sequence` はRender対象のCanonical snapshotを固定する。ConflictならWAVを作成せず、最新状態をInspectしてからRenderし直す
- 応答は `type: "job"` のジョブ ID。ワンショットAttached CLIの`job wait`は`--timeout-ms`を指定しなければterminal stateまで待ち、指定した場合だけその時間で打ち切る。内部では既存の`job.get`を繰り返し呼び、`completed`、`failed`、`cancelled`のいずれかになった時点の結果を返す。interactiveでは`job.get`を呼び出し側で繰り返し、進行中の停止には`job cancel`を使う。Control ProtocolでJobを操作するコマンドは`job.get`と`job.cancel`である
- Runtimeから意味のある進捗率を取得できない間の`progress`は`null`である
- 出力は `renders/render-{ms}/timeline.wav` と manifest として書き出される

`plugin scan-start` も同様に非同期ジョブ(`job get` で追跡)である。同期版の `plugin scan` は完了まで応答を返さない。

### Library・解析・Preview

| コマンド                   | 主要引数                                                                     |
| -------------------------- | ---------------------------------------------------------------------------- |
| `library search`           | `--query`(短い語は全 Asset がヒットして応答が膨らむので具体語で絞る)         |
| `library asset-update`     | `--id` [`--tag`] [`--note`]                                                  |
| `library related`          | `--id`                                                                       |
| `analysis start`           | `--asset-id` または `--path`(どちらか必須)                                   |
| `asset preview`            | `--asset-id` [`--start-ms`] [`--end-ms`] [`--looped true\|false`] [`--gain`] |
| `asset stop-preview`       | -                                                                            |
| `runtime projection get`   | -                                                                            |
| `runtime projection retry` | -                                                                            |

### Plugin カタログと欠落一覧

| コマンド                     | 主要引数   |
| ---------------------------- | ---------- |
| `plugin catalog list`        | -          |
| `plugin scan` / `scan-start` | [`--path`] |
| `missing list`               | -          |

## Windowsの入出力

ワンショットのJSON、interactive JSONL、Attached responseを標準出力へ書き出すバイト列はUTF-8である。UTF-8に対応したツールでは日本語をそのまま復元できる。PowerShell 5.1など親Shellがパイプ経由で別のコードページへ変換した後の表示はRiffraのProtocol保証に含まれない。複雑な構造JSONは`--*-file`またはinteractive JSONLで渡す。
