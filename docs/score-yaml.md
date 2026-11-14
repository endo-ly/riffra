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

`--bind` は `external: true` の楽器へ Definition を結び付ける。繰り返し指定でき、指定できるのは外部楽器だけである。CLI の `--bind` に指定する相対パスはコマンドの作業ディレクトリを基準に解決する。Score 内の `definition`、State、WAV の相対パスは Score ファイルのディレクトリを基準にする。
プリセットは Riffra に同梱されたカタログの対応する ID を利用する。

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
Score 本文が同じでも、参照している State・Definition・WAV の内容が変われば解決結果を比較し、実行状態が変わったときだけ確定する。読み込み中に Score 本体または参照ファイルが書き換わった場合は、その候補を適用しない。

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

## 楽譜の構造

Score は `meta`、`form`、`sections`、`instruments` を必須とする。`form` にセクションの再生順・小節数・主音を書き、`sections` にその演奏内容を書く。同じセクションを `form` に複数回置くと、それぞれの位置に展開される。

| 項目                                                      | 内容                                                           |
| --------------------------------------------------------- | -------------------------------------------------------------- |
| `meta`                                                    | 曲名、テンポ、拍子、調律、基準オクターブ、音価とマスター設定   |
| `form`                                                    | セクションの出現順、小節数、主音、任意のスケール・拍子・テンポ |
| `time_changes`                                            | 曲頭からの四分音符位置によるテンポ変更                         |
| `sections`                                                | コード進行、各パート、配置、演奏イベント                       |
| `melodies`、`drums`、`comp`、`arps`、`counters`、`macros` | 名前付きの演奏素材                                             |
| `scale_definitions`                                       | 組み込み以外のスケールの定義                                   |
| `instruments`、`kits`                                     | 楽器とキットの音源、演奏設定                                   |
| `mix`                                                     | トラック、バス、センド、エフェクト、オートメーション           |
| `transitions`、`notes`                                    | 再生に影響しない制作メモ                                       |

```yaml
meta: { title: Example, bpm: 120, meter: 4/4, tuning: 12edo }
form:
  - { section: verse, bars: 1, tonic: C, scale: ionian }
sections:
  verse:
    lines:
      main: { instrument: lead, melody: theme }
melodies:
  theme: ["1 - - - | 3 - - - | 5 - - - | 1' - - -"]
instruments:
  lead:
    source: { vst3: { plugin: Example Synth } }
```

`meta.bpm` は四分音符毎分、`meter` は拍子、`tuning` は `12edo`。`reference_octave` は度数 `1` の基準オクターブ、`gate` は音符の基本音価に掛ける倍率である。`form` の各項目は `section`、正の整数の `bars`、`tonic` を持ち、`scale`、`bpm`、`meter`、`last_bar_quarters`、`swing` を指定できる。テンポと拍子の変更は後続のセクションに引き継がれる。

`scale` の組み込み名は `ionian`、`dorian`、`phrygian`、`lydian`、`mixolydian`、`aeolian`、`locrian`、`harmonic_minor`、`melodic_minor`、`major_pentatonic`、`minor_pentatonic`、`whole_tone` である。独自のスケールは `scale_definitions` に度数の配列として定義する。省略した場合はスケール未指定となる。

内蔵音源は楽器の `preset` で同梱カタログから選ぶか、曲フォルダ内の `definition` を指定する。`external: true` の楽器には CLI の `--bind` で Definition を渡す。音源の指定に加えて、楽器には `gain_db`、`pan`、`gate`、`swing`、`timing_offset_steps`、音域や奏法の設定を指定できる。

## 音高とスケール

### 度数の意味

`1`は`tonic`、`2`は長2度、`3`は長3度、`4`は完全4度、`5`は完全5度、`6`は長6度、`7`は長7度。12平均律では、主音からの半音数は順に **0、2、4、5、7、9、11**。

| 記法 | 主音Cでの実音      | 定義                   |
| ---- | ------------------ | ---------------------- |
| `1`  | C                  | 基準オクターブの主音   |
| `b2` | Db                 | 短2度                  |
| `#1` | C#                 | 増1度                  |
| `b3` | Eb                 | 短3度                  |
| `#4` | F#                 | 増4度                  |
| `b6` | Ab                 | 短6度                  |
| `b7` | Bb                 | 短7度                  |
| `1'` | C（1オクターブ上） | `reference_octave` + 1 |
| `7,` | B（1オクターブ下） | `reference_octave` − 1 |

度数トークンの文法：`[b|#|bb|##]? [1-7] [ '... | ,... ]?`（空白なし）。`'`は1個につき1オクターブ上、`,`は1個につき1オクターブ下。混用は無効。`bb` / `##` はそれぞれ2半音変更。音符・コードで同じ半音計算を使う。`#1` と `b2` は12平均律では同じ高さだが、解析結果には元の音程綴りを保持する。出力MIDIは音高だけを保持する。

`tonic: A, scale: aeolian`でもA＝`1`、C＝`b3`、F＝`b6`、G＝`b7`。AドリアンならF#＝`6`。**実音を決めるのは基準音と音符トークン**であり、`scale`は作曲判断や表示・分析の参考とする。スケールに含まれない度数もそのまま発音する。

### 音域

`reference_octave: 4` の場合、セクション`tonic: Eb`では `1`＝Eb4（MIDI63）、`7,`＝D4（MIDI62）、`1'`＝Eb5（MIDI75）。`tonic: A`なら `1`＝A4（MIDI69）。ベースなどの低音域は、オクターブ記号を重ねる代わりに旋律パートの`register: bass`で配置し、それ以外の音域調整は`transpose`（半音単位）で行う。

`voicing_floor_midi` / `voicing_ceiling_midi` は和音の配置に使う**技術的な範囲**であり、旋律の音符記法ではない。コード構成音は度数から決め、その音域配置だけをMIDIノート番号で指定する。

### 基準音・音域の選択・主音の上書き

旋律素材と`placements`の度数は、**基準音**からの度数として音高になる。

