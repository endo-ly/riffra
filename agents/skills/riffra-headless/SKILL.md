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

1. **実行のしかた** — 実行モード / Host の起動と接続 / MCPクライアントからの操作 / Safe Mode の範囲
2. **要求と応答** — 要求の形式 / 応答の読み方 / エラーコード
3. **楽曲を組み立てる** — Session の照会と Undo / 一括適用(session apply) / Score YAML を正本にする / Track・Clip・Region / 楽曲の入力 / 和声・フレーズ・リズム
4. **音とミックス** — Device・Plugin / Instrument / ミックス / Automation
5. **音声と素材** — 録音 / 書き出しと測定 / Asset・解析・Preview
6. **共通の注意** — 引数と入出力

## 1. 実行のしかた

### 実行モード

| 形態                    | 起動                                            | 用途                                      |
| ----------------------- | ----------------------------------------------- | ----------------------------------------- |
| Standalone ワンショット | `riffra --data-root <path> <command> ...`       | 1 操作を実行し JSON 応答を出す            |
| Standalone 対話         | `riffra --data-root <path> --interactive`       | stdin へ JSON Lines 要求で連続操作        |
| Live Host               | `riffra --data-root <path> serve [--safe-mode]` | フォアグラウンド常駐し Audio Runtime 提供 |
| Attached                | `riffra --attach <command>`                     | current-user registryから選んだHostへ接続 |

正準状態は操作ごとに DataRoot へ永続化されるため、編集はプロセスを跨いで引き継がれる。Undo / Redo 履歴はプロセス(Host)の寿命に紐づく。

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

### MCPクライアントからの操作

MCPを使う場合もHostは同じで、`riffra-mcp` がstdio経由でControl Commandへ転送する。Hostを先に起動し、MCPクライアントの `command` に `riffra-mcp.exe` の絶対パスを指定する。複数のHostがある場合は `args` に `--host <instance-id>` を渡す。

ツールは操作対象ごとにまとまり、`action` と `params` でControl Commandを選ぶ。状況把握には `session_read.inspect`、複数変更の原子的な適用には `session_edit.apply`、書き出しや測定の完了確認には `job.wait` を使う。位置・音価・音高はCLIと同じく音楽座標を優先し、制作の手順もCLIと同じである。

MCPクライアントの設定とツールの一覧は [docs/mcp.md](../../../docs/mcp.md) を参照する。

### Safe Mode の範囲

正準状態の編集はSafe Modeでもそのまま使える。transport、live MIDI送信、`audio probe` 系、`audio driver set`、`plugin scan`、`record start`、`asset preview` は `runtimeUnavailable` になる一方で、`render` とjob管理、`asset`、`analysis`、`plugin catalog list`、`missing list`、`record list` などの管理系は使える。

## 2. 要求と応答

### 要求の形式

対話モードでは標準入力へ 1 行 1 要求を書き、標準出力から 1 行 1 応答を読む。空行は無視される。

```json
{
  "requestId": "42",
  "command": "track.add",
  "params": { "name": "Bass", "kind": "instrument" }
}
```

- `requestId`: 任意の文字列。応答へそのまま返る
- `command`: 操作名
- `params`: コマンドごとの引数。キー名は camelCase

interactiveで操作を連鎖させるときは、応答を受け取ってから次の要求を組み立て、生成IDは応答の `createdEntityIds` から取る。

### 応答の読み方

