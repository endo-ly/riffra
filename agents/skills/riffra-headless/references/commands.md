# riffra コマンドガイド

実行モード・DataRoot・プロトコルの基礎は [SKILL.md](../SKILL.md) を参照。

コマンドの引数・値域・排他規則は `riffra <command> --help` が正本である。本書は、コマンドの選び方、構造入力の書式、応答の読み方など help から読み取れない契約をまとめる。

コマンドは「正準状態の編集」と「Runtime サービス」の 2 系統しかない。

| 系統             | 主なコマンド                                                                                               | 実行できる場所                 |
| ---------------- | ---------------------------------------------------------------------------------------------------------- | ------------------------------ |
| 正準状態の編集   | session / track / music / clip / midi-note / marker / automation / instrument / effect / missing 復旧      | すべての実行形態               |
| Runtime サービス | transport / audio / midi 送信 / record / render / job / library / plugin 実体の操作 / device parameter set | Live Host(`serve`)+ `--attach` |

Live Host が必要なコマンドを Standalone で実行すると、引数の内容にかかわらず `runtimeUnavailable` になる。

## 正準状態の編集

### 基本サイクル

1. `session inspect` で現在の構造と `sequence` を把握する。`session get` は MIDI ノート全件や録音の詳細を含むフルスナップショットなので、Inspectにない詳細が必要なときだけ使う
2. 編集コマンドを実行する。Inspectまたは直前の応答の `sequence` を `--expected-sequence` に渡す
3. 編集後は同じ範囲を `session inspect` し、音を確認するときは確認後の `sequence` を指定した `render start` を行い、完了を `job wait`(ワンショット)または `job get`(interactive)で待つ。採用しない変更は変更後の `sequence` を指定した `undo` を実行して再Inspectする
4. `conflict` になったMutation・Render・Undo・Redoは自動再送せず、最新状態をInspectして内容を決め直す

```powershell
riffra --attach session inspect
riffra --attach session inspect --start 9:1 --end 13:1 --track-id track:01j...

# Track追加の応答からIDを取得し、後続の要求に渡す
riffra --attach --expected-sequence 0 track add --name Drums --kind audio
riffra --attach session inspect

# MIDI Clip 作成 → 再Inspectで状態を確認して音楽上のNoteを積む
riffra --attach --expected-sequence 1 music midi-clip create --track-id track:01j... --start 5:1 --end 13:1 --name Piano
riffra --attach session inspect --track-id track:01j...
riffra --attach --expected-sequence 2 music note insert --clip-id midi-clip:01j... --notes-json '[{"pitch":"C4","position":"5:1","duration":"1/8"}]'
```

生成されたIDはMutation応答の `createdEntityIds` へ種類ごとのキー(`tracks`、`midiClips`、`devices` など)で返る。キーは応答から読み、推測して使わない。

### 複数操作の一括適用

複数の正準Mutationを事前に決めている場合は、CLI側でIDを受け渡すスクリプトを書かずに `session apply` へJSONLを渡す。Batchの適用規則、`trackName` / `clipName` の名前解決、含められるコマンドの一覧は `riffra session apply --help` が正本である。Mixer周りも `busName`、`returnBusName`、ownerの `owner.name` で指定でき、同じBatch内で追加したTrackとBusを参照できる。

`--replace` はProjectの同一性を保ち、Sessionを初期状態に戻してからJSONLを適用する。録音素材ファイルは残る。

```powershell
riffra --data-root ./riffra-data --expected-sequence 42 session apply --file ./song.jsonl
riffra --attach session apply --file ./song.jsonl --include-created-ids
```

```jsonl
{"command":"track.add","params":{"name":"Lead","kind":"instrument"}}
{"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"9:1"}}
{"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"C4","position":"1:1","duration":"1/8"}]}}
```

既存ClipのNoteだけをまとめて置き換えるには、音楽座標なら同じBatchに `midi-note.clear` と `music.note.insert` をこの順で含める。raw MIDI値でよければ `midi-clip.update` の `patch.notes` へNote配列を渡す。

