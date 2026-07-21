# Riffra Arrange Workspace 完全仕様書（統合版）

## 1. 目的とスコープ

### 1.1 文書の目的

本書は、Riffra の `Arrange` ワークスペースにおける完成形を定義する。

Arrange は、音声ファイルを並べるだけの画面ではない。  
演奏・録音・MIDI・音素材を一つの時間軸上で組み合わせ、再生しながら直接編集し、音楽として構成するための中核ワークスペースである。

本書は製品仕様を定義するものであり、内部実装やコード構造そのものは規定しない。  
ただし、ユーザーから観測できる挙動や、機能間で一貫しているべき時間・再生・録音モデルは明確に定義する。

---

### 1.2 Arrange の位置づけ

Riffra は、既存 DAW のすべての機能を再実装することを目的としない。  
その一方で、Arrange に含める基本的なタイムライン操作については、主要な DAW と比べて明らかに劣る操作体験を許容しない。

Arrange は、次の二つを同時に満たす。

1. 音楽制作ソフトとして必要な基本編集・再生・録音が、高品質に動作する
2. Riffra 固有の Recording、Take、Raw / Processed、Rack、Asset、制作履歴と自然に接続する

Riffra 固有の機能によって、DAW として必要な基本品質の不足を正当化する設計にはしない。

---

### 1.3 Riffra 内での役割

Arrange は、Riffra にある音楽素材を「時間を持った音楽」へ変える場所である。

```text
Play
演奏する・音を作る
        │
        ▼
Recording
演奏を残す
        │
        ▼
Library
素材として蓄積する
        │
        ▼
Arrange
時間軸上で組み合わせる
        │
        ▼
構成された音楽
```

Arrange が Riffra 内のすべての制作機能を抱える必要はない。  
ただし、一度 Arrange に入ったあとは、楽曲を構成するための基本作業を別アプリへ移動せずに完結できる必要がある。

---

### 1.4 Arrange で完結する作業

Arrange では、以下の作業を完結できる。

| 分類            | 内容                                                                                                   |
| --------------- | ------------------------------------------------------------------------------------------------------ |
| 素材配置        | Audio Asset / MIDI Asset の配置、Library からの直接配置、録音結果の直接配置                            |
| 再生            | Timeline のリアルタイム再生、任意位置からの再生、任意位置への移動、範囲ループ、Render なしでの編集確認 |
| Audio Clip 編集 | 移動、Trim、Split、Duplicate、Copy / Paste、Mute、Loop、Gain、Pan、Fade、Crossfade                     |
| MIDI 編集       | Note の追加・移動・削除、長さ変更、Velocity 変更、Quantize、MIDI イベントの録音・保持・再生            |
| 録音            | Audio 録音、MIDI 録音、複数 Track 同時録音、Loop Recording、Take 管理、Punch Recording                 |
| 基本ミックス    | Track Volume、Track Pan、Mute、Solo、Automation                                                        |
| 楽曲構造        | BPM、拍子、小節・拍、Marker、Loop Range                                                                |

---

### 1.5 対象外

以下は Arrange の中核仕様には含めない。

| 対象外項目                                              |
| ------------------------------------------------------- |
| 楽譜制作                                                |
| 映像編集                                                |
| 映像同期                                                |
| Surround Mixing                                         |
| 大規模な専用 Mixer Console                              |
| 任意の Bus / Send / Return を組み合わせる高度な Routing |
| Mastering 専用機能                                      |
| Audio Restoration 専用機能                              |
| Tempo Map                                               |
| 拍子変更 Map                                            |
| 外部 MIDI 機器専用の MIDI Track                         |

本仕様における Project は、全体で一つの BPM と一つの拍子を持つ。

---

## 2. 基本設計原則

### 2.1 Timeline を直接再生する

Arrange の再生は、Timeline 全体を事前に WAV へ Render してから行う方式ではない。  
Timeline の状態をリアルタイムで再生し、編集結果をすぐに確認できる。

```text
Timeline
↓
Real-time Playback Engine
↓
Audio Output
```

Clip の移動・編集・追加のあと、Render を挟まずに変更後の再生を確認できる。  
Offline Render は、完成音声を書き出すときだけ使用する。

---

### 2.2 直接操作を主操作とする

Timeline 編集の基本は、マウスとキーボードによる直接操作である。

```text
Clip を移動する
→ Drag

Clip を短くする
→ Edge を Drag

Fade を変更する
→ Fade Handle を Drag
```

Inspector は、精密な値を指定するための補助である。  
数値入力を通常編集の主操作にはしない。

---

### 2.3 非破壊編集

Arrange 上の操作では、元 Asset を変更しない。  
Trim、Split、Loop、Gain、Fade などは、すべて Clip 側の編集情報として保持する。

```text
Source Asset

|--------------------------------|

Clip A

       |-------------|

Clip B

                    |----------|
```

Clip を削除しても、Source Asset は削除されない。

---

### 2.4 音楽時間と実時間を分離する

Arrange は、次の二種類の時間を区別して扱う。

#### 音楽時間

```text
Bar
Beat
Subdivision
```

例：

```text
5.3.1
```

#### 実時間

```text
Seconds
Samples
```

例：

```text
00:08.240
```