| 決める項目 | 指定箇所                                          | 値               | 意味                                                                                                                                 |
| ---------- | ------------------------------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| 基準       | 旋律素材の`relative_to`                           | `tonic`（既定）  | 主音を基準にする                                                                                                                     |
|            |                                                   | `chord_bass`     | 発音位置のコードのベース音を基準にする。分数コードはスラッシュ指定の音。`NC`上ではエラー                                             |
| 音域       | `lines.<名前>.register` / `placements[].register` | `melody`（既定） | 基準音の音高クラスを`reference_octave`のC〜Bに置く                                                                                   |
|            |                                                   | `bass`           | 基準音の音高クラスを、楽器の`bass_floor_midi`～`bass_ceiling_midi`（既定28～39）で最も低い位置に置く。範囲内に該当音がなければエラー |
| 主音       | `lines.<名前>.tonic` / `placements[].tonic`       | 音名             | その演奏に限り、セクションの`tonic`の代わりにこの主音で度数を解釈する                                                                |

音高は「配置した基準音 ＋ 度数（臨時記号・オクターブ記号を含む）＋ `transpose`」で決まる。度数適用後の音高は音域へ折り返さず、最終MIDIノート0～127を検証する。

`c`を付けたトークン（`c1`〜`c13`）は、その位置のコード構成音を指定する。`c3`はコードの長短に応じた3度を指し、`relative_to`ではなく**コードルート**を基準にする。コードルートの音高クラスを`register`の規則で配置し、その構成音の音程（複合音程を含む）とオクターブ記号を加える。臨時記号は同じ度数の変化音を特定する（`cb9` / `c#9`）。指定した度数がコードにない場合、複数の構成音に当てはまる場合、`NC`上で使った場合は検証エラーになる。`c`トークンはどの旋律素材でも使え、同じ形をコードに合わせて移すシーケンスを書ける。

主音の上書きは、調と関係なく鳴る物、転調をまたぐペダル音、別の調で書いた素材の引用に使う。`relative_to: chord_bass`の素材に`tonic`を指定するとエラーとする。`c`トークンとコードは、上書きした主音ではなくセクションの和声に従う。

## 時間とパターン

### 共通時間軸

演奏イベントの時間は**有理数の四分音符**で計算し、出力段階でTickへ変換する。標準的な4/4では1小節16ステップ、1ステップ＝16分音符。`bar`と`step`は**1始まり・セクション内相対**とする。`{bar: last}`は末尾の小節、`step: 13`は4拍目の先頭。

拍子`n/d`では、1小節は `n × 4/d` 個の四分音符に相当する。分母の音符1個分を**区間**と呼ぶ（4/4なら四分音符、6/8なら八分音符）。区間は記譜上の単位で、強拍を意味しない。連符の分数長は丸めず保持する。

`form`の開始位置を順に足すことで曲全体の小節位置と経過時間が確定する。曲中のテンポ・拍子変更は`form`のセクション開始時点、または途中のテンポだけを扱う`time_changes`へ記述し、音符の四分音符基準の位置は維持する。

### パターン文字列の共通文法

旋律素材の要素、ドラム、コード伴奏（`comp`）、アルペジオの`order`は、すべて同じ時間文法で書く。

- **トークン**：旋律・`comp`・`order`は空白区切り、ドラムは1文字が1トークン（空白は無視）。
- **長さ**：空白区切りのトークンは末尾に`:<長さ>`を付けられる。長さはステップ（16分音符）単位の正の整数または`"p/q"`形式の分数で、省略時は1。`5:6`と`5 - - - - -`は同じ演奏になる。ドラムのトークンは常に1ステップ。
- **連符**：`[1 2 b3]`は角括弧内の要素で1区間を等分する。`[1 2 b3]:4`のように括弧の後へ長さを付けると、その長さを等分する。長さを省略した連符は区間の境界から始まらなければならず、小節の残りが1区間より短い場合は残りの長さを等分する。括弧内のトークンには長さを付けられない。ドラムの連符（`[oooxxx]`）は長さを省略した形だけを使う。
- **区間の表明`|`**：その位置が区間の境界であることを表明する。
- **小節線の表明`||`**：その位置が小節の境界であることを表明する。

表明は任意であり、書いた位置が境界でなければエラーとする。トークンの間にだけ置け、文字列の先頭・末尾や連続した表明は書けない。境界は小節の先頭から数え、端数小節の終端も境界である。音がまたがる境界には表明を書かない。

ドラム・`comp`・`order`の1文字列は1小節を表し、その長さは小節長と一致しなければならない。旋律素材の要素は任意の長さを持てる。

### 旋律文字列

```yaml
melodies:
  hook:
    - '3:2 2 3 | -:2 5:2 | 4:3 2 | 2:2 .:2'
  hook_cells:
    - "5@110:3 2:3 3:3 .:3 | 6@112:3 3:3 5:3 .:3 | 7@114:3 3:3 5:3 .:3 | 1'@116:3 5:3 3:3 .:3"
```

`hook_cells`は3拍のまとまりを4回続けた3小節の要素であり、小節線をまたいだまとまりがそのまま見える。

トークンは次の順に並べる（空白なし）：

```text
<本体> [<奏法記号>...] [@<velocity>] [:<長さ>] [~]
```

- **本体**：度数、`c`トークン、`-`、`.`。
  - 度数・`c`トークン：その位置で新しい音を発音する。同じ度数が隣り合っても発音し直す。
  - `-`：直前の音を継続する。要素の先頭では前の要素から継続していてもよい。継続対象がないときはエラー。
  - `.`：休符。継続中の音を終了する。
  - `-`と`.`は長さだけを持てる。
- **奏法記号**：`^`（アクセント）、`!`（スタッカート）、`_`（テヌート）。1つの音に各記号を1回まで付けられる。`!`と`_`は同時に使えない。効き方は4.4で定める。
- **Velocity**：`@`の後に整数1～127。省略時はセクションの`velocity`（既定100）。
- **レガート**：末尾の`~`は次音へのレガート接続。

音符の基本長は、トークンの長さと後続の`-`の合計で決める。Gateはスタッカート・テヌートの記号があればその奏法のGate、なければ楽器の`gate`、`meta.gate`、既定値1の順に選び、基本音価に一度だけ掛ける。値は0より大きく1以下とする。ドラム・コード伴奏・アルペジオも、楽器の`gate`、`meta.gate`、既定値1の優先順位を使う。

`~`の接続元はGateによる短縮を受けず、次音の開始まで保持して短く重なる。重なりは接続先の発音中に収める。休符を越える接続、接続先のない`~`、`!`・`_`との併用はエラー。単音レガート・ポルタメントの音高遷移は音源定義の演奏設定に従い、ポリフォニック音源では音符の重なりとして演奏する。独立した同時発音や精密な音価は`placements`の構造化イベントを使用する。

