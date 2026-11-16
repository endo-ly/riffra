# Score YAML の書き出し

Project の正準状態（CreativeSession）から、読み込み直せる Score YAML の曲フォルダを作る。
書き出した YAML は、Project の中身を人とエージェントが一目で読める形であり、そのまま `import-score` や `--link` で Score を正本とする制作に移れる。

## 1. 目的と範囲

| 項目           | 内容                                                                                                                                                           |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 目的           | Data Root の Project を、Score YAML の曲フォルダとして書き出す。新しいセッションで Project を再開するとき、YAML を読めば曲の構成・音符・音源・ミックスがわかる |
| 対象           | Active Project の正準状態。Score にリンクしているかどうかを問わない                                                                                            |
| 保証すること   | 書き出した YAML を読み込むと、元の Project と同じ演奏内容になる。表現できないものは黙って捨てず、省略として報告する                                            |
| 保証しないこと | 元の作者が書いたであろう抽象（コード伴奏・アルペジオ・素材の変奏）の復元                                                                                       |

## 2. 入口

### 2.1 命令

`api/table.rs` の `project` 群に追加する。`--link` で Project を変更するため、`rebuild-score` と同じく読み取り専用にはしない。

```text
ProjectExportScore = "project.export-score" (ProjectExportScoreParams) -> ScoreExport, project;
```

```rust
pub struct ProjectExportScoreParams {
    /// Song folder to create; it must not exist.
    pub path: PathBuf,
    /// Links the Project to the exported Score.
    #[serde(default)]
    pub link: bool,
}

pub struct ScoreExport {
    /// Absolute path of the written `score.yaml`.
    pub path: String,
    /// Session content the Score cannot express.
    pub omissions: Vec<ScoreExportOmission>,
    /// Synchronization result when `link` was requested.
    pub link: Option<ScoreRebuild>,
}

pub struct ScoreExportOmission {
    /// Session object that was not exported, such as `track "Lead" / clip "Verse"`.
    pub subject: String,
    pub message: String,
}
```

- 相対パスは `api/client.rs` の `resolve_path` で、ほかのパス引数と同じように解決する
- 省略は Session の対象についての報告であり、Score の項目パス・行・列を持つ `ScoreDiagnostic` とは意味が違う。そのため別の型にする

### 2.2 CLI と MCP

```bash
riffra project export-score ./music/song
riffra project export-score ./music/song --link
riffra --attach project export-score ./music/song
```

MCP は `project` ツールに `export_score` のアクションを足す。ツールの説明文にも、書き出しがあることを足す。

### 2.3 出力先

- 出力先のフォルダは存在してはならない。既存の `score.yaml` を平らな写しで上書きしないためであり、`instrument export` と同じ規則でもある
- Score 本文とファイル一覧をメモリ上で作り、音声と音源パッケージは一時フォルダへ直接コピーしてから、最後に出力先へ名前を変える。途中で失敗したら一時フォルダを消す

```text
<path>/
├─ score.yaml
├─ states/   # VST3 の音源とエフェクトの State（PluginStateSnapshot JSON）
├─ sounds/   # 同梱品ではない内蔵音源のパッケージ
└─ audio/    # AudioClip の WAV
```

中身のないサブフォルダは作らない。

## 3. 変換の規則

### 3.1 基本方針

- **決定的に書く。** 同じ Session からは、毎回同じ本文が出る
- **推測しない。** 音符はすべて旋律文字列やドラム文字列で書く。`comp`、`pad`、`arp`、`counter`、`relative_to: chord_bass`、`c` トークンは使わない
- **読みやすさは完全一致だけで作る。** 同じ中身のセクションや素材は1つにまとめるが、似ているだけのものはまとめない
- **正準状態だけを読む。** Native 側にあって正準状態にまだ反映されていない State は含めない

### 3.2 曲の骨格

