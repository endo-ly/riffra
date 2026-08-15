# Riffra Arrange 画面仕様

## 1. 位置付け

本書は、Riffra の Arrange 画面における画面構造と操作仕様の正本である。Arrange の UI/UX を変更するときは、本書の仕様を基準に実装する。

制作データの責務は `data-model.md`、React / Tauri / Core / Native Runtime の責務分担は `architecture.md`、通信契約は `ipc.md` を参照する。本書では、それらを利用者がどのように操作し、画面上でどのように認識するかを定義する。

---

## 2. Arrange の役割

Arrange は Riffra の主画面であり、演奏、録音、音色設定、Audio / MIDI Clip の配置、MIDI 編集、再生確認を一つの作業空間でつなぐ。

基本的な MIDI 制作は、次の流れで完結する。

```text
Instrument Track を作成
        │
        ▼
Instrument を選択
        │
        ▼
Timeline に MIDI Clip を作成
        │
        ▼
Lower Panel で MIDI Editor を開く
        │
        ▼
Note を入力・選択・移動・複製・調整
        │
        ▼
再生しながら修正
        │
        ▼
Clip を Timeline 上で組み立てる
```

Audio も同じ Timeline を中心に扱い、素材投入、録音、Clip 編集、Track 設定、再生確認を Arrange 内で連続して行う。

---

## 3. 画面構成

```text
┌────────────────────────────────────────────────────────────────────────────┐
│ Global Bar                                                                 │
│ Project / Undo / Redo / Workspace / Command / Audio / Sync                │
├──────────────┬─────────────────────────────────────────────┬───────────────┤
│ Library      │ Arrange                                     │ Inspector     │
│              │                                             │               │
│ Assets       │ ┌─────────────────────────────────────────┐ │ Selection     │
│ Search       │ │ Arrange Toolbar                         │ │ details       │
│ Drag source  │ ├─────────────────────────────────────────┤ │               │
│              │ │ Ruler                                   │ │ Track         │
│              │ ├─────────────────────────────────────────┤ │ Clip          │
│              │ │ Track 1  ┃ Audio / MIDI Clips           │ │ Multi Clip    │
│              │ │ Track 2  ┃ Audio / MIDI Clips           │ │ Take          │
│              │ │ Track 3  ┃ Audio / MIDI Clips           │ │               │
│              │ │          ┃ Automation                    │ │               │
│              │ ├─────────────────────────────────────────┤ │               │
│              │ │ Lower Panel                             │ │               │
│              │ │ Play Surface / MIDI Editor              │ │               │
│              │ └─────────────────────────────────────────┘ │               │
├──────────────┴─────────────────────────────────────────────┴───────────────┤
│ Transport Bar                                                              │
│ Go to Start / Play / Stop / Record / Tempo / Meter / Master               │
└────────────────────────────────────────────────────────────────────────────┘
```

Timeline が通常の中心領域となる。Library、Inspector、Lower Panel は作業内容に応じて開閉・リサイズでき、MIDI 編集へ集中するときは Lower Panel を広く使える。

### 3.1 各領域の役割

| 領域            | 役割                                                          |
| --------------- | ------------------------------------------------------------- |
| Global Bar      | プロジェクト全体、Undo / Redo、ワークスペース、音声・同期状態 |
| Library         | Audio / MIDI などの素材を探し、Timeline へ投入する            |
| Arrange Toolbar | Timeline 全体の編集ツール、Snap、表示・追従設定               |
| Timeline        | Track、Clip、Ruler、Playhead、Automation を直接編集する       |
| Lower Panel     | MIDI の詳細編集と Instrument の演奏                           |
| Inspector       | 現在選択中の Track / Clip / Take の詳細設定                   |
| Transport Bar   | 再生、停止、録音、テンポ、メーターなど制作進行を制御する      |

---

## 4. 選択・編集対象・演奏先

Arrange では、似て見える四つの状態を分けて扱う。