Protocol上のparamsキーはCLIフラグと対応する(`--notes-json` など → `notes`、`session apply --file` → `operations`)。`music phrase insert` だけは `--phrase-json` の内容が `pattern` と `placements` の2つのparamsへ分かれる。

失敗時は `error.details.operationIndex` と `command` でoperationを特定でき、JSONL入力では物理行番号が `inputLine` に入る。paramsの検証エラーにはJSON Pointer形式の `path`、配列要素の `index`、問題の値が付く。

### 音楽操作

NoteとMIDI Clipの通常編集には `music note` / `music midi-clip` を使う。`position` はClip内部の相対位置ではなくArrangement全体の絶対位置である。複数Noteは1回の `music note insert` で渡す。

```powershell
riffra --attach music note list --track-id track:drums --start 5:1 --end 13:1
riffra --attach --expected-sequence 20 music note transform `
  --track-id track:drums --start 5:1 --end 13:1 `
  --pitch D2 --timing-offset +1/48 --velocity-offset 4
```

Regionは`Intro`や`Verse`などの自由な名前を持つ時間範囲であり、重複・入れ子・同名を許可する。

和声は一般的なChord Symbolを `music harmony insert` へ渡す。parserで表現できない音集合は、`chord` の代わりに `pitches`、任意の `root` / `bass`、`label` をイベントJSONへ渡す。

#### Phrase と Rhythm

`PhrasePattern` と `RhythmPattern` は操作入力であり、正準セッションへ二重保存しない。Patternの時間は2種類で、`position` / `placements[].position` は `bar:beat` または `bar:beat+fraction-of-beat`、`length`、`offset`、`duration` は全音符を1とする音価の分数(`1/8` は8分音符)である。

```powershell
riffra --data-root ./riffra-data music phrase insert --clip-id midi-clip:01j... `
  --phrase-json '{"pattern":{"length":"1/1","notes":[{"offset":"0/1","duration":"1/8","semitones":0},{"offset":"1/4","duration":"1/8","semitones":5}]},"placements":[{"position":"5:1","anchor":"C4","repeats":2}]}'

riffra --data-root ./riffra-data music harmony realize --clip-id midi-clip:01j... `
  --start 1:1 --end 5:1 --rhythm-json '{"length":"1/2","steps":[{"offset":"0/1","duration":"1/8"},{"offset":"1/4","duration":"1/16","velocity":112}]}'
```

同じ音高を繰り返すKickやSnareのドラムパターンも、`semitones` を揃えたPatternでPhraseとして表現できる。挿入前に展開規模を確認するときは `music phrase preview` を使う。

### Session・Timebase・Mixdown

Timebaseのテンポ・拍子は `timebase update` で変更し、変更点全体は `timebase get-map` / `set-map` で扱う。

Mixdown設定は `mixdown get` / `mixdown set --settings-json` で参照・置換する。Masteringは目標ラウドネスへMix全体を一定のゲインで合わせ、True Peak上限を超えるときは上限に合わせてゲインを下げる。`mastering.deviation.integratedLufs` は最終ラウドネス − 目標で、負なら目標に届いていない。`mastering.truePeakCorrectionDb` は True Peak上限のために下げたゲインで、0より大きければ上限で止まったことを示す。

```powershell
riffra --attach mixdown set --settings-json '{"musicalEndTick":0,"tailSeconds":2,"fadeOutSeconds":0.5,"mastering":{"integratedLufs":-14,"truePeakDb":-1,"loudnessRangeLu":11},"sampleRate":48000,"blockSize":1024}'
```

### Track・Clip・ミックス

Track・Clip・Marker・Rangeの編集コマンドの引数は `--help` を参照。複数IDはコンマ区切りとJSON形式のどちらでも渡せる。recording-slot由来などWindowsパスを含むIDは、コンマ区切りではなくJSON形式(`--*-ids-json`、またはfile)で渡すと安全である。

ミックスの正本は正準状態であり、Gain / VolumeはdB、Panは-1.0(左)..1.0(右)で指定する(Solo / Muteの挙動と値域は `track update --help` を参照)。RenderはTrackとClipのGain、Pan、Mute、Solo、Automation、Masterをすべて反映する。

