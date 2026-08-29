# riffra コマンドリファレンス

実行モード・DataRoot・プロトコルの基礎は [SKILL.md](../SKILL.md) を参照。

コマンドは次の 2 系統しかない。4 つの実行形態(ワンショット / 対話 / serve / attach)は同じコマンドへの要求経路の違いであり、引数は共通である。

| 系統             | 主なコマンド                                                              | 実行できる場所                 |
| ---------------- | ------------------------------------------------------------------------- | ------------------------------ |
| 正準状態の編集   | session / track / music / clip / midi-note / marker / rack / missing 復旧 | すべての実行形態               |
| Runtime サービス | transport / audio / midi 送信 / record / render / job / library / plugin  | Live Host(`serve`)+ `--attach` |

## 正準状態の編集

### 基本サイクル

1. 現在状態を把握する。俯瞰は `track list` / `audio-clip list` / `midi-clip list` などの個別照会で行う。`session get` は MIDI ノート全件や録音の詳細を含むフルスナップショットなので、セッション全体が必要なときだけ使う
2. 編集コマンドを実行する。応答の `result.value` に CreativeSession(`type: "session"`)または CanonicalState(`type: "arrangementMutation"`)が返るので、対象 ID(`track:*` / `clip:*` / `note:*` / `marker:*`)を読む
3. 複数クライアントが並走しうる場合は、直前応答の `sequence` を次の `--expected-sequence` に渡す

```powershell
# 現状把握
riffra --data-root ./riffra-data session get

# Track 追加 → 応答 result.value.arrangement.tracks[*] から ID を得る
riffra --data-root ./riffra-data track add --name Drums --kind audio

# MIDI Clip 作成 → clip ID を得て音楽上のNoteを積む
riffra --data-root ./riffra-data music midi-clip create --track-id track:01j... --start 5:1 --end 13:1 --name Piano
riffra --data-root ./riffra-data music note insert --clip-id midi-clip:01j... --notes-json '[{"pitch":"C4","position":"5:1","duration":"1/8"}]'
```

通常の作曲では `music.*` の音楽表現を使う。低レベルの `midi-*` 操作だけがtickとMIDI pitch番号を受け取る。Timebaseのテンポ・拍子は `timebase update` で変更できる。MIDI channel は1〜16の範囲で指定する。

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

### Undo / Redo

履歴はプロセス内にあるため、ワンショットではなく `--interactive` の連続要求として送る。

```powershell
riffra --data-root ./riffra-data --interactive
```

```json
{"requestId":"u1","command":"track.add","params":{"name":"Bass","kind":"instrument"}}
{"requestId":"u2","command":"undo","params":{}}
{"requestId":"u3","command":"redo","params":{}}
{"requestId":"u4","command":"history.get","params":{}}
```

以下のコマンドはすべての実行形態で同じ引数で使える。稼働中の Desktop や Live Host を相手にするときは `--attach` を付ける。引数はロングフラグ(camelCase を kebab-case 化)で渡し、完全な一覧は `cargo run -p riffra-cli -- <command> --help` で確認できる。

### Session と Timebase

| コマンド                  | 主要引数                                                                                          | 備考                                                                                        |
| ------------------------- | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `session get`             | -                                                                                                 | CreativeSession 全体(MIDI ノート全件・録音詳細込み)とシーケンスを返す。応答は大きいため注意 |
| `session settings update` | `--project-name` `--master-db` `--loop-enabled` `--count-in-beats` `--metronome-enabled` `--note` | 指定した項目だけ更新                                                                        |
| `history get`             | -                                                                                                 | 履歴状態                                                                                    |
| `undo` / `redo`           | -                                                                                                 | `--interactive` 限定                                                                        |
| `timebase update`         | [`--bpm`] [`--time-signature-numerator`] [`--time-signature-denominator`]                         | 指定した項目だけ更新。PPQは固定値で外部から変更しない                                       |

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

低レベルMIDI操作は、CC・Pitch Bendなどを含むイベントを直接編集する場合に使う。通常の音符の作成・配置には `music midi-clip` と `music note` を使う。

### Marker・Range・Automation

| コマンド           | 主要引数                                                                                    |
| ------------------ | ------------------------------------------------------------------------------------------- |
| `marker add`       | `--name` `--tick`                                                                           |
| `marker update`    | `--marker-id` [`--name`] [`--tick`]                                                         |
| `marker remove`    | `--marker-id`                                                                               |
| `loop-range set`   | [`--enabled true\|false`] `--start-tick` `--end-tick`                                       |
| `punch-range set`  | [`--enabled true\|false`] `--start-tick` `--end-tick`                                       |
| `automation set`   | `--track-id` `--parameter volume\|pan` `--points-json '[{"id":"p1","tick":0,"value":0.8}]'` |
| `automation clear` | `--track-id` `--parameter volume\|pan`                                                      |

Automation の points 配列は既存ポイントを置き換える。各要素は `id`・`tick`・`value` を持つ。

### Asset と Project

| コマンド            | 主要引数            | 備考                                               |
| ------------------- | ------------------- | -------------------------------------------------- |
| `asset import-midi` | `<path>` [`--name`] | SMF を正準 MIDI Asset へ取り込み、`assetId` を返す |
| `project export`    | -                   | DataRoot へ Project package を書き出す             |
| `project import`    | `<path>`            | Project package からセッションを置き換える         |