### 奏法

奏法記号は、Gateの選択とVelocityへの加算として効く。

| 記号 | 奏法       | Gate       | Velocityへの加算 |
| ---- | ---------- | ---------- | ---------------- |
| `^`  | `accent`   | 変更しない | 既定 +16         |
| `!`  | `staccato` | 既定 `1/2` | 既定 0           |
| `_`  | `tenuto`   | 既定 `1`   | 既定 0           |

楽器ごとに`instruments.<id>.articulations`で上書きできる。`accent`は`velocity`だけ、`staccato`と`tenuto`は`gate`と`velocity`を持てる。`velocity`は-126～126の整数、`gate`は0より大きく1以下。

```yaml
instruments:
  pizz:
    {
      preset: BASS/004-short-pluck-bass,
      articulations: { staccato: { gate: '1/4', velocity: -8 }, accent: { velocity: 24 } },
    }
```

音符のVelocityは「`@`またはセクションの`velocity`」に奏法の加算と`dynamics`の加算を足した値である。結果が1～127の外になる場合はエラーとし、範囲内へ丸めない。

### ドラム文字列

```yaml
drums:
  four:
    parts:
      kick: 'X... | X... | X... | X...'
      hat: 'XxXx | XxXx | XxXx | XxXx'
  phrase:
    parts:
      kick:
        - 'X... | ..X. | X... | ....'
        - 'X... | ..X. | X.X. | ..X.'
```

ドラム1マスは`.`（休み）、`o`（弱）、`x`（通常）、`X`（アクセント）。基本Velocityは`o=64`、`x=96`、`X=120`、発音長は1グリッドマス。`[oooxxx]`なら1区間を6等分し、各文字で同じ長さの音を発音する。音の種類は`parts`のキーが参照する音源で指定する。ドラム以外のメロディ・コード伴奏のVelocityとは独立する。

`parts.<楽器>`は、1小節の文字列か、小節の文字列のリストを取る。文字列はセクションの全小節で繰り返し、N個のリストはN小節のフレーズとして繰り返す。セクションの小節数がNの倍数でない場合はエラーとする。楽器ごとにNは異なってよい。

### スウィング

`meta.swing: {subdivision: 8, ratio: "2/3"}`で8分スウィング、`subdivision: 16`で16分スウィングを指定する。`ratio`は0より大きく1より小さい有理数で、`1/2`は均等、`2/3`は2対1。`form[].swing`は、そのセクションの出現だけで全体設定を上書きする。`instruments.<id>.swing`は、その楽器について`meta`と`form`の設定をすべての出現で上書きする。ほかの楽器が跳ねている中でまっすぐ弾く楽器には`ratio: "1/2"`を指定する。

旋律パート（`lines`）、ドラム、コード伴奏、アルペジオ、対旋律の格子位置と基本音価へ適用する。変換は小節ごとに行い、小節境界を固定する。小節をまたぐ音は開始と終了をそれぞれの小節で変換する。小節末の不完全な組は均等なまま扱う。連符は組の開始・終了を変換して内部を等分する。変換後に継続・Gate・レガートを解決する。`placements`と`events`の明示位置・音価は保持する。

### タイミングのずれ

`instruments.<id>.timing_offset_steps`は、その楽器の全イベントを指定ステップだけ前後にずらす（正で後ろ、負で前。符号付きの有理数）。スネアを少し遅らせる後ノリ、ハットを前に出す前ノリなど、パート間のずれを書く。

ずれはセクション・コード参照・リードの休み判定・`silence`・`part_window`・`dynamics`をすべて記譜上の時刻で解決した後に適用する。ずらした開始時刻が曲頭より前、または曲終端以降になる場合はエラーとし、音符の終端が曲終端を越える場合は曲終端で切る。

## コードと伴奏

### コード記法

1つのコードは `<root>:<quality>[/<bass>]` の形を取る。`root` と `bass` は「音高とスケール」で定める相対度数で、コードの構成音は`root`からの相対半音数で計算する。`bass`を省略した場合はルート音。`NC`はコードなしであり、コード参照演奏を発音させない。

| 種類    | 構成音（ルートからの半音数） |
| ------- | ---------------------------- |
| `maj`   | 0, 4, 7                      |
| `m`     | 0, 3, 7                      |
| `dim`   | 0, 3, 6                      |
| `aug`   | 0, 4, 8                      |
| `maj7`  | 0, 4, 7, 11                  |
| `m7`    | 0, 3, 7, 10                  |
| `7`     | 0, 4, 7, 10                  |
| `m7b5`  | 0, 3, 6, 10                  |
| `dim7`  | 0, 3, 6, 9                   |
| `sus4`  | 0, 5, 7                      |
| `7sus4` | 0, 5, 7, 10                  |

基本構造に6度、7度、9度、11度、13度を組み合わせて指定できる。

| コード名                   | 構成の規則                                           |
| -------------------------- | ---------------------------------------------------- |
| `sus2`                     | 1, 2, 5                                              |
| `6` / `m6`                 | 長三和音・短三和音に6度を追加                        |
| `mMaj7`                    | 短三和音に長7度を追加                                |
| `9` / `11` / `13`          | 長三和音に短7度と、9度・11度・13度を指定段階まで追加 |
| `maj9` / `maj11` / `maj13` | 長三和音に長7度と、指定段階までの拡張音を追加        |
| `m9` / `m11` / `m13`       | 短三和音に短7度と、指定段階までの拡張音を追加        |
| `add9` / `madd9`           | 長三和音・短三和音に9度を追加                        |

9、11、13は複合音程として保持する。コード品質の後ろに、`1:7(b9,#11,omit5)`のように修飾子を付けられる。

| 修飾子                               | 意味                                               |
| ------------------------------------ | -------------------------------------------------- |
| `add9`, `add11`, `add13`             | 指定度数を追加                                     |
| `b5`, `#5`, `b9`, `#9`, `#11`, `b13` | 同度数の通常音を変化音に置換。通常音がなければ追加 |
| `omit3`, `omit5`, `omit7`            | 指定度数の構成音を省略                             |

追加、変化、省略の順で正規化し、同一構成音の重複は除く。`b9`と`#9`など異なる変化音は共存できる。同じ度数の変化と省略の併用はエラー。