Audio と MIDI では時間の性質が異なるため、両者を単純なミリ秒値だけで扱わない。

---

### 2.5 Audio / MIDI の時間規則

Audio Clip と MIDI Clip では、BPM 変更時の挙動が異なる。

| 対象                  | 開始位置       | 長さ / 速度    | BPM 変更時の挙動                                                                                 |
| --------------------- | -------------- | -------------- | ------------------------------------------------------------------------------------------------ |
| Audio Clip            | 音楽時間に固定 | 実時間に固定   | 開始位置と音声速度は維持され、終了する小節位置だけが変化する                                     |
| MIDI Clip / MIDI Note | 音楽時間に固定 | 音楽時間に固定 | 小節位置・Beat 位置・Note Length の音楽的位置は維持され、実時間上の再生長は BPM に応じて変化する |

たとえば 5 小節目に 10 秒の Audio Clip がある場合、BPM を変更しても、開始位置は `5.1.1` のまま、音声長も 10 秒のままである。  
通常の BPM 変更で Audio を自動 Stretch しない。

---

### 2.6 再生中も編集できる

安全に反映できる操作は、再生を止めずに実行できる。  
対象には、最低限以下を含む。

- Clip 移動
- Clip 追加
- Clip 削除
- Mute
- Solo
- Volume
- Pan
- Clip Gain
- Loop Range 変更
- Automation 編集

編集の反映時に、クリックノイズ、Audio Engine の停止、再生位置のリセットが起きないようにする。

---

### 2.7 Asset との接続を維持する

Clip は単なるファイルパスではなく、Riffra Asset を参照する。  
Audio Clip は、必要に応じて以下の情報へ辿れる。

- Source Asset
- Recording
- Recording Session
- Take
- Raw Variant
- Processed Variant
- Tags
- History

Arrange で編集したことで、Asset との関連が失われてはならない。

---

## 3. 画面構成と UI

### 3.1 全体レイアウト

Arrange は Riffra 共通の Application Shell 内に配置する。

```text
┌────────────────────────────────────────────────────────────────────┐
│ App Header                                                         │
├──────────────┬─────────────────────────────────────┬───────────────┤
│              │ Timeline Toolbar                    │               │
│              ├─────────────────────────────────────┤               │
│   Library    │ Time Ruler / Marker / Loop          │   Inspector   │
│              ├──────────────┬──────────────────────┤               │
│              │ Track Header │                      │               │
│              │              │      Timeline        │               │
│              │              │                      │               │
│              │              │                      │               │
│              ├──────────────┴──────────────────────┤               │
│              │ MIDI Editor / Detail Editor         │               │
├──────────────┴─────────────────────────────────────┴───────────────┤
│ Transport                                                          │
└────────────────────────────────────────────────────────────────────┘
```

---

### 3.2 領域の役割

| 領域         | 役割                           |
| ------------ | ------------------------------ |
| Library      | Asset を探して配置する         |
| Track Header | Track 状態を操作する           |
| Timeline     | 音楽を時間軸上で編集する       |
| Editor Panel | MIDI などの詳細編集を行う      |
| Inspector    | 選択対象を精密編集する         |
| Transport    | 再生・録音・時間情報を操作する |

- Library と Inspector は個別に折りたためる
- Editor Panel は必要なときだけ開く
- Timeline を常に最大の作業領域として扱う

---

### 3.3 Timeline Toolbar

Toolbar には、Arrange で頻繁に使う操作だけを配置する。

```text
[Select] [Split]

Snap  1/16 ▼

[Automation]

120 BPM
4/4

[Follow]
```

常時大量の機能ボタンを並べない。

#### Select Tool

Select Tool を標準ツールとする。  
Select Tool だけで、以下の操作を行える。

- 選択
- Clip 移動
- Trim
- Fade 操作
- Marquee Selection

通常操作のために、頻繁な Tool 切り替えを要求しない。

#### Split Tool

任意位置をクリックして Clip を分割できる。  
ただし Split は `Ctrl + E` でも実行でき、Split Tool なしでも主要な編集フローは完結する。

---

### 3.4 UI 品質方針

Arrange は情報密度の高い画面になるが、単なる業務アプリ型のフォーム画面にはしない。

基本方針は以下である。

- Timeline を最優先する
- Waveform を主要情報として扱う
- MIDI Note を視覚的に表示する
- 選択状態を明確にする
- Drag 可能な場所が理解できる
- Resize Handle が理解できる
- Snap 位置を確認できる
- 不要な枠線を増やさない
- 主要操作を深い Menu だけに隠さない
- 未実装の操作を、機能しているように見せない

---

### 3.5 空状態

Track が存在しない場合は、中央に以下を表示する。

```text
Start arranging

Drag audio or MIDI here

[Add Audio Track]
[Add Instrument Track]
[Record]
```

Asset を Drop した場合は、対応する Track を自動作成する。  
Track が存在する場合は、空状態 UI を表示しない。

---

### 3.6 エラー表示

再生・録音エラーが起きても、Timeline 全体を Modal で塞がない。

例：

```text
Audio device disconnected
Playback stopped

[Open Audio Settings]
```

ユーザーが取るべき復旧操作を明示し、エラーによって編集状態を失わせない。

---

