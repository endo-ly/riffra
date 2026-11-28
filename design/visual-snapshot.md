# VisualSnapshot の生成と書き出し

Project の正準状態から、時刻同期された可視化データ `VisualSnapshot` を作り、JSON ファイルとして書き出す。
ピアノロール・コード進行・五度圏・リズムレーンの描画と、HyperFrames による動画化は、すべてこのデータだけを読む。全体構想は [showcase-ui.md](showcase-ui.md) を参照。

## 1. 目的と範囲

| 項目           | 内容                                                                                                                                 |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| 目的           | 描画側が Project・Score・テンポ計算を知らずに済むよう、時刻を秒へ展開し、和声とトラックの意味づけまで済ませたデータを出す            |
| 対象           | Active Project の正準状態。Score にリンクしている場合は、正準状態に無い主音・スケールとドラムの判定材料を、リンク済み Score から補う |
| 保証すること   | 同じ正準状態（と同じリンク済み Score）からは、常にバイト単位で同じ JSON が出る。描画に渡す音は、実際に鳴る音（§4.4）                 |
| 保証しないこと | 音声クリップの可視化、トラック分類の手動上書き、歌詞のフレーズ分割                                                                   |
| 前提の修正     | Score 取り込みで作る和声の音名を、主音と度数どおりに綴る（§5）                                                                       |

音源（VST3・内蔵音源）ごとの事情は判定に持ち込まない。音源名・プリセット名・ライブラリのカテゴリ名・特定の音源の鍵盤割り当て（GM ドラムマップなど）は使わず、Score と正準状態に構造として書かれた事実だけから決める。

## 2. 入口

### 2.1 命令

`music.export-midi` と同じく、Project を変えずにファイルを書く `project` 群の命令とする。

`api/table.rs` の `ProjectExportScore` の次の行:

```text
ProjectExportVisual = "project.export-visual" (ProjectExportVisualParams) -> VisualExport, project;
```

`api/params/project.rs`（`ProjectExportParams` の次）:

```rust
/// Writes the active Project's visual snapshot as JSON.
#[derive(Clone, Debug, Deserialize, Serialize, TS, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ProjectExportVisualParams {
    /// JSON file to write; an existing file is replaced.
    pub output: PathBuf,
}
```

`api/output/project.rs`（`MidiExport` の次）と、`ControlOutput` の `VisualExport(VisualExport)`:

```rust
/// Result of exporting a visual snapshot.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VisualExport {
    /// Path of the written JSON file.
    pub path: String,
}
```

| 箇所                                        | 内容                                                                                                                 |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `api/client.rs`                             | `ProjectCommand::ProjectExportVisual(params) => resolve_path(&mut params.output)?`                                   |
| `dispatcher/project.rs` / `host/project.rs` | `MusicExportMidi` と同じ形で `crate::visual::export(&session, &params)` を呼ぶ                                       |
| `crate::visual::export`                     | `Result<VisualExport, String>`。書き込み失敗は `could not write visual snapshot: {error}`                            |
| 書き込み                                    | `serde_json::to_vec_pretty` の結果を `std::fs::write` で書く。既存ファイルは置き換える（`music.export-midi` と同じ） |
| TypeScript                                  | `npm run gen:types` で `VisualExport` と `ControlOutput` を再生成する                                                |

### 2.2 CLI・MCP・スキル

```bash
riffra project export-visual --output ./visual.json
riffra --attach project export-visual --output ./visual.json
```

| 箇所                                     | 内容                                                                                                                                                                                         |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `apps/cli/src/args.rs`                   | `ProjectCommand::ExportVisual(ProjectExportVisualArgs)`。doc comment は `Write the active project's visual snapshot as JSON.`。引数は `project export` と同じ `#[arg(long)] output: PathBuf` |
| `apps/mcp/src/tools.rs`                  | `project` ツールに `action("export_visual", "project.export-visual")` を足し、説明文に `export_visual writes the active Project's visual snapshot as JSON` を加える                          |
| `agents/skills/riffra-headless/SKILL.md` | `project export-score` の行の次に `project export-visual --output <path>` の行を足す                                                                                                         |

## 3. データ契約

時刻はすべて曲頭からの秒（`f64`）で、`ProjectTimebase::ticks_to_seconds` で求める。Rust の型は `crate::visual` に置き、`Serialize` のみを derive する。直列化は `rename_all = "camelCase"`、省略可能値は `skip_serializing_if = "Option::is_none"`。