`1:(1,b3,5,b7,#11)`は構成音の直接指定。括弧内はコードルート基準の1～13度と、`b`・`#`・二重臨時記号を使う。基本コード名と同じボイシング指定を利用できる。

例：`tonic: Eb`で `5:7/7` はB♭7/D、`tonic: A`で `1:m` はAm、`tonic: C`で `#1:dim7` はC#dim7。`/`の後も**コードルートからの音程ではなくセクション主音からの度数**。コードのルートと構成音がスケール外でも許容する。

`chords`の各要素は1小節を表し、1小節のコードが1個なら全小節、2個なら前後半の同じ長さで割り当てる。より細かい和声変化はセクション内`chord_changes: [{at: ..., chord: ...}]`を用いる（時刻の競合はエラー）。

**食い**：`chords`のコードの先頭に`<`を付けると、そのコードは本来の位置より`<`の個数ぶんのステップだけ前から入る。`["4:maj9", "<3:m7"]`では2小節目の`3:m7`が16分1つ前から、`"<<3:m7"`なら8分1つ前から鳴る。和声上はその小節に属したまま、切り替わる時刻だけを前に出す。前のセクションへ食い込んでもよい。食い込んだ時刻が直前のコード変更以前になる場合、曲頭より前になる場合はエラーとする。コードを参照するすべての演奏（コード伴奏、アルペジオ、`chord_bass`基準の旋律、`c`トークン）は、切り替わった時刻からそのコードを使う。

### ボイシング

和音を音高へ配置する方法は、**明示配置**と**自動配置**の2つである。

`voicing: ["1", "3", "7", "9'"]`で演奏する構成音とオクターブを選ぶと明示配置になる。`3`はコード品質に含まれる3度を指し、短三和音では短3度になる。同じ度数の変化音が複数ある場合は`b9`や`#9`で特定する。含まれない音や曖昧な指定はエラーとする。指定先は`instruments.<id>.voicing`、`sections.<id>.comp.<instrument>`の`{pattern, voicing}`形式、`pad`、`arp`、`placements[]`。使用箇所、楽器既定の順に優先し、どちらもなければ自動配置を使う。

明示配置では、下限以上の指定ベース音からコードルートの配置基準を決め、各構成音の複合音程とオクターブ移動を適用して音高順に並べる。配列から外した構成音は演奏せず、分数コードのベースは最低音として保持する。オクターブ移動後の構成音が指定ベースより低くなる場合はエラーとし、音高を自動補正しない。明示配置はコードが変わっても同じ形のまま移動するため、平行移動させたい和音（スタブ、プレーニング）に使う。

自動配置は、コードの構成音を奏者のように**直前の和音から最小の動きでつなぐ**。

1. **ベース音**：`voicing_floor_midi`（既定48）以上で最も低い、コードのベース音の位置に置く。
2. **上声**：ベース音と異なる音高クラスの構成音を、ベース音より上、`voicing_ceiling_midi`（既定は下限＋24）以下に1音ずつ置く。`comp`、`pad`、`arp`の自動ボイシングは上声を最大6音高クラスまで扱う。これを超えると検証エラーになるため、演奏音を選ぶには明示配置を指定する。自動配置は構成音を音高クラスとして扱い、複合音程の高さを固定したい場合は明示配置を使う。配置できる位置がない構成音があればエラーとする。
3. **基準配置**：ベース音の直上に1～7度を最も近い位置へ、8度以上をベース以上で最も低いコードルートから複合音程のまま積んだ配置。例えば下限48の`1:9`は`48,52,55,58,62`となる。同じ音高は除く。
4. **選択**：上声の候補のうち、直前の上声との距離が最小のものを選ぶ。距離は「各候補音から直前の上声で最も近い音までの半音数の合計」と「直前の上声の各音から候補で最も近い音までの半音数の合計」の和とする。同点の場合は最高音が低い候補、さらに同点なら昇順に並べた音高列が辞書順で小さい候補を選ぶ。
5. **直前の和音**：同じ演奏（`comp.<楽器>`、`pad`、`arp`のそれぞれ）がそのセクションの出現内で直前に自動配置した和音。セクションの出現の最初の和音と`NC`の直後の和音では、そのコードの基準配置を直前の上声とみなす。基準配置が範囲内に収まれば、最初の和音は基準配置そのものになる。

`placements[]`の`chord`は単発の和音であり、自動配置では前後の配置を参照せず基準配置をそのまま使う。

配置結果は`score inspect`で確認できる。和音から生成した各音の`sources[].spelling`には、元のコード表記が入る。

### 伴奏・アルペジオ・対旋律

- `comp` の `x` は、現在のコードをその位置で発音。`-`で保持、`.`で休止。組込みパターン`hold`は各小節頭でその時点のコードを発音して1小節持続し、1小節中にコードが替わった地点で前の和音を終了して新しい和音を発音し直す。食いで小節頭より前に入ったコードは、同じセクションの出現の中であれば、その小節頭で発音し直さずに保持を続ける。`hold`は`comp`での定義・上書きができない予約名である。
- `arps.<name>.order`の各セルは、5.2のボイシングの構成音を下から並べた音列のインデックス（非負整数）か、`.`（休符）、`-`（直前の音の継続）である。`-`は直前に発音した同じ音高の音価を延長し、継続中にコードが変わっても発音済みの音高は変わらない。小節をまたぐ継続も許可し、先行する発音がない`-`はエラー。コード音が4個未満の場合は構成音を上オクターブへ反復して必要数を作る。`retrigger_each_step`の省略時または`true`では数値セルごとに再発音し（`-`は常に音価を延長）、`false`では隣接する同音の数値セルを結合してからGateを適用する。`.`はどちらの場合も休符として発音を区切る。Gateは音価が確定した後に一度だけ掛かる。
- `counters.<name>.when: silent_full_beat` は、セクションの`counter.follows`に挙げた旋律パートのどれもその拍の**全期間**に鳴っていない場合だけ、`cell`の各セルをその1拍に等分配置する。`cell`は空白区切りの度数・`.`（休符）・`-`（直前の音の継続）で、先行音のない継続はエラー。休みの判定はNote Off・Gate処理後の音価で行う。

## 素材の参照・反復・変奏

