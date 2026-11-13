# Score YAML

Score YAML を曲の編集正本とし、Riffra の Project をそこから生成される保存可能な実行用派生物として扱う。
楽譜の反復や度数表記は読み込み時に演奏イベントへ展開する。以後の演奏は、生成された CreativeSession を正準状態として実行する。
Score には共通の楽譜文法に加えて、本番の VST3 音源・キット・エフェクト・ミックス・音声クリップを記述する。
Sonalloy は共通の楽譜だけを解釈し、Riffra 専用の指定を既知の拡張として読み飛ばす。

## 正本とのリンク

Desktop のプロジェクトメニューで **Import Score YAML…** を選ぶと、Score YAML を新しい Project として読み込み、そのファイルへリンクする。ファイル選択ダイアログは `.yaml` と `.yml` を表示し、読み込み可否はファイル名や拡張子ではなく内容で決まる。CLI では次のコマンドを使う。実行中の Host への読み込みには `--attach` を指定する。

```bash
riffra project import-score ./song.yaml
riffra project import-score ./song.score.yaml --bind lead=./lead/definition.json
```

`--bind` は `external: true` の楽器へ Definition を結び付ける。繰り返し指定でき、指定できるのは外部楽器だけである。Definition の相対パスは Score ファイルのディレクトリを基準に解決する。
プリセットは Score の上位ディレクトリにある `presets/CATEGORY/NNN-name/definition.json` を参照し、参照元がない場合は同梱カタログの対応する ID を利用する。

リンクは `ScoreSource` が保持する。絶対パス、楽器 ID ごとの Definition パス、最後に成功して確定された YAML 本文である。リンク先の YAML をプロジェクトフォルダへ移すことはしない。パスやハッシュを Project ID の代わりには使わない。
ファイル名だけの `file_name` は、リンクが切れた後も表示に使う。

## 再構築

YAML を編集して保存し、次のコマンドで同じ Project を更新する。新しい Project は作らない。

```bash
riffra project rebuild-score
riffra project rebuild-score --path ./moved/song.yaml
riffra project rebuild-score --bind lead=./lead/definition.json
```

- `--path` を省略すると、Project がリンクしているファイルを読む
- `--path` を指定すると、そのファイルへリンクを切り替える
- `--bind` は既存の対応を置き換え・追加する。Score で使われなくなった対応は取り除かれる
- 必要な Definition が未指定なら、`scoreBindingsRequired` として不足する楽器 ID を返す

本文と演奏内容が同じでも、リンク先または外部音源のバインディングが変われば確定して保存する。演奏内容・本文・リンク先・バインディングのすべてが同じなら、保存も sequence の更新もしない。
Score 本文が同じでも、参照している library・State・Definition・WAV の内容が変われば解決結果を比較し、実行状態が変わったときだけ確定する。読み込み中に Score 本体または参照ファイルが書き換わった場合は、その候補を適用しない。

Live Host では開いている Project の YAML を監視する。保存した内容が 500 ms 安定すると再構築し、結果を `score-sync-status-changed` で通知する。監視はリンクされた 1 ファイルだけで、保存した内容が確定済みの本文と同じなら何もしない。エディタの一時ファイルや途中の書き込みでは前回の Session を維持する。恒久的な誤りは一度だけ診断として報告し、内容が変わるまで同じ診断を繰り返さない。

同期状態は「未リンク / 正常 / 読み込み中 / 失敗」と、対象 Project・パス・受理した sequence と更新時刻、最新の診断を保持する。Desktop は失敗したときだけ診断と再試行・別ファイルへのリンク操作を表示する。

### 同一性と再利用

再構築は Project ID・Session ID を変えず、sequence を進める。ID は YAML の見た目の文字列ではなく、意味上の所有関係で対応付ける。

| 対象               | 意味上のキー                        |
| ------------------ | ----------------------------------- |
| Instrument / Audio | 楽器 ID とゲインレーン              |
| Kit Track          | Kit ID                              |
| Instrument Device  | 所有 Track と VST3 の実パス         |
| Effect Device      | 所有チャネルと `effects[].id`       |
| Bus                | `mix.buses` のキーと種別            |
| Send               | 送り元 Track と Return Bus          |
| Sidechain          | 送り元 Track と宛先 Effect Device   |
| Automation         | 解決後の `AutomationTarget`         |
| AudioClip          | セクション出現と `audio_clips[].id` |