```ts
interface VisualSnapshot {
  schemaVersion: 1;
  title: string;
  durationSec: number;
  tempos: Array<{ t: number; bpm: number }>;
  meters: Array<{ t: number; bar: number; numerator: number; denominator: number }>;
  beats: Array<{ t: number; bar: number; beat: number; downbeat: boolean }>;
  sections: Array<{
    name: string;
    occurrence: number;
    startSec: number;
    endSec: number;
    key?: { tonic: string; scale?: string; tonicPc?: number; scalePcs?: number[] };
  }>;
  chords: Array<{
    startSec: number;
    endSec: number;
    symbol: string;
    degree?: string;
    rootPc?: number;
    bassPc?: number;
    tonesPc: number[];
  }>;
  tracks: Array<{
    id: string;
    name: string;
    category: 'vocal' | 'lead' | 'chords' | 'arp' | 'bass' | 'drums';
    notes: Array<{
      startSec: number;
      endSec: number;
      midi: number;
      velocity: number;
      chordRole?: 'root' | 'chordTone' | 'tension' | 'nonChord';
      top: boolean;
      lyric?: string;
    }>;
    sustain: Array<{ startSec: number; endSec: number }>;
  }>;
  drumLanes: Array<{
    trackId: string;
    pitch: number;
    label?: string;
    hits: Array<{ t: number; bar: number; step: number; velocity: number; accent: boolean }>;
  }>;
}
```

| 項目                            | 意味                                                                                          |
| ------------------------------- | --------------------------------------------------------------------------------------------- |
| `title`                         | `CreativeSession.project_name`                                                                |
| `bar` / `beat`                  | 1 始まり                                                                                      |
| `sections[].occurrence`         | 同名区間の中での 1 始まりの順番                                                               |
| `chords[].symbol` / `degree`    | 絶対表記（`Bbmaj7(#11)`）と度数表記（`bVImaj7(#11)`）。臨時記号は ASCII の `b` / `#`          |
| `rootPc` / `bassPc` / `tonesPc` | ピッチクラス（0 = C … 11 = B）。`tonesPc` は昇順・重複なし                                    |
| `tonicPc` / `scalePcs`          | 主音とスケール構成音のピッチクラス。`scalePcs` は昇順・重複なしで、スケール未指定のときは省略 |
| `notes[].midi`                  | 出力音高（§4.4）                                                                              |
| `drumLanes[].pitch`             | 出力音高                                                                                      |
| `hits[].step`                   | 小節頭からの 16 分音符単位の位置（0 始まり）                                                  |

## 4. 生成規則

### 4.1 時間軸

- `durationSec`: Render の Entire Arrangement の終端 Tick を秒にしたもの。`render/mod.rs` の `resolve_range` から `EntireArrangement` の終端計算を `pub(crate) fn entire_arrangement_end_tick(session: &CreativeSession) -> u64` として切り出し、`resolve_range` と共有する。0 のときは `visual export requires a positive-duration clip` で失敗する
- `tempos`: `timebase.tempo_changes` を順に秒へ変換する
- `meters`: `timebase.time_signature_changes` を順に秒へ変換する。`bar` は `tick_to_musical_position` の小節
- `beats`: 小節 1 から、小節頭が終端 Tick 未満の小節まで、各小節の拍子の分子ぶんの拍を `musical_position_to_tick(MusicalPosition { bar, beat, offset: 0 })` で求める。拍子変更で途中から切れる小節では、`musical_position_to_tick` が失敗した拍から後を出さない。`downbeat` は拍 1

### 4.2 セクションと調

- `sections` は `arrangement.regions` の配列順（並べ替えない）。`occurrence` は `render/sections.rs` の `score_ranges` と同じく、配列順に同名を数える
- `key` はリンク済み Score の form 出現から採る。名前と開始 Tick が一致する出現の `tonic` / `scale` を使い、一致が無い区間とリンクが無い Project では持たない。`tonicPc` は `tonic` のピッチクラスで、`tonic` が音名として読めるときだけ持つ
- `scalePcs` は `tonicPc` と `scale` の両方があるとき、そのスケールの構成音をピッチクラスへ展開する。Score の既知 12 スケールの半音オフセットは `score/scale.rs` の定義を共有し、カスタム `scale_definitions` は度数配列を `score::syntax::degree` で変換する。`scale` が未指定のときは推測せず空（JSON 省略）とする
- 突き合わせには `score_yaml/export.rs` の `Original` を使う。`Original` を `score_yaml/linked.rs` へ移して `LinkedScore` と改名し、`pub(crate) fn load(session: &CreativeSession) -> Option<LinkedScore>`（今の `original()`）、`pub(crate) fn occurrence`、`pub(crate) fn field`、`pub(crate) plan` をそこで定義する。Score 書き出しだけで使うメソッドは、`export.rs` の `impl LinkedScore` に残す