`melodies`は旋律素材を定義する。素材は**要素**の並びで、各要素は任意の長さのパターン文字列（4.2・4.3）である。1小節を1要素にしても、フレーズ単位で小節をまたぐ要素にしてもよい。素材の長さは全要素の長さの合計で、使う側で検証する。

```yaml
melodies:
  hook: # 要素のリスト
    - "5 . b3' . | 1':2 . 5 | b7 . 1'~ 2' | b3':2 .:2"
  hook_end: # 継承と要素の置換
    from: hook
    replace: { 4: "1':6 .:10" }
  walk: # 属性を持つ素材
    relative_to: chord_bass
    items:
      - '1:4 3:4 5:4 c7:4'
  verse:
    pickup_steps: 3
    items:
      - "5 6 7 || 1':4 3:4 5:8"
```

素材の葉は、要素のリスト、または`items`か`from`のどちらか一方を持つマッピングとする。どちらも持たないマッピングは名前空間として扱い、`melodies.answers.gap`のように`.`で区切った名前で参照する。マッピングの素材は次の属性を持てる。

| 属性           | 意味                                                                                                  |
| -------------- | ----------------------------------------------------------------------------------------------------- |
| `items`        | 要素のリスト                                                                                          |
| `from`         | 継承元の素材。要素と属性を継承し、明示した属性で上書きする。循環参照は禁止                            |
| `replace`      | `from`と併用し、展開後の要素番号（1始まり）の要素を文字列で置換する。置換前と同じ長さでなければエラー |
| `relative_to`  | `tonic`（既定）または`chord_bass`。度数の基準                                                         |
| `pickup_steps` | 先頭から指定ステップを弱起として扱う。正の有理数で、素材の長さより短い                                |

要素のリストには文字列と`{use: verse, items: [1, 2], shift_steps: -2}`を混在できる。`use`は参照先の展開後の要素から指定範囲を抜き出してそのまま要素として挿入し、音符イベントだけ指定ステップ平行移動する。`use`の負方向シフトで挿入位置より前に出るイベントは、出力曲の時刻0より前に配置できない。時間移動したイベントが素材の外に出る場合は、配置可能範囲を検証する。

**弱起**：`pickup_steps`を持つ素材は、先頭の指定ステップを使用位置より前に鳴らす。旋律パートではセクションの開始より前、`placements`では`at`より前になり、前のセクションと重なってよい。弱起が曲頭より前に出る場合はエラー。弱起を除いた部分の先頭が素材の**記譜上の原点**であり、`|`・`||`の表明とスウィングの小節はここから数える。弱起の部分は、原点の直前にある同じ長さの小節として扱う。

`sections`の`from`は`chords`、`lines`、`comp`、`events`などの**フィールドごと**に上書きする。`placements`は置換、`add_placements`は継承・置換後の配置リストの末尾へ順番どおり追記する。`add_placements`だけを持つ独立セクションでは、空の配置リストへ追加する。複数段継承では各段階で`add_placements`を実際の`placements`へ統合するため、`A → B → C`の追加結果は`A → B → C`の順番で保持され、解決済みのセクションに`add_placements`は残らない。統合後の配置も定義元は追跡され、その配置に関する診断の`path`は継承先のセクションではなく**配置を定義した元のYAML位置**（`sections.<元のセクション>.placements[i]`または`<そのセクション>.add_placements[i]`）を指す。`events`配列と`lines`・`comp`などのマップは追記ではなく置換。`drums`の`from`は`parts`を楽器ごとに統合し、`overrides`を継承後に追記する。継承後の参照は全て検証する。

`drums.overrides`は小節条件`when`を使う。指定できるのは`when: {odd: true}`、`when: {even: true}`、`when: {every: 4}`、`when: {bars: [3, 7, last]}`のいずれか1つ。小節番号はセクション内1始まりで、`every: n`はnの倍数小節（n, 2n, ...）、`bars`は列挙した小節（`last`は末尾の小節）に一致する。条件に一致した小節の該当楽器のパターンを、その1小節の文字列で完全に置換する。別楽器の変更は共存できるが、**同じ小節・同じ楽器への複数の上書きが同時成立した場合はエラー**。

## セクションの構成と配置

### セクションの基本フィールド

| フィールド       | 意味                                                                                                      |
| ---------------- | --------------------------------------------------------------------------------------------------------- |
| `chords`         | 小節数と同じ長さのコード列。食いの`<`を含められる                                                         |
| `chord_changes`  | `{at, chord}`の配列。小節内の任意位置のコード変更                                                         |
| `lines`          | `{名前: {instrument, melody, ...}}`。旋律素材を楽器で演奏する旋律パート                                   |
| `drums`          | `drums`内のパターン名                                                                                     |
| `comp`           | `{楽器名: パターン名}`または`{楽器名: {pattern, voicing}}`                                                |
| `pad`            | `{instrument, pattern, voicing?}`。組込みパターン`hold`を使える                                           |
| `arp`            | `{instrument, pattern, voicing?}`。`arps`の名前付き規則を参照                                             |
| `counter`        | `{instrument, pattern, follows}`。`counters`の規則を参照し、`follows`は休み判定に使う旋律パート名のリスト |
| `placements`     | `{instrument, at, ...}`の配列。特定の音符・コード・切り出しフレーズを指定                                 |
| `add_placements` | `placements[]`と同じ要素の配列。継承時に既存の配置リストの末尾へ追記                                      |
| `events`         | 構造化された演出・制御・マクロの配列                                                                      |
| `velocity`       | 旋律・コード等の既定MIDI Velocity（1～127）                                                               |
| `part_gains`     | `{楽器名: dB}`。そのセクション内の楽器別Gain（楽器Gainに加算）                                            |

### 旋律パート

```yaml
sections:
  dream:
    lines:
      flute: { instrument: flute, melody: dream_flute }
      double: { instrument: synth, melody: dream_flute, transpose: 12, gain_db: -6 }
      bass: { instrument: bass, melody: dream_bass, register: bass, loop: true }
      chime: { instrument: chime, melody: chime, tonic: F }
```

`lines`は名前付きの旋律パートのマップで、本数に制限はない。名前は`counter.follows`から参照する識別子であり、同じ楽器を複数のパートで使ってもよい。

