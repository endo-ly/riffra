# Riffra Arrange画面

Arrangeは、演奏、録音、音色調整、Audio/MIDIクリップの配置、MIDI編集を一つの時間軸でつなぐRiffraの主制作画面です。素材を探すBrowser、属性を調整するProperties、曲を組み立てるTimeline、詳細を編集するDetail Area、演奏するPlay Surfaceが同じセッションを参照します。

共通の画面骨格は [共通画面構造](application-layout.md)、データの意味は [データモデル](../data-model.md)、アプリケーションの責務は [アーキテクチャ](../architecture.md) を参照してください。

## 目次

- [画面の骨格](#1-画面の骨格)
- [操作対象の分け方](#2-操作対象の分け方)
- [Timeline](#3-timeline)
- [BrowserとProperties](#4-browserとproperties)
- [Detail Area](#5-detail-area)
- [Play SurfaceとTransport](#6-play-surfaceとtransport)
- [状態と復旧](#7-状態と復旧)
- [ショートカット](#8-ショートカット)
- [基本的な制作の流れ](#9-基本的な制作の流れ)

## 1. 画面の骨格

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ GLOBAL CONTROL BAR: Project / History / Transport / Audio / Safety           │
├────────────────────┬─────────────────────────────────────────────────────────┤
│ BROWSER            │ ARRANGE · MAIN CANVAS                                  │
│ 素材・録音・Plugin  │ ┌─────────────────────────────────────────────────────┐ │
│                    │ │ Arrange Toolbar / Ruler / Tracks / Clips            │ │
├────────────────────┤ ├─────────────────────────────────────────────────────┤ │
│ PROPERTIES         │ │ DETAIL AREA: MIDI Editor / Devices                  │ │
│ Track / Clip / Take│ └─────────────────────────────────────────────────────┘ │
├────────────────────┴─────────────────────────────────────────────────────────┤
│ PLAY SURFACE: Focused Instrument Track / Keyboard / Drum Pads               │
└──────────────────────────────────────────────────────────────────────────────┘
```

| 領域               | 役割                                                  |
| ------------------ | ----------------------------------------------------- |
| Global Control Bar | セッション全体の状態、Transport、音声の安全操作を扱う |
| Browser            | Asset、録音、Instrument、Effectを探して投入する       |
| Properties         | 選択中のTrack、Clip、Takeの属性を編集する             |
| Main Canvas        | Timeline上でTrack、Clip、Automationを構成する         |
| Detail Area        | Active MIDI ClipのノートやTrackのDevicesを編集する    |
| Play Surface       | Focused Instrument Trackを演奏する                    |

Timelineを画面の中心に置き、他の領域は異なる役割を保ちます。PropertiesでTrackの入力を確認し、Detail AreaでInstrumentを調整し、Play Surfaceで弾きながらTimelineの結果を聞く、という流れを画面切替なしで進められます。

## 2. 操作対象の分け方

Arrangeでは、選択している対象、編集している対象、演奏している対象を別々に管理します。

| 状態                     | 決めること                               | 主な表示先                 |
| ------------------------ | ---------------------------------------- | -------------------------- |
| Arrange Selection        | Timelineで選択しているTrackまたはClip群  | Properties、Timeline       |
| Track Context            | PropertiesとDevicesが編集するTrack       | Properties、Detail Area    |
| Active MIDI Clip         | MIDI Editorが編集するClip                | Detail Area                |
| MIDI Note Selection      | Active MIDI Clip内で選択しているノート群 | MIDI Editor                |
| Focused Instrument Track | Play Surfaceと演奏用MIDI入力の送り先     | Track Header、Play Surface |
| Record Arm               | 録音するTrack                            | Track Header、Transport    |

単一Trackに属するClipを選択した場合は、そのTrackをTrack Contextにします。複数Trackにまたがる選択では、最後に明示したTrack Contextを保ち、どのDevicesを編集しているかを曖昧にしません。

MIDI Clipを開くとActive MIDI Clipが決まります。MIDI Editorを表示したまま単一の別MIDI Clipを選択した場合は編集対象も追従し、複数選択ではActive MIDI Clipを保ちます。MIDI Clipの選択やNoteの編集はFocused Instrument Trackを変更しません。

Record Armは録音対象を決める状態です。Focusと分離することで、あるInstrument Trackを弾きながら別のTrackを編集したり、録音対象を明示したりできます。

## 3. Timeline

### Arrange Toolbar

Arrange ToolbarはTimeline全体へ作用する操作をまとめます。

| 操作        | 役割                                                 |
| ----------- | ---------------------------------------------------- |
| Select      | Clipの選択、移動、範囲変更など通常編集               |
| Split       | 指定位置でClipを分割                                 |
| Snap        | 移動、範囲変更、分割、Markerの位置を時間軸へ合わせる |
| Follow      | 再生中のPlayheadへ表示範囲を追従させる               |
| Automation  | 選択TrackのAutomation Laneを開閉                     |
| Bars / Time | Rulerの表示形式を切り替える                          |
| Zoom        | 時間方向の表示倍率を変える                           |

### Rulerと範囲

Rulerは位置の確認と範囲の操作を担います。

| 操作                          | 結果                                       |
| ----------------------------- | ------------------------------------------ |
| クリック                      | Playheadを移動                             |
| ドラッグ                      | Time Selectionを作成                       |
| Markerのドラッグ              | Markerを移動                               |
| LoopまたはPunchの端をドラッグ | 範囲を変更                                 |
| Context Menu                  | Markerの追加、選択範囲からLoop/Punchを設定 |

Playhead、Time Selection、Loop、Punch、Markerは同じ時間軸の上で位置関係を確認できます。

### Track Row

Track Headerは、演奏と録音中に頻繁に触る操作を持ちます。

| 項目              | 操作                                                   |
| ----------------- | ------------------------------------------------------ |
| Track Name / Kind | 名前の変更、AudioまたはInstrumentの確認                |
| Mute / Solo / Arm | Track単位の状態変更                                    |
| Monitoring        | Audio入力の監視状態の変更                              |
| Volume / Pan      | 再生中に確認しやすい簡易調整。精密値はPropertiesで扱う |
| Reorder / Height  | Trackの順序と表示密度の変更                            |
| Focus             | Instrument Trackを演奏先に設定                         |

入力、監視、Instrument、Effect ChainなどTrackの詳細はPropertiesまたはDevicesで扱います。Headerへすべての設定を詰め込まず、頻繁な操作と詳細な編集を分けます。

### Clipの共通操作

Audio ClipとMIDI Clipは、Timeline上で同じ基本操作を持ちます。

| 操作                    | 結果                         |
| ----------------------- | ---------------------------- |
| クリック                | 単一選択                     |
| Ctrl / Shift + クリック | 選択の追加または解除         |
| 空白のドラッグ          | Marqueeによる複数選択        |
| ドラッグ                | 時間位置またはTrackを変更    |
| 左右端のドラッグ        | 使用範囲を変更               |
| Duplicate               | 直後へ複製                   |
| Delete                  | 削除                         |
| Split                   | Playheadまたは指定位置で分割 |
| Mute / Loop             | Clip単位の状態を変更         |

### Audio Clip

Audio Clipは波形と使用範囲を表示します。左右端で範囲を調整し、Fade HandleでFade InとFade Outを調整します。開始位置、長さ、Gain、Pan、Fade、LoopはPropertiesでも確認・編集できます。

Clipの編集は素材そのものを書き換えません。Assetの参照範囲とTimeline上の設定を変更し、内容を変える処理は新しいAssetとして保存します。

### MIDI Clip

MIDI Clipは内部のノート配置を簡易表示します。ダブルクリックまたはEdit操作でActive MIDI Clipを設定し、Detail AreaのMIDI Editorを開きます。

Timeline上の範囲変更はClipが占める時間を扱います。ノートの開始位置、長さ、Pitch、Velocity、ChannelはMIDI Editorで編集します。

### 空のMIDI Clip

Instrument Trackの空白から、外部ファイルを用意せずに打ち込みを始められます。

1. Instrument Trackの空白をダブルクリックするか、`Insert MIDI Clip` を選ぶ。
2. Snapに従った位置へ新しいClipを作る。
3. Time Selectionがあればその範囲を初期長にし、なければ既定の長さを使う。
4. 作成したClipをActive MIDI ClipとしてMIDI Editorを開く。

### Automationと録音表示

Automationは対象Trackの直下へLaneとして展開し、Clipと同じ時間軸、Playhead、Snap、Zoomを共有します。Parameter Selectorで対象を選び、Pointを追加、移動、削除します。

録音中は、開始位置から現在位置までを対象Trackへ表示します。Track、現在のPass、録音範囲を一つの視線で確認でき、録音完了後のClipとTakeはSelectionとPropertiesから扱えます。

## 4. BrowserとProperties

### Browser

BrowserはAsset、Recording、Inbox、Instrument、Effectを探し、TimelineまたはDevicesへつなぎます。

| 対象               | 投入先                    |
| ------------------ | ------------------------- |
| Audio / MIDI Asset | Timeline                  |
| Recording / Inbox  | Timeline、Takeの確認      |
| Instrument         | Instrument TrackのDevices |
| Effect             | TrackのEffect Chain       |

Search、Preview、選択、Drag & Dropを一連の操作として扱います。追加ボタンから開くAdd Browserは、追加先のTrackと位置を引き継いで候補を絞ります。

### Properties

PropertiesはArrange Selectionに追従します。Browserを閉じたり再読み込みしたりせず、素材探索の文脈を保ちます。

| 選択対象       | 表示する内容                                     |
| -------------- | ------------------------------------------------ |
| Track          | 名前、種別、Input、Monitoring、Volume、Pan、状態 |
| Audio Clip     | 名前、位置、長さ、Gain、Pan、Fade、Mute、Loop    |
| MIDI Clip      | 名前、位置、長さ、Mute、Loop                     |
| 複数Clip       | Start、Muteなど共通する属性                      |
| Recording Take | 原音・加工音の比較、採用、別Clipとしての配置     |

InstrumentとEffect Chainの順序やパラメーターはDevicesで編集します。PropertiesはTrackやClip自体の属性に集中します。

### Takeの比較

RawとProcessedの両方を持つAudio Takeは、同じ位置から切り替えて比較します。`Use` は録音グループの正準Clipを更新し、`Place copy` は候補を別のClipとしてTimelineへ配置します。比較しても元のTakeは残ります。

## 5. Detail Area

Detail Areaは、Timelineで選択した対象の内部へ入る編集面です。MIDI EditorとDevicesを同時に置くのではなく、必要な面を開き、サイズ変更、折りたたみ、拡大、復元、閉じるを行います。閉じてもSelectionとActive MIDI Clipは維持します。

### MIDI Editor

MIDI EditorはPiano Roll、Ruler、Velocity Laneで構成し、すべて同じ時間軸を共有します。Active MIDI ClipがArrangementのどこにあるかをRulerで確認できます。

| 操作              | 結果                                        |
| ----------------- | ------------------------------------------- |
| Pointer           | Noteの選択、移動、長さ変更                  |
| Draw              | Noteの連続入力                              |
| Snap              | Noteの開始位置と初期長をGridへ合わせる      |
| Preview           | 編集中のNoteをActive MIDI ClipのTrackで試聴 |
| Quantize          | 選択したNoteの位置をGridへ合わせる          |
| Duplicate         | 選択フレーズを時間幅に沿って複製            |
| Time / Pitch Zoom | 横方向と縦方向を独立して拡大・縮小          |

空白のダブルクリックでもNoteを作成できます。開始位置と初期長はGridを基準にし、Velocityは直前の入力値を引き継ぎます。複数Noteの移動や長さ変更では、選択群の相対関係を保ちます。

CopyはNoteの相対時間、Pitch、長さ、Velocity、Channelを保持します。Pasteでは先頭NoteをPlayheadへ合わせ、新しいIDを割り当て、貼り付けたNoteを選択します。

Piano KeyboardはPitchの目盛りとNote Previewを兼ねます。MIDI EditorのPreviewはActive MIDI ClipのTrackを対象にし、Play SurfaceはFocused Instrument Trackを対象にします。Clip切替やEditor終了時には、押されたままのNoteを解放します。

### Devices

DevicesはTrack ContextのInstrumentとEffect Chainを編集します。処理順を左から右へ示し、Instrument TrackではInstrumentから、Audio TrackではAudio InputからEffect Chainへつながる構造を見せます。

| 操作                        | 結果                                           |
| --------------------------- | ---------------------------------------------- |
| Add Instrument / Add Effect | 追加位置へ候補を表示し、選択したデバイスを挿入 |
| Reorder                     | 処理順を変更                                   |
| Bypass                      | 一時的に処理経路から外す                       |
| Replace                     | 別のPluginへ差し替える                         |
| Remove                      | Chainから削除                                  |
| Edit                        | Plugin Editorを開く                            |
| Recover                     | Missing Pluginを再走査、差し替え、無効化       |

追加位置の `+` から開くAdd Browserは、Instrument slotならInstrument、Effect ChainならEffectを候補にします。選択後は同じTrackのDevicesへ戻ります。

DevicesとPlay Surfaceは同時に使えます。音源やEffectを調整し、その結果を演奏で確かめ、同じ編集面へ戻ることが基本の音作り導線です。

## 6. Play SurfaceとTransport

### 演奏先

Play Surface、コンピューターキーボード、演奏用MIDI入力はFocused Instrument Trackへ送ります。別のInstrument TrackへFocusを移すと、Play SurfaceのTrack名、Instrument、入力状態も切り替わります。

MIDI Editorと併用する場合、MIDI EditorはActive MIDI Clipの演奏内容を、Play SurfaceはFocused Instrument Trackへのライブ入力を担当します。二つの対象はTrack HeaderとFocus表示で区別します。

### Transportと録音

再生、停止、録音、位置移動、Loop、Metronome、Count-in、Tempo、SignatureはGlobal Control BarのTransportで扱います。Timeline、MIDI Editor、Play Surfaceは同じPlayheadとRecording stateを参照します。

録音を開始する前に、Focused Track、Record Arm、Input sourceを確認できます。ArmされたTrackがない場合は録音とTransportを開始せず、録音対象を準備するよう知らせます。録音中はTimelineへ進行を表示し、完了後はTake Propertiesで候補を比較します。

### Previewの区別

Browser Asset Preview、Take Preview、MIDI Editor Note Preview、Plugin内の試聴は、対象単位のPreviewです。Arrangement全体を再生するTransport Playとは別の状態として表示します。

## 7. 状態と復旧

Hover、Selected、Focused、Active Tool、Pending、Recording、Warningの見た目は共通画面構造の規則に従います。Arrangeでは、Selection、Focused Instrument Track、Active MIDI Clip、Record Arm、Previewを区別できることを優先します。

| 問題                   | 主な表示先                   | 復旧の入口                      |
| ---------------------- | ---------------------------- | ------------------------------- |
| Audio Runtime / device | Global Control Bar、全体通知 | Runtimeの回復、再試行、安全操作 |
| Missing Plugin         | Devices、Track status        | 再走査、差し替え、無効化        |
| Missing Audio source   | Clip、Properties             | Assetの再リンク                 |
| Runtimeの同期ずれ      | Timeline status              | 最新状態からの再投影            |
| 一時的な編集結果       | Toast                        | 操作の再試行またはUndo          |

問題は影響を受ける対象の近くから辿れるようにします。保存済みの制作状態と一時的なランタイム障害を同じエラーとして表示しません。

## 8. ショートカット

ショートカットは現在フォーカスしている編集文脈へ作用します。Text Inputにフォーカスがある場合は文字入力を優先します。

| 文脈        | `Ctrl+A`   | `Delete` | `Ctrl+D`     |
| ----------- | ---------- | -------- | ------------ |
| Timeline    | 全Clip選択 | Clip削除 | Clip複製     |
| MIDI Editor | 全Note選択 | Note削除 | Note複製     |
| Text Input  | 文字列選択 | 文字削除 | OSの既定動作 |

### Timeline

| キー     | 操作                             |
| -------- | -------------------------------- |
| `Ctrl+C` | 選択ClipをCopy                   |
| `Ctrl+V` | Playhead位置へPaste              |
| `Ctrl+E` | Playhead位置でSplit              |
| `M`      | Playhead位置へMarkerを追加       |
| `Z`      | Time SelectionへZoom             |
| `F`      | Arrangement全体が見える範囲へFit |
| `Esc`    | 一時選択または一時UIを閉じる     |

### MIDI Editor

| キー                           | 操作                      |
| ------------------------------ | ------------------------- |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | NoteのCopy、Cut、Paste    |
| `Ctrl+D`                       | NoteをDuplicate           |
| `←` / `→`                      | Grid単位で時間移動        |
| `↑` / `↓`                      | 半音単位でPitch移動       |
| `Shift + ↑` / `Shift + ↓`      | オクターブ単位でPitch移動 |
| `Ctrl+Z` / `Ctrl+Y`            | Undo / Redo               |
| `Esc`                          | Note Selectionを解除      |

Transport、Workspace、Emergency Muteなどアプリ全体へ作用するショートカットはGlobal commandとして働きます。

## 9. 基本的な制作の流れ

### MIDIを打ち込む

1. Instrument Trackを作り、Instrumentを選ぶ。
2. Timelineの空白からMIDI Clipを作る。
3. Detail AreaでNoteを入力し、移動、長さ、Velocity、Quantizeを調整する。
4. Global Transportで再生し、DevicesとPlay Surfaceで音色を確認する。
5. 必要なフレーズをDuplicateし、Arrangementへ組み立てる。

### Audio素材を配置する

1. Browserで素材を検索し、Previewで確認する。
2. Audio Trackへドラッグする。
3. Timelineで移動、範囲変更、Fade、Splitを行う。
4. Propertiesで数値を確認し、複製やLoopを設定する。
5. Transportで再生してArrangement全体を確認する。

### 音色を作る

1. Instrument TrackをFocusし、Devicesを開く。
2. InstrumentとEffectの順序やパラメーターを調整する。
3. Play Surfaceで演奏し、変更結果を聞く。
4. Bypass、Reorder、Compareで候補を比べる。
5. 必要な状態をSnapshotやAssetとして残す。

### 演奏を録音する

1. Instrument TrackをFocusする。
2. Input sourceとRecord Armを確認する。
3. MetronomeとCount-inを設定する。
4. Global Transportから録音を開始し、Play SurfaceやMIDI機器で演奏する。
5. Timelineで生成されたClipを確認し、Take Propertiesで候補を比較する。

### Takeを採用する

1. Take PropertiesでRaw、Processed、別テイクをPreviewする。
2. 採用する候補は `Use` で正準Clipへ反映する。
3. 別の配置で試す候補は `Place copy` でTimelineへ置く。
4. 比較前の録音と由来を残したまま、Arrangementを続ける。