Group・Return・Send・Sidechainの正準設定は `mixer get` でまとめて参照する。Sidechainは別Track・Group・Return・Master上のEffect Deviceを指定し、Live Hostの `device inspect` で `capabilities.sidechain` を確認してから接続する。Instrument Deviceと送信元自身のTrack上のEffectは指定しない。Source TrackのMuteはSidechainも止め、Soloで主出力が分離されてもSidechainは維持する。Processed Variantに必要な制御信号が録音から再利用できない場合はRaw Takeを保持し、`record list` の `processedUnavailableReason` で理由を確認できる。

### Automation

`automation set` は指定対象のLane全体を置き換える。Pointは `id`、`tick`、`value` を持ち、Tickは楽曲全体の絶対位置であり、音楽座標からの変換には `session inspect` の `project.ppq` と拍子を使う。値の単位、値域、一意性と上限は[データモデル](../../../../docs/data-model.md)に従う。Deviceの `value` は正規化数値、またはPluginの表示文字列(例:`"250 ms"`)を指定できる。文字列の変換にはLive Hostが必要で、変換できない場合は保存しない。

更新はLane IDを維持し、新規LaneのIDは `createdEntityIds.automationLanes` へ返る。空のPoint配列と `clear` はLaneを削除する。

`session apply` へ `automation.set` を含める場合は、対象をCLI引数ではなく `target` オブジェクトで指定する。Pointは `tick` または `position` の一方だけを持ち、`position` はそのoperationの時点のテンポ・拍子からTickへ変換する。

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

### 音源制御イベント

`instrument-event` はMIDI Clip内の音源制御(`sustainPedal`、`pitchBend`、`modWheel`、`aftertouch`、`parameterChange`)を編集する。Parameter Changeは `parameter` と `nativeValue` を保持する。

### 低レベル MIDI Note

`midi-note` は、量子化・変形・複製など既知のNote IDをraw tick・MIDI値で直接編集するときだけ使う。通常のNote操作は `music note` を使う(SKILL.md「楽曲制作の入力契約」)。

### Asset と Project

取り込んだMIDI Asset(`asset import-midi`)の配置は `audio-clip add-asset` / `midi-clip add-asset` で行う。

### Channel Device (Instrument / Effect)

EffectとDeviceのOwnerは `--track-id`、`--bus-id`、`--master` のいずれか1つで指定する。Master・BusにはEffectsだけを追加できる。

`--plugin-id` は現在のDataRootのCatalogにあるValidated・Role一致のPluginへ解決され、Projectには解決後のパスが保存される。IDはインストールパスに依存するため、別PCへ持ち運べる識別子ではない。Standaloneではパスだけを登録し実体のロードはRuntimeが行うため、VST3が無い環境でも安全に実行できる。`device inspect`、`device parameter list/get/set`、`plugin state`、`plugin preset` は実体のPluginを扱うため Live Host 専用である。

Built-in instrumentはparameter、state、preset、editorをサポートしない。

`device parameter list` の主要フィールドは次のとおり。

| フィールド     | 意味                                                                          |
| -------------- | ----------------------------------------------------------------------------- |
| `parameterId`  | Automationへ指定するStable ID。一意で非空のIDを公開できない場合は`null`       |
| `index`        | set/get へ渡すParameter index                                                 |
| `name`         | Pluginが返したParameter名                                                     |
| `value`        | normalized 0..1 の現在値                                                      |
| `defaultValue` | Pluginのnormalized既定値                                                      |
| `displayValue` | Pluginが返した現在値の表示文字列                                              |
| `choices`      | 有限離散値が列挙できる場合の `{ value, displayValue }` 配列。それ以外は空配列 |
| `automatable`  | PluginのAutomation対応情報                                                    |

手動操作では `name` でParameterを特定し、離散値は `choices` の表示から目的の状態を選んでその `value` を渡す。Continuousの表示が空の場合も、値をPlugin固有の単位へ変換して推測しない。Automationは `automatable: true` かつ `parameterId` が非nullのParameterを選ぶ。Defaultへの復元には `defaultValue` を渡す。