## 4. 時間軸・Transport・選択

### 4.1 Project Tempo / Time Signature / Start

| 項目                | 仕様                                                                                                          |
| ------------------- | ------------------------------------------------------------------------------------------------------------- |
| BPM                 | Project は一つの BPM を持つ。標準値は `120 BPM`                                                               |
| 拍子                | Project は一つの拍子を持つ。標準値は `4/4`                                                                    |
| 開始位置            | Timeline の開始位置は `1.1.1`                                                                                 |
| BPM 変更            | Transport または Toolbar から変更できる。再生中の変更も可能だが、音楽時間への再生スケジュールは安全に更新する |
| Audio の速度        | BPM を変更しても、Audio Clip 自体の再生速度は変化しない                                                       |
| 拍子変更            | Project 途中での拍子変更は本仕様に含めない                                                                    |
| Clip 配置下限       | 通常の Clip を `1.1.1` より前へ配置できない                                                                   |
| Count-in / Pre-roll | Timeline 外の再生処理として扱う                                                                               |

---

### 4.2 Time Ruler と表示モード

Timeline 上部には Time Ruler を表示する。

```text
      1               2               3
      │               │               │
──────┼───┬───┬───┬───┼───┬───┬───┬───┼────
     1.1 1.2 1.3 1.4 2.1
```

表示密度は Zoom に応じて自動的に変わる。

| Zoom 状態 | 表示例                          |
| --------- | ------------------------------- |
| 遠距離    | `1        5        9        13` |
| 通常      | `1.1  1.2  1.3  1.4`            |
| 拡大      | Subdivision まで表示            |

ユーザーは、以下の表示モードを切り替えられる。

- Bars / Beats
- Time

Transport の現在位置表示にも、同じ表示方式を適用する。

---

### 4.3 選択モデル

Arrange は、次の三つを明確に区別する。

| 選択             | 対象                           | 作成方法                     | 主な用途                              |
| ---------------- | ------------------------------ | ---------------------------- | ------------------------------------- |
| Object Selection | Audio Clip / MIDI Clip / Track | Click、Ctrl + Click、Marquee | 編集対象の指定                        |
| Time Selection   | Timeline 上の時間範囲          | Time Ruler 上の Drag         | Loop Range、Punch Range、範囲単位操作 |
| Playhead         | 単一の時間位置                 | Transport / Ruler 操作       | 再生・編集の基準位置                  |

Time Selection の例：

```text
3.1.1 ├================┤ 5.1.1
```

Playhead は、Object Selection や Time Selection とは独立する。

---

### 4.4 Playhead

現在位置を示す縦線を、Timeline 全体へ表示する。

```text
               ▼
───────────────│──────────────
███████████████│████
        ███████│████████
               │
```

- 再生中は、実際の Audio 再生位置と同期する
- 停止時も位置を保持する
- Time Ruler をクリックすると移動する
- Time Ruler 上でドラッグできる
- Timeline 再生中は滑らかに表示する

UI アニメーション上の Playhead と、実際の Audio Engine 時間を別々の時計として動かさない。  
基準は Audio Engine 側の再生位置である。

---

### 4.5 Transport

#### Transport UI

最低限、以下を表示する。

```text
|<   ▶   ■   ●

3.2.1

120 BPM
4/4

Loop
Metronome
Count-in
```

操作項目：

- Go to Start
- Play
- Stop
- Record
- Loop
- Metronome
- Count-in

#### Play / Stop / Seek

| 操作               | 挙動                                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------- |
| 停止中に Space     | 現在の Playhead 位置から再生する                                                                   |
| 再生中に Space     | 現在位置で停止する                                                                                 |
| 停止後に再度 Space | 停止位置から再生する                                                                               |
| Go to Start        | Playhead を `1.1.1` へ移動する                                                                     |
| Seek               | 停止中・再生中の両方で任意位置へ移動できる。再生中に Seek した場合は、新しい位置から再生を継続する |

Seek によって Audio Engine 全体を再起動しない。

#### Follow Playhead

Follow が ON の場合、再生位置が表示領域を越えると Timeline を自動スクロールする。  
再生中にユーザーが手動で Scroll または Zoom した場合は、自動 Follow を一時停止する。

再度 Playhead がユーザー操作の対象になった時点、または Follow を再度 ON にした時点で追従を再開する。

---

### 4.6 Loop

Timeline Loop は、Time Selection とは独立した永続的な再生範囲として保持する。

```text
1        2        3        4
         ├================┤
```

Loop Start / End は、Time Ruler 上で直接変更できる。

操作：

- Enable / Disable Loop
- Set Loop to Time Selection
- Set Loop to Selected Clip

Loop End に到達した場合は、再生を止めずに Loop Start へ戻る。  
Audio と MIDI は、同じ Loop 境界で同期する。

---

### 4.7 Snap

Snap Grid として、以下を正式にサポートする。

- 1 Bar
- 1/2
- 1/4
- 1/8
- 1/16
- 1/32
- 1/8 Triplet
- 1/16 Triplet
- Off

`1/2` 以下の表記は音価を表す。

Snap は以下に適用する。

- Clip 移動
- Trim
- Split
- MIDI Note
- Loop 境界
- Time Selection
- Marker
- Playhead 配置