| Session                | Score                                                                                                                                                                                                                                                                         |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `project_name`         | `meta.title`                                                                                                                                                                                                                                                                  |
| テンポ・拍子の最初の値 | `meta.bpm`、`meta.meter`                                                                                                                                                                                                                                                      |
| TimelineRegion         | 重なりがなく、小節の頭で区切れていれば、開始位置の順に `form` の各出現にする。Region の間の隙間は `untitled_<n>` のセクションで埋める。重なりや入れ子がある場合、または小節の途中で区切れている場合は、Region を使わずに曲全体を `song` の1セクションにし、省略として報告する |
| Region がない          | 曲全体を `song` の1セクションにする                                                                                                                                                                                                                                           |
| 曲の長さ               | Mixdown の楽曲終端が設定されていれば、それと最後の内容の終端の大きい方。最後の小節が半端なら `last_bar_quarters` で書く。楽曲終端が未設定なら、最後の内容の終端を含む小節の終わりまでとする（読み込んだ Project では、楽曲終端がこの位置に設定される）                        |
| テンポの変更           | セクションの頭なら `form[].bpm`、それ以外は `time_changes`（`at_quarters`）。段階的な変化も個々の変更点として書き、`over_quarters` にまとめ直さない                                                                                                                           |
| 拍子の変更             | セクションの頭なら `form[].meter`。セクションの途中にあれば、その小節でセクションを分ける（`<名前>_<小節番号>`）                                                                                                                                                              |
| 主音・スケール         | 3.7 で引き継げなければ、主音は `tonic: C` にしてスケールは指定しない                                                                                                                                                                                                          |

セクション名は Region 名を Score の識別子として使える形に直したものにする。同じ名前でも中身が違うセクションは `<名前>_2`、`<名前>_3` と分ける。中身が同じかどうかは、出力するセクションの本文（音符・イベント・音声クリップ）が一致するかで決める。本文は主音を基準に書くので、主音が違う出現は自動的に別のセクションになる。

### 3.3 楽器とトラック

| Track                                         | Score                                                                                                                                                                                                                                                                                       |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Instrument Track                              | `instruments.<id>`。ID は `score_instrument` があればそれを使い、なければトラック名を識別子にした名前（例: `lead`）。識別子にできる文字が残らなければ `track_<トラックの順番>` にする。重複したら `_2` を付ける。音源が割り当てられていないトラックは書かず、音符があれば省略として報告する |
| Kit Track（`score_instrument` が `kit:<id>`） | `kits.<id>` と、トリガーノートごとの部品 `instruments.<部品>`（`kit`、`trigger_note_midi`）。部品名は 3.7 でわかる元の部品 ID を使い、わからないノートは `<kit>_<MIDI 番号>` にする                                                                                                         |
| Audio Track                                   | `instruments.<id>` に `audio: true`                                                                                                                                                                                                                                                         |
| External Audio の入力元                       | 入力元のトラックが1つの楽器にまとまる場合は `audio_input`。それ以外は省略として報告する                                                                                                                                                                                                     |
| 同じ `score_instrument` を持つ複数のトラック  | トラックごとに別の楽器として書く（`lead`、`lead_2`）。レーンのゲイン差は、それぞれの楽器の `gain_db` になる                                                                                                                                                                                 |
| ゲイン・パン                                  | 楽器の `gain_db` / `pan`。キットは `mix.tracks.<kit>`。Pan Law は下記のとおり換算する                                                                                                                                                                                                       |
| MIDI の移調                                   | 移調した後の音高で音符を書き、`transpose` は書かない                                                                                                                                                                                                                                        |

ミュート・ソロ・アーム・モニタリング・物理入力・色は作業状態なので書かない。省略としても報告しない。

#### Pan Law の換算

Score から作る Track と Bus は、すべて `LinearBalance` になる。これに対して、GUI で作る Track と Bus の既定は `EqualPower` である。Native の計算では、`EqualPower` は中央で左右とも約 −3 dB になり、`LinearBalance` は中央で左右とも 0 dB になる。そのため、パンが中央でも値をそのまま写すと音量が変わる。