- トラックは Score の楽器 ID とレーンのゲインで前回のトラックへ対応付ける。トラック順の変更では取り違えない
- キットは Kit ID、音声は Audio Track の楽器 ID で対応付ける。Track の種類が変わったときは別の Track として扱う
- 対応したトラックは Track ID を維持する。同じ VST3 を使う限り Device ID を維持し、State・Patch・Params の変更は同じ Device へ反映する
- プラグイン自体を変えた場合は Track ID を維持し、Instrument / Effect の実体を差し替える。以前の State を別プラグインへ流用しない
- エフェクトの順序だけを変えた場合は、同じ Effect Device ID のまま順序を更新する
- Bus・Send・Sidechain・Automation・AudioClip は解決後の意味上のキーが同じなら ID を維持する
- AudioClip の参照ファイル内容が同じなら Asset ID を維持し、内容が変わったときだけ新しい Audio Asset を作る
- 対応しなかった古い Score 由来のトラックと、それが所有するクリップ・自動化は取り除く

ループ・パンチ、メトロノーム・カウントイン、マーカー、各トラックのミュート・ソロ・armed・monitoring、MIDI/Audio 入力、表示色、Desktop の選択と表示、Bus の mute/solo は引き継ぐ。PPQ を変更する場合は音楽的位置を維持するよう既存の換算規則で Tick を移す。作業用マーカーと Score 由来の `regions` は別のものとして扱う。

録音中または録音後の処理中は自動再構築を開始しない。録音による正本外の音声クリップ確定など、YAML で表せない変更もリンクされた Project では制限する。

### 確定と履歴

Live Host は候補の解析・音源解決・Native 準備を済ませてから、Writer のロック下で確定する。Native の昇格に失敗した場合も、Core の保存に失敗した場合も、直前の正準状態と音声グラフを維持し、復元に失敗した場合はその事実も診断へ含める。確定後に成功応答を返さない片側だけの更新は行わない。

確定に成功すると、その時点までの Undo / Redo を空にする。YAML の編集を Riffra の `history.undo` で巻き戻す操作は、GUI から YAML を書き戻す仕組みと一緒に扱う。変更がない再構築は確定しないため、sequence も履歴も変えない。演奏内容が変わらない本文だけの更新は、既存の Native グラフをそのまま使い、再生時の再準備を起こさない。

新しい音源スナップショットと Audio Asset は候補が確定するまで追跡し、失敗した候補のために作ったものだけを破棄する。既存 Asset は削除しない。

リンクされた Project では、YAML が所有する部分を変更する操作を拒否する。対象は時間軸、トラック構造（Score に由来しないトラックの追加・編集を含む）、楽器、演奏イベント、和声、Region、Automation（デバイスパラメーターを含む）、ミックス構造、楽曲パラメータ、ScoreSource 本文である。ミュート・ソロ・armed・monitoring などの作業状態だけを変更できる。GUI のボタン抑止だけに依存せず、Core の確定境界で判定する。

## 制作の記述

### 本番音源

```yaml
instruments:
  bass:
    role: low end
    sound: Warm fingered electric bass
    preset: BASS/013-soft-round-bass
    gain_db: -4
    pan: 0
    source:
      use: modo-bass-60s-p-bass
      vst3:
        state: states/song-bass.json
        patch:
          - op: setXml
            path: Plugin
            values:
              - { xmlPath: Part0/InstrumentPath, value: '...' }
        params:
          'FX Type': { display: Phaser }
      velocity: [[1, 18], [64, 68], [127, 120]]
```

`source` の有効フィールドは `use`、`vst3`、`velocity`。`vst3` は `plugin`（`use` がない場合は必須）、`identifier`、`state`、`patch`、`params` を持つ。