| フィールド    | 意味                                                           | 必須 |
| ------------- | -------------------------------------------------------------- | ---- |
| `instrument`  | 演奏する楽器                                                   | 必須 |
| `melody`      | 旋律素材名                                                     | 必須 |
| `slice_items` | `[start, end]`。素材の展開後の要素から両端を含む範囲だけを使う | 任意 |
| `loop`        | `true`なら素材を繰り返してセクションを満たす                   | 任意 |
| `register`    | `melody`（既定）または`bass`                                   | 任意 |
| `tonic`       | 主音の上書き                                                   | 任意 |
| `transpose`   | 半音単位の移調。音高を計算した後に加える                       | 任意 |
| `gain_db`     | その演奏の追加ゲイン                                           | 任意 |

弱起を除いた素材の長さは、セクションの長さと一致しなければならない。`loop: true`ではセクションの長さが素材の長さの整数倍でなければならず、弱起は最初の1回だけ鳴らす。`slice_items`で最初の要素を含めない場合、弱起は使わない。

### placements

各`placement`は `instrument` と `at: {bar, step}`、および次のいずれか1つの演奏対象を持つ。

- `melody`：既存の素材名。素材の記譜上の原点を`at`に置く。`slice_steps: {start, count}`で弱起を含む素材の先頭から数えた開始ステップと切り出すステップ数、`take_notes`で先頭からの発音数を指定できる。切り出した場合は選択範囲の先頭を`at`に置く。
- `degree`：指定した度数の1音。`duration_steps`、`velocity`を付与できる。
- `chord`：指定した度数コードの和音。必要なら`voicing`、`voicing_floor_midi`、`add_root_degree`を指定する。

追加の`transpose`（半音）、`gain_db`（dB）、`tonic`（主音の上書き）、`mark`（タイムライン表示用の識別子）は許可する。`register`は`melody`と`degree`に指定でき、`degree: "1"`と`register: bass`で楽器のベース音域の主音になる。`duration_steps`は正の有理数として解析する。YAMLの数値`1.5`は3/2として正確に扱う。イベントがセクション境界を越える場合は、明示された長さと出力先の許容範囲で検証する。

## 詳細時間・端数小節

### 時間単位と範囲

イベントの位置と長さの詳細規則を定める。曲中のテンポ変更は`time_changes`へ記述する。

```yaml
time_changes:
  - { at_quarters: '13/2', bpm: 140 }
  - { at_quarters: '120', bpm: 72, over_quarters: '8' }
```

`at_quarters`は曲頭からの四分音符数（非負）。同時刻の重複・`form`側の同時刻テンポ指定との衝突はエラー。曲終端を超える位置もエラー。

`over_quarters`を指定すると、`at_quarters`から`over_quarters`をかけて、直前のテンポから`bpm`へ線形に変化させる（リタルダンド・アッチェレランド）。`over_quarters`は1/4の正の倍数とし、変化区間の終端は曲終端以下とする。変化は1ステップ（16分音符）ごとの段階的なテンポ変更として展開し、区間をnステップとすると、k番目（0始まり）のステップのテンポは「直前のテンポ＋（`bpm`−直前のテンポ）×（k＋1）/n」とする。WAV・MIDI・`inspect`は同じ段階的テンポを使う。変化区間の内部にほかのテンポ変更（`form`の`bpm`を含む）がある場合はエラー。

**位置**：`at: {bar: 1, step: 1}`は、セクション内・1始まりであり、`step`は16分音符単位。4/4は1～16、6/8は1～12。`at: {bar: last, step: 1}`も許す。グリッドに乗らないイベントは`at: {bar: 1, offset_quarters: "1/3"}`を使う。`offset_quarters`はその小節頭からの0始まりの量。`step`と`offset_quarters`の併用は禁止。

**長さ**：`duration_steps`は16分音符数であり、正の整数・有限10進数・分数文字列を受理する。`duration_quarters`は四分音符数で、同じく正の有理数。両方の指定は禁止。パターンの`gate`が掛かるのは自動計算音価のみ。`placements`や明示ノートの`duration_*`にはGateを再適用しない。

**比率の解析**：整数・有限小数・`p/q`は丸めずに分数として扱う。分母0と負の値はエラー。全イベントを整数Tickで表すPPQは自動で決まり、上限32767を超える細かい分数の組合せはエラーになる。

### 部分小節

`form[]`は`last_bar_quarters: "p/q"`を許す。`bars`は**最後の短い小節を含む正の整数**。最後の小節長を`L`、通常小節長を`B=n*4/d`とすると、`0 < L < B`を要求し、セクション長は`(bars-1)*B + L`。最終小節の発音位置は`[0,L)`であり、明示ノートの終端は曲終端以下とする。Sustain解除などの瞬時制御は末尾小節の`offset_quarters: L`でセクション終端にも配置できる。短い小節の素材は**実際の有効長**を消費し、端数分のトリミングや無音の補填は行わない。

```yaml
form:
  - { section: fx_preview, bars: 8, last_bar_quarters: '2', tonic: C, scale: ionian }
```

これは4/4の通常7小節＋最後2四分音符＝7.5小節。

### 記法の単位と連符

4/4は1区間が四分音符で4ステップ、6/8は1区間が八分音符で2ステップ。1/4より短い分母単位の拍子でも、16分以上の長さを持つ区間には正確なステップ数を割り当てる。分母が32以上で16分ステップを持たない区間では文字列グリッドを使わず、`placements`/構造化イベントで書く。拍子の分母として1,2,4,8,16,32,64,128を受理する。

ドラム・`comp`・`order`の1小節の文字列は、端数小節ではその有効長に合わせて短縮したパターンを要求する。16分境界以外の最終切断では構造化配置で厳密に指定する。

## 演奏イベント・マクロ・優先規則

### イベント形式

セクションの`events`は構造化された演奏イベントのマップのリスト。`kind`は言語に組み込まれたイベント種別であり、利用者は種別を追加できない。`macro: <name>`は、そのScore YAMLのトップレベル`macros`で明示的に宣言された名前を参照し、`kind`と`macro`の同時指定はエラーとする。マクロ名やメロディ素材名は言語が内蔵する予約語ではなく、利用者が定義する識別子である。全イベントは、対象音源を含めてセクション内座標を解決する。