パンが固定なら、左右のゲインが一致する `gain_db` と `pan` に換算できる。`a = (pan + 1)π/4`、左右のゲインを `L`・`R` とする。

| 元の Pan Law        | 左右のゲイン                                   | 換算後（`L ≥ R`）                                   | 換算後（`L < R`）                                   |
| ------------------- | ---------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `EqualPower`        | `L = cos a`、`R = sin a`                       | `gain_db` に `20 log10 L` を足し、`pan = R / L − 1` | `gain_db` に `20 log10 R` を足し、`pan = 1 − L / R` |
| `UnityCenterStereo` | `L = min(1, √2 cos a)`、`R = min(1, √2 sin a)` | 同上                                                | 同上                                                |
| `LinearBalance`     | —                                              | そのまま                                            | そのまま                                            |

- Track の音量・Bus のゲインのオートメーションがあれば、すべての点に同じ dB を足す
- Pan Law が `LinearBalance` でない Track・Bus にパンのオートメーションがある場合は、補間の形が変わって左右のゲインが一致しなくなるので換算できない。省略として報告する

### 3.4 音符

対象は、ミュートしていない MIDI クリップのノートを、曲頭からの位置に直したものである。

1. **声部に分ける。** トラックごとに開始位置の順に並べ、前の音が終わっている最も若い番号の声部に入れる。声部1は `lines.<楽器 ID>`、声部 n は `lines.<楽器 ID>_<n>` になる。和音や重なりはこれで別の声部に分かれる
2. **セクションごとに切る。** 声部ごとにセクションの範囲を1つの旋律素材にする。素材は1小節を1要素にした文字列のリストで書き、小節をまたぐ音は次の要素の先頭に `-:<長さ>` を書いて続ける。旋律素材はセクションの長さに収める決まりなので、セクションの境目をまたぐ音は素材に入れない。代わりに、始まるセクションの `placements` に `degree`、`duration_steps`、`velocity` で書く
3. **トークンにする。** 音は `<度数>[@<velocity>]:<長さ>`、隙間は `.:<長さ>` で書く。長さはステップ単位で、整数にならなければ `"p/q"` で書く。区間の境目に `|` は書かない。gate は既定の1なので、書いた長さがそのまま音価になる
4. **度数にする。** セクションの主音と `reference_octave`（既定値の4）から数えた半音数を、オクターブ記号と度数に直す。スケールがわかっていれば、スケールの音はそのスケールの綴りで書く。それ以外は次の固定の綴りで書く

   | 半音 | 0   | 1    | 2   | 3    | 4   | 5   | 6    | 7   | 8    | 9   | 10   | 11  |
   | ---- | --- | ---- | --- | ---- | --- | --- | ---- | --- | ---- | --- | ---- | --- |
   | 度数 | `1` | `b2` | `2` | `b3` | `3` | `4` | `#4` | `5` | `b6` | `6` | `b7` | `7` |

5. **Velocity をまとめる。** セクションで最も多い Velocity を、そのセクションの `velocity` にする。値が違う音にだけ `@` を付ける。最も多い値が複数あるときは、小さい方を選ぶ
6. **素材を共有する。** 素材の名前は `<セクション>_<パート>` にする。同じ中身の素材は、最初に現れた名前を共有する
7. **発音できるか確かめる。** 内蔵音源の音符は、取り込みと同じ照合（音源定義の Key / Velocity Trigger）にかける。発音できる Layer がない音符は、読み込むとエラーになるので書かずに、省略として報告する

### 3.5 ドラム

Kit Track の音符は `drums` に書く。

- セクションのすべての打撃が16分の格子に乗り、長さが1ステップで、Velocity が 64 / 96 / 120 のどれかなら、部品ごとの1小節の文字列（`o` / `x` / `X`）で書く。小節の列が繰り返しになっていれば、最も短い周期のリストにまとめる
- 1つでも当てはまらない打撃があれば、そのセクションのキットは `hit` イベント（`instrument`、`at`、`velocity`、長さ）で書く