- `plugin` は検証済み VST3 カタログ上の正確なプラグイン名。`identifier` は同じ VST3 クラスへ絞り込む
- `state` は `PluginStateSnapshot` 形式の JSON。`schemaVersion`、`pluginPath`、`parameterValues`、`stateData` を持ち、読み込み時に現在のプラグイン実パスへ組み直す
- `patch` は `state` で得た状態文書への編集操作。`op: set | setTree | setXml`、`path`、`value` / `tree` / `values` を使う既存の `PluginStatePatchOperation` と同一の形式である。`state` がない `patch` はエラー
- `params` の値は `0..=1` の数値、または `{display: <文字列>}`。数値文字列は display 値として扱わない。名前が重複するパラメーターは `<名前>#<0始まりの index>` で指定する
- 適用順序は `state` → `patch` → `params`。`params` を指定した場合は State 復元後の全パラメーター配列を取得し、指定分だけを上書きする。指定のないパラメーターをゼロで埋めない
- `velocity` は `[[入力, 出力], ...]` の折れ線で、始点 `1`・終点 `127`、入力は重複せず増加する。1〜127 の整数を線形補間し、四捨五入して 1〜127 に収める。MIDI Note を生成するときに一度だけ適用し、Score の音楽イベント自体は変更しない

`source` のある楽器は Riffra で本番音源を鳴らす。`preset` / `definition` / `external` は Sonalloy 用の指定として保持し、Riffra は VST3 の準備のために外部 Definition のバインディングを要求しない。VST3 の不足・不一致・壊れた State を内蔵音源へ黙って切り替えない。`source` だけの楽器も Riffra で有効である。

同じプラグインと最終状態を使う複数のトラックは状態の値を共有するが、演奏インスタンスはトラックごとに独立させる。

### キット

```yaml
instruments:
  kick: { preset: DRUM/013-soft-round-kick, kit: drums, trigger_note_midi: 36 }
  snare: { preset: DRUM/003-tone-snare, kit: drums, trigger_note_midi: 38 }
  hat: { preset: DRUM/019-needle-closed-hat, kit: drums, trigger_note_midi: 42 }
kits:
  drums:
    source: { use: addictive-drums-2 }
    effects:
      - { id: drum-eq, use: surge-eq }
```

`kits.<id>` の有効フィールドは `source` と `effects`。`source` は VST3 音源を要求する。各部品の `kit` は存在するキットを参照し、部品側には `source`、`effects`、`audio`、`audio_input` を書けない。キットと楽器の ID は衝突せず、メンバーを持たないキットとキットによるキットの参照はエラーになる。

部品の演奏は 1 つの Kit Track、1 つの MIDI クリップ、1 つの VST3 インスタンスへ集約する。部品ごとに Instrument Device やトラックを作らない。`trigger_note_midi` は部品が送る出力 MIDI ノートであり、部品にはドラムなどのトリガー由来の Note だけを書ける。音高付きの旋律・和音や、ParameterChange などの制御イベントはエラーになる。複数の部品が同じトリガー番号を使うことは許容する。

部品の `gain_db` / `pan` は Sonalloy の音作りに残し、Riffra の個別音量には適用しない。キット全体のゲイン・パンは `mix.tracks.<kit>` に書く。部品ごとの追加ゲインレーン（`part_gains`、`lines.*.gain_db`、`placements[].gain_db` など）はエラーになる。キットの `source.velocity` は集約後のノートへ共通適用する。

### エフェクト

```yaml
instruments:
  bass:
    effects:
      - { id: eq, use: surge-eq }
      - id: duck
        vst3:
          plugin: Example Compressor
          params: { Threshold: 0.45 }
        bypassed: false
```

`effects[]` は `id`（全要素で必須）、`use`、`vst3`、`bypassed`（既定 false）からなる。`use` は `library/effects` のテンプレートを参照し、そこにある `vst3` / `bypassed` と曲側の値を統合する。統合後に 1 つの VST3 として確定する。
`id` はそのチェイン内で一意である。Instrument Track・Audio Track・Kit Track・Bus・Master に付けられる。

`bypassed` は Score 所有の設定であり、GUI の作業状態として残さない。複数のゲインレーンを持つ楽器のエフェクトは所有者が一意にならないためエラーになる。

### ミックス

```yaml
mix:
  tracks:
    bass:
      output: rhythm
      sends:
        room: -18
    drums:
      gain_db: -2
      pan: 0
      output: rhythm
      sends:
        room: { level_db: -14, position: pre, enabled: true }
  buses:
    rhythm: { kind: group, gain_db: -2, pan: 0 }
    room:
      kind: return
      effects: [{ id: reverb, use: room-plate }]
  sidechain:
    - from: drums
      to: { instrument: bass, effect: duck }
      level_db: 0
      position: post
  master:
    gain_db: -1
    effects: [{ id: limiter, use: limiter }]
```

