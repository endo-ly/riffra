# Riffra Arrange 画面仕様

## 1. 位置付け

本書は、Riffra の Arrange ワークスペースに固有の画面構造と操作仕様の正本である。Global Control Bar、Navigation Rail、Side Panel、Main Canvas、Detail Area、Performance Panel の Arrange 上の配置と共通挙動も本書で定義する。

Arrange は Riffra の主制作画面であり、演奏、監視、録音、音色調整、Audio / MIDI Clip の配置、MIDI 編集、再生確認を一つの Arrangement 上でつなぐ。Timeline を中心に曲を組み立て、Browser と Inspector が素材探索・属性調整を支え、Detail Area が Clip や Track の内部編集、Performance Panel が演奏入力を担当する。

制作データは `../data-model.md`、アプリケーション内部の責務分担は `../architecture.md`、通信契約は `../ipc.md` を参照する。

## 目次

- [1. 位置付け](#1-位置付け)
- [2. Arrange の作業構造](#2-arrange-の作業構造)
- [3. Timeline](#3-timeline)
- [4. Side Panel](#4-side-panel)
- [5. Detail Area](#5-detail-area)
- [6. Performance Panel](#6-performance-panel)
- [7. 再生・録音とフィードバック](#7-再生録音とフィードバック)
- [8. 操作文脈とショートカット](#8-操作文脈とショートカット)
- [9. 基本制作シナリオ](#9-基本制作シナリオ)

---

## 2. Arrange の作業構造

### 2.1 画面構成

Arrange の Main Canvas は Timeline である。Browser または Inspector が Side Panel に現れ、MIDI Editor は Timeline の下側に Detail Area として開く。Devices は Inspector で扱い、Performance Panel は演奏入力が必要な場面で最下部へ独立して展開する。

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ GLOBAL CONTROL BAR                                                           │
│ Project / History                 Transport                 Audio / Safety   │
├──────┬──────────────────┬────────────────────────────────────────────────────┤
│      │                  │ ARRANGE · MAIN CANVAS                              │
│ NAV  │ SIDE PANEL       │ ┌────────────────────────────────────────────────┐ │
│      │                  │ │ Arrange Toolbar                                │ │
│      │ Browser          │ ├────────────────────────────────────────────────┤ │
│      │ Inspector        │ │ Ruler                                          │ │
│      │                  │ ├────────────────────────────────────────────────┤ │
│      │                  │ │                                                │ │
│      │                  │ │ Timeline · Tracks / Clips / Automation         │ │
│      │                  │ │                                                │ │
│      │                  │ ├────────────────────────────────────────────────┤ │
│      │                  │ │ DETAIL AREA                                    │ │
│      │                  │ │ MIDI Editor                                    │ │
├──────┴──────────────────┴────────────────────────────────────────────────────┤
│ PERFORMANCE PANEL · optional                                                 │
│ Focused Instrument Track / Keyboard / Drum Pads / Octave / Velocity         │
└──────────────────────────────────────────────────────────────────────────────┘
```

制作の中心は Timeline に置く。Side Panel は探索・属性、Detail Area は内部編集、Performance Panel は演奏という異なる役割を持つため、同時利用の意味も明確になる。たとえば Devices で Instrument を調整しながら Performance Panel で弾く、Timeline を再生しながら MIDI Editor で Note を直す、といった制作フローを画面切替だけに頼らず進められる。

### 2.2 選択・編集対象・演奏先

Arrange では、似て見える状態を役割ごとに分けて扱う。

```text
                         Arrange Selection
                         Track / Clip(s)
                               │
                 ┌─────────────┴─────────────┐
                 ▼                           ▼
            Inspector                   Track Context
                                             │
                                             ▼
                                           Devices

MIDI Clip を編集
        │
        ▼
Active MIDI Clip ───────────────→ MIDI Editor
        │
        └────────────────────────→ MIDI Note Selection

Focused Instrument Track ───────→ Performance Panel / Computer MIDI

Active MIDI Clip の Track ──────→ MIDI Editor Note Preview
```

#### Arrange Selection

Timeline 上で選択している Track または Clip 群を表す。Inspector が表示中の場合は、その内容が Arrange Selection に追従する。Browser の表示状態は素材探索の文脈として保持される。

#### Track Context

Devices が扱う Track を表す。Track を選択した場合はその Track、単一 Track に属する Clip を選択した場合は所属 Track が Track Context となる。

複数 Track にまたがる Clip 群では、一つの Device Chain を操作する意味が曖昧になるため、最後に明示された Track Context を Inspector に表示する。利用者は Track Header または Inspector から対象 Track を切り替える。

#### Active MIDI Clip

MIDI Editor が編集している MIDI Clip を表す。MIDI Clip のダブルクリックや明示的な Edit 操作で Active MIDI Clip を設定し、Detail Area に MIDI Editor を開く。

MIDI Editor が表示中で、単一の別 MIDI Clip を通常選択した場合は編集対象もその Clip へ追従する。複数選択では Active MIDI Clip を維持し、どの Clip の Note を編集しているかは MIDI Editor の編集対象として保持する。

#### MIDI Note Selection

Active MIDI Clip 内で選択している Note 群である。MIDI Editor の Pointer、Marquee、Keyboard Shortcut はこの Selection を対象とする。

#### Focused Instrument Track

Performance Panel、Computer Keyboard、演奏用 MIDI 入力の送り先である。Arrange Selection と Active MIDI Clip から独立して保持し、曲を編集しながら同じ Instrument を演奏できる。

Record Arm は Track の録音状態として Focus と分けて扱う。録音時は Arm 状態と Focus / MIDI routing の関係を画面上で確認できるようにする。

---

## 3. Timeline

### 3.1 Arrange Toolbar

Arrange Toolbar は Timeline 全体へ作用する頻出操作をまとめる。

```text
┌─────────────────────────────────────────────────────────────────────┐
│ [Select|Split]  Snap [1/16 ▾]  [Follow] [Automation]                │
│                                       Bars/Time   Zoom [−][＋] 100% │
└─────────────────────────────────────────────────────────────────────┘
```

左側は編集操作、右側は表示操作としてまとまりを持たせる。

| 要素          | 挙動                                          |
| ------------- | --------------------------------------------- |
| Select        | Clip の選択、移動、Trim、Marquee など通常編集 |
| Split         | 指定位置で Clip を分割                        |
| Snap          | Timeline の時間編集に使う Grid                |
| Follow        | 再生中の Playhead を表示範囲へ追従            |
| Automation    | 選択 Track の Automation Lane を開閉          |
| Bars / Time   | Ruler の表示形式を切替                        |
| Timeline Zoom | 時間方向の拡大・縮小                          |

Snap は Clip 移動、Trim、Split、Time Selection、Marker 移動など Timeline 上の時間操作で共通に使う。

### 3.2 Ruler と時間範囲

```text
        Marker
          ▼
┌──────────────────────────────────────────────────────────────────┐
│  1.1        1.2        1.3        1.4        2.1        2.2     │
│      ├──────────── Loop ────────────┤                            │
│                       │ Playhead                                  │
└──────────────────────────────────────────────────────────────────┘
```

Ruler は時間位置の確認と範囲操作を担う。

| 操作                        | 結果                                          |
| --------------------------- | --------------------------------------------- |
| クリック                    | Playhead をその位置へ移動                     |
| ドラッグ                    | Time Selection を作成                         |
| Marker ドラッグ             | Marker を移動                                 |
| Loop / Punch の端をドラッグ | 範囲を変更                                    |
| Context Menu                | Marker 追加、選択範囲から Loop / Punch を設定 |

Playhead、Time Selection、Loop / Punch、Marker は同じ時間軸上で同時に認識できる表示を使う。

### 3.3 Track Row

Track Header は Track の識別と、演奏・録音中に頻繁に触る操作を持つ。

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

| 項目              | 仕様                                          |
| ----------------- | --------------------------------------------- |
| Track Name        | 選択と名前変更の入口                          |
| Track Kind        | Audio / Instrument の識別                     |
| Mute / Solo / Arm | Header から直接変更                           |
| Monitoring        | Audio Track の現在状態を表示・変更            |
| Mix               | 表示密度に応じて Volume / Pan を操作          |
| Reorder           | Track の並べ替え                              |
| Height            | Track ごとの表示密度変更                      |
| Focus             | Instrument Track を演奏先として Focus         |
| Devices           | Track の Instrument / Effect 編集面を開く     |
| Performance       | Focused Track として Performance Panel を開く |

Input、Monitoring、名称など Track 自体の詳細属性は Inspector が扱う。Instrument と Effect Chain は Devices が扱う。Volume / Pan は制作中の確認頻度が高いため Track Header に簡易操作を置き、Inspector では数値確認と精密調整を行える。

### 3.4 Clip 共通操作

Audio Clip と MIDI Clip は Timeline 上の素材として共通の操作体系を持つ。

| 操作                    | 結果                            |
| ----------------------- | ------------------------------- |
| クリック                | 単一選択                        |
| Ctrl / Shift + クリック | 選択を追加・解除                |
| 空白をドラッグ          | Marquee による複数選択          |
| ドラッグ                | 時間位置または対応 Track を変更 |
| 左右端をドラッグ        | Clip 範囲を Trim                |
| Duplicate               | 直後へ複製                      |
| Delete                  | 選択 Clip を削除                |
| Split                   | Playhead または指定位置で分割   |
| Mute / Loop             | Clip 単位の状態を変更           |
| Context Menu            | Merge など補助操作へアクセス    |

### 3.5 Audio Clip

Audio Clip は波形を中心に表示し、素材の使用範囲と音の位置関係を Timeline 上で把握できるようにする。

```text
┌──────────────────────────────────────────┐
│ Guitar Take 3                            │
│ ▂▃▅▆▅▃▂▂▃▄▆▇▆▄▂▂▃▅▆▄▃                  │
│ ◢                                      ◣ │
└──────────────────────────────────────────┘
  ↑ Trim / Fade                    Trim / Fade ↑
```

左右端は Trim、Fade Handle は Fade In / Fade Out を担当する。Start、Length、Gain、Pan、Fade、Loop など数値確認を伴う属性は Inspector からも調整できる。

将来 Audio Editor を導入する場合は、Clip の内部波形や高度な音声処理を Detail Area で扱い、Timeline 上の構成編集との責務を分ける。

### 3.6 MIDI Clip

MIDI Clip は内部 Note の配置を簡易表示する。Clip のダブルクリックで Active MIDI Clip を設定し、Detail Area に MIDI Editor を開く。

Timeline 上の Trim は Clip が Arrangement 上で占める範囲を扱い、Note の開始・長さ・Velocity など演奏内容は MIDI Editor で扱う。

### 3.7 空 MIDI Clip の作成

Instrument Track の空白から、外部 MIDI ファイルを用意せずに打ち込みを始められる。

```text
Instrument Track の空白
        │
        ├─ Double Click
        │
        └─ Insert MIDI Clip
                │
                ▼
          New MIDI Clip
                │
                ▼
       Detail Area / MIDI Editor
```

作成位置は Timeline Snap に従う。Time Selection がある場合はその範囲を初期長として使い、通常時はクリック位置から一小節を初期長とする。作成後はその Clip を Active MIDI Clip として開き、すぐ Note 入力へ移れる状態にする。

### 3.8 素材の投入

Browser または OS から Audio / MIDI 素材を Timeline へドラッグできる。ドラッグ中は投入候補 Track と配置位置を視覚的に示し、Track Kind と Asset Kind の関係も同じ場所で理解できるようにする。

Browser の Preview は素材確認、Transport の Play は Arrangement 全体の再生として状態を分けて表示する。

### 3.9 空の Arrangement

空の Arrangement では Main Canvas 中央を制作開始の入口とする。

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

Instrument Track の作成では Add Instrument Browser へつなぎ、音源選択後に Track と Focus を自然に設定できる。

### 3.10 Automation

Automation は対象 Track の直下へ Lane として展開し、Timeline と同じ時間軸を使う。

```text
Track: Synth
├─ MIDI Clips        [======]       [======]
└─ Automation: Volume
       •───────•
                ╲
                 •────────────•
```

Parameter Selector から編集対象を選び、Point の追加、移動、削除を Lane 上で行う。Playhead、Snap、Zoom は Clip と Automation で同じ基準を共有する。表示状態は Track ごとに保持する。

### 3.11 録音中の表示

録音中は、録音開始位置から現在位置までを対象 Track 上へ表示する。

```text
Instrument Track
│                     REC · PASS 2
│              ├██████████████████│
│              ▲                  ▲
│           record start       current
```

録音対象 Track、現在の Pass、録音範囲を一つの視線で確認できる構成とする。録音完了後に生成された Clip / Take は Arrange Selection と Inspector から扱える。

---

## 4. Side Panel

Arrange の Side Panel は Browser と Inspector を扱う。本章では開閉・リサイズ・Navigation Rail との関係を含め、Arrange 固有の内容を定義する。

### 4.1 Browser

Browser は Audio / MIDI Asset、Recording、Inbox、Instrument、Effect などを探し、Timeline または Devices へ投入する。

```text
Browser
├─ Audio / MIDI Assets ─────→ Timeline
├─ Recordings / Inbox ──────→ Timeline / Take workflow
├─ Instruments ─────────────→ Track / Devices
└─ Effects ─────────────────→ Devices
```

Asset は Search、Preview、選択、Drag & Drop を通じて Timeline へつながる。Plugin は Track Context と追加位置を組み合わせ、Instrument または Effect として Devices へ追加する。

Instrument や Effect の追加ボタンから開く Add Browser は、現在の追加先を引き継いで候補を絞る。

### 4.2 Inspector

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

#### Track Inspector

Track 自体の属性を扱う。

| 領域       | 内容                                                                  |
| ---------- | --------------------------------------------------------------------- |
| Identity   | Track 名、種別                                                        |
| Input      | Audio / MIDI Input routing                                            |
| Monitoring | Input Monitoring                                                      |
| Mix        | Volume / Pan の数値確認と精密調整                                     |
| Status     | Input source、recording、missing dependency など Track に関係する状態 |

Instrument と Effect Chain は Devices へ集約し、Track Inspector は Track 属性へ集中する。

#### Audio Clip Inspector

Audio Clip の属性を扱う。

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

Timeline 上の Trim / Fade と同じ Clip を参照しながら、数値確認と精密調整を行える。

#### MIDI Clip Inspector

MIDI Clip 自体の属性を扱う。

```text
MIDI CLIP
────────────────
Name
Start
Length
Mute / Loop

[Duplicate] [Delete]
```

Pitch、Velocity、Note Length など演奏内容は MIDI Editor が担当する。

#### Multi Clip Inspector

複数 Clip の共通属性をまとめて調整する。Audio / MIDI が混在する場合は Start、Mute など意味を共有できる項目を中心に表示する。変更結果が選択対象全体へどのように反映されるかを確認できる表示を使う。

#### Take Inspector

Take Inspector は同じ録音意図を持つ候補を比較し、採用と配置を行う。

```text
TAKES
────────────────────────────────
              [Record another take]
Recording group       [Group 2]

Take 1                         CURRENT
          Audio source
          ○ Raw    ○ Processed
          [Place copy]                         [Preview]

Take 2                         MIDI
          [Use] [Place copy]

Take 3
          [Use] [Place copy]                   [Preview]
```

Raw / Processed の両方を持つ Audio Take は同じ位置から切り替えて比較できる。Use は録音グループの正準 Clip を更新し、Place copy は候補を別 Clip として Timeline へ配置する。

---

## 5. Detail Area

Detail Area は Timeline で扱う対象へ一段深く入り、演奏内容や信号経路を編集する。Arrange では MIDI Editor と Devices を主要な内容とする。

Detail Area は、Timeline から明示的に開いた MIDI Editor を表示する。外側に対象名を繰り返す文言ヘッダーは置かず、MIDI Editor 自身の Toolbar と編集対象を保ったまま作業を続けられる。

### 5.1 共通操作

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ MIDI Editor toolbar                              Collapse  Expand   ×   │
├──────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│                           Active MIDI Clip                               │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

Detail Area は、Resize、Collapse / Restore、Expand / Restore、Close を提供する。対象の切替は Timeline の MIDI Clip 選択から行い、Detail Area を閉じても Arrange Selection と Active MIDI Clip は維持する。Performance Panel は Detail Area と独立して開閉できる。

### 5.2 MIDI Editor

#### 画面構造

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

Piano Roll、Musical Ruler、Velocity Lane は一つの時間軸を共有する。横 Scroll / Zoom も同じ基準で動き、Active MIDI Clip が Arrangement のどこに位置しているかを Ruler 上で把握できる。

Toolbar は左側へ編集操作、右側へ表示操作をまとめる。Pointer / Draw、Snap、Preview、Quantize、Duplicate、Velocity、Time Zoom、Pitch Zoom が主な操作となる。

#### Note の作成と編集

Pointer は選択・移動・長さ変更、Draw は連続入力を担う。空白のダブルクリックでも Note を作成できる。開始位置と初期長は現在の Grid を基準とし、Velocity は直前の入力値を引き継ぐ。

| 操作                       | 結果                    |
| -------------------------- | ----------------------- |
| Note をクリック            | 単一選択                |
| Ctrl / Shift + Note        | 選択を追加・解除        |
| 空白をドラッグ             | Marquee 選択            |
| Note をドラッグ            | 時間位置と Pitch を変更 |
| Note 右端をドラッグ        | Note Length を変更      |
| 空白をダブルクリック       | Note を作成             |
| Draw でクリック / ドラッグ | Note を連続作成         |

複数 Note の移動では相対関係を保ち、時間と Pitch をまとめて変更する。Length 変更でも選択群へ同じ差分を適用できる。

#### Clipboard と Duplicate

Copy は Note 群の相対時間、Pitch、Length、Velocity、Channel を保持する。Paste では先頭 Note を Playhead へ合わせ、新しい ID を割り当てたうえで貼り付けた Note 群を選択する。

Duplicate は選択フレーズの時間幅を基準に直後へ複製する。

#### Quantize と Velocity

Quantize は現在の Grid を基準に MIDI Note Selection へ適用する。Velocity Lane は Piano Roll と同じ Note Selection を共有し、複数 Note の Velocity をまとめて調整できる。

```text
Piano Roll        ┌───────┐       ┌──────────┐
                  └───────┘       └──────────┘

Velocity              █                 █
                      █          █      █
                      █          █      █
────────────────────────────────────────────────
```

#### Piano Keyboard と Note Preview

Piano Keyboard は Pitch の目盛りと Note Preview を兼ねる。Preview が有効な間、押した Key の Note On / Off を Active MIDI Clip の所属 Track へ送る。

この Preview は「編集中の Note がどの音になるか」を確認する機能であり、Performance Panel の演奏入力とは役割が異なる。Performance Panel は Focused Instrument Track を演奏し、MIDI Editor Preview は Active MIDI Clip の所属 Track を確認対象とする。

Clip 切替や Editor 終了時には Held Note を解放し、Preview の発音状態を終了させる。

#### Ruler / Grid / Zoom

Ruler は Arrangement 上の小節位置を表示する。Timeline の 9 小節目に置かれた Clip なら、Editor でも 9.1、9.2、9.3… と表示する。

Snap Grid は Piano Roll の細分線へ反映し、Zoom に応じて Bar、Beat、Subdivision の階層を視認できる密度へ変化する。時間方向と Pitch 方向は独立して拡大縮小できる。

### 5.3 Devices

Devices は Track Context の Instrument と Effect Chain を扱う。

```text
Track: Synth Lead

[ Instrument ] → [ EQ ] → [ Compressor ] → [ Reverb ]
```

処理順を左から右へ表示し、Track の音がどの順序で生成・加工されるかを視覚的に対応させる。

| 操作           | 内容                                      |
| -------------- | ----------------------------------------- |
| Add Instrument | Instrument Track の音源を選択             |
| Add Effect     | 指定位置へ Effect を挿入                  |
| Reorder        | Device の処理順を変更                     |
| Bypass         | Device を一時的に処理経路から外す         |
| Replace        | 別の Plugin へ差し替える                  |
| Remove         | Chain から削除                            |
| Edit           | Plugin Editor を開く                      |
| Recover        | Missing Plugin の再走査・差し替え・無効化 |

Instrument Track では Instrument が信号列の先頭となり、その後へ Effect Chain が続く。Audio Track では Audio Input から Effect Chain へつながる。

#### Add Browser

Device Chain の追加位置にある `+` から Add Browser を開く。

```text
[Instrument] → [+] → [Compressor] → [+] → [Reverb]
                 │
                 ▼
          Add Effect Browser
```

Instrument slot から開いた場合は Instrument、Effect Chain から開いた場合は Effect を候補として提示する。選択後は同じ Track Context の Devices へ戻る。

#### Performance Panel との連携

Devices と Performance Panel は同時に利用できる。

```text
Devices
[Instrument] → [EQ] → [Reverb]
      ▲
      │ parameter editing
      │
Performance Panel
[ Keyboard / Drum Pads ]
      │
      └─ play and evaluate
```

Instrument / Effect を調整した結果をすぐ演奏で確認し、同じ画面のまま調整へ戻れることを基本の音作り導線とする。

---

## 6. Performance Panel

Performance Panel の配置、Closed / Compact / Expanded の表示段階、Keyboard / Drum Pads の Mode Selector は本節で定義する。Arrange では、演奏先となる Focused Instrument Track と、録音・編集との関係を定義する。

### 6.1 Focused Instrument Track

Performance Panel、Computer Keyboard、演奏用 MIDI 入力は Focused Instrument Track へ送る。

Track Header の Focus / Performance 操作から演奏先を変更できる。MIDI Clip や別 Track を編集している間も Focus は演奏文脈として保持されるため、Arrangement の編集と Instrument の演奏を並行できる。

```text
Arrange Selection ───────────────→ Inspector / Timeline editing

Active MIDI Clip ────────────────→ MIDI Editor

Focused Instrument Track ────────→ Performance Panel / Computer MIDI
```

別の Instrument Track を Focus すると、Performance Panel の Track 名、Instrument、入力状態も同じ文脈へ更新する。

### 6.2 Detail Area との連携

Performance Panel と Detail Area は同時に利用できる。特に Devices との組み合わせを、Instrument の音作りにおける基本導線とする。

```text
Devices
[Instrument] → [EQ] → [Reverb]
      ▲
      │ parameter editing
      │
Performance Panel
[ Keyboard / Drum Pads ]
      │
      └─ play and evaluate
```

MIDI Editor と併用する場合は、MIDI Editor が Active MIDI Clip の演奏内容、Performance Panel が Focused Instrument Track へのライブ入力を担当する。両者の対象は Header と Focus 表示から判別できる。

### 6.3 録音との関係

Performance Panel から送られた MIDI は Focused Instrument Track で演奏される。録音時は Track の Record Arm と MIDI routing に従って Session へ記録する。

録音開始前には Focused Track、Arm、Input source を確認できる状態を作る。Count-in、Metronome、Record の開始操作は Global Control Bar の Transport が担当し、録音中の進行は Timeline 上へ表示する。

---

## 7. 再生・録音とフィードバック

### 7.1 Global Transport との関係

Arrange の再生・録音は Global Control Bar に含まれる Transport を使う。

```text
Global Control Bar
Position / Go Start / Stop / Play / Record
Loop / Metronome / Count-in / Tempo / Signature
                │
                ▼
         Arrangement Transport
                │
      ┌─────────┼─────────┐
      ▼         ▼         ▼
   Timeline  MIDI Editor  Performance
```

Timeline、Detail Area、Performance Panel のどこへ Keyboard Focus があっても同じ Playhead と Recording state を参照する。

Browser Asset Preview、Take Preview、MIDI Editor Note Preview、Plugin 内部の試聴は、それぞれ対象単位の Preview として扱う。Transport Play と Preview の状態は画面上で判別できる。

### 7.2 即時表示と確定

Clip や Note の Drag、Velocity、Trim、Automation Point など連続操作は Pointer の動きへ追従して画面上の Preview を更新する。操作確定時に Canonical edit を実行し、Core から返る Session と一致させる。

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

利用者は操作結果を即座に確認でき、制作状態の正本は Core 側へ一本化される。

### 7.3 状態と復旧

Hover、Selected、Focused、Active Tool、Pending、Recording、Warning などの視覚表現は全体仕様と共通にする。Arrange では特に、Clip / Track Selection、Focused Instrument Track、Active MIDI Clip、Recording、Preview の違いを判別しやすくする。

Missing source、Missing Plugin、Audio device fault、runtime out-of-sync など制作継続へ影響する問題は、作用範囲に応じて表示先を決める。

| 問題                   | 主な表示先                     |
| ---------------------- | ------------------------------ |
| Audio runtime / device | Global Control Bar + 全体通知  |
| Missing Plugin         | Devices / Track status         |
| Missing Audio source   | Clip / Inspector               |
| Runtime sync           | Timeline status + retry action |
| 一時的な編集結果       | Toast                          |

復旧操作は問題が発生した対象の近くから辿れるようにする。

---

## 8. 操作文脈とショートカット

Keyboard Shortcut は現在の編集文脈へ作用する。

| 文脈        | 主な対象 | `Ctrl+A`     | `Delete`  | `Ctrl+D`             |
| ----------- | -------- | ------------ | --------- | -------------------- |
| Timeline    | Clip     | 全 Clip 選択 | Clip 削除 | Clip 複製            |
| MIDI Editor | Note     | 全 Note 選択 | Note 削除 | Note 複製            |
| Text Input  | 文字列   | 文字列選択   | 文字削除  | OS / Text の既定動作 |

Timeline の主要操作は次の通りである。

| キー     | Timeline                           |
| -------- | ---------------------------------- |
| `Ctrl+A` | 全 Clip 選択                       |
| `Ctrl+C` | 選択 Clip を Copy                  |
| `Ctrl+V` | Playhead 位置へ Paste              |
| `Ctrl+D` | 選択 Clip を直後へ Duplicate       |
| `Ctrl+E` | Playhead 位置で Split              |
| `Delete` | 選択 Clip / Marker / Range を削除  |
| `M`      | Playhead 位置へ Marker を追加      |
| `Z`      | Time Selection へ Zoom             |
| `F`      | Arrangement 全体が見える範囲へ Fit |
| `Esc`    | 現在の一時選択や一時 UI を閉じる   |

MIDI Editor の主要操作は次の通りである。

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

Transport、Workspace、Command、Emergency Mute などアプリ全体へ作用する Shortcut は Global command として働く。

Computer Keyboard を演奏入力へ使う場合は Performance input mode を明示し、Text Input へ Focus がある間は文字入力を優先する。

---

## 9. 基本制作シナリオ

### 9.1 MIDI の打ち込み

```text
Add Instrument Track
        ↓
Choose Instrument
        ↓
Double Click empty lane
        ↓
MIDI Clip created
        ↓
Detail Area / MIDI Editor
        ↓
Draw / Double Click notes
        ↓
Move / Resize / Velocity / Quantize
        ↓
Duplicate phrase
        ↓
Play from Global Transport
        ↓
Edit while listening
```

Timeline から MIDI Editor へ自然に深く入り、Global Transport で Arrangement を再生しながら Note 編集を続ける。Track の音色調整は Devices へ移り、Clip 編集と信号経路編集の意味を分ける。

### 9.2 Audio 素材からの構成

```text
Open Browser
      ↓
Search / Preview audio
      ↓
Drag to Audio Track
      ↓
Move / Trim / Fade
      ↓
Adjust Clip properties in Inspector
      ↓
Duplicate / Split / Arrange
      ↓
Play and review
```

素材探索、Timeline への投入、直接編集、属性調整が Side Panel と Main Canvas の間で連続する。

### 9.3 Instrument と Effect の音作り

```text
Select / Focus Instrument Track
        ↓
Open Devices
        ↓
Open Performance Panel
        ↓
Play
        ↓
Edit Instrument / Effect
        ↓
Play again
        ↓
Reorder / Bypass / Compare
        ↓
Return to Timeline
```

Devices と Performance Panel を同時に使うことで、音色変更と演奏確認を画面切替に依存せず往復できる。

### 9.4 演奏から録音

```text
Focus Instrument Track
      ↓
Open Performance Panel
      ↓
Arm Track
      ↓
Set Metronome / Count-in
      ↓
Record from Global Transport
      ↓
Play Keyboard / Drum Pads / MIDI controller
      ↓
Recording appears on Timeline
      ↓
Stop
      ↓
Review Take
```

録音開始・停止は Global Transport、入力は Performance Panel / MIDI controller、進行表示は Timeline、候補比較は Take Inspector が担当する。

### 9.5 Take の比較と採用

```text
Finish recording
      ↓
Open Take Inspector
      ↓
Preview Raw / Processed
      ↓
Compare takes
      ↓
Use
      ↓
Canonical Clip updated

or

Place copy
      ↓
Alternative Clip placed on Timeline
```

Take の比較操作は録音素材の文脈へ集約し、Timeline は採用後の Arrangement 編集へ集中する。