キット以外のトラックは、打楽器の音源であっても 3.4 の旋律として書く。

### 3.6 制御イベント

| 正準                                                                        | Score                                                                            |
| --------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `SustainPedal`                                                              | `kind: sustain_pedal`                                                            |
| `PitchBend`                                                                 | `kind: pitch_bend`                                                               |
| `ModWheel`                                                                  | `kind: mod_wheel`                                                                |
| `Aftertouch`                                                                | `kind: aftertouch`                                                               |
| `ParameterChange`                                                           | `kind: parameter_change`                                                         |
| `ParameterRamp`                                                             | `kind: automation`（`parameter`、`from_value`、`to_value`、`duration_quarters`） |
| `ControlChange`                                                             | `kind: cc`                                                                       |
| VST3 トラックの MidiEvent の CC                                             | `kind: cc`                                                                       |
| `KeySwitch`                                                                 | `source.articulations` と `kind: articulation`（下記）                           |
| それ以外の MidiEvent（ピッチベンド、チャンネルプレッシャー、内蔵音源の CC） | 省略                                                                             |
| VST3 の楽器・キットにある `SustainPedal`〜`ParameterRamp`                   | 省略（Score では VST3 に送れない）                                               |

- 位置は、16分の格子に乗っていれば `{bar, step}`、乗っていなければ `{bar, offset_quarters}` で書く。同じ位置の制御は `source_order` の順に書く
- `parameter_change` と `automation` の Parameter ID と値は、内蔵音源の定義で取り込みと同じように確かめる。定義にない ID や範囲外の値は、省略として報告する
- **点の列はまとめる。** `automation` の `control`（`mod_wheel`・`aftertouch`・`pitch_bend`・`cc:N`）は、読み込むと 1/64 四分音符ごとの点の制御イベントに展開される。そのため、書き出すときは同じ種類の点が続く列を先頭から見ていき、取り込みと同じ規則で展開し直すと元の点と完全に一致する最長の区間を、1つの `automation` にまとめる。一致しない点は単発のイベントで書く。こうしないと、オートメーション1本が数百行のイベントになる

#### 奏法

`KeySwitch` を作るのは Score の奏法の展開（`score/lower.rs`）だけで、GUI や CLI の命令は作らない。そのため、`KeySwitch` を持つ Project には通常 `ScoreSource` が残っていて、元の `source.articulations` を読める。宣言は 3.7 で引き継ぎ、送信位置から奏法の区間を戻して `articulation` イベントにする。宣言が見つからない `KeySwitch` は省略として報告する。

展開では、区間の開始の 1/32 四分音符前に押下を送る。ただし、同じ音源の直前の区間の終わりよりは前に送らない。`latch` は区間の終わりで `default` に戻し、後続のキースイッチが隣接している場合と曲末では戻さない。隣接した同じキースイッチの区間は1つにつながる。区間はこの規則を逆にたどって戻す。

| 送信                    | 戻した区間                                                                                                                                  |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| `hold` の押下と解除     | 解除した位置で終わる                                                                                                                        |
| `latch` の選択          | 次の `default` への復帰で終わる。復帰がなく、次のキースイッチの選択が続く場合はその位置で終わる（隣接）。どちらもなければ曲の終わりで終わる |
| 曲頭の `default` の選択 | 区間にしない。読み込むと同じ規則で作り直される                                                                                              |

区間の開始は、押下の位置 `t` から次のように決める。

1. `t` が曲頭なら、曲頭から始める
2. 同じ音源の直前の区間が `t` で終わり、その区間との間で `default` への復帰がなく、同じ `hold` のキースイッチの解除と再押下もなければ、直前の区間に隣接させて `t` から始める
3. それ以外は、`t` の 1/32 四分音符後から始める。区間の終わりがそれより手前なら、`t` と区間の終わりの中間から始める。どちらの場合も、読み込むと押下は `t` に戻る