`mix.tracks` のキーは通常楽器 ID、Audio Track の楽器 ID、または Kit ID。通常楽器と Audio Track の基本 `gain_db` / `pan` は `instruments` 側が正本であり、`mix.tracks` での再指定はエラーになる。キット全体の `gain_db` / `pan` は `mix.tracks.<kit>` に置く。

- `output` は既存の Group Bus または `master`。省略時は master
- `sends` のキーは Return Bus。値は dB 数値（post、enabled）または `{level_db, position, enabled}`。`position` は `pre | post`（既定 post）、`enabled` は既定 true
- 複数ゲインレーンの通常楽器では、同じ出力と Send を全レーンへ適用する
- `mix.buses.<id>` は `{kind: group | return, gain_db: 0, pan: 0, effects: []}`。Bus の主出力は Master 固定。Bus → Bus、Group → Return、Bus からの Send は追加しない。Send は Track → Return のみ
- `mix.sidechain[]` は `{from, to, level_db, position, enabled}`。`from` は楽器またはキット、`to` は `{instrument, effect}` / `{kit, effect}` / `{bus, effect}` / `{master: true, effect}`。宛先エフェクトが sidechain 入力に対応しない場合や、送り元と宛先が同じトラックの場合はエラー
- `mix.master.gain_db` は `-90..=0` dB。`mix.master.effects` はマスターのエフェクトチェイン
- `meta.master` の LUFS / True Peak / LRA は完成目標であり、ミックスの値と混同しない

既存 Bus の mute/solo は作業状態として保持するが、Bus の種別を変えた場合は同じ Bus ID を使い回さない。YAML から削除された Bus・Send・Sidechain・エフェクトは残さない。

### オートメーション

`mix.automation` の `target` は次の表記を既存の `AutomationTarget` へ変換する。

| 対象               | YAML `target` の例                                                 | 正準                 |
| ------------------ | ------------------------------------------------------------------ | -------------------- |
| トラック音量・パン | `{instrument: bass, parameter: volume}` / `pan`                    | `TrackMix`           |
| キット音量・パン   | `{kit: drums, parameter: volume}`                                  | `TrackMix`           |
| バス音量・パン     | `{bus: rhythm, parameter: gain}` / `pan`                           | `BusMix`             |
| マスターゲイン     | `{master: true, parameter: gain}`                                  | `MasterGain`         |
| センド量           | `{send: {instrument: bass, bus: room}}`                            | `SendLevel`          |
| サイドチェイン量   | `{sidechain: {from: drums, to: {instrument: bass, effect: duck}}}` | `SidechainSendLevel` |
| エフェクト         | `{effect: {instrument: bass, id: eq}, parameter: Mix}`             | `DeviceParameter`    |
| VST3 楽器本体      | `{source: bass, parameter: Cutoff}`                                | `DeviceParameter`    |

```yaml
mix:
  automation:
    - target: { effect: { instrument: bass, id: duck }, parameter: Mix }
      section: chorus
      occurrence: 2
      interpolation: linear
      points:
        - { at: { bar: 1, step: 1 }, value: 0.1 }
        - { at: { bar: 4, step: 13 }, value: 0.5 }
```

- `section` は `form` にあるセクション名。`occurrence` は同名セクションの何回目の出現かを 1 始まりで指定する。省略時は全出現へ同じポイントを展開する
- `at` はそのセクション内の `{bar, step}` または `{bar, offset_quarters}`。既存の分数位置と端数小節の解釈を使う。セクション範囲外はエラー
- `interpolation` は `linear | step`（既定 linear）。プラグインの正規化値は 0〜1。表示値の文字列は VST3 の `DeviceParameter` にだけ許可し、既存の表示値変換で数値化する。離散パラメーターは `step` を要求する
- ポイントは曲全体の絶対 Tick に変換し、対象ごとに 1 本の `AutomationLane` へ統合する。同じ対象への複数指定は補間が一致する場合だけ統合し、同一 Tick に異なる値を与える指定はエラー
- `source.vst3.params` は初期値であり、同じパラメーターをオートメーションで指定できる
- 1 つでも `part_gains` がある楽器へ明示的な Track 音量オートメーションを書くとエラーになる。`part_gains` がなければ、明示的な音量オートメーションを優先し、生成されるセクションゲインのレーンを抑止する
- 複数ゲインレーンの通常楽器では、Track Mix オートメーション・Send・Sidechain を全レーンへ適用する。宛先エフェクトが複数レーンに複製されて一意に選べない場合はエラー