| `kind`             | 必須フィールド（共通`at`は省略）                                                                                                  | 意味                                                                                                        |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `hit`              | `instrument`                                                                                                                      | 指定Trigger Noteを1回発音。Velocityの既定値はセクションの`velocity`（省略時100）、長さの既定値は1ステップ。 |
| `hits`             | `instruments`                                                                                                                     | 指定音源群への`hit`と同値。順序はリスト順。                                                                 |
| `degree`           | `instrument`, `degree`                                                                                                            | その度数の1音。マクロ本体にも指定可。                                                                       |
| `silence`          | `duration_steps`または`duration_quarters`; `except_instruments`は任意                                                             | 指定範囲の対象Noteを抑制・切断。明示の例外は抑制しない。                                                    |
| `part_window`      | `instrument`, `bars: [start,end]`                                                                                                 | 対象楽器の演奏をその小節範囲だけ有効にする。                                                                |
| `dynamics`         | `instrument`, `from`, `to`, `duration_steps`または`duration_quarters`                                                             | 指定範囲で始まる音符のVelocityに、`from`から`to`へ線形に変わる値を加算する。                                |
| `step_roll`        | `instrument`, `bar`, `from_step`, `through_step`, `velocity_from`, `velocity_to`                                                  | 両端を含む16分連続打撃を線形Velocityで生成。                                                                |
| `drum_roll`        | `instrument`, `bars`, `early_hits_per_beat`, `final_hits_per_beat`, `last_bar_end_beat`, `velocity_from`, `velocity_to`, `riser?` | 対象末尾小節で密度が変わるロール。                                                                          |
| `sustain_pedal`    | `instrument`, `down`                                                                                                              | MIDI CC64相当。                                                                                             |
| `pitch_bend`       | `instrument`, `value`                                                                                                             | −1～1の既存値。                                                                                             |
| `mod_wheel`        | `instrument`, `value`                                                                                                             | 0～1。                                                                                                      |
| `aftertouch`       | `instrument`, `value`                                                                                                             | 0～1。                                                                                                      |
| `parameter_change` | `instrument`, `parameter`, `native_value`                                                                                         | 音源ParameterのNative値に設定。                                                                             |
| `automation`       | `instrument`, `parameter`または`control`, `from_value`, `to_value`, `duration_steps`または`duration_quarters`                     | 音源Parameterまたは演奏コントロールの線形変化。                                                             |

- `silence`は指定した小節区間のNote Onを抑制し、境界にまたがるNoteの音価を切断する。演奏制御とParameter Rampには作用しない。Release音・Reverb・External Audioの残響まで完全ミュートする意味ではない。
- `part_window`の`bars: [start, end]`は**1始まりの小節番号**であり、`start`と`end`の両小節を含む。`bars: [2, 3]`は2小節目と3小節目の両方を対象とし、`last`は最後の小節、端数小節（`last_bar_quarters`で短くなった小節）も通常の1小節として数える。内部では、開始小節の先頭から終了小節の末尾までを`[開始時刻, 終了時刻)`の半開区間として扱う。
- `part_window`は対象楽器の、そのセクション内で始まるNote Onと点の演奏制御を半開区間`[start,end)`に限定する。前のセクションで始まり当該セクションへ持続するNoteは、このセクションの窓では変更しない。窓に含まれるNoteの音価が終端を越える場合は境界で切断し、点の演奏制御は区間内のイベントだけを残す。区間開始では以前の状態をニュートラル値またはParameter定義の既定値へ戻し、区間終端でも同じ既定状態へ戻して状態を後続セクションへ漏らさない。窓をまたぐParameter Rampは許可区間との交差部分へ切り詰め、両端値を元の線形Rampから補間する。窓が空の場合はセクション中に制御を残さない。同じ楽器・セクションへ複数の窓を指定した場合は許可区間の共通部分を使う。
- `dynamics`は強弱の流れを書く。`from`と`to`は-126～126の整数で、区間`[at, at+長さ)`で始まる対象楽器の全音符（全Lane、ドラム・ロールを含む）に、開始時刻で線形補間した値を加算する。区間の終端で`to`となる直線を使い、整数への丸めは`round half away from zero`とする。加算後のVelocityが1～127の外になればエラー。区間はセクション内に収め、同じ楽器への`dynamics`の区間が重なればエラー。音符ごとのVelocityの差（`@`、アクセント）は保たれる。
- `automation`は`parameter`と`control`のどちらか一方を指定する。`parameter`は音源Parameterの既存Parameter Rampと同じ正規化空間で線形補間する。`control`は`mod_wheel`・`aftertouch`（値0～1）、`pitch_bend`（値−1～1）のいずれかで、`at`から1/64四分音符ごとと区間終端に、線形補間した点の制御イベントを生成する。音源側でこれらのコントロールを音量や音色に割り当てると、長い音の中の息のふくらみなど、音符の中の変化を書ける。区間内に同じ楽器・同じコントロールの別の制御があればエラー。
- 同一楽器・同時刻の`drum_roll`/`step_roll`と通常ドラム打撃が衝突すればロールが勝つ。**ロール同士**、同一Parameterへの異なる値・重複矛盾、複数の同一楽器Overrideが重なればエラー。
- Note、Sustain、Bend、Wheel、Aftertouch、ParameterなどのEventが同時刻に起きる場合は、ソース定義順を保持し、Note Off→Control→Note Onの基本優先順位を適用する。
- 展開は、パターン・旋律・配置・イベントの生成、ロールの優先、`silence`と`part_window`、`dynamics`、楽器ごとの`timing_offset_steps`の順に行う。
- `kind: drum_roll`で`bars: all`はセクションの全小節、正整数なら末尾N小節。発音順Velocityを指定両端間で線形補間し、整数値への丸め方は`round half away from zero`とする。末尾の`riser`がある場合はロール開始時に1回打撃する。
- `macro: name`に付いた上書きフィールドはマクロ本体の同名フィールドを置換し、展開結果はイベント型として検証する。マクロの循環参照はエラーとする。
- `drums`、`hit`/`hits`、`step_roll`、`drum_roll`、およびそれらを含むMacroは、対象楽器の`trigger_note_midi`を必須とし、省略やMIDI 0～127外の値はエラーになる。`recommended_range`は音域の目安であり、Trigger Noteの許容範囲ではない。音符を発音しない音声処理専用の楽器（外部音声の受のみ）も許可し、ダミーNoteは生成しない。

### 音源能力と演奏制御