- セクションの境目をまたぐ区間は境目で分け、同じ名前の区間を隣接させて書く。隣接した同じキースイッチは押し直されないので、送信は変わらない
- 奏法の名前は、宣言のうちキースイッチのノートと `mode` が一致するものを使う。複数あれば、宣言の順で最初のものを使う
- CC の奏法は、送信が `cc` イベントと区別できず、`cc` で書いても送信は変わらない。そのため区間には戻さず、`kind: cc` で書く

### 3.7 ScoreSource から引き継ぐもの

`ScoreSource` を持つ Project では、`ScoreSource.yaml` に Session からは復元できない記述が残っている。これを YAML として読み（`score/parse.rs`）、次のものを引き継ぐ。`path` のない Project は取り込んだ後に編集できるので、本文と Session がずれていることがある。そのため、引き継ぐのは下表の対応が取れたものだけにする。

| 引き継ぐもの                                | 対応の付け方                                                                                                                 |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `form[].tonic`、`form[].scale`              | 開始位置の順で i 番目の Region が、元の `form` の i 番目の出現と名前・開始 Tick で一致する場合に使う                         |
| `instruments.<id>.role`、`sound`            | `score_instrument` と同じ ID の楽器                                                                                          |
| キットの部品 ID                             | 元のキットの部品で、`trigger_note_midi` が同じもの                                                                           |
| `sections.<名前>.notes`                     | 元のセクション名と同じ名前のセクション。`<名前>_2` のように分けたセクションにも付ける                                        |
| `notes`、`transitions`                      | そのまま                                                                                                                     |
| `source.articulations` のキースイッチの宣言 | `score_instrument` と同じ ID の楽器またはキット。3.6 で戻した奏法が使うものだけを書く。`latch` を使う場合は `default` も書く |

奏法の宣言は、それだけでは何も送らない。3.6 で戻した区間を書くための語彙であり、送信が二重になることはない。
それ以外の、演奏に効く項目（`source.velocity`、楽器の `articulations` の `^`・`!`・`_` の調整、`gate`、`swing` など）は引き継がない。Session の音符にはこれらがすでに反映されているので、引き継ぐと二重に効いてしまう。

### 3.8 和声

Score のコード表記は、主音からの度数で書く `<root>:<quality>[/<bass>]` である。HarmonyEvent は次の順で表記にする。

1. 3.7 で主音を引き継いだセクションで、`chord.name` が Score のコード表記として読めれば、それを使う。Score から作った HarmonyEvent の `name` は元の表記である
2. それ以外は、`root` と `bass` を 3.4 の固定の綴りで度数に直し、`root` から数えた構成音の半音の集合が一致する品質（`maj`、`m7` など）を選ぶ
3. どちらにも当てはまらなければ書かず、省略として報告する

小節の頭で1小節に1つずつ変わる場合は `chords` に、それ以外は `chord_changes` に書く。セクションの境目をまたぐ HarmonyEvent は、境目で分ける。和声は演奏しないので、音符には影響しない。

### 3.9 音源とエフェクト

| 正準                                 | Score とファイル                                                                                                                                                                                          |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 内蔵音源（同梱プリセット）           | `preset: <カタログのプリセットパス>`                                                                                                                                                                      |
| 内蔵音源（ユーザースナップショット） | パッケージを内部の管理情報を除いて `sounds/<楽器 ID>/` にコピーし、`definition` で参照する                                                                                                                |
| VST3 音源                            | `source.vst3.plugin` と `identifier`（パスからカタログを引く）、`state: states/<楽器 ID>.json`                                                                                                            |
| エフェクト                           | `effects[]` に `id` と `vst3`（`plugin`、`identifier`、`state: states/<所有者>-<id>.json`）、バイパス中なら `bypassed: true`。ID はデバイス名を識別子にした名前で、チェインの中で重複したら `_2` を付ける |
| State の中身                         | Vst3Plugin の `parameter_values` と `state_data` から作った `PluginStateSnapshot`（`schemaVersion: 2`）                                                                                                   |