さらに、以下への吸着も行う。

- 他 Clip の Start / End
- Marker
- Loop 境界

`Alt` を押している間だけ、一時的に Snap を無効化できる。  
吸着した位置は、ガイド線や強調表示で視覚的に確認できる。

---

### 4.8 Zoom / Scroll

| 方向            | 仕様                                                                                                                                                |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| Horizontal Zoom | 曲全体から細かな波形編集まで拡大できる。Zoom 中心はマウスカーソル位置で、見ていた時間位置が突然画面外へ飛ばない                                     |
| Vertical Zoom   | Track Height は Compact / Normal / Large の標準状態を持つ。個別 Track の高さも変更できる                                                            |
| Scroll          | Horizontal / Vertical Scroll を独立して行える。Track Header は Horizontal Scroll で画面外へ消えず、Time Ruler は Vertical Scroll で画面外へ消えない |

---

### 4.9 Marker

Time Ruler 上に Marker Lane を配置する。

```text
Intro       Verse        Chorus
│           │            │
1           9            17
```

Marker は以下を持つ。

- Position
- Name

操作：

- Add
- Move
- Rename
- Delete

Marker 名称は自由入力とする。  
Marker 自体は音響処理に影響しない。

---

## 5. Track と Clip の基本モデル

### 5.1 Track 種別と信号経路

Arrange は、次の二種類の Track を持つ。

#### Audio Track

```text
Audio Clip / Audio Input
        ↓
       Rack
        ↓
Track Volume / Pan
        ↓
      Master
```

用途例：

- Guitar
- Vocal
- Recorded Audio
- Imported Audio

#### Instrument Track

```text
MIDI Clip / MIDI Input
        ↓
    Instrument
        ↓
      Rack
        ↓
Track Volume / Pan
        ↓
      Master
```

Instrument Track には、音を生成する Instrument が必要である。  
Instrument が存在しない場合、MIDI データは保持されるが音は鳴らない。

#### Output

すべての Track は Master へ出力する。  
任意の Bus Routing は本仕様に含めない。

---

### 5.2 Track Header

基本表示：

```text
Guitar             M  S  ●
```

含む情報：

- Track Name
- Track Type
- Mute
- Solo
- Record Arm
- Monitoring 状態

Track Header は、次の二状態を持つ。

| 状態     | 表示内容                                                               |
| -------- | ---------------------------------------------------------------------- |
| Compact  | 主要操作だけを表示する                                                 |
| Expanded | Volume、Pan、Input、Monitoring、Rack / Instrument の概要を追加表示する |

詳細設定は Inspector で行う。

---

### 5.3 Track 操作

以下をサポートする。

- Add Audio Track
- Add Instrument Track
- Delete
- Duplicate
- Rename
- Reorder
- Resize
- Mute
- Solo
- Record Arm

Track Header を Drag して並び替えられる。

---

### 5.4 Mute / Solo / Record Arm

| 機能                | 挙動                                                                                             |
| ------------------- | ------------------------------------------------------------------------------------------------ |
| Mute                | Mute された Track は再生しない                                                                   |
| Solo                | 一つ以上の Track が Solo の場合、Solo された Track だけを再生する。複数 Track の同時 Solo が可能 |
| Mute と Solo の競合 | 同一 Track で Mute と Solo が同時に有効な場合、Mute を優先する                                   |
| Record Arm          | 複数 Track を同時に Record Arm できる。Record 開始時は、Arm されている Track をすべて録音する    |
| Arm なしで Record   | 何も Arm されていない場合は録音を開始せず、`No tracks are armed for recording` を表示する        |

---

### 5.5 Monitoring

Audio Track の Monitoring は、次の三状態を持つ。

| 状態 | 挙動                                         |
| ---- | -------------------------------------------- |
| Off  | 入力音を Monitor しない                      |
| Auto | Record Arm されている間だけ Monitor する     |
| On   | Record Arm 状態にかかわらず常に Monitor する |

Monitoring 音は、録音対象となる信号と明確に分離する。

---

### 5.6 Clip 共通仕様

Clip は Timeline 上の非破壊編集オブジェクトである。  
Audio / MIDI を問わず、共通して以下を持つ。

- Timeline Position
- Length
- Source Reference
- Mute
- Loop
- Selection State

Audio / MIDI 固有情報は、各 Clip 種別が持つ。

---

### 5.7 Clip Selection

| 操作              | 挙動                               |
| ----------------- | ---------------------------------- |
| Click             | 単一選択                           |
| Ctrl + Click      | 選択を追加・解除                   |
| Marquee           | 矩形範囲に含まれる Clip を複数選択 |
| Empty Space Click | Clip 選択を解除                    |

複数選択時は、相対位置を維持したまま移動できる。

---

### 5.8 Clip 移動

Clip 本体を Drag して移動する。

| 方向            | 挙動                    |
| --------------- | ----------------------- |
| Horizontal Drag | Timeline 位置を変更する |
| Vertical Drag   | 対応 Track へ移動する   |

- Audio Clip を Instrument Track へ配置できない
- MIDI Clip を Audio Track へ配置できない
- 複数 Clip 移動時は相対位置を維持する

---

### 5.9 Trim / Split / Duplicate