```text
                  Arrange Selection
                  Track / Clip(s)
                        │
                        ├──────────────→ Inspector
                        │
                        │ MIDI Clip を編集
                        ▼
                 Active MIDI Clip
                        │
                        ▼
                MIDI Note Selection

Focused Instrument Track ───────────→ Play Surface / Computer MIDI

Active MIDI Clip の Track ──────────→ MIDI Editor の Note Preview
```

### 4.1 Arrange Selection

Timeline 上で選択している Track または Clip 群を表す。Inspector はこの選択を表示対象とする。

### 4.2 Focused Instrument Track

Play Surface や Computer Keyboard から MIDI を送る演奏先である。Clip の編集対象を変えても、演奏先は自動的には切り替わらない。

### 4.3 Active MIDI Clip

MIDI Editor が現在編集している Clip である。MIDI Clip をダブルクリックすると Active MIDI Clip となり、Lower Panel が MIDI Editor を表示する。

MIDI Editor が開いている状態で別の MIDI Clip を通常選択した場合は、その Clip へ編集対象も追従する。複数 Clip を追加選択している最中は、編集対象を不用意に変更しない。

### 4.4 MIDI Note Selection

Active MIDI Clip 内だけで有効な Note 選択である。Clip を切り替えたときは、新しい Clip の Note だけを対象にする。

---

## 5. Arrange Toolbar

```text
┌─────────────────────────────────────────────────────────────────────┐
│ [Select|Split]  Snap [1/16 ▾]  [Follow] [Automation]                │
│                                       Bars/Time   Zoom [−][＋] 100% │
└─────────────────────────────────────────────────────────────────────┘
```

Toolbar には Timeline 全体へ作用する頻出操作を置く。状態を持つ操作は、現在の有効状態が画面上で判別できる。

左側に編集操作、右側に表示切り替えを置く。Select / Split / Follow / Automation はアイコンボタンで、名前はツールチップで示す。ボタンはアイコンのみの固定サイズで統一し、ラベル文字列の長さが Toolbar の幅に影響しない。Zoom は段階的な拡大・縮小と現在倍率の表示を持つ。

| 要素          | 挙動                                          |
| ------------- | --------------------------------------------- |
| Select        | Clip の選択、移動、Trim、Marquee など通常編集 |
| Split         | 指定位置で Clip を分割                        |
| Snap          | Timeline の時間編集で使うグリッド単位         |
| Follow        | 再生中、Playhead を表示範囲へ追従させる       |
| Automation    | 選択 Track の Automation Lane を開閉する      |
| Bars / Time   | Ruler の表示形式を切り替える                  |
| Timeline Zoom | Timeline の時間方向を段階的に拡大・縮小する   |

Snap は Clip 移動、Trim、Split、Time Selection、Marker 移動など Timeline 上の時間操作で共通に使う。

---

## 6. Timeline

### 6.1 Ruler

```text
        Marker
          ▼
┌──────────────────────────────────────────────────────────────────┐
│  1.1        1.2        1.3        1.4        2.1        2.2     │
│      ├──────────── Loop ────────────┤                            │
│                       │ Playhead                                  │
└──────────────────────────────────────────────────────────────────┘
```

Ruler は時間位置の確認と範囲操作をまとめる。

| 操作                        | 結果                                          |
| --------------------------- | --------------------------------------------- |
| クリック                    | Playhead をその位置へ移動                     |
| ドラッグ                    | Time Selection を作成                         |
| Marker ドラッグ             | Marker を移動                                 |
| Loop / Punch の端をドラッグ | 範囲を変更                                    |
| Context Menu                | Marker 追加、選択範囲から Loop / Punch を設定 |

Time Selection、Loop / Punch、Playhead は別の状態として同時に認識できる表示にする。

### 6.2 Track Row

```text
┌──────────────────┬──────────────────────────────────────────────────────┐
│ ● Guitar         │                                                      │
│ AUDIO            │  [ Audio Clip ]        [ Audio Clip ]               │
│ [M] [S] [R] [IN] │                                                      │
│ VOL ─────  PAN   │                                                      │
├──────────────────┼──────────────────────────────────────────────────────┤
│ ● Synth          │      [ MIDI Clip ]                 [ MIDI Clip ]     │
│ INSTRUMENT       │                                                      │
│ [M] [S] [R]      │                                                      │
│ VOL ─────  PAN   │                                                      │
└──────────────────┴──────────────────────────────────────────────────────┘
```