- `patch` と `params` は書かない。State だけで状態が決まる
- パスがカタログにないプラグインや、`disabled_placeholder` のデバイスがある場合は、書き出し全体をエラーにする。欠落プラグインの解決（`missing relink` など）を先に求める
- 音源のバイパスは Score で書けないので、省略として報告する

### 3.10 ミックスとオートメーション

| 正準                             | Score                                                                                                                                                                                  |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Track の主出力先の Group         | `mix.tracks.<id>.output`                                                                                                                                                               |
| Send                             | `mix.tracks.<id>.sends.<return>`。既定の post・enabled なら dB の数値だけで書く                                                                                                        |
| Group / Return                   | `mix.buses.<id>`（`kind`、`gain_db`、`pan`、`effects`）。`gain_db` と `pan` は 3.3 の Pan Law の換算を通す                                                                             |
| SidechainSend                    | `mix.sidechain[]`                                                                                                                                                                      |
| Master Gain、Master のエフェクト | `mix.master.gain_db`、`mix.master.effects`                                                                                                                                             |
| Loudness Mastering の目標        | `meta.master`。Score が持たない Mixdown の項目は省略として報告する                                                                                                                     |
| AutomationLane                   | `mix.automation[]`。Point は、位置を含むセクションの出現ごとに `section`・`occurrence`・`at` で書く。曲の終端にある Point は、最後のセクションの終端に置く。`interpolation` はそのまま |
| DeviceParameter の対象           | Stable Parameter ID を、プラグインのパラメーター名（重複する名前は `<名前>#<index>`）に直す。名前の照会は取り込みと同じプラグインスキャナーで行う                                      |

Score が作ったセクション別ゲインの Lane も、ほかの Lane と同じく明示的なオートメーションとして書く。

### 3.11 音声クリップ

- ミュートしていない AudioClip を、開始位置を含むセクションの `audio_clips` に書く。`at` は 3.6 と同じ形で書き、`offset_seconds` / `duration_seconds` は `source_range` から、`gain_db`、`pan`、フェードはそのまま写す
- Asset の内容を `audio/<Asset 名を識別子にした名前>.wav` にコピーする。同じ Asset は1つのファイルを共有する

### 3.12 省略として報告するもの

3.2〜3.11 で挙げたもののほかに、次のものも省略として報告する。

- ミュートしたクリップ、ループを有効にしたクリップ
- `processed` の録音テイクを使う AudioClip
- MIDI クリップのゲイン、パン、フェード
- ノートの歌詞
- 演奏可能音域、Keyswitch の設定

報告は、Track やクリップの名前で対象がわかるように書く。

## 4. `--link`

`--link` を付けると、書き出した Score を元の Project にリンクし、それ以後はこの YAML が Project の正本になる。

### 4.1 条件

- リンクできるのは、Score のファイルにリンクしていない Project である。`ScoreSource` がない Project と、`ScoreSource` はあっても `path` がない Project（`.riffra` から取り込んだものなど）が対象になる
- `path` を持つ Project は、Score が持つ部分を直接編集できない（`riffra-core` の `ensure_score_owned`）。リンク先の YAML がすでに正本なので、`--link` は拒否する
- 省略が1件でもあればリンクせず、ファイルも書かずに、省略の一覧をエラーとして返す。リンクすると Project は YAML から作り直されるので、省略したものが Project から消えてしまうためである

### 4.2 手順

1. 2.3 のとおり曲フォルダを書き出す
2. 再構築と同じ処理で `score.yaml` を読み込み、元の Project を前の状態として照合する。照合の前に、3.3 で決めた楽器 ID を各 Track の `score_instrument` に入れておく。レーンとトラックの照合は楽器 ID が一致すれば成り立つので、Track ID・名前・作業状態が保たれる
3. 確定は `rebuild-score` と同じ経路（Standalone の dispatcher と Live Host の両方）を通す。履歴の区切り、`sequence`、ファイルの監視の開始も同じになる