| 操作                     | 挙動                                                                                         |
| ------------------------ | -------------------------------------------------------------------------------------------- |
| Trim                     | Clip 端を Drag し、使用範囲を変更する。非破壊で行い、Trim した範囲は再度拡張して復元できる   |
| Split                    | 選択 Clip を指定位置で分割する。分割後も同じ Source Asset を参照する                         |
| Duplicate / Copy / Paste | `Ctrl + D`、`Ctrl + C`、`Ctrl + V`、`Alt + Drag` に対応する。複製時に Asset 自体は複製しない |

#### Trim の補足

- Audio Clip では、Source Asset の参照範囲を変更する
- MIDI Clip では、Clip として表示・再生する範囲を変更する

#### Split の補足

基本ショートカット：

```text
Ctrl + E
```

- Playhead 上に対象 Clip がある場合は、Playhead 位置で分割する
- Split Tool では、クリック位置で分割する

#### Paste の補足

- Paste 位置は Playhead を基準とする
- 複数 Clip の場合は相対配置を維持する

---

### 5.10 Clip Loop

Audio Clip と MIDI Clip の両方が Loop を持つ。  
Loop の 1 周期は、現在の Clip 内容である。

| Clip 種別  | 1 周期の定義            |
| ---------- | ----------------------- |
| Audio Clip | Trim 済みの Source 範囲 |
| MIDI Clip  | Clip 内部の MIDI Event  |

```text
|------|------|------|
```

Clip End を 1 周期より後ろへ伸ばすことで繰り返す。  
繰り返し境界は、Clip 内に視覚表示する。

---

## 6. Audio Clip と MIDI Clip

### 6.1 Audio Clip 表示

Audio Clip には、最低限以下を表示する。

- Name
- Waveform
- Selection
- Mute
- Loop Boundary
- Fade

```text
┌────────────────────────────┐
│ Guitar Take 12             │
│ ▁▂▅█▆▃▂▂▅██▅▃▂▅▆          │
└────────────────────────────┘
```

単なる長方形表示を完成形とはしない。

---

### 6.2 Waveform

Waveform は、事前生成された表示用データを使用してもよい。  
ただし、ユーザーからは Asset の実際の音声内容と一致して見える必要がある。

Waveform は以下に追従する。

- Zoom
- Trim
- Clip Gain
- Loop

Scroll や Zoom 中に、操作を妨げるほどの描画遅延を起こさない。

---

### 6.3 Gain / Pan / Fade / Crossfade / 重なり

#### Clip Gain / Pan

Audio Clip は、Track とは独立した Gain と Pan を持つ。

信号順序：

```text
Audio Source
↓
Clip Gain / Pan
↓
Track Rack
↓
Track Volume / Pan
↓
Master
```

Clip Gain の変更は、Waveform 表示上の振幅にも反映する。

#### Fade

Audio Clip は以下を持つ。

- Fade In
- Fade Out

Clip 上の Handle を Drag して変更し、Inspector からの数値指定もできる。  
Fade Curve の標準形状は、滑らかな等パワー系カーブとする。

#### Audio Clip の重なり

同一 Track 上で Audio Clip が時間的に重なることを許可する。  
音響上は両方を加算再生する。

重なった Clip は、Track 内の Sub-lane へ自動的に積み重ねて表示する。

```text
Guitar
├ Clip A  █████████████
└            Clip B ███████████
```

どちらか一方を、視覚的に完全に隠してはならない。

#### Crossfade

Audio Clip が重なっている場合、ユーザーは明示操作によって Crossfade を作成できる。  
Crossfade は自動生成しない。

- 標準カーブは Equal Power
- Crossfade 範囲は Clip 上で視覚的に確認できる
- Crossfade は非破壊編集情報として保持する

---

### 6.4 MIDI Clip 表示

MIDI Clip は、Clip 内部の Note 概要を表示する。

```text
┌───────────────────────────┐
│ Piano                     │
│ ━━   ━━━       ━━         │
│    ━      ━━━             │
└───────────────────────────┘
```

ダブルクリックすると、画面下部の MIDI Editor Panel を開く。

---

### 6.5 MIDI Editor

MIDI Editor は、Arrange 下部にリサイズ可能な Panel として表示する。

```text
Timeline
──────────────────────────

MIDI Editor
──────────────────────────
```

Panel を閉じると、Timeline 領域を拡大する。

#### Piano Roll

Piano Roll では、以下を直接操作できる。

- Add Note
- Select Note
- Move Note
- Resize Note
- Delete Note
- Duplicate Note
- Multiple Selection
- Velocity

数値フォームを主要操作にはしない。

---

### 6.6 MIDI Event と Quantize

#### MIDI Event

録音・保存・再生の対象として、最低限以下を扱う。

- Note On
- Note Off
- Control Change
- Pitch Bend
- Channel Pressure

Piano Roll 上に編集 UI を持たないイベントも破棄しない。  
保存・再生時には、元のイベントを保持する。

#### Quantize

Quantize は、選択中の MIDI Note に対して実行する。

| 項目        | 仕様                                            |
| ----------- | ----------------------------------------------- |
| 対象        | 選択中の MIDI Note                              |
| Grid        | 1/4、1/8、1/16、1/32、1/8 Triplet、1/16 Triplet |
| Strength    | 本仕様では 100%                                 |
| 変更対象    | Note Start                                      |
| Note Length | 自動変更しない                                  |
| Undo        | 可能                                            |