成功応答は次の形で、`sequence` がその結果の正準シーケンスである:

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
    "message": "active project changed",
    "details": { "expectedProjectId": "project:a", "currentProjectId": "project:b" }
  }
}
```

### エラーコード

分岐は必ず `error.code` で判定する。

| code                 | 意味                                                                                | 対処                                           |
| -------------------- | ----------------------------------------------------------------------------------- | ---------------------------------------------- |
| `invalidRequest`     | 要求形式・params・未知のコマンドが不正                                              | params のキー名(camelCase)と型を見直す         |
| `commandFailed`      | Core / Host / 保存処理の失敗(ID 不存在など)                                         | message の内容に対処する                       |
| `conflict`           | `expectedProjectId` が現在のProjectと不一致、または長期命令の実行中に状態が変化した | Inspectし直して編集内容を決め直す              |
| `hostUnavailable`    | Attached が Host へ接続できない                                                     | `riffra host list` で登録とhandshake状態を確認 |
| `runtimeUnavailable` | Runtime を利用できない(Safe Mode、Standalone)                                       | `serve` + `--attach` に切り替える              |

## 3. 楽曲を組み立てる

### Session の照会と Undo

現在の構造と `sequence` は `session inspect` で照会する。`session get` はNote全件・録音詳細を含むフルスナップショットなので、Inspectにない詳細が必要なときだけ使う。

編集後は同じ範囲をInspectして確認する。取り消しは `history undo` で行う。

```powershell
# 生成IDは createdEntityIds から取る
riffra --attach session inspect
riffra --attach track add --name Drums --kind audio
riffra --attach music midi-clip create --track-id track:01j... --start 5:1 --end 13:1 --name Piano
riffra --attach music note insert --clip-id midi-clip:01j... --notes-json '[{"pitch":"C4","position":"5:1","duration":"1/8"}]'
```

### 一括適用(session apply)

複数の正準編集を事前に決めているなら、CLI側でIDを受け渡すスクリプトを書かずに `session apply` へJSONLを渡す。全operationが成功したときだけ1回のcommitとして適用される。途中結果を確認しながら進めたい場合や、1操作ずつUndoしたい場合は `--interactive` を使う。

JSONLでは `trackName` / `clipName` / `busName` / `returnBusName`、ownerの `owner.name` で名前解決でき、同じBatch内で追加したTrack・Bus・Clipを参照できる。`--replace` はSessionを初期状態に戻してからJSONLを適用する。Projectの同一性は保たれ、録音素材ファイルも残る。

```powershell
riffra --data-root ./riffra-data session apply --file ./song.jsonl
riffra --attach session apply --file ./song.jsonl --include-created-ids
```

```jsonl
{"command":"track.add","params":{"name":"Lead","kind":"instrument"}}
{"command":"music.midi-clip.create","params":{"trackName":"Lead","name":"Verse","start":"1:1","end":"9:1"}}
{"command":"music.note.insert","params":{"trackName":"Lead","clipName":"Verse","notes":[{"pitch":"C4","position":"1:1","duration":"1/8"}]}}
{"command":"effect.add","params":{"owner":{"kind":"track","name":"Lead"},"pluginPath":"C:/Plugins/Effect.vst3"},"bind":"effect"}
{"command":"device.parameter.set","params":{"owner":{"kind":"track","name":"Lead"},"deviceId":"@effect","parameterIndex":0,"value":0.45}}
```

デバイスを1件作る操作に `bind` で名前を付けると、後続操作のデバイスIDに `@名前` を使える。名前はそのBatchの中だけで有効な一意名である。

既存ClipのNoteだけをまとめて置き換えるには、同じBatchに `clip.midi.note.clear` → `music.note.insert` の順で含める。raw MIDI値でよければ `clip.midi.update` の `patch.notes` へNote配列を渡す。

paramsキーはCLIフラグ名と対応する(`--notes-json` → `notes`、`--file` → `operations`)。`music phrase insert` だけは `--phrase-json` の内容が `pattern` と `placements` の2つのparamsへ分かれる。

失敗時は `error.details.operationIndex` と `command` でどのoperationか特定でき、JSONL入力では物理行番号が `inputLine` に入る。paramsの検証エラーには、JSON Pointer形式の `path` と配列要素の `index`、問題の値が付く。

### Score YAML を正本にする

Score にリンクしていない Project の中身を読むときは、まず `project export-score` で曲フォルダへ書き出す。そのまま YAML で作業を続けるなら `--link` を付けて、書き出した YAML を正本にする。

Score YAML から Project を作ると、Project はそのファイルへリンクされ、YAML が曲の編集正本になる。YAML を直したら `project rebuild-score` で同じ Project を更新する。Live Host では保存を検知して自動で更新される。

- `project import-score <path>`: YAML を新しい Project として読み込み、リンクする。`--bind ID=DEFINITION` で外部楽器の Definition を結び付ける
- `project rebuild-score [--path PATH] [--bind ID=DEFINITION]`: アクティブな Project をリンク先の YAML で更新する。`--path` を省略するとリンク先を読む
- `project export-score <path> [--link]`: アクティブな Project を Score の曲フォルダとして書き出す。`--link` を付けると書き出した YAML を Project の正本にする。リンク済みの Project と、表現できない内容が1件でもある Project はリンクしない。表現できない内容は書き出さずに `omissions` として返す
- `score validate <path>`: Project を作らずに文法・参照・音源・ミックス・オートメーション・音声を診断する。`--bind` を受け取る
- `score inspect <path>`: Project を作らずに展開結果(トラック・キット・エフェクト・ルーティング・オートメーション・音声クリップ)を来源位置つきで確認する。`--bind` を受け取る

Score は内蔵音源だけでなく、VST3 の `source`、キット、エフェクト、Group / Return / Send / Sidechain、オートメーション、Audio Track と WAV クリップを記述できる。VST3 名と State、パラメーターは Score に直接指定し、State と WAV は曲フォルダ内に置く。

リンクされた Project では、YAML が所有する部分(時間軸・Score 由来の構成・演奏・和声・ミックス構造など)は編集対象外で、作業状態(ループ・パンチ・ミュート・ソロ・armed・monitoring など)だけを変更できる。YAML に未実装の拡張を書いた場合は、項目パスと行・列を持つ `notImplemented` 診断で拒否される。書式と未実装の一覧は [docs/score-yaml.md](../../../docs/score-yaml.md) を参照する。

`.riffra` へ書き出すとリンクは無効になり、持ち出し先では `project rebuild-score --path ... --bind ...` で結び直す。失敗した同期は直前の状態を維持する。Score にリンクしていない Project は `project export-score` で曲フォルダへ書き出せる。

### Track・Clip・Region

Trackは `track add`、MIDI Clipは `music midi-clip create`(「楽曲の入力」)で作成し、`track list` / `session inspect` で確認する。取り込んだMIDI Asset(`asset import-midi`)の配置は `clip audio add-asset` / `clip midi add-asset` で行う。Regionは `Intro` や `Verse` などの自由な名前を持つ時間範囲で、重複・入れ子・同名を許可する。

隣接するClipの結合は `clip audio merge` / `clip midi merge` で、`--first-clip-id` と `--second-clip-id` に時間順の隣接クリップを指定し、一回の編集として行う。音声は同じ素材の連続範囲で音量・パン・ミュート・テイク・フェード形状が一致し、境界のフェードやループが無いことを要する。MIDIは同じトラックでミュートが一致し、ループしていないことを要する。録音セッションのスロットはコピーを配置してから結合する。

### 楽曲の入力

通常の作曲では `music.*` を優先する。位置・音価・音高は音楽座標のまま渡し、Coreがテンポ・拍子に基づいて正準TimelineTickへ変換する。テンポ・拍子の変更は `music timebase update`、変更点全体は `music timebase get-map` / `set-map` で扱う。

```text
位置: 5:1、5:3+1/2
音価: 1/4、1/8、3/8、1/12
音高: C4、F#4、Bb3
```

コマンドの使い分け:

- `music note` / `music midi-clip`: 音楽座標によるNote・MIDI Clipの参照・作成・更新・削除・配置。通常のNote操作はこちら
- `clip midi note`: 量子化・変形・複製など、既知のNote IDをraw tick・MIDI値で直接編集する操作
- `clip midi`・`clip midi instrument-event` のraw tick系操作: CCやPitch Bendなど、音楽上の基本操作に含まれないMIDIイベントのtickによる直接編集

tickを直接扱う操作での換算基準は `session inspect.project.ppq` である(Instrument previewの `ticksPerBeat` はpreview固有の値)。

Noteの取得は `music note list` で範囲を絞って行う。一括調整は `music note transform`、個別の更新・削除は `--include-ids` で取得したIDに対して行い、raw tickやMIDI値で扱うときだけ `--raw` を付ける。`position` はClip内部の相対位置ではなくArrangement全体の絶対位置である。複数Noteは1回の `music note insert` で渡す。Clip末尾を越えるNoteは `--clip-to-end` を指定すると末尾までに短縮できる。開始位置はClip内でなければならない。

各Noteの `lyric` は `music note update --lyric` / `--clear-lyric` で変更・削除し、一括変更は `clip midi note update-many` のpatchの `lyric`(`null` で削除)で扱う。

```powershell
riffra --attach music note transform `
  --track-id track:drums --start 5:1 --end 13:1 `
  --pitch D2 --timing-offset +1/48 --velocity-offset 4