`device parameter set` の応答は、`mutation`に正準変更、`parameter`に対象の読み戻し値、`changedParameters`に値が変わったほかのParameterをindex順で返す。`--display` はLive HostのPluginに文字列からの変換を依頼し、表示へ戻した結果が入力と等価な場合だけ適用する。`session apply` 内では正規化値だけを正準状態へ書き、`--display` は使えない。

`effect add` は Effect Chain の末尾へ追加し、応答の `createdEntityIds.devices` に新しい Device ID を返す。

Instrumentの割り当ては `instrument apply`(`builtin:<presetId>` または `user:<id>`)で行う。Built-in instrumentの割り当てはSafe Modeでも実行できる。Instrument毎のMIDI設定は `instrument midi set --settings-json` で置換し、`transposeSemitones`、`playableNotes`(`{"type":"range","start":36,"end":84}`、`{"type":"set","notes":[...]}`、制限なしの`null`)、`keyswitches`(`[{"note":24,"name":"C0"}]`)を持つ。

```powershell
riffra --attach instrument midi set --track-id track:01j... --settings-json '{"transposeSemitones":12,"playableNotes":{"type":"range","start":36,"end":84},"keyswitches":[{"note":24,"name":"C0"}]}'
```

User Instrumentは `instrument save` で登録し、応答のIDを割当へ渡す。定義から素材への参照を保つため、定義JSONと相対パスの素材を同じパッケージ構成で用意する。複数Trackへまとめて割り当てるときは、`session apply` に `instrument.apply` を含める。

### Missing 復旧

欠落AssetやPluginは `missing relink` / `disable-plugin` / `replace-plugin` で復旧する。欠落の一覧表示(`missing list`)は Live Host 専用である。

## Runtime サービス(Live Host 必須)

### Host の起動と終了

```powershell
# 通常モード(Native audio engine を使用)
riffra --data-root ./riffra-data serve

# Safe Mode(音声・MIDI・外部プラグインをオフライン化し Host だけを起動)
riffra --data-root ./riffra-data serve --safe-mode
```

`serve` は終了シグナル(SIGINT / SIGTERM)を受けるまでフォアグラウンドで動作する。エージェントからはバックグラウンド起動し、`riffra --attach host status` で稼働を確認してから使う。停止は `riffra --attach host shutdown`、またはプロセスへのSIGINT / SIGTERM。

通常モードは Native audio engine サイドカー(`riffra-audio`)を実行ファイルと同じ `target/` から自動解決する。無ければ `native/audio-engine/build.ps1`(Windows)/ `build.sh`(Linux・macOS)でビルドする。Safe Modeではサイドカー不要。Linuxの通常モードはALSA入出力デバイスを必要とし、デバイスを開けない環境ではHostは起動できてもRuntimeがReadyにならない(`riffra --attach audio status` で確認)。

### Safe Mode の範囲

正準状態の編集は Safe Mode でもそのまま使える。transport、live MIDI送信、`audio probe` 系、`audio driver set`、`plugin scan`、`record start`、`asset preview` は `runtimeUnavailable` になる(`serve --help` と各コマンドの `--help` に記載)。`render` とjob管理、`library`、`analysis`、`plugin catalog list`、`missing list`、`record list` などの管理系はSafe Modeでも使える。

### Audio と Transport

デバイス異常からの回復は `audio status` で状態(faulted等)を確認し、`recover`、改善しなければ `startup-retry` の順で試す。`audio diagnostics` はデバイス、Safetyのミュート理由、Realtime負荷、出力状態、InstrumentごとのfaultとMIDI dropをまとめて返す読み取り専用コマンドで、`--json` で機械可読、`--debug` で調査用の内部情報を追加する。

### 録音

録音対象はarm済み(`track update --armed true`)のTrackであり、1つも無ければ開始は失敗する。キャプチャはDataRootの `recordings/` 配下に置かれ、`record promote` により正準セッションへ反映される。