---

## 7. 素材配置・録音・Take

### 7.1 Library からの配置

Library の Asset を Timeline へ Drag & Drop できる。

| Drop 対象                     | 結果                                  |
| ----------------------------- | ------------------------------------- |
| Audio Asset → Audio Track     | Audio Clip を作成する                 |
| MIDI Asset → Instrument Track | MIDI Clip を作成する                  |
| 対応 Track がない空白領域     | 対応する Track を自動作成して配置する |

- 配置位置は Drop 位置とする
- Snap が有効な場合は Snap 位置へ配置する

---

### 7.2 Audition

Library Preview は、Timeline Playback とは独立した Audition Bus で再生する。  
Audition 音は、以下へ混入しない。

- Timeline Recording
- Processed Recording
- Offline Render
- Track Rack

録音中でも同じ規則を適用する。

---

### 7.3 Timeline Recording 共通

基本フロー：

```text
Track を Record Arm
↓
Record
↓
Count-in
↓
Timeline Playback 開始
↓
Recording
↓
Stop
↓
Clip 確定
```

Audio 録音中は、Timeline 上に成長する録音領域を表示する。  
録音終了後、別画面で配置操作を要求しない。

---

### 7.4 Audio Recording

Audio Track ごとに、以下を持つ。

- Input
- Monitoring
- Rack
- Record Arm

既存 Clip と同じ時間範囲へ録音しても、既存 Clip を破壊しない。  
録音結果は新しい Clip として追加する。

---

### 7.5 Raw / Processed

一回の Audio Recording から生成された Raw と Processed は、同じ Take の Variant として扱う。

```text
Take
├ Raw
└ Processed
```

Clip は以下を持つ。

```text
Active Variant
Raw | Processed
```

Variant を切り替えても、以下は維持する。

- Timeline Position
- Trim
- Gain
- Pan
- Fade
- Take Relation

Raw と Processed は同じ録音開始位置を共有し、サンプル単位で比較可能な同期を維持する。

---

### 7.6 Recording Session と Take

Take は必ず Recording Session に属する。

```text
Recording Session
├ Take 1
├ Take 2
├ Take 3
└ Take 4
```

以下の場合に同一 Session となる。

- 同一 Loop Recording 内の各周回
- 同じ Recording Session に対して明示的に「Record Another Take」を実行した場合

別々に開始した Recording を、音声類似度だけで自動的に同一 Take Group へ統合しない。

---

### 7.7 Loop Recording

Loop が有効な状態で録音した場合、周回ごとに Take を生成する。

```text
Loop 1 → Take 1
Loop 2 → Take 2
Loop 3 → Take 3
```

- 録音停止時は、最後に録音された Take を Active Take として Timeline に表示する
- 最後の周回途中で停止した場合、その部分録音も Take として保存する
- 他 Take は失われない

Take 一覧から、以下を実行できる。

- Preview
- Activate
- Compare
- Place as Separate Clip

Take 切り替え時も、Clip の Timeline 編集情報を維持する。

---

### 7.8 Punch Recording

Time Selection を Punch Range として設定できる。

```text
Punch 前
Playback

Punch Range
Recording

Punch 後
Playback
```

元 Clip を破壊せず、録音結果は新しい Take / Clip として保持する。

---

### 7.9 MIDI Recording

Instrument Track を Record Arm して録音する。  
Timeline 位置を基準として MIDI Event を記録し、録音後は即座に MIDI Clip として表示する。

演奏タイミングを自動 Quantize しない。  
Quantize は録音後にユーザーが明示的に実行する。

---

### 7.10 複数 Track 同時録音

複数の Audio Track および Instrument Track を同時に Record Arm できる。  
Record 開始時には、Arm されたすべての Track へ同じ Transport Start を適用する。

すべての録音結果は、共通の Timeline 基準で同期する。

---

### 7.11 Metronome / Count-in

Metronome は ON / OFF できる。

Count-in：

- Off
- 1 Bar
- 2 Bars

Count-in は現在の BPM と拍子に従う。  
Metronome 音は、以下へ混入しない。

- Raw Recording
- Processed Recording
- Offline Render

---

## 8. 再生品質・レンダリング・保存・障害

### 8.1 Latency 補正

Audio Device の Input / Output Latency を考慮して、録音結果の Timeline 位置を補正する。

毎回同じ量だけ録音 Clip が演奏位置からずれる状態を許容しない。  
補正後の Clip 位置は、ユーザーが実際に Transport に合わせて演奏した位置と一致することを目指す。

---

### 8.2 Plugin Delay Compensation

Track Rack または Instrument が報告する処理遅延を考慮する。  
異なる Track 間で、Plugin Delay による再生ずれを発生させない。

```text
Track A
Plugin Delay 20 ms

Track B
Plugin Delay 0 ms
```

この場合でも、最終出力では Timeline 上の同じ位置が同期して聞こえるよう補正する。  
Playback と Offline Render の両方に、同じ遅延補正規則を適用する。

---

### 8.3 Sample Rate / Channel Format