### 4.3 コード

`arrangement.harmony_events` から、重ならないコードの列を作る。

- 区間の切り直し: すべてのイベントの開始・終了 Tick で時間軸を区切り、区切りごとに、その区切りを含むイベントのうち開始 Tick が最も遅いもの（同じなら ID が小さいもの）を採る。隣り合う区切りが同じイベントなら一つにまとめる
- `tonesPc` / `rootPc` / `bassPc`: `HarmonyChord` の `tones` / `root` / `bass` のピッチクラス

`degree` と `symbol` は、`HarmonyChord.name` が Score のコード記法として読めるか（`score::syntax::chord(name)` が `Ok(Some(_))`）で分ける。

| 名前の種類                 | `degree`                                                                                                                                                                                                                       | `symbol`                                                         |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------- |
| Score 記法（`b6:maj7/5`）  | ルートの度数トークンをローマ数字にし（臨時記号はそのまま前に付ける。`b6` → `bVI`）、品質を付け、分数なら `/` と `/` の後の度数トークンのローマ数字を付ける。主音を問わず出す                                                   | `HarmonyChord.root` ＋品質＋（分数なら）`/`＋`HarmonyChord.bass` |
| それ以外（`Dm9`、`Dm9/F`） | 区間に `key` があり、`root` があり、名前が `root` の表記で始まるときだけ出す。`rootPc - tonic` を下の表で度数にし、名前からルート表記と `/` 以降を除いた部分を品質とし、`bass` があれば `/` と `bassPc - tonic` の度数を付ける | `HarmonyChord.name`                                              |

- 品質は Score 記法の `:` の後（`/` の前）の文字列。`maj` だけは空文字列として扱う（`1:maj` → `I` / `C`）
- 度数の表（主音からの半音差）: `0 I`, `1 bII`, `2 II`, `3 bIII`, `4 III`, `5 IV`, `6 #IV`, `7 V`, `8 bVI`, `9 VI`, `10 bVII`, `11 VII`
- コードの開始・終了は、その区間のイベントのものではなく、切り直した区間の Tick を秒にしたもの

### 4.4 トラックとノート

- 対象は Instrument Track のうち、ミュートしておらず、`instrument` があり、その Instrument がバイパスされていないもの。Solo とバス・マスターの状態は見ない
- その Track のミュートしていない MIDI Clip のノートを、`clip.start_tick + note.start_tick` の絶対 Tick に置く
- `loop_enabled` の Clip は、音声エンジンと同じく Clip の長さごとに繰り返す。繰り返しの開始が終端 Tick 未満のものまで出す
- 終端 Tick 以降に始まるノートは除き、終端 Tick を越えるノートの終わりは終端 Tick で切る
- 音高は `InstrumentMidiSettings::output_note` の結果。`None` のノートと、出力音高がキースイッチの音高に当たるノートは除く。演奏可能範囲外のノートは除かない
- ノートの並び順は `(startSec, midi, note id)`。Track は `arrangement.tracks` の順。ノートが 1 つも残らない Track は出さない
- `sustain`: Clip ごとに、`InstrumentControlEvent::SustainPedal { down }` と、`MidiEvent` の Control Change 64（`data2 >= 64` で踏む、それ以外で離す）を Tick 順（同じ Tick は離すが先）に並べ、踏んでから離すまでを区間にする。Clip の終わり（繰り返しではその回の終わり）で閉じる。Track 内で重なる・接する区間は一つにまとめる

### 4.5 ノート単位の意味づけ

`chordRole` は、ノート開始 Tick を含む §4.3 のコードに対して、ノートのピッチクラスとルートとの半音差 `i` で決める。コードが無いノートと、`drums` の Track のノートでは省略する。

| 条件                                                               | 値          |
| ------------------------------------------------------------------ | ----------- |
| `pc == rootPc`                                                     | `root`      |
| 構成音に含まれ、`i` が 3・4・10・11                                | `chordTone` |
| 構成音に含まれ、`i` が 7                                           | `chordTone` |
| 構成音に含まれ、`i` が 6・8、コードが `i = 7` を含まない           | `chordTone` |
| 構成音に含まれ、`i` が 6・8、コードが `i = 7` を含む               | `tension`   |
| 構成音に含まれ、`i` が 9、コードが `i = 10`・`11` を含まない       | `chordTone` |
| 構成音に含まれ、`i` が 9、コードが `i = 10`・`11` のどちらかを含む | `tension`   |
| 構成音に含まれ、`i` が 1・2・5                                     | `tension`   |
| 構成音に含まれない                                                 | `nonChord`  |
| `rootPc` が無く、構成音に含まれる                                  | `chordTone` |
| `rootPc` が無く、構成音に含まれない                                | `nonChord`  |