- MIDI Noteは0～127、Velocityは1～127。内蔵音源は Generator の Key / Velocity Trigger と照合し、発音可能な Layer がなければエラーにする。VST3 はプラグイン自体が発音範囲を決める。シンセによっては音程と無関係の Trigger Note が必要になる。
- 内蔵音源に対する `parameter_change` / `automation` の Parameter ID はその音源定義に存在し、Native 値は範囲内の有限数とする。Mod Wheelなどのレンダラー標準演奏操作と、音源固有Parameterの対象は区別する。

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
      vst3:
        plugin: MODO BASS 2
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

`source` の有効フィールドは `vst3` と `velocity`。`vst3` は必須の `plugin` と、任意の `identifier`、`state`、`patch`、`params` を持つ。

- `plugin` は検証済み VST3 カタログ上の正確なプラグイン名。`identifier` は同じ VST3 クラスへ絞り込む
- `state` は `PluginStateSnapshot` 形式の JSON。`schemaVersion: 2`、`pluginIdentifier`、`pluginPath`、`parameterValues`、`stateData` を持つ。識別は `pluginIdentifier` で行い、`pluginPath` は取得時の実行パスとして扱う。現在のカタログで同じ識別子のプラグインを選び、その実行パスを使う
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
    source: { vst3: { plugin: Addictive Drums 2 } }
    effects:
      - { id: drum-eq, vst3: { plugin: Surge XT Effects } }
```

`kits.<id>` の有効フィールドは `source` と `effects`。`source` は VST3 音源を要求する。各部品の `kit` は存在するキットを参照し、部品側には `source`、`effects`、`audio`、`audio_input` を書けない。キットと楽器の ID は衝突せず、メンバーを持たないキットとキットによるキットの参照はエラーになる。

部品の演奏は 1 つの Kit Track、1 つの MIDI クリップ、1 つの VST3 インスタンスへ集約する。部品ごとに Instrument Device やトラックを作らない。`trigger_note_midi` は部品が送る出力 MIDI ノートであり、部品にはドラムなどのトリガー由来の Note だけを書ける。音高付きの旋律・和音や、ParameterChange などの制御イベントはエラーになる。複数の部品が同じトリガー番号を使うことは許容する。

部品の `gain_db` / `pan` は Sonalloy の音作りに残し、Riffra の個別音量には適用しない。キット全体のゲイン・パンは `mix.tracks.<kit>` に書く。部品ごとの追加ゲインレーン（`part_gains`、`lines.*.gain_db`、`placements[].gain_db` など）はエラーになる。キットの `source.velocity` は集約後のノートへ共通適用する。

### エフェクト

```yaml
instruments:
  bass:
    effects:
      - { id: eq, vst3: { plugin: Surge XT Effects } }
      - id: duck
        vst3:
          plugin: Example Compressor
          params: { Threshold: 0.45 }
        bypassed: false
```

`effects[]` は必須の `id` と `vst3`、任意の `bypassed`（既定 false）からなる。
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
      effects: [{ id: reverb, vst3: { plugin: Example Reverb } }]
  sidechain:
    - from: drums
      to: { instrument: bass, effect: duck }
      level_db: 0
      position: post
  master:
    gain_db: -1
    effects: [{ id: limiter, vst3: { plugin: Example Limiter } }]
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
      - { id: filter, vst3: { plugin: Surge XT Effects } }
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

State と WAV は Score のある曲フォルダ内に置き、Score から相対パスで指定する。上位ディレクトリへの参照やシンボリックリンクによる曲フォルダ外への参照は使えない。

## 展開前の検証

Project を作らずに Score を確認する。

```bash
riffra score validate ./song.yaml
riffra score inspect ./song.yaml
```

`score validate` は文法・構造・楽器参照・VST3・State・Patch・パラメーター・キット・音声・ミックス・オートメーションを診断し、`valid` と展開サマリーを返す。`valid` は error が 0 件のときに true となり、warning だけでは import / rebuild を止めない。
`score inspect` は展開済みのセクション・楽器・レーン・トラック・キット・エフェクト・バス・Send・Sidechain・オートメーション・音声クリップを返す。`--events` を指定すると、各イベントを来源位置つきで返す。

どちらも Project・Session・音源スナップショット・永続 Audio Asset を作らない。参照ファイルの読み込みと、分離された plugin-scanner による一時的なパラメーター照会は行う。プラグイン未インストール、State の不正、音声範囲超過、ミックスの循環、オートメーションの不正はエラーになる。
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

VST3 の State は Score の `state` で指定した曲フォルダ内のファイルから読み込み、選択したプラグインの実パスへ組み直す。AudioClip の WAV は Data Root の Asset として不変に保存し、元ファイルを後から編集・削除しても再生に影響しない。

再構築で音源・エフェクト・Asset を再利用するかは「同一性と再利用」の規則で決める。プラグインの入力と選択されたカタログ情報が一致し、Device の状態も前回 Score から生成した状態と一致する場合、その Device と Score の宣言ごとに解決したオートメーション値を再利用する。

`.riffra` のエクスポート・インポートには、音源スナップショット・Audio Asset・ScoreSource も含まれる。
パッケージに書き出す Session のコピーでは、リンクの `path` を消して楽器の対応を空にする。元 PC の絶対パスを有効なリンクとして持ち込まないためであり、エクスポート元の Project は変更しない。インポート後に必要なら `project rebuild-score --path ... --bind ...` でリンクし直す。

## 失敗時の扱い

Score の構文・意味・音源参照の誤り、リンク先の欠損、Native 準備の失敗では、最後に成功した Session と `ScoreSource.yaml` を維持する。勝手にファイルを探索したり、ハッシュで別ファイルを結び付けたりしない。
リンク先がない場合は、確定済みの Session と本文をそのまま読み取れる。以前のリンクを持たない ScoreSource もそのまま読み取れ、`rebuild-score --path` で明示的にリンクできる。

時間軸の PPQ を変更すると、既存の音符・制御・範囲・録音位置と、テンポ・拍子の変更位置を新しい PPQ へ換算する。
PPQ の変更時はテンポ・拍子マップの値と変更点の列をそのまま指定する。マップの編集は PPQ の変更とは別の操作で行い、その時点の PPQ に基づく Tick を指定する。同時編集と、既存の音楽的位置を整数 Tick で正確に表せない変更は拒否する。

データモデル全体の規則は [data-model.md](data-model.md)、音声実行は [audio-engine.md](audio-engine.md) を参照。