異なる Sample Rate の Audio Asset を、同一 Timeline 上で使用できる。

例：

- 44.1 kHz
- 48 kHz
- 96 kHz

ユーザーへ事前変換を要求しない。  
Playback Engine は、現在の Audio Device / Project 再生環境へ必要な変換を非破壊で行う。

Mono / Stereo Asset も同一 Timeline 上で扱える。  
元 Asset 自体は変更しない。

---

### 8.4 Offline Render / Export

Arrange から完成音声を書き出せる。  
最低限、以下の Render Range を選択できる。

- Entire Arrangement
- Loop Range
- Time Selection

Offline Render は、実際の Timeline Playback と同じ状態を使用する。

対象：

- Clip Position
- Clip Trim
- Clip Gain
- Clip Pan
- Fade
- Crossfade
- Mute
- Solo
- Track Rack
- Track Volume
- Track Pan
- Automation
- Plugin Delay Compensation

通常再生で聞いていた結果と、Offline Render 結果が意図せず異ならないことを保証する。

---

### 8.5 Project 保存

Arrange の状態は、Project / Session の一部として保存する。  
最低限、以下を保存する。

- Project Tempo
- Time Signature
- Track
- Track Order
- Track Settings
- Clip
- Clip Position
- Trim
- Gain
- Pan
- Fade
- Crossfade
- Loop
- Mute
- MIDI Events
- Marker
- Timeline Loop Range
- Automation
- Take Relation
- Active Take
- Active Variant

Project を再度開いた場合、Arrange の編集状態を復元する。

---

### 8.6 Missing Asset / Missing Plugin

#### Missing Asset

Asset が見つからない場合でも、Project 全体を開く。  
該当 Clip は `Missing` 状態で Timeline に残す。

ユーザーは以下を実行できる。

- Search Again
- Locate File
- Replace Asset

Clip の Timeline 編集情報は保持する。  
Missing Asset を黙って削除しない。

#### Missing Plugin

Project で使用していた Plugin が見つからない場合でも、Project を開く。  
該当 Device は `Missing Device` として保持する。

- Track を削除しない
- 他 Track は再生可能とする
- Missing Plugin の設定情報を保持する
- Plugin 再検出後に復元できる
- 別 Plugin へ置換できる

Missing Plugin の存在によって、Arrange 全体を使用不能にしない。

---

### 8.7 Riffra の制作履歴への接続

Arrange 上の Clip から、Recording Session へ辿れる。

```text
Clip
↓
Take
↓
Recording Session
├ Take 1
├ Take 2
├ Take 3
└ Take 4
```

各 Take は、必要に応じて以下を持つ。

```text
Take
├ Raw
└ Processed
```

通常の Timeline 画面へ、履歴情報を常時大量表示しない。  
必要な場合に、Inspector や Take Browser からアクセスする。

---

## 9. 操作・Inspector・Automation

### 9.1 Inspector

Inspector は、選択対象によって内容を変更する。

| 選択対象         | 主な表示項目                                                                                               |
| ---------------- | ---------------------------------------------------------------------------------------------------------- |
| Audio Clip       | Position、Length、Source Start、Source End、Gain、Pan、Fade In、Fade Out、Loop、Mute、Active Variant、Take |
| MIDI Clip        | Position、Length、Loop、Mute、Note Count、Quantize 操作                                                    |
| Audio Track      | Name、Input、Monitoring、Volume、Pan、Rack                                                                 |
| Instrument Track | Name、MIDI Input、Instrument、Rack、Volume、Pan                                                            |
| 複数 Clip        | 共通変更可能な項目のみ表示。異なる値を持つ項目は `Mixed` と表示                                            |

Audio Clip 選択時は、以下へもアクセスできる。

- Source Asset
- Recording Session
- Related Takes

---

### 9.2 Automation

Track ごとに Automation Lane を表示できる。  
対象 Parameter は以下である。

- Volume
- Pan

Automation 表示は、Track 単位で開閉する。

#### Automation Point

- クリックで Point を追加する
- Drag で移動する
- Delete で削除する
- Point 間は線形補間する

#### Automation Playback

Playback および Offline Render の両方へ反映する。  
Automation 値は、Track の通常 Parameter 値を時間軸上で制御する。

#### 対象外

- リアルタイム Automation Recording
- Plugin Parameter Automation

これらは本仕様に含めない。

---

### 9.3 Undo / Redo

原則として、すべての編集操作を Undo 可能とする。

対象：

- Clip Move
- Trim
- Split
- Delete
- Duplicate
- Fade
- Gain
- Pan
- Track 操作
- MIDI 編集
- Quantize
- Marker
- Automation
- Loop Range

Shortcut：

```text
Ctrl + Z
Ctrl + Shift + Z
```

録音済みの実 Asset は、Undo だけで物理削除しない。  
Recording Asset のライフサイクルと、Timeline 編集履歴は分離する。

---

### 9.4 Context Menu

重要機能を Context Menu だけに隠さない。  
Context Menu は、操作の近道として使用する。

#### Audio Clip

- Split
- Duplicate
- Mute
- Loop
- Crossfade
- Set Loop to Clip
- Show Source
- Show Takes
- Delete

#### MIDI Clip

- Split
- Duplicate
- Mute
- Loop
- Quantize
- Delete