`top` は、同じ Track で、このノートの開始 Tick に鳴っている（開始 ≤ Tick < 終了）ほかのノートのどれよりも音高が低くないとき `true`。

### 4.6 トラック分類

上から順に、最初に当てはまったものにする。しきい値は `category.rs` の定数に置く。

| 順  | カテゴリ | 条件                                                                                                                                                                                                                                                       |
| --- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `drums`  | `score_instrument` が `kit:` で始まる。または、リンク済み Score の `plan.lanes` に `id == score_instrument` の Lane が 1 つ以上あり、それらの `sources` が 1 つ以上で、すべて `trigger` である。または、§4.4 で残ったノートがすべて `channel == 10` である |
| 2   | `vocal`  | `lyric` を持つノートがある                                                                                                                                                                                                                                 |
| 3   | `bass`   | 同時発音率 < 0.2、かつ（音高の中央値 < 55 または最低音 < 43）                                                                                                                                                                                              |
| 4   | `chords` | 同時発音率 ≥ 0.3、かつ平均音価 ≥ 1 拍                                                                                                                                                                                                                      |
| 5   | `arp`    | 同時発音率 < 0.25、かつ平均音価 ≤ 0.5 拍                                                                                                                                                                                                                   |
| 6   | `lead`   | それ以外                                                                                                                                                                                                                                                   |

- `MidiNote.channel` は 1 始まり。チャンネル 10 は、MIDI データの作者が GM の打楽器チャンネルに書いたという事実として使い、受け手の音源がそれをドラムとして鳴らすかは問わない。MIDI ファイルの取り込みは全トラックを 1 つのノート列にまとめるため、一部だけがチャンネル 10 の Track は当たらない
- GUI・CLI で作った Track は、ノートが既定でチャンネル 1 になり、ドラムであることを示す構造データも正準状態に無いため、チャンネルを指定しない限り 1 に当たらない
- 計算は §4.4 で残ったノート（出力音高・繰り返し展開後）で行う
- 同時発音率: 同じ開始 Tick のノートを 1 打点にまとめ、2 音以上の打点の数を打点の数で割る
- 音高の中央値: 昇順に並べた `(n - 1) / 2` 番目（0 始まり）
- 平均音価: `duration_ticks / ppq` の平均

### 4.7 ドラムレーン

- `drums` の Track のノートを、Track と出力音高の組ごとに 1 レーンにする。並びは Track の順、同じ Track の中は音高の昇順
- `label`:
  - キットの Track（`kit:<id>`）: リンク済み Score の `plan.kit_members` で `<id>` に属するメンバーのうち、`plan.instruments` の `trigger_note_midi` がそのレーンのノートの元の音高（`MidiNote.note`）と一致するもの。複数あれば ID が最小のもの
  - それ以外の `drums` の Track: `score_instrument`
  - リンクが無い、または一致が無いときは持たない
- `drums` の Track のノートは `tracks[].notes` にも出す
- `bar`: `tick_to_musical_position` の小節
- `step`: `(tick - 小節頭の Tick) * 4 / ppq`（切り捨て）
- `accent`: `velocity >= 110`

## 5. Score 取り込みの和声の綴り

`score_yaml/import.rs` の `harmony_events` は、和声の音名をシャープだけの表（`C`, `C#`, …, `A#`, `B`）で作っている。主音 F の `4:maj7` のルートは `Bb` ではなく `A#` になる。構成音は `syntax::voicing` で床 48 に並べた実音から作られ、分数でないコードにもベースが入る。音高は正しいので演奏は変わらないが、正準状態に残る音名と構造が `HarmonyChord::resolve` の作る和声と食い違っている。

### 5.1 規則

| 値      | 規則                                                                                                           |
| ------- | -------------------------------------------------------------------------------------------------------------- |
| `root`  | セクションの主音名（`Occurrence.tonic`）から、ルートの度数（1〜7）と、主音からの半音数を 12 で割った余りで綴る |
| `bass`  | 分数コードのときだけ、主音名から `/` の後の度数と半音数で綴る。分数でないときは `None`                         |
| `tones` | `root` から、`syntax::Chord` の構成音（ルートを含む、正規化後の定義順）の度数と半音数で綴る                    |
| `name`  | 今と同じ Score のコード記法                                                                                    |

