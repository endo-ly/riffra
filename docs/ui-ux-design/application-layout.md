# Riffra 共通画面構造

Riffraの画面は、素材を探す場所、選択対象を調整する場所、時間軸で組み立てる場所、対象を深く編集する場所、演奏する場所を同時に扱います。領域を切り替えても、選択、検索、演奏先の文脈が失われないことを共通の基準にします。

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ GLOBAL CONTROL BAR                                                           │
│ Project / History          Transport                 Audio / Safety           │
├────────────────────┬─────────────────────────────────────────────────────────┤
│ BROWSER            │                                                         │
│ 素材・録音・Plugin  │                        MAIN CANVAS                      │
│                    │                        Timeline                         │
├────────────────────┤                                                         │
│ PROPERTIES         ├─────────────────────────────────────────────────────────┤
│ Track / Clip /     │ DETAIL AREA                                             │
│ Take                │ MIDI Editor / Devices                                  │
├────────────────────┴─────────────────────────────────────────────────────────┤
│ PLAY SURFACE · optional                                                      │
│ Focused Instrument Track / Keyboard / Drum Pads                             │
└──────────────────────────────────────────────────────────────────────────────┘
```

## 1. Global Control Bar

Global Control Barは、セッション全体へ作用する操作をまとめます。Project、履歴、Transport、音声状態、安全操作、検索、設定をここから辿ります。

Transportは画面下部の独立したバーではなく、この領域の一部です。どの編集面を開いていても、同じ再生位置、再生状態、録音状態を参照します。

## 2. Left Column

Left ColumnはBrowserとPropertiesを上下に並べます。

| 領域       | 役割                                                                              |
| ---------- | --------------------------------------------------------------------------------- |
| Browser    | Asset、録音、Inbox、Instrument、Effectを検索・試聴し、TimelineやDevicesへ投入する |
| Properties | 選択中のTrack、Clip、Takeの属性を確認・編集する                                   |

BrowserはPropertiesの選択が変わっても検索語と表示位置を保ちます。Left Columnの幅はMain Canvasとの境界で、BrowserとPropertiesの高さは二つの境界で変更できます。どちらも必要な情報を隠さない最低表示領域を保ちます。

## 3. Main Canvas

Main Canvasは現在の制作領域を表示します。ArrangeではTimelineが中心となり、Track、Clip、Automation、Rulerを同じ時間軸で扱います。

素材の検索はBrowser、選択対象の属性はProperties、時間軸の構成はMain Canvasという役割を保つことで、一つの領域へ機能を詰め込みません。

## 4. Detail Area

Detail Areaは、Timelineで選んだ対象を一段深く編集する場所です。ArrangeではMIDI EditorとDevicesを表示します。

Detail AreaはPropertiesの子ではありません。独立した編集面として、次の表示操作を持ちます。

- サイズ変更
- 折りたたみと復元
- 拡大表示と復元
- 閉じる

Detail Areaを閉じても、Timelineの選択状態とMIDI編集対象は保持します。編集面を開く操作が対象を決めるため、機能を選ぶだけのタブを置きません。

## 5. Play Surface

Play Surfaceは、Focused Instrument Trackを鍵盤、ドラムパッド、コンピューターキーボード、外部MIDIから演奏するための領域です。Main CanvasとDetail Areaから独立して開閉でき、MIDI Editorと同時に表示できます。

演奏先は、Timelineで選択している対象やMIDI Editorで編集しているクリップとは別に保持します。これにより、曲を編集しながら同じ音源を弾いて確認できます。

## 6. 状態の分け方

画面全体で、似て見える状態を別の意味として扱います。

| 状態                     | 意味                                      |
| ------------------------ | ----------------------------------------- |
| Selection                | Timelineで現在選択しているTrackまたはClip |
| Track Context            | PropertiesとDevicesが編集対象にするTrack  |
| Active MIDI Clip         | MIDI Editorが編集しているClip             |
| Focused Instrument Track | Play Surfaceと演奏用MIDI入力の送り先      |
| Record Arm               | 録音対象として準備されたTrack             |

SelectionやActive MIDI Clipが変わっても、Focused Instrument Trackは必要な限り維持します。Record Armは録音の対象を決める状態であり、演奏先のFocusとは統合しません。

## 7. 共通の操作原則

選択、ドラッグ、試聴、取り消し、エラー表示は、ArrangeとDesignで同じ意味を持ちます。操作の結果はすぐ画面へ反映し、確定時にCoreから返された正準状態へ揃えます。

Missing Asset、Missing Plugin、デバイス障害、ランタイムの同期ずれなど、制作へ影響する問題は対象に近い場所から復旧できるようにします。画面全体へ関係する状態だけをGlobal Control Barと全体通知へ広げます。