#### Track

- Rename
- Duplicate
- Add Track
- Delete

#### Time Ruler

- Add Marker
- Set Loop to Selection
- Set Punch Range

---

### 9.5 Keyboard Shortcuts

最低限、以下を提供する。

| 操作               | Shortcut         |
| ------------------ | ---------------- |
| Play / Stop        | Space            |
| Go to Start        | Home             |
| Undo               | Ctrl + Z         |
| Redo               | Ctrl + Shift + Z |
| Split              | Ctrl + E         |
| Duplicate          | Ctrl + D         |
| Delete             | Delete           |
| Copy               | Ctrl + C         |
| Paste              | Ctrl + V         |
| Select All         | Ctrl + A         |
| Temporary Snap Off | Alt              |
| Duplicate Drag     | Alt + Drag       |

ショートカットの変更機能自体は、本仕様に含めない。

---

## 10. 品質基準・受入条件・中心体験

### 10.1 Core Timeline 品質基準

以下は、Arrange の一部機能ではなく、Timeline 基盤そのものの完成条件である。

| 項目            | 品質                                                        |
| --------------- | ----------------------------------------------------------- |
| Playback        | Play 操作後、実用上不自然な待ち時間なく再生を開始する       |
| Seek            | 任意位置から即座に再生できる                                |
| Playhead        | 実際の再生位置と同期して滑らかに動く                        |
| Waveform        | Zoom / Scroll 時に、操作を妨げる描画遅延を発生させない      |
| Drag            | Clip が Pointer 操作へ自然に追従する                        |
| Snap            | どの位置へ吸着したか視覚的に分かる                          |
| Undo            | 編集操作を安全に試せる                                      |
| Recording       | 停止後すぐに結果が Timeline へ表示される                    |
| Synchronization | Audio、MIDI、Loop、Automation が共通 Transport 上で同期する |
| Stability       | 長時間利用しても Audio Engine が不安定にならない            |

---

### 10.2 Core Acceptance Criteria

以下を満たした状態を、「リアルタイム Timeline 基盤が成立した状態」とする。

1. Timeline を Render なしでリアルタイム再生できる
2. Playhead が Audio Engine の再生位置と同期する
3. Seek できる
4. Loop 再生できる
5. BPM と拍子に基づく音楽時間を扱える
6. Audio Clip を Waveform 付きで表示できる
7. Clip を Drag 移動できる
8. Trim できる
9. Split できる
10. Duplicate できる
11. Undo / Redo できる
12. Snap が動作する
13. Zoom / Scroll が実用的な品質で動作する
14. Track Mute / Solo / Volume / Pan が動作する
15. Audio と MIDI が共通 Transport で同期する
16. Library から Drag & Drop できる
17. Project 再読み込み後に Timeline 状態を復元できる

この条件は、Arrange 全体の完成条件ではない。

---

### 10.3 Full Arrange Acceptance Criteria

Arrange 完成時には、Core Acceptance Criteria に加えて以下を満たす。

1. Timeline へ Audio を直接録音できる
2. MIDI を直接録音できる
3. 複数 Track を同時録音できる
4. Loop Recording と Take 管理が動作する
5. Punch Recording が動作する
6. Raw / Processed を同一 Take として比較・切り替えできる
7. Recording Session と Take の関係を保持できる
8. MIDI Piano Roll で直接編集できる
9. MIDI Quantize が動作する
10. MIDI Control Change 等を失わず保存・再生できる
11. Fade が動作する
12. Crossfade が動作する
13. Audio Clip Loop が動作する
14. MIDI Clip Loop が動作する
15. Marker を編集できる
16. Volume / Pan Automation が動作する
17. 異なる Sample Rate の Audio を同じ Timeline で再生できる
18. Mono / Stereo Asset を同じ Timeline で扱える
19. 録音レイテンシー補正が動作する
20. Plugin Delay Compensation が動作する
21. Offline Render 結果が Timeline Playback と一致する
22. Missing Asset から復旧できる
23. Missing Plugin が存在しても Project を開ける
24. Recording、Asset、Take、Raw / Processed の関連を維持できる

---

### 10.4 Arrange の中心体験

完成した Arrange では、以下の操作を一つの連続した制作行為として行える。

```text
Guitar Track を作る
↓
Input と Rack を設定する
↓
4 小節を Loop する
↓
Record
↓
演奏する
↓
複数 Take を録音する
↓
最後の Take が Timeline へ表示される
↓
Space で即座に確認する
↓
別 Take へ切り替える
↓
Raw / Processed を比較する
↓
気に入った Take を選ぶ
↓
Clip を移動・Trim する
↓
ドラム MIDI を追加する
↓
Piano Roll で編集する
↓
Timeline を再生しながら構成する
↓
完成結果を書き出す
```

この一連の流れの中で、以下を要求しない。

- 毎回 WAV へ Render する
- 録音ファイルを手動で探す
- 録音後に別画面で Timeline へ配置する
- 基本編集のために大量の数値フォームを操作する
- Asset との関係を手動で管理する

Arrange は、DAW として必要な基本品質を備えながら、Riffra の「演奏した結果と試行の履歴を失わず、そのまま音楽へ育てる」という制作体験の中心になる。