```

`music export-midi --clip-id <id> --output <path>` はClip内のNoteとMIDIイベントをClip先頭から書き出す。`--track-id` を指定するとTrack内の全MIDI Clipをプロジェクト上の位置で書き出す。歌詞とプロジェクトのテンポ・拍子もSMFに含まれる。

`clip midi instrument-event` はMIDI Clip内の音源制御(`sustainPedal`、`pitchBend`、`modWheel`、`aftertouch`、`parameterChange`)を編集する。Parameter Changeは `parameter` と `nativeValue` を保持する。

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

`device inspect`、`device parameter list/get/set`、`plugin state save/load/inspect/read/patch`、`plugin preset` は実体のPluginを扱うため Live Host 専用である。Built-in instrumentはparameter・state・preset・editorをサポートしない。Bypass中のDeviceはインスタンスを持たないため、これらのコマンドの対象にならない（`plugin state import` だけは正準状態へ直接書き込み、Bypassを維持する）。解除すると次の投影でインスタンスが復帰する。

JUCE製PluginのStateは、ファイルを受け渡さず構造を指定して読み書きできる。`inspect` で構造と値の種類を確認し、`read` で1つの値を取り出し、`patch` で指定箇所だけを変更する。

```powershell
riffra --attach plugin state inspect --track-id <t> --device-id <d>
riffra --attach plugin state read --track-id <t> --device-id <d> --path Plugin --xml-path Part0/InstrumentPath
riffra --attach plugin state read --track-id <t> --device-id <d> --path Plugin --output multi.xml
riffra --attach plugin state patch --track-id <t> --device-id <d> --set PROJECT/Slot/Level=0.5
riffra --attach plugin state patch --track-id <t> --device-id <d> --path Plugin --xml-set "Part0/InstrumentPath=//Instruments/a.st3i" --xml-set "Part0/@id=1"
```

| 記法                | 意味                                                                                                            |
| ------------------- | --------------------------------------------------------------------------------------------------------------- |
| `--path`            | ValueTreeのパス。子は `Type` か `Type[1]`、最後はプロパティ名。エンコードされた入れ子のTreeも同じ書き方でたどる |
| `--xml-path`        | `--path` のXMLのルート直下からのパス。属性は `@name`                                                            |
| `--set`             | 既存プロパティへの代入。値はプロパティの型に変換される。XMLを持つプロパティへは文書全体の置換                   |
| `--set-file`        | ファイルの中身で文字列・XML文書全体を置換                                                                       |
| `--xml-set`         | XMLの要素・属性への代入。最後の要素が無ければ親の末尾へ追加。子要素を持つ要素には代入できない                   |
| `--operations-file` | `set` / `setTree` / `setXml` のJSON配列。Treeの置換や複数のXML文書の編集に使う                                  |

複数の変更は1回の `patch` にまとめる。Stateの適用は音源の読み込みを待つため、数十秒かかることがある。

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

`device parameter set` の応答は `mutation`(正準変更)、`parameter`(対象の読み戻し)、`changedParameters`(値が変わったほかのParameter、index順)を持つ。`--display` はPluginに文字列からの変換を依頼し、表示へ戻した結果が入力と等価な場合だけ適用される。数値表示には `--nearest` を併用でき、単位が一致する範囲で最も近い適用可能な表示値を選ぶ。適用値は応答の `parameter.value` と `displayValue` で確認できる。`session apply` 内では正規化値だけを書き、`--display` は使えない。

### Instrument

役割(リード、ベース、コードなど)とプリセットの `category` は直接対応しない。`category` はライブラリ上の主分類であり、`instrument list` が返す全プリセットから `description`・`tags`・`recommended_range` で選ぶ。主旋律には `Lead` だけでなく `Pluck`・`Mallet`・`Keys` も候補になる。

割り当ては `instrument apply`(`builtin:<presetId>` または `user:<id>`)。Built-in instrumentはSafe Modeでも割り当てられる。音源付きトラックの作成は `instrument create-track --name Keys --instrument-id builtin:<presetId>` で、`--instrument-id` の代わりに `--plugin-path` も使える。トラック作成と割り当てをまとめて確定し、準備や保存の失敗ではトラックも追加しない。一回のUndoで全体を戻せる。Instrument毎のMIDI設定は `instrument midi set --settings-json` で置換し、`transposeSemitones`、`playableNotes`(`{"type":"range","start":36,"end":84}` / `{"type":"set","notes":[...]}` / 制限なしの`null`)、`keyswitches`(`[{"note":24,"name":"C0"}]`)を持つ。

User Instrumentは `instrument save` で登録し、応答のIDを割当へ渡す。定義が参照するサンプルなどの素材は登録時にパッケージへコピーされ、元のファイルから独立して利用できる。複数Trackへの割当は `session apply` の `instrument.apply` に含める。

### ミックス

Track・Clipの音量・定位は `track update` / `clip audio update` で、Masterのゲインは `session settings update --master-db` で調整する。Gain / VolumeはdB、Panは-1.0(左)..1.0(右)で指定する。

Group・Return・Send・Sidechainは `mixer` コマンド群で編集し、状態は `mixer get` で確認する。RenderはTrackとClipのGain、Pan、Mute、Solo、Automation、Masterをすべて反映する。

Sidechainの送信先は別Track・Group・Return・Master上のEffect Deviceであり、Instrument Deviceと送信元自身のTrack上のEffectは対象外である。接続前にLive Hostの `device inspect` で `capabilities.sidechain` を確認する。Source TrackのMuteはSidechainも止まり、Soloで主出力が分離されてもSidechainは維持される。

### Automation

`automation set` は指定対象のLane全体を置き換える。Pointは `id`・`tick`・`value` を持ち、Tickは楽曲全体の絶対位置である。音楽座標からの変換は `project.ppq` と拍子で行う。

Deviceの `value` には正規化数値またはPluginの表示文字列(例:`"250 ms"`)を渡せる。文字列はLive Hostが変換できた場合だけ保存される。

更新はLane IDを維持し、新規LaneのIDは `createdEntityIds.automationLanes` に入る。空のPoint配列と `clear` はLaneを削除する。補間後の予定制御値は `automation evaluate` で確認でき、指定したSample Rateと音楽グリッド上で返る。静的なミキサー値も評価できる。Plugin内部の量子化やGain Reductionの測定値ではないため、実際の音量はCaptureとLevelsで確認する。

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

transport / audio / midi 送信 / record / render / job / asset / plugin 実体の操作 / device parameter set は Live Host(`serve`)+ `--attach` が必要で、Standaloneでは引数の内容にかかわらず `runtimeUnavailable` になる。

### 録音

録音にはarm済み(`track update --armed true`)のTrackが少なくとも1つ必要である。キャプチャはDataRootの `recordings/` 配下に置かれ、`record promote` で正準セッションへ反映する。

Processed Variantに必要な制御信号を録音から再利用できない場合はRaw Takeが保持され、`record list` の `processedUnavailableReason` で理由を確認できる。

### 書き出しと測定

音声の書き出しは `render start` で行う。ワンショットでは `render start --wait` / `render stems --wait` で完了結果を待てる。

Synthesizer V や VoiSona Song のような、合成を裏で進めるプラグインは、速い書き出しでは合成が追いつかず無音になる。Riffra はこれらのクラスを知っており、書き出す範囲の信号経路（読み込まれるプラグイン）に1つでもあれば、書き出し全体を自動で実時間（1.0x）にし、準備時間にはそのうち最長のものを使う。歌を含まない書き出しは自動で最速になる。結果の `pacing` に実際のモード・準備時間・理由のプラグインが出る。`render start --render-mode` は調べるときの強制指定で、実時間が必要な経路への `offline` はプラグイン名つきのエラーで止まる。実時間の書き出しは範囲の長さだけ実時間を要するため、`job wait` のタイムアウトは範囲の長さ以上にする。

応答は `type: "job"` のジョブIDである。ワンショットは `job wait JOB_ID`、interactiveは `job get` の繰り返しで追跡し、`job cancel` で停止する。待機のタイムアウトは `job wait --timeout-ms` で指定し、無指定なら終了まで待つ。`plugin scan --background` と `analysis measure` も同じ非同期ジョブである。`ok: true` は要求の受付成功を表すので、Job自体の成功は `state: "completed"` で判断する。出力はDataRootの `renders/` 配下へ書き出される。

Scoreから取り込んだProjectでは、`--section <name> --occurrence <n>` でセクションの出現を指定できる。`--occurrence` は同名セクションの1始まりの出現番号で、`--section` と組み合わせる。`--start` / `--end` / `--range loop-range` とは併用せず、`--track-id` / `--track-ids` とは併用できる。`render start` と `render stems` は同じ指定を受ける。

```powershell
riffra --attach render start --section walk --occurrence 2 --wait
riffra --attach render stems --all-tracks --section walk --occurrence 2 --wait
```

完成WAVをDataRootの外へ出すときは、Mixが `render start --output-path`、Stemが `render stems --output-directory` で出力先を指定する。保存済みRenderの取り出しは `render export --render-id --output-path`、削除は `render delete --render-id` で行う。

ミックスの往復（音量を少し変えて測り直す）は、書き出し済みの stems から `render remix --from <render-id> --track <track-id>=<dB> [--pan <track-id>=<値>] --wait` で行う。ステムを元のトラックの代わりに置いて、プラグインを読み込まずオフラインで書き出し、通常のRenderとして履歴に残る（測定は `render start` と同じ）。`--track <id>=0` はステムそのままで、dB は stems に含まれるゲインへの差分である。プリフェーダーのSend・パンの変更（既にパンされたステムへのバランス）・Sidechainの検出元は近似になる。stems を書き出し直すと基準が変わる。

各パートの比較は `render stems` でTrack別WAVを作る。全WAVで開始Tick、終了Tick、Sample Rate、フレーム数が揃い、個別の正規化とMasteringは行われない。共有する非線形Effectでは、Stemの合計と全体Mixが一致しない場合がある。StemとMasteringの設計上の扱いは[音声エンジンの設計](../../../docs/audio-engine.md)を参照。

現在のミックス経路を変えずに複数の出力を同時に取得するのは `render capture` である。Track・Group・Return・Masterを選んで同時に取得し、結果の `renderId` を `analysis levels` へ渡すと、各出力のRMSとSample Peakを同じ音楽座標で比較できる。`--wait` はRenderとCaptureの両方のJobで使える。Captureの外部コピーは `--output-directory`、保存済みCaptureの一括取り出しは `render export` で行う。

音符ごとの音量変化を比べるときは `analysis levels` を使う。グリッド `1/16` は全音符の16分の1で、4/4の `20:1` の次は `20:1+1/4` となる。範囲の先頭と末尾は指定位置で切り詰められ、無音のRMSとPeakは `null` である。TimebaseはRender時点のものを使い、Tailは含めない。

Job結果と `render.json` の `measurements` には完成WAVのSample Peakとラウドネス測定が含まれる。区間ごとの音量・ピーク位置・帯域バランスは、Marker区間は `measurements.sections`、Scoreのセクション出現は `measurements.scoreSections` で読む。

| 項目                               | 読み方                                                                              |
| ---------------------------------- | ----------------------------------------------------------------------------------- |
| `name` / `start` / `end`           | Marker名と区間の音楽座標                                                            |
| `loudness.integratedLufs`          | 区間の統合ラウドネス。有効な400msブロックがない区間では`null`                       |
| `loudness.truePeak`                | 最大True Peakの`db`・`seconds`・`position`。無音なら`null`。位置は100msフレーム単位 |
| `bands.lowDb` / `midDb` / `highDb` | 250Hz未満・250Hz〜4kHz・4kHz超のRMSレベル(dBFS)。無音なら`null`。帯域間比較の目安   |

ScoreにリンクしたProjectでは `measurements.scoreSections` も返る。`name`・`occurrence`・RegionのTick範囲・WAV先頭からの秒数・`complete` を持ち、`loudness`・`bands` の表現は `sections` と同じである。`complete: false` はRender範囲がRegionの一部だけを含むことを示す。Render範囲と交差する部分だけを測り、全曲用のテールは最終セクションへ含めない。Render結果の `sectionName` と `sectionOccurrence`、Render履歴の `rangeKind: "section"` でも指定したセクションを追跡できる。

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
riffra --attach job wait --id job:analysis-measure:0
```