Track Header は、Track の識別と演奏・録音時に頻繁に触る操作を持つ。入力ルーティング、Instrument、Plugin Chain など詳細設定は Inspector で扱う。

| 項目              | 仕様                                               |
| ----------------- | -------------------------------------------------- |
| Track Name        | 選択と名前変更の入口                               |
| Track Kind        | Audio / Instrument を識別できる                    |
| Mute / Solo / Arm | Header から直接変更できる                          |
| Monitoring        | Audio Track で現在状態を直接確認・変更できる       |
| Mix               | Track の表示密度に応じて Volume / Pan を操作できる |
| Reorder           | Track を並べ替えられる                             |
| Height            | Track ごとに表示密度を変更できる                   |
| Play Surface      | Instrument Track から演奏面を開ける                |

### 6.3 Clip 共通操作

Audio Clip と MIDI Clip は、Timeline 上の素材として共通した操作体系を持つ。

| 操作                    | 仕様                                         |
| ----------------------- | -------------------------------------------- |
| クリック                | 単一選択                                     |
| Ctrl / Shift + クリック | 選択を追加・解除                             |
| 空白をドラッグ          | Marquee で複数選択                           |
| ドラッグ                | 時間位置、対応 Track を変更                  |
| 左右端をドラッグ        | Clip 範囲を Trim                             |
| Duplicate               | 直後へ複製                                   |
| Delete                  | 選択 Clip を削除                             |
| Split                   | Playhead または指定位置で分割                |
| Mute / Loop             | Clip 単位の状態を変更                        |
| Context Menu            | 直接操作と同じ意味の操作、Merge など補助操作 |

### 6.4 Audio Clip

Audio Clip は波形を中心に表示し、Timeline 上で素材の使用範囲と接続関係を把握できる。

```text
┌──────────────────────────────────────────┐
│ Guitar Take 3                            │
│ ▂▃▅▆▅▃▂▂▃▄▆▇▆▄▂▂▃▅▆▄▃                  │
│ ◢                                      ◣ │
└──────────────────────────────────────────┘
  ↑ trim / fade                    trim / fade ↑
```

左右端は Trim、上部の Fade handle は Fade In / Fade Out を操作する。Mute、Loop、Duplicate、Split、Merge などは Clip の Context Menu と Inspector からも実行できる。

### 6.5 MIDI Clip

MIDI Clip は内部 Note の配置を Timeline 上で簡易表示する。Clip のダブルクリックで MIDI Editor を開く。

Trim は Clip 範囲の編集であり、Note 自体の長さ変更とは別の操作として扱う。

### 6.6 空 MIDI Clip の作成

Instrument Track では、外部 MIDI ファイルを用意せずに打ち込みを始められる。

```text
Instrument Track の空白
        │
        ├─ Double Click ───────────────┐
        │                              │
        └─ Context Menu                │
            "Insert MIDI Clip"        │
                                       ▼
                             ┌──────────────────┐
                             │ New MIDI Clip    │
                             └──────────────────┘
                                       │
                                       ▼
                               MIDI Editor を開く
```

作成位置は Timeline の Snap に従う。Time Selection がある場合はその範囲を Clip に使用し、通常はクリック位置から一小節の Clip を作成する。作成後はその Clip を選択し、MIDI Editor で入力を始められる状態にする。

### 6.7 素材の投入

Library または OS から Audio / MIDI 素材を Track へドラッグできる。ドラッグ中は投入可能な Track を視覚的に示し、種別が合わない場合はその理由を短く表示する。

### 6.8 空の Arrangement

Track が一つもない状態では、Audio Track / Instrument Track の作成と素材 Drop の入口を中央に提示する。