読み込みや確定に失敗した場合は、書き出した曲フォルダを残し、Project は変更しない。エラーには再構築の診断をそのまま返す。

### 4.3 リンクで変わるもの

リンクでは、Project が Score から作り直される。省略がなくても、次のものは Score の形に変わる。

| 変わるもの                         | 変わり方                                                                                           |
| ---------------------------------- | -------------------------------------------------------------------------------------------------- |
| MIDI クリップ                      | トラックごとに、曲全体を覆う1つのクリップにまとまる。音符と制御は変わらない                        |
| Track と Bus の Pan Law            | `LinearBalance` になる。3.3 の換算で、左右のゲインは変わらない                                     |
| デバイスの ID                      | Score の規則で振り直される。そのため、リンクの直後に一度だけ VST3 のインスタンスが読み込み直される |
| Score の外で作ったオートメーション | Score のオートメーションとして持たれる                                                             |

### 4.4 実装

`score_yaml/import.rs` の `rebuild` は、`ScoreSource` のない Project を拒否する今の動作のままにする。`--link` には、`rebuild` の内部処理（`rebuild_with_probe`）を共有する `link` 関数を `pub(crate)` で足す。`rebuild` との違いは、前の `ScoreSource` を要求しないことと、読み込むパスを必ず受け取ることである。

## 5. モジュール構成

```text
crates/riffra-runtime/src/
├─ score/
│  └─ emit.rs             # 新規。Score の文書（serde_json::Value）を YAML の文字列にする
└─ score_yaml/            # score_yaml_import から名前を変える
   ├─ mod.rs              # mod import; mod export; mod parameters; mod plugin_source;
   ├─ import.rs           # 今の score_yaml_import.rs
   ├─ export.rs           # 入口、骨格（3.2）、楽器 ID（3.3）、引き継ぎ（3.7）、曲フォルダの書き出し、--link
   ├─ export/
   │  ├─ performance.rs   # 音符・ドラム・制御イベント・和声（3.4〜3.6、3.8）
   │  └─ production.rs    # 音源・エフェクト・ミックス・オートメーション・音声クリップ（3.9〜3.11）
   ├─ parameters.rs       # 今のまま
   └─ plugin_source.rs    # 今のまま
```

- `score/emit.rs` は Project を知らない言語の層に置く。フロー形式の判断は次のとおり
  - スカラーだけを持ち、1行に収まるマップと配列（`form` の各出現、`at`、`effects` の要素など）は `{...}` / `[...]` で書く
  - それ以外はブロック形式で書く
  - 旋律文字列は常に引用符で囲む
- `score/mod.rs` にあるテスト専用の `to_yaml` は削除し、テストでも `emit` を使う
- 公開範囲は `pub(crate)` までにする。`export` の外から使うのは、入口の関数と `link` だけである

## 6. 不変条件

- 書き出した `score.yaml` は `score validate` でエラーにならない
- 同じ Session からは、同じ本文とファイルが出る
- 書き出した YAML を読み込んだ Project は、元の Project と同じ演奏内容になる。ここでいう演奏内容は次のものであり、ID・PPQ・クリップの分け方は比べない。省略として報告したものは除く
  - 音符、制御、キースイッチの送信、音声クリップ、オートメーション、テンポと拍子（位置は四分音符単位）
  - ミックスの構造と、Track・Bus の左右のゲイン（Pan Law の換算は浮動小数の誤差の範囲で一致すればよい）
- 書き出しは、`--link` を付けない限り Project を変更しない
- 出力先が存在すれば、何も書かずにエラーにする

## 7. テスト

テストは対象の実装と同じファイルに書く。

