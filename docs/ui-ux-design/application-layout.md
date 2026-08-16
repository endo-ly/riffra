# Riffra 共通画面構造

Riffra の制作画面は、音を探す場所、選択対象を調整する場所、時間軸で組み立てる場所、演奏内容を編集する場所、Instrument Track を演奏する場所を同時に扱える。各領域は表示の切替で入れ替えるのではなく、役割を保ったまま連携する。

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ GLOBAL CONTROL BAR                                                           │
│ Project / History                    Transport             Audio / Safety     │
├────────────────────┬─────────────────────────────────────────────────────────┤
│ BROWSER            │                                                         │
│                    │                        MAIN CANVAS                      │
│ Search             │                        Timeline                         │
│ Assets             │                                                         │
│ Recordings         │                                                         │
│ Plugins            │                                                         │
├────────────────────┤                                                         │
│ PROPERTIES         ├─────────────────────────────────────────────────────────┤
│ Track / Clip /     │ DETAIL AREA                                             │
│ Take properties    │ MIDI Editor                                             │
├────────────────────┴─────────────────────────────────────────────────────────┤
│ PLAY SURFACE · optional                                                      │
│ Focused Instrument Track / Keyboard / Drum Pads                             │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Global Control Bar

Global Control Bar は、セッション名、履歴、Transport、音声状態、安全操作、検索、設定をまとめる。Transport はこの領域の一部として扱い、独立した下部バーには置かない。

## Left Column

Left Column は Browser と Properties を上下に並べる。Browser は素材の検索・試聴・投入を担当し、Properties は Arrange の選択状態に応じて Track、Clip、Take の内容を表示する。両方を同時に表示するため、選択対象が変わっても Browser の検索や表示文脈は維持される。

Left Column の幅は Main Canvas との境界で変更できる。Browser と Properties の境界は上下に変更でき、Browser の最低表示領域を保つ。

## Main Canvas

Main Canvas は現在の制作領域を表示する。Arrange では Timeline が中心となり、残りの横幅と高さを使って Track、Clip、Automation を表示する。

## Detail Area

Detail Area は Timeline から開いた対象の編集場所である。Arrange では MIDI Editor を表示し、Resize、Collapse / Restore、Expand / Restore、Close を提供する。Detail Area に機能選択タブは置かない。

## Play Surface

Play Surface は Focused Instrument Track へ Keyboard または Drum Pads から入力するための領域である。Main Canvas と Detail Area から独立して開閉でき、MIDI Editor と同時に表示できる。