```text
┌───────────────────────────────────────────────┐
│                                               │
│                Start arranging                │
│                                               │
│      [ Add Audio Track ] [ Add Instrument ]   │
│                                               │
│           Drop Audio / MIDI here              │
│                                               │
└───────────────────────────────────────────────┘
```

### 6.9 Automation

Automation は対象 Track の直下へ Lane として展開する。Timeline の時間軸を共有し、Playhead、Snap、Zoom と同じ位置関係で編集する。

```text
Track: Synth
├─ MIDI Clips        [======]       [======]
└─ Automation: Volume
       •───────•
                ╲
                 •────────────•
```

Parameter selector から現在対応している Automation 対象を選び、Point の追加・移動・削除を Lane 上で行う。Automation の表示状態は Track ごとに保持する。

### 6.10 録音中の Timeline 表示

録音中は、開始位置から現在位置までを Track 上で明確に表示する。録音対象 Track と現在の Take / Pass を確認でき、通常の Playhead と録音範囲を見分けられる。

```text
Instrument Track
│                     REC · PASS 2
│              ├██████████████████│
│              ▲                  ▲
│           record start       current
```

録音完了後は作成された Clip / Take を通常の Arrange Selection と Inspector から扱える。

---

## 7. Lower Panel

Lower Panel は、Timeline を見ながら詳細編集または演奏を行う領域である。

```text
┌───────────────────────────────────────────────────────────────────────┐
│ [Play Surface | MIDI Editor]       Synth Lead / Verse MIDI    [—][□]│
├───────────────────────────────────────────────────────────────────────┤
│                                                                       │
│                         Active Panel                                  │
│                                                                       │
└───────────────────────────────────────────────────────────────────────┘
```

Header には View 切替と現在の編集文脈をまとめる。View 切替は Segmented Control で Header の左端に置き、MIDI Clip を選択していない間は MIDI Editor を選択できない。同じ Track 名や Clip 名を各 Panel 内で大きく繰り返さない。

Lower Panel は、折りたたみ、リサイズ、集中表示を行える。MIDI Editor を集中表示したあとも、元の Timeline 中心の状態へ戻れる。

### 7.1 Play Surface

Play Surface は Focused Instrument Track を演奏するための画面である。Keyboard / Drum Pads などの演奏 UI はここに置く。

```text
Focused Instrument Track
        │
        ▼
┌────────────────────────────────────────────┐
│ Track: Synth Lead      Instrument: VST3    │
│                                            │
│  [ Keyboard ]  /  [ Drum Pads ]           │
│                                            │
│ Octave / Velocity / Input status           │
└────────────────────────────────────────────┘
```

演奏先 Track、Instrument、Runtime の状態を同じ場所で確認できる。演奏できない場合も MIDI Clip 編集や Timeline 操作は継続できる。

---

## 8. MIDI Editor