### 音声クリップ

```yaml
instruments:
  ambience:
    audio: true
    gain_db: -5
    pan: 0
    effects:
      - { id: filter, use: surge-filter }
sections:
  intro:
    audio_clips:
      - id: rain
        instrument: ambience
        file: audio/rain.wav
        at: { bar: 1, step: 1 }
        offset_seconds: 0.25
        duration_seconds: 8
        gain_db: -3
        fade_in_seconds: 0.2
        fade_out_seconds: 0.5
```

`audio: true` は Audio Track を 1 本作る。`preset`、`definition`、`external`、`source`、`audio_input` と併記できず、音符やドラムトリガーの対象にもできない。楽器の `gain_db` / `pan` と `effects` は Audio Track に適用する。

`audio_clips[]` は `id`、`instrument`、`file`、`at` を必須とし、`offset_seconds`（既定 0）、`duration_seconds`（省略時は残り全体）、`gain_db`（既定 0）、`pan`（既定 0）、`fade_in_seconds` / `fade_out_seconds`（既定 0）を任意に持つ。`id` はセクション内で一意である。

- クリップはセクションのすべての出現へ配置する。`at` は音楽位置で固定し、再生時間・ソースオフセット・フェードは実時間で扱う
- `file` はその Score ファイルからの相対パス。WAV の sample rate と frames を読み、正準 `AudioClip` の frame range と duration へ変換する
- ソース範囲外・ゼロ長・負値・長さを超えるフェードは拒否する。セクション終端を超えて残るクリップは許可する
- 取り込んだ音声は Data Root の Asset として不変に保存する。同じ内容の Asset は再利用し、内容が変わったときだけ新しい Asset を作る
- 音楽的な配置やゲインだけを変えた場合は、AudioClip ID と Asset ID を維持する

### ライブラリ

```yaml
# library/instruments/modo-bass-60s-p-bass.yaml
source:
  vst3:
    plugin: MODO BASS 2
    state: ../states/modo-bass-60s-p-bass.json
range: [28, 72]
notes: Fingered electric bass, measured settings and caveats
```

```yaml
# library/effects/surge-eq.yaml
vst3:
  plugin: Surge XT Effects
  state: ../states/surge-eq.json
bypassed: false
```

`source: {use: name}` は `<library-root>/instruments/<name>.yaml`、`effects: [{id: foo, use: name}]` は `<library-root>/effects/<name>.yaml` を参照する。`library-root` は Score ファイルのディレクトリから上位へ探索し、最初に見つかった `library/` を使う。見つかった `library/` に該当名がなければ、さらに上位へ名前ごとの探索を続けない。

- `use` に任意パス、`..`、絶対パス、拡張子を書くことはできない。名前は `[a-z0-9][a-z0-9-]*` に限る
- 相対パスは、その値を記述した YAML ファイルを基準に解決する
- ライブラリの `source` を基本値とし、曲側の `source` で上書きする。マップはキー単位で統合し、スカラーは曲側を優先する。配列は置換し、`vst3.patch` だけはライブラリの操作列の後ろへ曲側の操作を連結する。`params` はキーごとに統合する
- 曲側で `vst3.state` を置換した場合は、以前の State を前提とするライブラリの `patch` を破棄し、曲側の `patch` だけを適用する
- ライブラリと曲側で `plugin` / `identifier` が異なる混在状態は作らない。別プラグインへ変える場合は、曲側で完結した `vst3` 設定を書くか、別テンプレートを使う
- 再帰的な `use` は実装しない。ライブラリの音源・エフェクトは 1 つの完結したテンプレートとする
- `range` と `notes` はライブラリのメタデータであり、Score の `source` へ文字列として統合しない。現時点では値の妥当性と `score inspect` での表示に使う