例: 主音 Eb の `b6:maj7` は root `Cb`、tones `Cb, Eb, Gb, Bb`、bass なし。主音 F の `4:maj7/5` は root `Bb`、bass `C`。

- 綴りは `MusicalNoteName::from_interval` で行う。二重臨時記号を超える綴りになるコードは、取り込みエラー（`sections.<name>.chords`、`chord spelling requires more than two accidentals`）とする
- `harmony realize` は `bass` が無い和声を構成音の並びのまま配置するので、Score 由来の和声も CLI で入れた和声と同じ配置になる

### 5.2 変更箇所

| 箇所                                           | 変更                                                                                                                                                                                                                                      |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `riffra-core` `MusicalNoteName::from_interval` | `pub(crate)` から `pub` にし、doc comment と `# Errors` を付ける                                                                                                                                                                          |
| `score/syntax.rs` `Chord`                      | ルートとベースの度数（1〜7）を `pub(crate) root_step: u8` / `pub(crate) bass_step: u8` として持ち、`slash_bass` を `pub(crate)` にする。構成音の度数と半音数を返す `pub(crate) fn tones(&self) -> impl Iterator<Item = (u8, i32)>` を足す |
| `score_yaml/import.rs` `harmony_events`        | §5.1 の規則で作る。シャープだけの表と `syntax::voicing` の使用をやめる                                                                                                                                                                    |

保存済みの Project は書き換えない。リンクした Score を `rebuild-score` すれば、同じ関数を通って正しい綴りになる。

## 6. 置き場所

`crates/riffra-runtime/src/visual/` に置く。リンク済み Score のコンパイル結果を使うため Runtime 層にする。`VisualSnapshot` と配下の型は `ControlOutput::VisualSnapshot` として IPC に出るため `pub`、組み立てと書き出しは `pub(crate)` 以下。

| ファイル      | 内容                                                                    |
| ------------- | ----------------------------------------------------------------------- |
| `mod.rs`      | `VisualSnapshot` の型、`export`、組み立て（時間軸・セクション・ノート） |
| `harmony.rs`  | コードの切り直し、`degree` / `symbol`、`chordRole`                      |
| `category.rs` | トラック分類                                                            |
| `drums.rs`    | ドラムレーン                                                            |

単体テストは各ファイル内に置く。

## 7. テスト観点

| 観点                | 置き場所                | 確かめること                                                                                                                                                                               |
| ------------------- | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 和声の綴り          | `score_yaml/import.rs`  | 主音 Eb の `b6:maj7` → root `Cb`・tones `Cb, Eb, Gb, Bb`・bass なし。主音 F の `4:maj7/5` → root `Bb`・bass `C`。既存の取り込みテストの期待値もこれに合わせる                              |
| 時間軸              | `visual/mod.rs`         | テンポ変更・拍子変更をまたぐ拍とノートが正しい秒になる                                                                                                                                     |
| 調                  | `visual/mod.rs`         | リンク済み Score の転調で区間ごとの `key` が変わり、リンクが無いと `key` を持たない                                                                                                        |
| 実際に鳴る音        | `visual/mod.rs`         | ミュート・バイパス・キースイッチが除かれ、トランスポーズとループの繰り返しが反映される                                                                                                     |
| サステイン          | `visual/mod.rs`         | Sustain Pedal と CC64 が区間になり、Clip の終わりで閉じる                                                                                                                                  |
| コード              | `visual/harmony.rs`     | 重なるイベントが切り直される。Score 記法と絶対表記のそれぞれで `degree` / `symbol` が規則どおりになる                                                                                      |
| `chordRole` / `top` | `visual/harmony.rs`     | §4.5 の表の各行。保持中の和音の上で始まる旋律が `top` になる                                                                                                                               |
| 分類                | `visual/category.rs`    | 各カテゴリに当てはまる Track（`drums` はキット・トリガー演奏・全ノートがチャンネル 10 の 3 通り）と、一部のノートだけがチャンネル 10 で `drums` にならない Track、どれにも当たらない Track |
| ドラム              | `visual/drums.rs`       | キットの Track が音高ごとのレーンになり `label` が入る。4/4 と 6/8 で `step` が正しく、`accent` が分かれる                                                                                 |
| 命令                | `dispatcher/project.rs` | 書き出した JSON を読み直すと Project の内容になり、2 回書き出すと同じバイト列になる                                                                                                        |