### 8.1 画面構造

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ [Pointer|Draw]  Snap [1/16 ▾]  [Preview]  [Quantize] [Duplicate]        │
│ VEL [───●── 96]                  Time [−][＋]   Pitch [−][＋]           │
├─────────────┬────────────────────────────────────────────────────────────┤
│             │  9.1       9.2       9.3       9.4       10.1            │
│ Piano       ├────────────────────────────────────────────────────────────┤
│ Keyboard    │                                                            │
│             │     ┌──────────┐             ┌──────┐                     │
│ C5          │     │          │             │      │                     │
│             │     └──────────┘     ┌──────────────┐                     │
│ B4          │                      │              │                     │
│             │                      └──────────────┘                     │
│ A#4         │        ┌───────┐                                            │
│             │        └───────┘                                            │
│             │                         │ Playhead                           │
├─────────────┼────────────────────────────────────────────────────────────┤
│ Velocity    │        │       │        │        │                         │
│             │        █       █        █        █                         │
└─────────────┴────────────────────────────────────────────────────────────┘
```

MIDI Editor は Piano Roll、Musical Ruler、Velocity Lane を一つの時間軸で共有する。横 Scroll / Zoom は三領域で同期する。

Toolbar の構成は Arrange Toolbar と同じ規則に従う。左側に Pointer / Draw、Snap、Preview、Quantize / Duplicate、選択 Note の Velocity を置き、右側に Time / Pitch の Zoom を置く。Pointer / Draw / Preview / Quantize / Duplicate はアイコンボタンで、名前はツールチップで示す。Velocity は選択 Note の値を示し、確定時に選択 Note 全体へ適用する。

### 8.2 Pointer と Draw

#### Pointer

Pointer は通常編集に使う。

| 操作                  | 結果                    |
| --------------------- | ----------------------- |
| Note をクリック       | 単一選択                |
| Ctrl / Shift + Note   | 選択を追加・解除        |
| 空白をクリック        | Note Selection を解除   |
| 空白をドラッグ        | Marquee 選択            |
| Note をドラッグ       | 時間位置と Pitch を変更 |
| Note の右端をドラッグ | Note 長を変更           |
| 空白をダブルクリック  | Note を作成             |

#### Draw

Draw は連続入力に使う。空白へのクリックで Note を追加し、横方向へドラッグして作成した場合はドラッグ幅を Note 長へ反映する。

### 8.3 Note 作成

Note の開始位置と初期長は現在の Grid に従う。Velocity は直前に入力または調整した値を引き継ぎ、打ち込みを続けたときに毎回同じ値へ戻らない。

```text
Grid = 1/8

        click
          ▼
│───────┬───────┬───────┬───────│
        └───────┘
         1/8 Note
```

### 8.4 Note の移動

選択 Note をドラッグすると、横方向で時間、縦方向で Pitch を変更する。複数選択中は Note 同士の相対関係を維持してまとめて移動する。

```text
Before                       After

C5   ┌────┐                  C5       ┌────┐
B4        ┌────┐       →     B4            ┌────┐
A4             ┌────┐        A4                 ┌────┐
     1.1  1.2                       1.2  1.3
```

ドラッグ中は移動後の位置を即時表示し、Pointer を離したときに一つの編集として確定する。

### 8.5 Note 長

Note の右端をドラッグして長さを変更する。複数 Note が選択されている場合は、選択群へ同じ長さ差分を適用できる。

### 8.6 Copy / Cut / Paste / Duplicate

Editor 内の Clipboard は Note 群の相対時間、Pitch、長さ、Velocity、Channel を保持する。

```text
Copy
  ┌───┐   ┌─────┐
  └───┘   └─────┘
    │       │
    └── Δtime ──┐
                ▼
Paste at Playhead
                        │ Playhead
                        ▼
                        ┌───┐   ┌─────┐
                        └───┘   └─────┘
```

Paste はコピーした Note 群の先頭を Playhead へ合わせる。貼り付けた Note には新しい ID が割り当てられ、貼り付け後はその Note 群を選択する。

Duplicate は選択したフレーズの長さを基準に、その直後へ複製する。

### 8.7 Delete

複数 Note の削除は一回の編集として扱う。Undo では削除した Note 群がまとめて復元される。

### 8.8 Quantize

Quantize は現在の Grid を基準に MIDI Note Selection へ適用する。

```text
Before        Quantize 1/8        After

  ●      ●          ─────→          ●   ●
│   │   │   │                      │   │   │
```

選択されている Note が操作対象であり、Clip 全体の処理とは分けて扱う。

### 8.9 Velocity Lane

Velocity Lane は Piano Roll と同じ時間位置に各 Note の Velocity を表示する。

```text
Piano Roll        ┌───────┐       ┌──────────┐
                  └───────┘       └──────────┘

Velocity              █                 █
                      █          █      █
                      █          █      █
────────────────────────────────────────────────
```

Note と Velocity は同じ選択状態を共有する。Velocity のドラッグ操作では画面上の値を連続表示し、操作終了時に編集を確定する。複数 Note を選択している場合は選択群をまとめて調整できる。

### 8.10 Piano Keyboard と Preview

Piano Keyboard は Pitch の目盛りと試聴操作を兼ねる。Preview が有効なとき、Key を押している間だけ Active MIDI Clip が属する Instrument Track へ Note On / Off を送る。

```text
MIDI Editor Preview