## 展開前の検証

Project を作らずに Score を確認する。

```bash
riffra score validate ./song.yaml
riffra score inspect ./song.yaml
```

`score validate` は文法・構造・楽器参照・ライブラリ・VST3・State・Patch・パラメーター・キット・音声・ミックス・オートメーションを診断し、`valid` と展開サマリーを返す。`valid` は error が 0 件のときに true となり、warning だけでは import / rebuild を止めない。
`score inspect` は展開済みのセクション・楽器・レーン・トラック・キット・エフェクト・バス・Send・Sidechain・オートメーション・音声クリップを返す。`--events` を指定すると、各イベントを来源位置つきで返す。

どちらも Project・Session・音源スナップショット・永続 Audio Asset を作らない。参照ファイルの読み込みと、分離された plugin-scanner による一時的なパラメーター照会は行う。プラグイン未インストール、ライブラリ欠損、State の不正、音声範囲超過、ミックスの循環、オートメーションの不正はエラーになる。
診断は `error` / `warning` の severity、YAML の項目パス、行・列、根拠のファイルを持つ。通常 VST3 の楽器で追加ゲインレーンが増える場合は warning を出し、演奏は止めない。

`session inspect` は既に確定した Session を調べる操作であり、役割が異なる。

## 未実装の拡張

次の指定は既知の拡張として明示的に拒否する。未知のキーや未知のイベント種別は、Sonalloy と Riffra の両方で文法エラーになる。

| 位置                                                        | 内容                          |
| ----------------------------------------------------------- | ----------------------------- |
| `source.voisona`                                            | VoiSona の歌声制御            |
| `source.articulations` / `kind: articulation`               | 奏法ごとの keyswitch・CC 出力 |
| `sections.<name>.lines.<name>.lyrics`                       | 旋律へ当てる歌詞              |
| イベント `cc` / `breath` / `voice_dynamics` / `voice_style` | 奏法制御・歌の表現            |
| GUI 操作の YAML 書き戻し                                    | GUI で加えた編集の正本反映    |
| 音域外警告・セクション単位レンダー・セクション LUFS         | 後続の制作支援                |

表示用の項目 `instruments.<id>.sound` と `sections.<name>.notes` は共通の項目であり、文字列として検証するが展開・演奏には影響させない。

## 演奏内容の対応

Score の解析と展開は Runtime の `score/` が行い、`score_yaml_import.rs` が正準データへ変換する。
下表の演奏に関わる情報は保存後も Native Audio Engine へ投影される。

