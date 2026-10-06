# Riffra Arrange 画面仕様

## 1. 位置付け

本書は、Riffra の Arrange ワークスペースに固有の画面構造と操作仕様の正本である。Browser、Properties、Timeline、Lower Area、Play Surface の Arrange 上の配置と共通挙動を定義する。共通の画面骨格は [共通画面構造](application-layout.md) を参照する。

Arrange は Riffra の主制作画面であり、演奏、監視、録音、音色調整、Audio / MIDI Clip の配置、MIDI 編集、再生確認を一つの Arrangement 上でつなぐ。Timeline を中心に曲を組み立て、Browser と Properties が素材探索・属性調整を支え、Lower Area が Clip や Track の内部編集とミックス確認、Play Surface が演奏入力を担当する。

制作データは `../data-model.md`、アプリケーション内部の責務分担は `../architecture.md`、通信契約は `../ipc.md` を参照する。

## 目次

- [1. 位置付け](#1-位置付け)
- [2. Arrange の作業構造](#2-arrange-の作業構造)
- [3. Timeline](#3-timeline)
- [4. Left Column](#4-left-column)
- [5. Lower Area](#5-lower-area)
- [6. Play Surface](#6-play-surface)
- [7. 再生・録音とフィードバック](#7-再生録音とフィードバック)
- [8. 操作文脈とショートカット](#8-操作文脈とショートカット)
- [9. 基本制作シナリオ](#9-基本制作シナリオ)

---

## 2. Arrange の作業構造

### 2.1 画面構成

Arrange の Main Canvas は Timeline である。Browser と Properties は Left Column に常時表示し、MIDI Editor、Mixer、Devices は Timeline の下側に Lower Area として開く。Play Surface は演奏入力が必要な場面で独立して展開する。

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ GLOBAL CONTROL BAR                                                           │
│ Project / History                 Transport                 Audio / Safety   │
├────────────────────┬─────────────────────────────────────────────────────────┤
│                    │ ARRANGE · MAIN CANVAS                                  │
│ BROWSER            │ ┌─────────────────────────────────────────────────────┐ │
│                    │ │ Arrange Toolbar                                     │ │
│                    │ ├─────────────────────────────────────────────────────┤ │
│                    │ │ Ruler                                               │ │
│                    │ ├─────────────────────────────────────────────────────┤ │
│                    │ │ Timeline · Tracks / Clips / Automation              │ │
│                    │ ├─────────────────────────────────────────────────────┤ │
│                    │ │ LOWER AREA                                          │ │
│ PROPERTIES         │ │ MIDI Editor / Mixer / Devices                       │ │
├────────────────────┴─────────────────────────────────────────────────────────┤
│ PLAY SURFACE · optional                                                      │
│ Focused Instrument Track / Keyboard / Drum Pads / Octave / Velocity         │
└──────────────────────────────────────────────────────────────────────────────┘
```

制作の中心は Timeline に置く。Browser は探索、Properties は属性調整、Lower Area は内部編集とミックス確認、Play Surface は演奏という異なる役割を持つため、同時利用の意味も明確になる。たとえば Properties で Track 属性を確認し、Lower Area の Mixer で音を調整しながら Play Surface で弾く、Timeline を再生しながら MIDI Editor で Note を直す、といった制作フローを画面切替だけに頼らず進められる。Devices では選択 Track の音源と Effect Chain を編集する。

### 2.2 選択・編集対象・演奏先

Arrange では、似て見える状態を役割ごとに分けて扱う。

```text
                         Arrange Selection
                         Track / Clip(s)
                               │
                 ┌─────────────┴─────────────┐
                 ▼                           ▼
            Properties                  Devices
                                    (selected Track)

MIDI Clip を編集
        │
        ▼
Active MIDI Clip ───────────────→ MIDI Editor
        │
        └────────────────────────→ MIDI Note Selection

Focused Instrument Track ───────→ Play Surface / Computer MIDI

Active MIDI Clip の Track ──────→ MIDI Editor Note Preview
```

#### Arrange Selection

Timeline 上で選択している Track または Clip 群を表す。Properties の内容は Arrange Selection に追従する。Browser の表示状態は素材探索の文脈として保持される。

#### Devices の編集対象

Devices は Arrange Selection で明示的に選択した Track を扱う。Track Header の Device 操作、Properties の Open Devices、Mixer の FX から開く。Track を切り替えると Device 選択を解除し、選択 Device を削除した場合も詳細を閉じる。選択 Track を削除した場合は Devices を閉じる。Effect の順序変更では Device の選択を維持する。

#### Active MIDI Clip

MIDI Editor が編集している MIDI Clip を表す。MIDI Clip のダブルクリックや明示的な Edit 操作で Active MIDI Clip を設定し、Lower Area に MIDI Editor を開く。

MIDI Editor が表示中で、単一の別 MIDI Clip を通常選択した場合は編集対象もその Clip へ追従する。複数選択では Active MIDI Clip を維持し、どの Clip の Note を編集しているかは MIDI Editor の編集対象として保持する。

#### MIDI Note Selection

Active MIDI Clip 内で選択している Note 群である。MIDI Editor の Pointer、Marquee、Keyboard Shortcut はこの Selection を対象とする。

#### Focused Instrument Track

Play Surface、Computer Keyboard、演奏用 MIDI 入力の送り先である。Arrange Selection と Active MIDI Clip から独立して保持し、曲を編集しながら同じ Instrument を演奏できる。

Focused Instrument Track への入力は、その Track の Arrangement MIDI と同じ Instrument Runtime、
Effect Chain、Volume / Pan / Automation を通る。ライブ演奏専用の音源やエフェクト経路を持たないため、
Play Surface で確認した音は再生・録音時の Track の処理経路と一致する。ライブ入力が処理できない
場合は、Track の状態と Audio Status の診断値で確認できる。

Record Arm は Track の録音状態として Focus と分けて扱う。録音時は Arm 状態と Focus / MIDI routing の関係を画面上で確認できるようにする。

---

## 3. Timeline

### 3.1 Arrange Toolbar

Arrange Toolbar は Timeline 全体へ作用する頻出操作をまとめる。

```text
┌─────────────────────────────────────────────────────────────────────┐
│ [Select|Split]  Snap [1/16 ▾]  [Automation] [Mixer]                 │
│                                       Bars/Time   Zoom [−][＋] 100% │
└─────────────────────────────────────────────────────────────────────┘
```

左側は編集操作、右側は表示操作としてまとまりを持たせる。

| 要素          | 挙動                                            |
| ------------- | ----------------------------------------------- |
| Select        | Clip の選択、移動、Trim、Marquee など通常編集   |
| Split         | 指定位置で Clip を分割                          |
| Snap          | Timeline の時間編集に使う Grid                  |
| Automation    | 選択 Track の Automation Lane を開閉            |
| Mixer         | Timeline 下側の Mixer Lower Area を開閉         |
| Play Surface  | Focused Instrument Track の Play Surface を開閉 |
| Bars / Time   | Ruler の表示形式を切替                          |
| Timeline Zoom | 時間方向の拡大・縮小                            |

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

再生中はビューが Playhead へ連続的に追従し、Playhead を表示範囲の一定位置に保つ。手動 Scroll は追従に優先し、Playhead が表示範囲内にある間はビューを動かさない。Playhead が表示範囲外に出ると追従を再開する。Timeline と MIDI Editor で同じ挙動を共有する。

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

| 項目              | 仕様                                  |
| ----------------- | ------------------------------------- |
| Track Name        | 選択と名前変更の入口                  |
| Track Kind        | Audio / Instrument の識別             |
| Mute / Solo / Arm | Header から直接変更                   |
| Monitoring        | Audio Track の現在状態を表示・変更    |
| Mix               | 表示密度に応じて Volume / Pan を操作  |
| Reorder           | Track の並べ替え                      |
| Height            | Track ごとの表示密度変更              |
| Focus             | Instrument Track を演奏先として Focus |
| Properties        | Track の属性と状態を表示・編集する    |

Input、Monitoring、名称など Track 自体の詳細属性は Properties が扱う。Instrument / Effect Chain は Devices で編集し、Properties は Track 属性へ集中する。Volume / Pan は制作中の確認頻度が高いため Track Header に簡易操作を置き、Properties では数値確認と精密調整を行える。

Track Menu は Track 単位の操作をまとめる。Audio Track と Instrument Track で同じ構造を持ち、Open Devices で選択 Track の音源と Effect Chain を開く。Plugin Editor は Devices 内の選択 Device から開く。Track の複製・削除もこの Menu から行う。

| 項目               | 仕様                                            |
| ------------------ | ----------------------------------------------- |
| Open Devices       | Track を選択して Devices を開く                 |
| Duplicate / Delete | Track の複製と削除。Delete は Clip 数を確認する |

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

左右端は Trim、Fade Handle は Fade In / Fade Out を担当する。Start、Length、Gain、Pan、Fade、Loop など数値確認を伴う属性は Properties からも調整できる。

将来 Audio Editor を導入する場合は、Clip の内部波形や高度な音声処理を Lower Area で扱い、Timeline 上の構成編集との責務を分ける。

### 3.6 MIDI Clip

MIDI Clip は内部 Note の配置を簡易表示する。Clip のダブルクリックで Active MIDI Clip を設定し、Lower Area に MIDI Editor を開く。

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
       Lower Area / MIDI Editor
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

録音対象 Track、現在の Pass、録音範囲を一つの視線で確認できる構成とする。録音完了後に生成された Clip / Take は Arrange Selection と Properties から扱える。

---

## 4. Left Column

Arrange の Left Column は Browser と Properties を上下に常時表示する。本章では素材探索、選択対象の編集、縦分割のリサイズを含め、Arrange 固有の内容を定義する。

### 4.1 Browser

Browser は Audio / MIDI Asset、Recording、Inbox、Instrument、Effect などを探し、Timeline の Track へ投入する。

```text
Browser
├─ Audio / MIDI Assets ─────→ Timeline
├─ Recordings / Inbox ──────→ Timeline / Take workflow
├─ Instruments ─────────────→ Track の Instrument
└─ Effects ─────────────────→ Track の Effect Chain
```

Browser の項目は種類によらず同じ操作で扱う。選択すると下端に表示し、Space で試聴、ダブルクリックまたは Enter で投入、Drag & Drop で投入先を指定する。投入先は次のとおり決まる。

| 項目                          | Track へ投入                        | Track 以外へ投入               |
| ----------------------------- | ----------------------------------- | ------------------------------ |
| Instrument / 音源プラグイン   | Instrument Track の音源を置き換える | 新しい Instrument Track を作る |
| Effect プラグイン             | Track の Effect Chain の末尾に追加  | 投入しない                     |
| Recording / Audio・MIDI Asset | 同種の Track へ Clip として配置     | 同種の Track へ自動で配置      |

ダブルクリックと Enter では選択中の Track を投入先の候補とし、種類が合わない場合は Track 以外へ投入したときと同じ扱いにする。Drag & Drop では落とした Track を投入先とし、種類が合わない場合は投入しない。

Devices 内の Picker は現在の Track と Plugin role に合わせて候補を絞る。

### 4.2 Properties

Properties は Arrange Selection に応じて内容を更新する。Browser はアンマウントせず、検索語と表示中の素材を維持する。

```text
Arrange Selection
      │
      ├─ Track ─────────────→ Track Properties
      ├─ Audio Clip ────────→ Audio Clip Properties
      ├─ MIDI Clip ─────────→ MIDI Clip Properties
      ├─ Multiple Clips ────→ Multi Clip Properties
      └─ Recording Take ────→ Take Properties
```

#### Track Properties

Track 自体の属性を扱う。

| 領域       | 内容                                                                  |
| ---------- | --------------------------------------------------------------------- |
| Identity   | Track 名、種別                                                        |
| Input      | Audio / MIDI Input routing                                            |
| Monitoring | Input Monitoring                                                      |
| Mix        | Volume / Pan の数値確認と精密調整                                     |
| Status     | Input source、recording、missing dependency など Track に関係する状態 |
| Devices    | Instrument 名、Effect 件数、Open Devices                              |

Track Properties は Track 属性と Device の概要を表示する。音源の変更・削除、Effect Chain の編集、VST3 の Parameter・Preset・Plugin Editor、Missing の復旧は Devices で行う。

#### Audio Clip Properties

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

#### MIDI Clip Properties

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

#### Multi Clip Properties

複数 Clip の共通属性をまとめて調整する。Audio / MIDI が混在する場合は Start、Mute など意味を共有できる項目を中心に表示する。変更結果が選択対象全体へどのように反映されるかを確認できる表示を使う。

#### Take Properties

Take Properties は同じ録音意図を持つ候補を比較し、採用と配置を行う。

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

## 5. Lower Area

Lower Area は Timeline で扱う対象へ一段深く入り、演奏内容や信号経路を編集・確認する。MIDI Editor、Mixer、Devices のいずれか一つを表示する。

Lower Area は、Timeline Toolbar または対象を開く操作から明示された編集面を表示する。MIDI Editor、Mixer、Devices は同じ領域を共有し、表示面を切り替えても Canonical state、Arrange Selection、Active MIDI Clip はそれぞれの責務を保つ。外側に対象名を繰り返す文言ヘッダーは置かず、各編集面自身の Toolbar と編集対象を保ったまま作業を続けられる。

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

Lower Area は、Resize、Collapse / Restore、Expand / Restore、Close を提供する。対象の切替は MIDI Clip の編集操作、Track の Devices 操作、Arrange Toolbar の Mixer 操作から行い、Lower Area を閉じても Arrange Selection と Active MIDI Clip は維持する。Play Surface は Lower Area と独立して開閉できる。MIDI Editor と Devices は同じ高さを共有し、Mixer の高さは別に保持する。Mixer を閉じると、その直前に開いていた MIDI Editor または Devices へ戻る。直前が閉じた状態なら Lower Area を閉じる。

### 5.2 Mixer

Mixer は Track ごとの音量・Pan・Meter・M/S/R と、固定された Master 出力を横並びで確認する。Track 列は横スクロールし、Master 列は右側に固定する。Track を選択しても Focused Instrument Track は変更しない。FX は Effect 件数にかかわらず、その Track を選択して Devices を開く。Missing Effect は警告を表示する。

Gain と Pan のドラッグ中は Native Runtime の一時プレビューへ値を集約して送り、操作の確定時に一度だけ `updateTrack` を実行する。プレビュー値は Canonical state、Undo/Redo、保存、Runtime 再起動の復元対象にしない。M/S/R は Track の Canonical state を更新する。

各 Track の Meter は Effect Chain、Fader、Pan、Automation、Mute を通過した Track 出力を左右別に表示する。Master Meter は Safety limiter と最終ハードクリップ後の左右出力を表示し、Limiter gain reduction、Hard clip、Feedback protection の状態を診断欄に示す。Meter が未接続の間は値を補間せず、利用不可として表示する。

### 5.3 MIDI Editor

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

Piano Roll、Musical Ruler、Velocity Lane は一つの時間軸を共有する。横 Scroll / Zoom も同じ基準で動き、Active MIDI Clip が Arrangement のどこに位置しているかを Ruler 上で把握できる。再生中の Playhead 追従は Timeline と同じ挙動に従う。

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

Note の作成・移動・長さ変更・Pitch 変更が確定したら、その Note を所属 Track の Instrument で短く試聴する。Preview が有効な場合に限る。

同時刻に鳴る他 Clip の Note は、Active MIDI Clip の背後に参照表示する。参照表示は選択・編集の対象にならず、表示の有無を切り替えられる。

#### Clipboard と Duplicate

Copy は Note 群の相対時間、Pitch、Length、Velocity、Channel を保持する。Paste では先頭 Note を Playhead の位置へ Snap し、新しい ID を割り当てたうえで貼り付けた Note 群を選択する。

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

この Preview は「編集中の Note がどの音になるか」を確認する機能であり、Play Surface の演奏入力とは役割が異なる。Play Surface は Focused Instrument Track を演奏し、MIDI Editor Preview は Active MIDI Clip の所属 Track を確認対象とする。

Clip 切替や Editor 終了時には Held Note を解放し、Preview の発音状態を終了させる。

#### Ruler / Grid / Zoom

Ruler の小節線と拍線は Arrangement 上の境界に揃える。小節頭には小節番号を表示し、小節途中から始まる Clip の先頭には拍も添える。たとえば 9.3 から始まる Clip では、先頭を「9.3」、次の小節頭を「10」と表示する。

Piano Roll と Velocity Lane の小節線・拍線は Ruler と同じ Arrangement 上の境界に揃える。Snap Grid の細分線も同じ時間軸を使い、Note の追加・移動・右端の Resize・Quantize はその境界へ吸着する。Clip より前へ吸着する場合は Clip 先頭に留める。Note 群を移動するときは操作対象の Note を基準に吸着し、Note 間の相対時間を保つ。

Zoom に応じて Bar、Beat、Subdivision の階層を視認できる密度へ変化する。時間方向と Pitch 方向は独立して拡大縮小できる。

### 5.4 Devices

Devices は選択 Track の信号経路を左から右へ表示する。Instrument Track は音源、Audio Track は Audio Input が先頭になり、その後に Effect Chain が続く。

```text
[Instrument / Audio Input] → [Effect A] → [Effect B] → [+ Effect]
```

音源の Choose / Change では Built-in と検証済み VST3 Instrument を選ぶ。Built-in は Change、Clear、Bypass を提供する。Effect は末尾の追加ボタンから検証済み VST3 Effect を追加し、左右の移動操作で処理順を変更する。各 Device には Bypass 状態を明示し、Missing または Disabled Placeholder は Re-scan、Replace、Disable で復旧する。

VST3 Device を選択すると、同じ汎用 Editor で Parameter と公開された Preset を扱う。Parameter の名前、表示値、単位、離散選択肢は Plugin 自身が返した内容を表示する。列挙可能な離散値は選択欄、それ以外はスライダーで操作し、Default は Plugin の既定値へ戻す。スライダーのドラッグ中は画面内の下書きだけを更新し、Pointer Up、Keyboard 操作の終了、Blur で最終値を一度保存する。離散値と Preset は選択時に保存し、Parameter 情報を再取得する。

Plugin Editor と Preset は Device が公開している場合に表示する。Plugin Editor 側の変更は正準状態へ反映され、選択中 Device の詳細も更新される。Missing と Disabled Placeholder では Parameter を問い合わせず、読み込み失敗時は Device を残して詳細欄にエラーを表示する。

Devices と Play Surface は同時に利用できる。Devices で音色を調整し、Focused Instrument Track を演奏して結果を確認する。Browser は探索と Project 外の Plugin Audition を担当し、Devices 内の Picker は現在の Track への追加・変更を担当する。

---

## 6. Play Surface

Play Surface の配置、Closed / Compact / Expanded の表示段階、Keyboard / Drum Pads の Mode Selector は本節で定義する。Arrange では、演奏先となる Focused Instrument Track と、録音・編集との関係を定義する。

### 6.1 Focused Instrument Track

Play Surface、Computer Keyboard、演奏用 MIDI 入力は Focused Instrument Track へ送る。これは入力先を選ぶ状態であり、別のランタイムグラフを生成する操作ではない。

Arrange Toolbar の Play Surface で Play Surface を開閉し、Focused Instrument Track へ入力する。MIDI Clip や別 Track を編集している間も Focus は演奏文脈として保持されるため、Arrangement の編集と Instrument の演奏を並行できる。

```text
Arrange Selection ───────────────→ Properties / Timeline editing

Active MIDI Clip ────────────────→ MIDI Editor

Focused Instrument Track ────────→ Play Surface / Computer MIDI
```

別の Instrument Track を Focus すると、Play Surface の Track 名、Instrument、入力状態も同じ文脈へ更新する。

### 6.2 Lower Area との連携

Play Surface と Lower Area は同時に利用できる。Mixer では Instrument の出力を確認し、Devices では Instrument と Effect を調整しながら演奏する。

```text
Devices
[Instrument] → [EQ] → [Reverb]
      ▲
      │ parameter editing
      │
Play Surface
[ Keyboard / Drum Pads ]
      │
      └─ play and evaluate
```

MIDI Editor と併用する場合は、MIDI Editor が Active MIDI Clip の演奏内容、Play Surface が Focused Instrument Track へのライブ入力を担当する。両者の対象は Header と Focus 表示から判別でき、発音経路は同じ Instrument Runtime と Effect Chain へ統合される。

### 6.3 録音との関係

Play Surface から送られた MIDI は Focused Instrument Track で演奏される。録音時は Track の Record Arm と MIDI routing に従って Session へ記録する。

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
   Timeline  MIDI Editor  Play Surface
```

Timeline、Lower Area、Play Surface のどこへ Keyboard Focus があっても同じ Playhead と Recording state を参照する。

Play は投影済みのグラフがあれば直ちに再生し、グラフ準備中なら Transport を `Starting` と表示して待機する。準備中の Play は UI をブロックせず、Stop は保留中の Play より優先される。Stop 後に準備が終わっても自動再生せず、投影失敗時は Play 意図を解除して失敗を通知する。

Track Arm は録音対象の Track を選択する操作であり、Focus とは分離する。Global Record は Arm された Track の Timeline Recording を開始する操作で、Count-in の設定に従い Recording 開始とともに Arrangement Transport も進行する。Arm された Track が存在しない場合は Recording と Transport を開始せず、録音対象を Arm するよう利用者へ通知する。

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
| Missing Plugin         | Track status と Devices        |
| Missing Audio source   | Clip / Properties              |
| Runtime sync           | Timeline status + retry action |
| 一時的な編集結果       | Toast                          |

Audio device、Runtime projection、Transport は別の状態として表示する。デバイスが利用可能でも
投影が準備中なら Transport は `Starting` になり、投影が失敗した場合は Audio device の復旧と
Runtime projection の再試行を同じ操作として扱わない。Global Control Bar では Audio Status の
ミュート理由と診断値を確認でき、Track では Live MIDI の入力先とドロップなど処理状態を確認できる。

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

Computer Keyboard を演奏入力へ使う場合は Play Surface の入力モードを明示し、Text Input へ Focus がある間は文字入力を優先する。

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
Lower Area / MIDI Editor
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

Timeline から MIDI Editor へ自然に深く入り、Global Transport で Arrangement を再生しながら Note 編集を続ける。Track の音色調整は Devices、音量と Pan の調整は Mixer で行う。Clip 編集と Track 属性の意味は分ける。

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
Adjust Clip properties in Properties
      ↓
Duplicate / Split / Arrange
      ↓
Play and review
```

素材探索、Timeline への投入、直接編集、属性調整が Browser、Properties、Main Canvas の間で連続する。

### 9.3 Instrument と Effect の音作り

Instrument と Effect の音作りは、Devices と Play Surface を組み合わせて行う。

```text
Select / Focus Instrument Track
        ↓
Open Devices
        ↓
Open Play Surface
        ↓
Play
        ↓
Edit Instrument / Effect in Devices
        ↓
Play again
        ↓
Reorder / Bypass / Compare
        ↓
Return to Timeline
```

Devices と Play Surface を同時に使うことで、音色変更と演奏確認を画面切替に依存せず往復できる。

### 9.4 演奏から録音

```text
Focus Instrument Track
      ↓
Open Play Surface
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

録音開始・停止は Global Transport、入力は Play Surface / MIDI controller、進行表示は Timeline、候補比較は Take Properties が担当する。

### 9.5 Take の比較と採用

```text
Finish recording
      ↓
Open Take Properties
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