Active MIDI Clip
      │
      ▼
Instrument Track
      │
      ▼
Current Instrument
```

Play Surface の Focused Instrument Track とは別系統として扱う。Clip 切替や Editor 終了時には Held Note を残さない。

### 8.11 Ruler / Grid / Playhead

MIDI Editor の Ruler は Clip 内の相対時間だけでなく、Arrangement 上の小節位置を表示する。Timeline で 9 小節目に置かれた Clip を開いた場合、Editor でも 9.1、9.2、9.3… のように位置関係を把握できる。

現在の Snap Grid は Piano Roll 上にも細分線として反映する。Zoom に合わせて線の密度を調整し、Bar、Beat、Subdivision の階層を視認できるようにする。

Timeline の Playhead は MIDI Editor 上にも表示し、Ruler から Seek できる。

### 8.12 Zoom / Scroll

MIDI Editor は時間方向と Pitch 方向を独立して拡大縮小できる。Scroll と Zoom の中心は編集中の位置を維持し、拡大縮小によって対象 Note が突然見失われにくい動きにする。

### 8.13 MIDI Editor のキーボード操作

MIDI Editor に編集フォーカスがある間は、次のキーを Note 操作として解釈する。

| キー              | MIDI Editor                 |
| ----------------- | --------------------------- |
| `Ctrl+A`          | 全 Note 選択                |
| `Ctrl+C`          | Copy                        |
| `Ctrl+X`          | Cut                         |
| `Ctrl+V`          | Playhead へ Paste           |
| `Ctrl+D`          | Duplicate                   |
| `Delete`          | 選択 Note を削除            |
| `← / →`           | Grid 単位で時間移動         |
| `↑ / ↓`           | 半音単位で Pitch 移動       |
| `Shift + ↑ / ↓`   | オクターブ単位で Pitch 移動 |
| `Esc`             | Note Selection を解除       |
| `Ctrl+Z / Ctrl+Y` | Undo / Redo                 |

```text
Keyboard Event
      │
      ▼
┌─────────────────────┐
│ Focused edit region │
└─────────┬───────────┘
          │
     ┌────┴────┐
     ▼         ▼
MIDI Editor   Timeline
Note command  Clip command
```

テキスト入力中は、入力欄の編集操作を優先する。

---

## 9. Inspector

Inspector は Arrange Selection に応じて内容を切り替える。

```text
Arrange Selection
      │
      ├─ Track ─────────────→ Track Inspector
      ├─ Audio Clip ────────→ Audio Clip Inspector
      ├─ MIDI Clip ─────────→ MIDI Clip Inspector
      ├─ Multiple Clips ────→ Multi Clip Inspector
      └─ Recording Take ────→ Take Inspector
```

### 9.1 Track Inspector

Track の詳細設定を扱う。

| 領域         | 内容                                |
| ------------ | ----------------------------------- |
| Identity     | Track 名、種別                      |
| Input        | Audio / MIDI Input routing          |
| Instrument   | Instrument の選択、変更、Editor     |
| Plugin Chain | Effect の追加、順序、Bypass、Editor |
| Monitoring   | 入力 Monitoring                     |
| Mix          | Volume / Pan                        |
| Recovery     | Missing Plugin などの復旧           |

### 9.2 Audio Clip Inspector

Audio Clip Inspector は Timeline 上の Audio Clip 自体の属性を扱う。波形編集面で頻繁に触る Trim / Fade は Timeline の直接操作を主とし、Inspector では数値や状態を確認・調整する。

```text
AUDIO CLIP
────────────────
Name
Start / Length
Gain / Pan
Fade In / Fade Out
Mute / Loop

[Duplicate] [Delete]
```

### 9.3 MIDI Clip Inspector

MIDI Clip Inspector は Clip 自体の属性を扱う。

```text
MIDI CLIP
────────────────
Name
Start
Length
Mute / Loop

