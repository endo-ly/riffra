# Riffra 共通画面構造

Riffra の制作画面は、音を探す場所、選択対象を調整する場所、時間軸で組み立てる場所、演奏内容を編集する場所、Instrument Track を演奏する場所を同時に扱える。各領域は表示の切替で入れ替えるのではなく、役割を保ったまま連携する。

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ GLOBAL CONTROL BAR                                                           │
│ Project / Host / History             Transport             Audio / Safety    │
├────────────────────┬─────────────────────────────────────────────────────────┤
│ BROWSER            │                                                         │
│ Search             │                        MAIN CANVAS                      │
│ Plugins            │                        Timeline                         │
│ Recordings         │                                                         │
├────────────────────┤                                                         │
│ PROPERTIES         ├─────────────────────────────────────────────────────────┤
│ Track / Clip /     │ DETAIL AREA                                             │
│ Take properties    │ MIDI Editor / Devices                                  │
├────────────────────┴─────────────────────────────────────────────────────────┤
│ PLAY SURFACE · optional                                                      │
│ Focused Instrument Track / Keyboard / Drum Pads                             │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Global Control Bar

Global Control Bar は、現在のセッション、Host、履歴、Transport、音声状態、安全操作、検索、設定をまとめる。Transport はこの領域の一部として扱い、独立した下部バーには置かない。

### Project / Host Selector

Project / Host Selector はGlobal Control Barの左端に置き、現在のProjectとHost接続を常時表示する。1行目にProject名と自動保存状態、2行目にHost名と接続状態を示す。Project名はActive Projectの `CreativeSession.project_name` から取得し、未命名の場合は `Untitled Project` と表示する。Host名はHost bootstrapの情報、DataRootのbasename、PIDまたはinstance IDの補助情報から構成し、Registryに表示名を別管理しない。

選択するとpopoverが開き、PROJECTとHOSTのセクションを提供する。PROJECTではProject一覧、新規作成、Active Projectの改名、Import ProjectとExport Projectを実行する。Project一覧の項目はDataRoot内のProjectを切り替えるため、ファイルダイアログを開かない。Import Projectだけがファイルダイアログで `.riffra` packageを選択し、その内容を新しいProjectとしてDataRootへ取り込んでActive Projectにする。Export ProjectはユーザーがSave dialogで指定した場所へportable `.riffra` packageを書き出し、成功後に保存先の絶対パスを通知する。

- Embeddedは `Local Desktop` と表示する
- Attachedは接続先Hostのproject nameまたはDataRoot basenameと、PID・Runtime状態・DataRootを表示する
- Project切替中は現在の画面を保持し、`Opening...`を表示してProject-bound操作、Transport、録音を無効にする
- Host切替中は現在の画面を保持し、`Connecting...`を表示してHost-bound操作を無効にする
- Disconnectedは最後の接続先を再接続候補として表示し、`Reconnect`、`Local Desktop`、`Refresh`を提供する
- `Connect to Local Host...`はDesktopのfolder dialogでDataRootを選択し、そのHostの`host.json`へ接続する

Hostの切替後はCanonical state、履歴、Runtime、Audio、Transport、Plugin、Recording、Library、Missing、Jobの表示を新Hostのbootstrap基準へ置き換える。最後に表示していたSessionを参照表示として残す場合も、Hostへ接続していない間は編集・再生・録音・Audio設定を実行できる状態にしない。

Projectの切替後はHostを維持したまま、Canonical state、履歴、Runtime投影、Projectに依存する選択・詳細表示をActive Projectの状態へ置き換える。Project一覧はHostが返す `ProjectState` を使い、Desktop側で別のProjectカタログを作らない。切替に失敗した場合は切替前のProjectを表示し続ける。

## Left Column

Left Column は Browser と Properties を上下に並べる。Browser は素材の検索・試聴・投入を担当し、共通の検索と Plugins / Recordings の折りたたみ可能なセクションで構成する。セクションは互いに独立して開閉でき、同時に表示される。検索はセクションを横断する絞り込みとして共有する。Properties は Arrange の選択状態に応じて Track、Clip、Take の内容を表示する。両方を同時に表示するため、選択対象が変わっても Browser の検索や表示文脈は維持される。

Left Column の幅は Main Canvas との境界で変更できる。Browser と Properties の境界は上下に変更でき、Browser の最低表示領域を保つ。

## Main Canvas

Main Canvas は現在の制作領域を表示する。Arrange では Timeline が中心となり、残りの横幅と高さを使って Track、Clip、Automation を表示する。

## Detail Area

Detail Area は Timeline から開いた対象の編集場所である。Arrange では MIDI Editor を表示し、Instrument と Effect Chain を扱う Devices もこの領域に配置する。Resize、Collapse / Restore、Expand / Restore、Close を提供し、Properties の子には置かない。Detail Area に機能選択タブは置かず、対象を開く操作が編集面を決める。

## Play Surface

Play Surface は Focused Instrument Track へ Keyboard または Drum Pads から入力するための領域である。Main Canvas と Detail Area から独立して開閉でき、MIDI Editor と同時に表示できる。