### レンダリング(非同期ジョブ)

`render start` の `--expected-sequence` はRender対象のCanonical snapshotを固定する。ConflictならWAVを作成せず、最新状態をInspectしてからRenderし直す。応答は `type: "job"` のジョブIDで、ワンショットは `job wait`(無指定ならterminal stateまで待機)、interactiveは `job get` の繰り返しで追跡し、停止には `job cancel` を使う。出力はDataRootの `renders/` 配下へ書き出される。`plugin scan-start` と `analysis measure` も同じ非同期ジョブである。`ok: true` は状態取得の成功を表し、Job自体の成功は `state: "completed"` で判断する。

Stem Render(`render stems`)は一度の要求で複数Trackを同じ時間基準のWAVへ書き出す。全WAVで開始Tick、終了Tick、Sample Rate、フレーム数を揃え、個別の正規化とMasteringは行わない。共有する非線形Effectでは、Stemの合計と全体Mixが一致しない場合がある。StemとMasteringの設計上の扱いは[音声エンジンの設計](../../../../docs/audio-engine.md)を参照。

Job結果と`render.json`の`measurements`には完成WAVのSample Peakとラウドネス測定を含む。通常Renderで測定器(FFmpeg)を利用できない場合は成功結果のラウドネス測定に`unavailable`と理由を返し、Masteringを要求したJobでの測定失敗はJob失敗になる。区間ごとの音量や帯域バランスの判断には`measurements.sections`を使う。

| 項目                               | 読み方                                                                              |
| ---------------------------------- | ----------------------------------------------------------------------------------- |
| `name` / `start` / `end`           | Marker名と区間の音楽座標                                                            |
| `loudness.integratedLufs`          | 区間の統合ラウドネス。有効な400msブロックがない区間では`null`                       |
| `loudness.truePeak`                | 最大True Peakの`db`・`seconds`・`position`。無音なら`null`。位置は100msフレーム単位 |
| `bands.lowDb` / `midDb` / `highDb` | 250Hz未満・250Hz〜4kHz・4kHz超のRMSレベル(dBFS)。無音なら`null`。帯域間比較の目安   |

区間の切り分けは書き出し時点の正準Markerで決まる。`measurements.sectionLoudnessState.state` が `measured` なら無音・短区間の値が `null` でも測定は正常に完了している。`unavailable` なら `reason` を確認する。全体と区間のラウドネスは独立して測定し、区間ラウドネスの測定が失敗してもWAVと帯域値は保持する。

### 保存済みWAVの区間測定

セクション間の音量差やステム間の帯域バランスを比較するときは、比較したい区間をまとめて `analysis measure` へ渡す。Render IDは `render list` の結果から取得する。

```json
{
  "renderId": "render-...",
  "ranges": [
    { "name": "Intro", "startSeconds": 0, "endSeconds": 8 },
    { "name": "Verse", "startSeconds": 8, "endSeconds": 24 }
  ]
}
```

外部WAVの比較では、上の入力の `renderId` を `"paths": ["C:/music/drums.wav", "C:/music/bass.wav"]` に置き換える。

```powershell
riffra --attach analysis measure --file measurements.json
riffra --attach job wait --id job:analysis-measure:0 --timeout-ms 120000
```

待機には開始応答のJob IDを使い、結果の `sources` のパス・Track情報を比較対象と照合する。区間True Peakの位置はWAV先頭からの秒数なので、プロジェクト上の位置と混同しない。帯域値・測定状態の読み方はRenderと共通である。

### Library・解析・Preview

`library search` はAsset metadataへのfree-text検索である(短い語は全Assetがヒットして応答が膨らむので具体語で絞る)。素材の解析は `analysis start`、試聴は `asset preview` / `stop-preview`、Runtime projectionの状態は `runtime projection get` / `retry` で扱う。

## Windows の入出力

CLIが標準出力へ書くJSONはUTF-8である。PowerShell 5.1など親Shellがパイプ経由で別のコードページへ変換した後の表示はRiffraのProtocol保証に含まれない。複雑な構造JSONは`--*-file`で渡す。