[Duplicate] [Delete]
```

Note の Pitch、Velocity、Note 一覧など、演奏内容の編集は MIDI Editor に集約する。

### 9.4 Multi Clip Inspector

複数 Clip に共通して変更できる項目だけを表示する。Audio / MIDI が混在する場合も、意味が共通する操作だけを提示する。

### 9.5 Take Inspector

録音 Clip または Track を選択したとき、Take Inspector は録音候補を確認し、試聴、採用、配置までを一つの領域で完結させる。複数の録音グループがある場合は、Track 選択時にグループを選べる。選択中のグループへ続けて録音する操作もこの領域に置く。

```text
TAKES
────────────────────────────────
              [Record another take]
Recording group       [Group 2  ]

Take 1                         CURRENT
          Audio source
          ○ Raw    ○ Processed
          [Place copy]                         [Preview]

Take 2                         MIDI
          [Use] [Place copy]

Take 3
          [Use] [Place copy]                   [Preview]
```

Preview は現在の Arrangement 再生とは独立した一回限りの試聴で、再生中は同じボタンが Stop になる。音声の終端に達した場合も、再生中の表示を解除する。Raw / Processed の両方を持つ Take は Source で音源を選び、再生中に切り替えると同じ位置から比較できる。この比較は試聴だけの操作であり、Timeline 上の Clip の音源設定は変更しない。

Use は選択した Take を録音グループの正準 Clip として採用する。現在採用中の Take は `CURRENT` と表示し、Use を無効にする。Place copy は候補を別 Clip として Timeline に配置し、既存の正準 Clip と重なって二重再生しないようミュートした状態で作成する。MIDI Take は音声試聴を表示せず、Use と Place copy を提供する。

---

## 10. Transport

Transport は、Timeline、MIDI Editor、Play Surface のどこを操作していても同じ意味を持つ。

```text
┌──────────────────────────────────────────────────────────────────┐
│ |◀  ▶  ■  ● |    120 BPM   4/4        Meter        Master       │
└──────────────────────────────────────────────────────────────────┘
```

| 操作                   | 役割                         |
| ---------------------- | ---------------------------- |
| Go To Start            | Arrangement の開始位置へ移動 |
| Play                   | Arrangement を再生           |
| Stop                   | 現在の再生を停止             |
| Record                 | Arm された Track を録音      |
| Tempo / Time Signature | Session の時間基準を編集     |
| Meter                  | 入出力状態を表示             |
| Master                 | Master Gain を表示・調整     |

MIDI Editor の Preview は Transport 再生とは独立した短い試聴であり、Play / Stop の状態とは混同しない。

---

## 11. 編集中のフィードバック

Arrange では、操作結果を待つ時間があっても「押したのか分からない」状態を作らない。

### 11.1 即時表示と確定

Clip や Note の Drag、Velocity の変更など連続操作は、画面上では操作に追従して Preview を表示する。操作終了後、Core から確定した制作状態へ収束する。

```text
Pointer move
    │
    ▼
UI Preview
    │
Pointer up
    │
    ▼
Canonical edit
    │
    ▼