| Score の情報                                               | Riffra での表現と動作                                                                                                                       |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| PPQ・分数拍                                                | Score の PPQ と整数 Tick を保持する。既定の新規プロジェクトは 960 PPQ                                                                       |
| テンポ・拍子の変更                                         | ProjectTimebase の変更点として保持し、表示・スナップ・サンプル位置の変換で共有する                                                          |
| メロディ・ドラム・コード・反復                             | Score の展開結果を MidiNote に変換する。音高・開始位置・長さ・ベロシティを保持する                                                          |
| 複数レーン                                                 | レーンごとに Instrument Track と楽曲全体を覆う MIDI クリップを生成する。同じ楽器のレーンも独立した音源インスタンスで演奏する                |
| サステイン・ピッチベンド・モジュレーション・アフタータッチ | InstrumentControlEvent として精度を保ち、同 Tick の優先順位とソース順で実行する                                                             |
| ParameterChange・ParameterRamp                             | 内蔵音源では音源のパラメータ定義で検証する。VST3 の音源では使用せず、音色変化は `mix.automation` で指定する                                 |
| PartWindow                                                 | 内蔵音源の展開済み音符範囲を保持し、区間境界に演奏制御のリセットを追加する                                                                  |
| Preset・Definition・External・Sample                       | 内蔵音源の参照ファイルを解決する。保存リソースは共有し、演奏状態はトラックごとに分離する                                                    |
| VST3 音源・State・Patch・Params                            | カタログから選んだ実パスへ State を組み直し、Patch とパラメーターを適用する。音符だけの変更では同じ Device と Native インスタンスを維持する |
| キット                                                     | 部品のトリガーノートを 1 つの Kit Track と 1 つの VST3 インスタンスへ集約する                                                               |
| ベロシティ補正                                             | `source.velocity` を MIDI Note の生成時に一度だけ適用する。Score の音楽イベントと Sonalloy の展開結果は変更しない                           |
| ゲイン・パン                                               | レーンゲインを維持し、パンには中央の左右ゲインを保つ線形バランス方式を使う。編集用フェーダーの操作範囲は保存値を制限しない                  |
| セクション別ゲイン                                         | Track Volume Automation に実効音量を保持し、境界から指定秒数で振幅を線形補間する。Score では 5 ms の遷移を使う                              |
| エフェクト・ミックス                                       | トラック・Bus・Master の Effect Device、Bus、Send、Sidechain、Master Gain を正準 Mixer へ変換する                                           |
| オートメーション                                           | セクションの出現ごとに絶対 Tick へ変換し、対象ごとに 1 本の AutomationLane へ統合する                                                       |
| 音声クリップ                                               | Audio Track と AudioClip を作り、WAV の frame range・実時間長・フェードを保持する                                                           |
| 外部音声入力                                               | 内蔵音源の参照パートの全レーンを入力元として登録する。ゲイン・パン適用後の音声を遅延補正して合算する                                        |
| マスタリング・フェード                                     | MixdownSettings にラウドネス・True Peak の目標とフェード時間を保持する。LRA 目標は任意の測定・警告条件として扱う                            |
| セクション                                                 | 各出現を独立した TimelineRegion として保持する                                                                                              |
| 和声                                                       | Score の主音とコード構成音を HarmonyEvent に投影する。和声イベントは編集・参照用であり、展開済みの音符を再生成しない                        |
| 元 Score                                                   | ScoreSource にファイル名・絶対パス・Definition の対応・最後に確定した元 YAML 全文を保持する                                                 |

## 保存と再利用

同梱プリセットと Definition・アセットの内容が一致する場合は、同梱リソースを利用する。
それ以外の内蔵音源は Data Root 内のプロジェクト音源スナップショットへコピーする。
Definition 本体と実際に参照するアセットだけを収集し、保存先を基準とする参照に書き換える。パッケージ外の参照も収集対象とし、同じディレクトリにある無関係なファイルは保存しない。
参照先にシンボリックリンクや読み込めないリソースがある場合はエラーにする。
同じ Definition を使う複数のレーンはスナップショットを共有する。

VST3 の State は Score の `state` と `library/states` のファイルから読み込み、選択したプラグインの実パスへ組み直す。AudioClip の WAV は Data Root の Asset として不変に保存し、元ファイルを後から編集・削除しても再生に影響しない。

再構築で音源・エフェクト・Asset を再利用するかは「同一性と再利用」の規則で決める。判断は既存の Track・Device・Asset の内容を比較して行い、別のキャッシュや索引は作らない。

`.riffra` のエクスポート・インポートには、音源スナップショットと ScoreSource も含まれる。
パッケージに書き出す Session のコピーでは、リンクの `path` を消して楽器の対応を空にする。元 PC の絶対パスを有効なリンクとして持ち込まないためであり、エクスポート元の Project は変更しない。インポート後に必要なら `project rebuild-score --path ... --bind ...` でリンクし直す。

## 失敗時の扱い

Score の構文・意味・音源参照の誤り、リンク先の欠損、Native 準備の失敗では、最後に成功した Session と `ScoreSource.yaml` を維持する。勝手にファイルを探索したり、ハッシュで別ファイルを結び付けたりしない。
リンク先がない場合は、確定済みの Session と本文をそのまま読み取れる。以前のリンクを持たない ScoreSource もそのまま読み取れ、`rebuild-score --path` で明示的にリンクできる。

時間軸の PPQ を変更すると、既存の音符・制御・範囲・録音位置と、テンポ・拍子の変更位置を新しい PPQ へ換算する。
PPQ の変更時はテンポ・拍子マップの値と変更点の列をそのまま指定する。マップの編集は PPQ の変更とは別の操作で行い、その時点の PPQ に基づく Tick を指定する。同時編集と、既存の音楽的位置を整数 Tick で正確に表せない変更は拒否する。

データモデル全体の規則は [data-model.md](data-model.md)、音声実行は [audio-engine.md](audio-engine.md) を参照。