取り込んだ Asset の配置は `audio-clip add-asset` / `midi-clip add-asset` で行う。

### Rack 状態(Instrument / Effect / Device)

| コマンド               | 主要引数                                                                                       |
| ---------------------- | ---------------------------------------------------------------------------------------------- |
| `plugin instrument`    | `--track-id` `--plugin-path`(VST3 パス)                                                        |
| `plugin effect`        | `--track-id` `--plugin-path`                                                                   |
| `instrument clear`     | `--track-id`                                                                                   |
| `effect remove`        | `--track-id` `--device-id`                                                                     |
| `effect reorder`       | `--track-id` `--device-ids a,b,c` または `--device-ids-json '[...]'`(チェーン順に全 ID を列挙) |
| `device bypass`        | `--track-id` `--device-id` [`--bypassed true\|false`]                                          |
| `device parameter-set` | `--track-id` `--device-id` `--parameter-index` `--value`                                       |

パスだけを登録し実体のロードは Runtime が行うため、VST3 が無い環境でも安全に実行できる。

### Missing 復旧

| コマンド                 | 主要引数                   |
| ------------------------ | -------------------------- |
| `missing relink`         | `--asset-id` `--new-path`  |
| `missing disable-plugin` | `--device-id`              |
| `missing replace-plugin` | `--device-id` `--new-path` |

欠落の一覧表示(`missing list`)は Live Host 専用である。

## Runtime サービス(Live Host 必須)

再生・録音・レンダリング・プラグインなど Audio Runtime を伴う操作は、`serve` で Live Host を起動し `--attach` で接続して行う。正準状態の編集は Safe Mode でもそのまま使える。

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
riffra --data-root ./riffra-data --attach host status
riffra --data-root ./riffra-data --attach session get
riffra --data-root ./riffra-data --attach --interactive   # 1 接続で連続要求
riffra --data-root ./riffra-data --attach host shutdown
```

- `--attach` は `control/host.json` を読み、handshake(`instanceId` と `pid` 一致)後に要求を転送する。DataRoot の排他所有は Host が持つため、attach 側が開き直すことはない
- 初期状態を取得するprotocol clientはevents connectionを先に確立し、command connectionで`host.bootstrap`を要求する。bootstrap中のeventは受信順に適用する
- 接続できない場合は `hostUnavailable`。Standalone への自動フォールバックはないので、Host の生存を確認してから再試行する
- Host の停止は `host shutdown`、またはプロセスへの SIGINT / SIGTERM

### Safe Mode の範囲

Runtime 系コマンドのうち、次のグループが `runtimeUnavailable` になる。

| 不可                                                                          | 可能                                                          |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------- |
| transport 全般(play / stop / go-to-start / seek)                              | `audio status` / `audio driver get`                           |
| `audio probe` / `channels-probe` / `recover` / `startup-retry` / `driver set` | `plugin catalog list` / `missing list`                        |
| `midi send` / `midi panic` / `asset preview`                                  | `render start` と job 管理                                    |
| `plugin scan` / `scan-start` / `record start`                                 | `record list` / `status` などの管理系、`library` / `analysis` |

### Transport 制御

| コマンド                | 主要引数                 |
| ----------------------- | ------------------------ |
| `transport play`        | `--transport-sequence N` |
| `transport stop`        | `--transport-sequence N` |
| `transport go-to-start` | `--transport-sequence N` |
| `transport seek`        | `--tick N`               |

`--transport-sequence` はトランスポート要求の順序を示す番号で、古い要求が新しい状態を上書きしないために単調に増やして使う。

### Audio デバイス

| コマンド                          | 主要引数                                                                                                           |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `audio status`                    | -                                                                                                                  |
| `audio probe`                     | -                                                                                                                  |
| `audio channels-probe`            | `--driver` `--input-device` `--output-device`                                                                      |
| `audio driver get` / `driver set` | `set` は `--driver` [`--input-device`] [`--input-channel`] [`--output-device`] [`--sample-rate`] [`--buffer-size`] |
| `audio recover` / `startup-retry` | -                                                                                                                  |

デバイス異常からの回復は `status` で状態(faulted 等)を確認し、`recover`、改善しなければ `startup-retry` の順で試す。

### 録音

```powershell
riffra --data-root ./riffra-data --attach record start
riffra --data-root ./riffra-data --attach record status
riffra --data-root ./riffra-data --attach record stop
riffra --data-root ./riffra-data --attach record list
riffra --data-root ./riffra-data --attach record promote --id rec:01j...
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
riffra --data-root ./riffra-data --attach render start --range loop-range --normalize true
riffra --data-root ./riffra-data --attach job get --id job:01j...
riffra --data-root ./riffra-data --attach job cancel --id job:01j...
```

- `render start --range entire-arrangement|loop-range|time-selection`。`time-selection` は `--start-tick` と `--end-tick` が必須。[`--normalize true|false`] と [`--track-id`](ソロ書き出し)を指定できる
- 応答は `type: "job"` のジョブ ID。完了は `job get` で確認し、進行中の停止は `job cancel`
- 出力は `exports/render-{ms}/timeline.wav` と manifest として書き出される

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