Confirmed Session
```

### 11.2 状態の見分け方

同じ意味の状態は Arrange 全体で同じ視覚言語を使う。

| 状態              | 表現の役割                                     |
| ----------------- | ---------------------------------------------- |
| Hover             | 操作可能な対象を示す                           |
| Selected          | 編集対象を示す                                 |
| Focused           | MIDI の演奏先など継続的な対象を示す            |
| Active Tool / Tab | 現在の操作モードを示す                         |
| Pending           | 操作受付済みで確定待ちであることを示す         |
| Recording         | 録音中であることを明確に示す                   |
| Warning           | Missing source / plugin など制作上の問題を示す |
| Disabled          | 現在利用できない操作を示す                     |

選択色、録音色、警告色などの意味を Panel ごとに変えない。Controls、Surface、Typography は共通 UI の視覚規則を利用し、Piano Roll、Waveform、Clip といった音楽固有の描画だけを専用表現とする。

### 11.3 通知と復旧

一時的な成功・失敗は Toast へ集約し、制作を続けるために継続表示が必要な問題だけを残す。Runtime の同期ずれ、Missing source / plugin、Audio device fault などは、状態だけでなく復旧操作も同じ場所から辿れる。

```text
┌──────────────────────────────────────┐
│ Playback runtime is out of sync      │
│                              [Retry] │
└──────────────────────────────────────┘
```

---

## 12. 操作文脈とショートカット

同じキーでも、現在操作している領域によって対象が変わる。

| 文脈        | 主な対象 | `Ctrl+A`     | `Delete`  | `Ctrl+D`             |
| ----------- | -------- | ------------ | --------- | -------------------- |
| Timeline    | Clip     | 全 Clip 選択 | Clip 削除 | Clip 複製            |
| MIDI Editor | Note     | 全 Note 選択 | Note 削除 | Note 複製            |
| Text Input  | 文字列   | 文字列選択   | 文字削除  | OS / Text の既定動作 |

この優先順位によって、MIDI Editor を編集中に Timeline の Clip が誤操作されることを防ぐ。

### 12.1 Timeline の主要ショートカット

| キー     | Timeline                                   |
| -------- | ------------------------------------------ |
| `Ctrl+A` | 全 Clip 選択                               |
| `Ctrl+C` | 選択 Clip をコピー                         |
| `Ctrl+V` | Playhead 位置へペースト                    |
| `Ctrl+D` | 選択 Clip を直後へ複製                     |
| `Ctrl+E` | Playhead 位置で Split                      |
| `Delete` | 選択 Clip / 選択中の Marker・Range を削除  |
| `M`      | Playhead 位置へ Marker を追加              |
| `Z`      | Time Selection へ Zoom                     |
| `F`      | Arrangement の Clip 全体が見える範囲へ Fit |
| `Esc`    | 現在の一時選択・Dialog を閉じる            |

Undo / Redo、Transport、Workspace などアプリ全体のショートカットは Global command として扱う。

---

## 13. 基本制作シナリオ

Arrange の基本編集は、次の一連の操作が自然につながることを基準とする。

### 13.1 MIDI の打ち込み

```text
Add Instrument Track
        ↓
Choose Instrument
        ↓
Double Click empty lane
        ↓
MIDI Clip created + MIDI Editor opened
        ↓
Draw / Double Click notes
        ↓
Select phrase
        ↓
Move / Resize / Velocity / Quantize
        ↓
Ctrl+D
        ↓
Play
        ↓
Edit while listening
```

### 13.2 MIDI フレーズの修正

```text
Open MIDI Clip
      ↓
Ctrl+A
      ↓
Quantize
      ↓
Select several notes
      ↓
Shift + ↑
      ↓
Adjust Velocity Lane
      ↓
Ctrl+Z / Ctrl+Y
```

### 13.3 Timeline の組み立て

```text
Select Clip
    ↓
Duplicate / Move / Trim / Split
    ↓
Multi-select clips
    ↓
Copy / Paste
    ↓
Set Loop
    ↓
Play and review
```

---

## 14. 関連する実装領域

本書の仕様を主に担う実装は次の領域にある。ファイル構成は実装上の都合で変更できるが、責務は Arrange feature 内に保つ。

| 領域                       | 現在の主な実装                                           |
| -------------------------- | -------------------------------------------------------- |
| Arrange 全体               | `apps/desktop/src/features/arrange/WorkspaceArrange.tsx` |
| Timeline                   | `apps/desktop/src/features/arrange/timeline/`            |
| Arrange 操作               | `apps/desktop/src/features/arrange/hooks/`               |
| MIDI Editor                | `apps/desktop/src/features/arrange/midi-editor/`         |
| Lower Panel / Play Surface | `apps/desktop/src/features/arrange/play-surface/`        |
| Inspector                  | `apps/desktop/src/features/arrange/inspector/`           |
| Native capability          | `apps/desktop/src/features/arrange/arrange-api.ts`       |
| Core の Arrangement 編集   | `crates/riffra-core/src/application/arrangement/`        |