| 置き場所                | 観点                                                                                                                                                                                                                                                                                                                                                                      |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `score/emit.rs`         | フロー形式とブロック形式の選び方、文字列の引用。出力をもう一度読み込むと同じ Value になる                                                                                                                                                                                                                                                                                 |
| `export/performance.rs` | 声部の分け方、小節とセクションの境目をまたぐ音、半端な長さ、度数の綴り（スケールあり・なし）、ドラムの格子と `hit` の切り替え、点の列の `automation` へのまとめ（一致しない点が残る場合を含む）。`hold` と `latch` の奏法（隣接する区間、曲頭、`default` への復帰、曲末を含む）を読み込み、書き出し、もう一度読み込んだとき、キースイッチの送信の位置と順序が元と一致する |
| `export.rs`             | **往復**: 旋律・キット・音声トラック・バス・Send・オートメーション・テンポ変更を含む1曲の Score を読み込み、書き出し、もう一度読み込んだ結果が、6 の意味で最初の Project と一致する。さらに、2回目の書き出しの本文が1回目と一致する                                                                                                                                       |
| `export.rs`             | 引き継ぎ: `ScoreSource` を持つ Project で、主音・スケール・`role`・`sound`・`notes`・奏法の宣言が残る。Region を編集して元の `form` と合わなくなった出現では、主音を引き継がない                                                                                                                                                                                          |
| `export.rs`             | 省略: ミュートしたクリップと歌詞を含む Project で、省略が報告され、出力は読み込める。`--link` は省略があると拒否され、何も書かない                                                                                                                                                                                                                                        |
| `export.rs`             | 出力先が存在すると拒否される                                                                                                                                                                                                                                                                                                                                              |
| `export/production.rs`  | Pan Law の換算: `EqualPower` と `UnityCenterStereo` の Track（パンが中央・左・右）を書き出して読み込むと、左右のゲインが元と一致する。音量のオートメーションの点にも同じ dB が足される                                                                                                                                                                                    |
| `host/project.rs`       | `--link` で同じ Project がリンクされ、Track ID と名前が保たれ、`sequence` が進む。`path` を持つ Project では拒否される                                                                                                                                                                                                                                                    |

往復テストの準備（音源カタログ、プラグインスキャナーの代わり）は、取り込み側のテストと同じものを使う。

## 8. ドキュメント

- `docs/score-yaml.md` に「書き出し」の章を足す。位置は「正本とのリンク」「再構築」の後とし、命令、出力先の規則、変換の方針（推測しない、省略を報告する）、`--link` を書く
- `agents/skills/riffra-headless/references/commands.md`: Score の段落に `project export-score` を足す
- `agents/skills/riffra-headless/SKILL.md` と `agents/skills/composition/references/score-workflow.md`: Score にリンクしていない Project を再開するときは、まず `export-score` で中身を読み、YAML で作業を続けるなら `--link` すると書く
- `docs/mcp.md`: `project` ツールの説明を、アクションを列挙している箇所に合わせて更新する

## 9. 対象外

- コード伴奏・アルペジオ・対旋律・素材の変奏の推測による復元
- Desktop のメニューからの書き出し
- 書き出した YAML を Sonalloy で鳴らせること。VST3 だけの楽器には Sonalloy 用の音源指定がない

## 10. 禁止事項

- 既存のファイルやフォルダへの上書き
- 表現できないものを、報告せずに捨てること
- 演奏に効く項目を `ScoreSource.yaml` から引き継ぐこと（奏法の宣言は 3.7 のとおり例外）
- `--link` で、省略のある Score をリンクすること
- 書き出し専用の要約形式など、Score YAML 以外の表現を足すこと

## 11. コミットの分け方

1. `refactor(runtime): move score_yaml_import into score_yaml/import`（移動と参照の付け替えだけ）
2. `feat(runtime): emit score documents as YAML`（`score/emit.rs`、テスト用 `to_yaml` の置き換え）
3. `feat(runtime): export session performance as score YAML`（3.2〜3.8）
4. `feat(runtime): export production and song folder files`（3.9〜3.12、曲フォルダの書き出し）
5. `feat: add project export-score command`（命令、CLI、MCP、`--link`、TS 型の再生成）
6. `docs: describe score YAML export`