待機には開始応答のJob IDを使い、結果の `sources` のパス・Track情報を比較対象と照合する。区間True Peakの位置はWAV先頭からの秒数である。帯域値・測定状態の読み方はRenderと共通である。

Mixdown設定は `render settings get` / `render settings set --settings-json` で参照・更新する。`set` は渡した項目だけを変え、`null` は `mastering`・`sampleRate`・`blockSize` を外す。Scoreに紐づくProjectでは `musicalEndTick`・`fadeOutSeconds`・`mastering` をScoreが持つため、食い違う値はその項目名を示すエラーになる。Masteringは目標ラウドネスへMix全体を一定のゲインで合わせ、True Peak上限を超えるときは上限に合わせてゲインを下げる。

`mastering.deviation.integratedLufs` は最終ラウドネス − 目標で、負のときは目標に届いていない。`mastering.truePeakCorrectionDb` はTrue Peak上限のために下げたゲインで、0より大きいときは上限で止まったことを示す。

```powershell
riffra --attach render settings set --settings-json '{"musicalEndTick":0,"tailSeconds":2,"fadeOutSeconds":0.5,"mastering":{"integratedLufs":-14,"truePeakDb":-1,"loudnessRangeLu":11},"sampleRate":48000,"blockSize":1024}'
```

### Asset・解析・Preview

`asset search` はAsset metadataへのfree-text検索である。短い語は全Assetがヒットして応答が膨らむため、具体語で絞る。

素材の解析は `analysis inspect`、試聴は `asset preview` / `stop-preview`、Audio projectionの状態は `audio projection get` / `retry` で扱う。

## 6. 共通の注意

構造入力は小さければinline JSON、大きければ `--*-file`、連続操作はinteractive JSONLを使う。複数IDはコンマ区切りとJSON形式のどちらでも渡せるが、recording-slot由来などWindowsパスを含むIDはJSON形式(`--*-ids-json`、またはfile)が安全である。

CLIが標準出力へ書くJSONはUTF-8である。PowerShell 5.1など親Shellがパイプ経由で別のコードページへ変換した後の表示はRiffraのProtocol保証に含まれない。
