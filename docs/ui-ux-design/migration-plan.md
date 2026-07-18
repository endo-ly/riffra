# Riffra UI/UX移行作業計画

## 1. 目的

本計画は、[現行UI評価](current-ui-audit.md)で整理したUIを、[Riffra UI/UX設計](ui-ux-design.md)に示したUIへ置き換えるための作業手順を定める。

対象は、画面構成、Navigation、表示内容、操作方法、共通Component、Style、Frontend上の選択状態と表示状態である。既存の機能を新しいUIから利用できるように接続し直すが、Audio Engine、Native処理、Domain処理、保存処理そのものは実装しない。

UIが必要とする機能が存在しない場合は、Production UIに入口を表示しない。UI側では必要な値と操作をPropsまたはFrontend Interfaceとして定義し、機能実装とは分離する。

## 2. 完了状態

UI/UX移行は、次の状態を満たしたときに完了する。

- Global Bar、Browser、Main Workspace、Inspector、Transportが一つのSession Shellとして構成されている
- Play、Design、Arrangeが同じLayout、共通Component、状態表現を使用している
- 表示される値が、Frontendへ渡された実データと一致している
- 固定Meter、固定時刻、固定Asset、未接続ButtonがProduction UIにない
- 利用できる機能だけがNavigation、Menu、Categoryに表示されている
- Browser、Workspace、Inspectorの選択対象が視覚的に一致している
- Empty、Ready、Working、Muted、Recoverable、Failedを区別できる
- 旧UIのRoute、Component、Style、専用Stateが残っていない
- 主要操作をPointerとKeyboardで行える
- 代表的なWindow幅で、文字や操作が欠けずに表示される

## 3. 移行方針

### 3.1 共通基盤を先に確定する

各画面を個別に作り直す前に、色、文字、余白、共通Component、状態表現、Session Shellを確定する。Play、Design、Arrangeを別々の見た目で作ってから統合する手戻りを防ぐためである。

### 3.2 既存機能の接続を保つ

動作しているButtonや表示は、対応する新UIが利用可能になるまで残す。置換時は既存CallbackとStateを新Componentへ接続し、同じ操作結果になることを確認してから旧Componentを削除する。

### 3.3 見た目だけの機能を作らない

将来必要になるCategoryやButtonであっても、現在利用できないものは表示しない。完成像の確認に必要なUIは、Component PreviewまたはTest Fixtureで作成し、Production Navigationには追加しない。

### 3.4 新旧の入口を並べない

新UIは内部で組み立て、確認が終わった時点で既存の入口を置き換える。利用者にNew / Legacyを選ばせる状態は作らない。切替後は、同じ作業単位の中で旧UIを削除する。

### 3.5 UI作業の境界を守る

UI作業で扱うもの:

- React ComponentとCSS
- LayoutとPanelの開閉
- Frontend上のSelectionと表示状態
- 既存APIとCallbackへの接続
- Loading、Empty、Error、Disabledの表示
- Pointer、Keyboard、Focus
- Component Test、Interaction Test、Visual確認

UI作業で扱わないもの:

- Audio処理方式
- VST3 Host機能
- Native Window管理
- 録音、分析、生成、分離、Renderの処理本体
- SessionやAssetの保存方式
- Tauri CommandやNative APIの新規実装

後者が必要になった場合は、UIに必要な入力値、出力値、Action、状態をInterfaceとして記録し、この移行作業から切り離す。

## 4. 作業順序

| 順序 | 作業                 | 成果                                                    |
| ---- | -------------------- | ------------------------------------------------------- |
| 0    | 既存UIの処分確定     | 全UIの残す・組み替える・隠す・削除するが決まる          |
| 1    | 誤認表示の整理       | 現在利用できる機能だけが表示される                      |
| 2    | 最小UI基盤           | Token、共通Component、状態表現、Layout規則が揃う        |
| 3    | Session Shell        | Global Bar、Browser、Inspector、Transportの共通枠が揃う |
| 4    | Play UI              | 入力、Rack、出力を理解できるPlayへ置き換わる            |
| 5    | 旧Home・旧Play撤去   | 重複した入口と旧Componentがなくなる                     |
| 6    | Recording / Inbox UI | 録音結果を確認、整理、再利用できる                      |
| 7    | Design UI            | Source、Tool、Resultの関係が分かる                      |
| 8    | Arrange UI           | TrackとClipを時間軸上で直接操作できる                   |
| 9    | 全体調整と最終整理   | 表示、操作、旧コード、Accessibilityが統一される         |

前工程の成果を次工程で使用する。最小UI基盤を作らずにWorkspaceへ着手せず、各Workspaceを置き換える前にProduction Navigationへ新しい入口を出さない。

## 5. 既存UI処分表

処分区分は次のとおりである。

| 区分       | 内容                                                                           |
| ---------- | ------------------------------------------------------------------------------ |
| 維持       | 既存の接続を保ち、新しい共通Componentへ載せ替える                              |
| 再構成     | 同じ機能を、TO-BEの配置と操作方法で組み直す                                    |
| 統合       | 別の共通領域へ移し、元の入口を削除する                                         |
| 非表示     | Production UIから外す。処理や保存互換性は変更しない                            |
| 削除       | 代替UIの確認後にComponent、Style、専用Stateを削除する                          |
| UI設計のみ | ComponentとInterfaceだけを用意し、機能が利用可能になるまでProductionへ出さない |

### 5.1 共通シェル

| 既存UI                             | 処分   | TO-BE                         | 作業順序   | 切替条件                                                   |
| ---------------------------------- | ------ | ----------------------------- | ---------- | ---------------------------------------------------------- |
| Session名と自動保存表示            | 再構成 | Global Bar / Session Menu     | 3          | Inline編集と保存状態表示を確認できる                       |
| Undo / Redo                        | 維持   | Global Bar / History          | 3          | 既存Callbackが新Buttonから動く                             |
| Home / Play / Arrange / Designタブ | 再構成 | Play / Design / Arrange       | 3、5、7、8 | 各Workspaceの新UIが利用可能になる                          |
| Command Palette                    | 非表示 | Search                        | 9          | Command、Asset、Settings検索が利用可能になるまで表示しない |
| Audio Engine表示                   | 再構成 | Global Bar / Audio Status     | 3          | 既存Audio状態を表示できる                                  |
| Emergency Mute                     | 維持   | Global Bar / Safety           | 3          | 全Workspaceから既存Actionを実行できる                      |
| Background Job表示とCancel         | 再構成 | Global Bar Status / Inspector | 9          | 既存Job状態とCancelを表示できる                            |
| Missing Dependencies               | 再構成 | Inspector / Recovery          | 9          | 既存Relink、Ignore、無効化Actionを利用できる               |
| Focus Mode                         | 再構成 | Browser / Inspectorの表示管理 | 3          | Panelを閉じる操作とFocus Modeを区別できる                  |

### 5.2 LibraryとInbox

| 既存UI                  | 処分   | TO-BE                          | 作業順序 | 切替条件                                                |
| ----------------------- | ------ | ------------------------------ | -------- | ------------------------------------------------------- |
| 横断Asset検索           | 再構成 | Browser検索                    | 3、6     | 無検索時の一覧、検索、選択を表示できる                  |
| Plugins                 | 再構成 | Browser / Plugin Catalog       | 4        | 既存Plugin一覧とLoad Actionを接続できる                 |
| Racks                   | 再構成 | Browser / Rack Asset           | 6        | 既存Rack一覧とLoad Actionを接続できる                   |
| Recordings / Inbox      | 再構成 | Browser / Inbox                | 6        | 既存Recording操作を新UIから行える                       |
| Presets                 | 非表示 | PluginまたはRackのPreset       | 1        | 固有データと操作が利用可能になるまで表示しない          |
| Samples                 | 非表示 | Audio Asset                    | 1、7     | 実AssetとPrimary Actionが利用可能になるまで表示しない   |
| MIDI                    | 非表示 | MIDI Asset                     | 1、8     | 一覧、Preview、配置Actionが利用可能になるまで表示しない |
| Projects                | 統合   | Session Menu / Recent Sessions | 3        | 既存Project操作をSession Menuから行える                 |
| References              | 非表示 | Reference Asset                | 1、7     | 実Assetと利用Actionが存在する場合だけ表示する           |
| Library上部の追加ボタン | 非表示 | WorkspaceごとのPrimary Action  | 1        | 対象と操作が接続されたButtonだけを表示する              |
| Inbox固定ボタン         | 統合   | Browser内Inbox                 | 6        | BrowserからRecordingへ到達できる                        |

### 5.3 Inspector

| 既存UI                                   | 処分           | TO-BE                            | 作業順序 | 切替条件                                   |
| ---------------------------------------- | -------------- | -------------------------------- | -------- | ------------------------------------------ |
| Plugin名、Vendor、Load、Bypass状態       | 再構成         | Plugin Inspector                 | 4        | 選択Pluginの既存データとActionを表示できる |
| Session更新時刻とData Root               | 統合           | Session概要 / Advanced           | 3、9     | Basic情報と診断情報を分けて表示できる      |
| Input Mono、Gain、Safe                   | 非表示、再構成 | Play Input Inspector             | 1、4     | Frontendへ渡された実値だけを表示できる     |
| Provenance                               | 非表示、再構成 | Selection Inspector / Provenance | 1、6、7  | 選択対象の実データがある場合だけ表示する   |
| Clip、Track、Recording、Asset、Padの編集 | 統合           | 共通Inspector                    | 6〜8     | 対象別の既存ActionをInspectorへ接続できる  |

### 5.4 Transport

| 既存UI            | 処分               | TO-BE                             | 作業順序   | 切替条件                                               |
| ----------------- | ------------------ | --------------------------------- | ---------- | ------------------------------------------------------ |
| Play / Stop       | 再構成             | 再生対象を示すTransport           | 3、4、7、8 | 対象名と既存Actionを同時に表示できる                   |
| Record            | 維持、再構成       | Play Transport                    | 4、6       | 既存録音Actionと状態を表示できる                       |
| Loop              | 再構成             | Design Preview / Arrange Timeline | 7、8       | Loop対象と実値を表示できる                             |
| Master Gain       | 維持               | Transport / Master                | 3          | 既存値とActionを接続できる                             |
| IN / OUT Meter    | 再構成             | Play Signal Flow / Transport      | 4          | Frontendへ渡された実Meter値を表示できる                |
| Previous Position | 非表示             | なし                              | 1          | Production UIから削除する                              |
| 小節、拍、時刻    | 非表示、再構成     | Arrange Transport                 | 1、8       | Timelineの実値が利用可能な場合だけ表示する             |
| BPMと拍子         | 非表示、UI設計のみ | Arrange Transport                 | 1、8       | 値と編集Actionが利用可能になるまでProductionへ出さない |

### 5.5 Home

| 既存UI                              | 処分   | TO-BE                     | 作業順序 | 切替条件                                  |
| ----------------------------------- | ------ | ------------------------- | -------- | ----------------------------------------- |
| Playへ                              | 削除   | 起動時のWorkspace表示     | 5        | Session ShellとPlay UIの切替が完了する    |
| Quick Record                        | 統合   | Play Transport / Record   | 4、6     | 既存録音ActionをPlayから行える            |
| Export / Import Manifest            | 統合   | Session Menu              | 3        | 既存ActionをSession Menuから行える        |
| Safe ModeとRecovery世代             | 統合   | Session Recovery          | 3、9     | 既存Recovery操作を新Dialogから行える      |
| Recover Audio Device                | 統合   | Audio Status              | 3        | 既存Recover Actionを実行できる            |
| Driver、Device、Sample Rate、Buffer | 統合   | Audio Settings            | 3        | 既存設定値とActionを表示できる            |
| Device一覧Refresh                   | 統合   | Audio Settings            | 3        | 既存Refresh Actionを利用できる            |
| Count-in                            | 非表示 | 未定                      | 1        | UI/UX設計で役割が定義されるまで表示しない |
| 前回の状態                          | 非表示 | Session復元後のWorkspace  | 1        | 固定波形と固定操作を削除する              |
| 最近の制作資産                      | 非表示 | Browser / Recent Sessions | 1、6     | 実データを表示できる場合だけ提供する      |
| Startup Volume Meter                | 非表示 | Play Input / Output Meter | 1、4     | 固定Meterを削除する                       |
| ENGINE NEXT                         | 削除   | なし                      | 1        | 表示と専用Styleを削除する                 |

### 5.6 Play

| 既存UI                      | 処分           | TO-BE                             | 作業順序 | 切替条件                                                  |
| --------------------------- | -------------- | --------------------------------- | -------- | --------------------------------------------------------- |
| A/B Snapshot                | 再構成         | Play Snapshot                     | 4        | 既存Capture / Recallを意味の分かるUIへ接続できる          |
| Inputカード                 | 非表示、再構成 | Play Input                        | 1、4     | 実Device、Channel、Meter、Gainを表示できる                |
| Plugin Load                 | 維持、再構成   | BrowserからRackへ追加             | 4        | 既存Load Actionを新UIから実行できる                       |
| Plugin Bypass / Remove      | 維持、再構成   | Rack Device操作                   | 4        | 選択対象とActionの対象が一致する                          |
| Add Device                  | 非表示、再構成 | Rack / Add Device                 | 1、4     | 既存Load Actionへ接続できる                               |
| Outputカード                | 非表示、再構成 | Play Output                       | 1、4     | 実Device、Channel、Meterを表示できる                      |
| Save / Load Rack            | 再構成         | Browser / Rack Asset              | 6        | 既存Save / Loadを新UIから実行できる                       |
| Common Parameter View       | 再構成         | Plugin Inspector                  | 4        | 既存Parameter値とActionを表示できる                       |
| Pluginネイティブ画面        | UI設計のみ     | VST3 Editorを開くAction           | 4        | Open ActionがFrontendへ提供されるまでProductionへ出さない |
| Macro                       | 再構成         | Plugin Inspector / Macro          | 4        | 既存Mappingと値を表示できる                               |
| Session Note                | 統合           | Session Inspector                 | 6        | 既存Note値と保存Actionを利用できる                        |
| Input / Outputルーティング  | 統合           | Play Signal Flow / Audio Settings | 3、4     | 既存設定値とActionを表示できる                            |
| 空Rackの検出済みPlugin表示  | 削除           | BrowserのPlugin Catalog           | 1        | Rackが読込み済みDeviceだけを表示する                      |
| 複数カードの単一Runtime操作 | 非表示、再構成 | 選択Device単位の操作              | 1、4     | 対象を一意に識別できるデータだけを表示する                |

### 5.7 Design

| 既存UI                 | 処分       | TO-BE                             | 作業順序 | 切替条件                                          |
| ---------------------- | ---------- | --------------------------------- | -------- | ------------------------------------------------- |
| Design内の道具切替     | 再構成     | Design Tool Rail                  | 7        | 利用できるToolだけを表示できる                    |
| Sample Pad Mapping     | 再構成     | Design Map                        | 7        | 既存Pad操作を新CanvasとInspectorへ接続できる      |
| MIDI Device / Monitor  | 統合       | Play MIDI Input / Design Map      | 7        | 用途ごとに既存表示を配置できる                    |
| Analyze                | 再構成     | Design Analyze                    | 7        | 既存分析結果をSourceと対応付けて表示できる        |
| Reference Compare      | 再構成     | Reference Match                   | 7        | 既存比較結果をAnalyze内で表示できる               |
| AI Context / ChangeSet | 再構成     | InspectorのSuggestion / ChangeSet | 7        | 実際に利用できるActionだけを表示できる            |
| Separate               | 再構成     | Design Derive / Channel Split     | 7        | 現在の処理内容を正しい名称で表示できる            |
| 数式と信号生成         | UI設計のみ | Design Generate                   | 7        | 必要なInterfaceを定義し、Productionには表示しない |
| 波形編集               | 再構成     | Design Edit Canvas                | 7        | 既存Range操作を波形上の操作へ接続できる           |

### 5.8 Arrange

| 既存UI                           | 処分         | TO-BE                             | 作業順序 | 切替条件                                                |
| -------------------------------- | ------------ | --------------------------------- | -------- | ------------------------------------------------------- |
| Track追加、Gain、Pan、Mute、Solo | 再構成       | Track Header / Inspector          | 8        | 既存Actionを直接操作と数値編集から利用できる            |
| Recordingの配置                  | 維持、再構成 | BrowserからTimelineへ配置         | 8        | 既存配置ActionをPrimary ActionとDrag & Dropへ接続できる |
| Timeline表示                     | 再構成       | Track別Timeline                   | 8        | 既存TrackとClipを同じ時間軸へ表示できる                 |
| Clip編集                         | 再構成       | Timeline直接操作 / Clip Inspector | 8        | 既存編集ActionをPointer操作へ接続できる                 |
| MIDI Import / Edit / Export      | 再構成       | MIDI Asset / Piano Roll           | 8        | 利用可能なActionだけを直接操作へ接続できる              |
| WAV / Stem Render                | 再構成       | Arrange Export / Job Status       | 8        | 既存Render Actionと進捗を表示できる                     |
| Tempo、拍子、Marker、Loop Region | UI設計のみ   | Arrange Transport / Timeline      | 8        | 値とActionが利用可能になるまでProductionへ出さない      |
| Automation                       | UI設計のみ   | Automation Lane                   | 8        | 値とActionが利用可能になるまでProductionへ出さない      |
| Track別Lane                      | 再構成       | Track別Timeline Lane              | 8        | 既存TrackとClipをLaneごとに表示できる                   |

## 6. 作業0 — 既存UI処分の確定

### 作業

1. Section 5の各項目を、実際のComponentとStyleへ対応付ける
2. Productionから即時に外す項目を一覧化する
3. 既存Callbackを維持して載せ替える項目を一覧化する
4. UI Interfaceだけを作る項目を一覧化する
5. 各Workspaceで削除する旧Componentと削除条件を確定する

### 成果物

- UI項目とComponentの対応表
- 非表示対象一覧
- 既存Callback一覧
- UI Interface不足一覧
- Workspace別の削除対象一覧

### 完了条件

- すべての現行UIに処分と作業順序が設定されている
- 処分未定の項目がない
- UI作業と機能実装作業を区別できる

## 7. 作業1 — 誤認表示の整理

### 作業

- 固定Asset、固定波形、固定Meter、固定時刻を削除する
- 未接続Buttonを削除する
- 実データと固有操作がないLibrary Categoryを隠す
- Rackへ読込まれていないPluginをRack Deviceとして表示しない
- 一つの対象を操作しているように見えない重複カードを隠す
- Inspectorの固定値を削除する
- Focus ModeとPanel Closeの表示を分ける

### 完了条件

- Production UIに固定サンプルがない
- すべての表示Buttonが既存Actionへ接続されているか、理由付きでDisabledになっている
- Rack、Meter、Asset、Audio状態がFrontendへ渡された値だけで表示される
- この作業で新しい機能入口を追加していない

## 8. 作業2 — 最小UI基盤

### 8.1 Design Token

- Background、Surface、Border、Text、Muted、Accent、Danger
- Font Family、Font Size、Font Weight、Line Height
- Spacing、Panel幅、Control高さ、Radius
- Hover、Active、Selected、Focus、Disabled
- Ready、Working、Muted、Recoverable、Failedの状態色
- Motion DurationとReduced Motion

TokenはCSS変数として定義し、各Component内に同じ値を重複して持たせない。

### 8.2 共通Component

- Button、Icon Button、Toggle、Tabs
- Text Field、Number Field、Select、Search Field
- Menu、Popover、Tooltip、Dialog
- Panel Header、Section、Property Row
- List、List Item、Tree、Filter
- Status、Badge、Progress、Meter
- Empty State、Loading State、Error State
- Splitter、Scroll Area、Resize Handle

各Componentは、Default、Hover、Focus、Selected、Disabled、Loading、Errorを確認できる状態を持つ。

### 8.3 Layout規則

- Global Barの高さと情報密度
- BrowserとInspectorの標準幅、最小幅、最大幅
- Main Workspaceの最小表示領域
- Transportの高さとWorkspace別内容
- Panelを閉じたときのMain Workspace拡張
- 狭いWindowでのPanel優先順位
- Dialog、Popover、Native Windowを開くButtonの配置

### 8.4 状態表現

| 状態             | 表示内容                                    |
| ---------------- | ------------------------------------------- |
| Empty            | 対象がない理由と、利用できる最初の操作      |
| Ready            | 実値、選択対象、主要操作                    |
| Working          | 対象、進捗、Cancel Action                   |
| Muted / Bypassed | 対象、理由、解除Action                      |
| Recoverable      | 保持されたデータ、影響範囲、Recovery Action |
| Failed           | 失敗対象、理由、利用可能な回避Action        |

### 8.5 Component Preview

共通Componentと状態は、Production Navigationから独立したPreviewで確認する。固定データはPreview内だけで使用し、Production Componentには渡さない。

### 完了条件

- 共通ComponentがTokenだけで描画される
- Keyboard Focusを視認できる
- Componentの主要状態をPreviewとTestで確認できる
- Session Shellと各Workspaceが使用するLayout寸法が決まっている
- Workspace固有Componentの作成前に、再利用するComponentを選べる

## 9. 作業3 — Session Shell

### 作業

- Global Bar、Browser、Main Workspace、Inspector、TransportのLayoutを作る
- BrowserとInspectorの開閉、幅変更、Focus Modeを作る
- Global BarへSession、Undo / Redo、Workspace、Audio Status、Emergency Muteを配置する
- Session Menuへ既存のSession操作を配置する
- Audio StatusとAudio Settingsへ既存のAudio表示とActionを配置する
- Transportに再生対象を表示する領域を作る
- 共通Selectionを表示するInspectorの枠を作る
- Workspace切替時にPanelと表示状態を保持する

この段階では、既存のPlay、Design、ArrangeをMain Workspace内に表示し、新旧Workspaceを並べない。共通領域の切替確認後に各Workspaceを順番に置き換える。

### 完了条件

- 全Workspaceが同じSession Shell内で表示される
- BrowserとInspectorの開閉でMain Workspaceが正しく伸縮する
- Global BarとTransportに固定値がない
- 既存のSession、Audio、Undo / Redo、Emergency Mute Actionを失っていない
- KeyboardでWorkspaceとPanelへ移動できる

## 10. 作業4 — Play UI

### 作業

- Main WorkspaceをInput、Rack、OutputのSignal Flowとして構成する
- Browserに実Pluginと実Rackだけを表示する
- InputにDevice、Channel、Meter、Gainを表示する
- Rackに読込み済みDeviceを処理順で表示する
- Device選択時にInspectorへ状態、Parameter、Routingを表示する
- OutputにDevice、Channel、Meter、Muteを表示する
- TransportにLive Input / Rack、Monitoring、Record、Meter、Masterを表示する
- Add、Load、Bypass、Remove、Parameter、Macro、Snapshotを既存Actionへ接続する
- VST3 Editorを開くActionはUIを定義するが、Actionが提供されるまでProductionでは表示しない
- Audio、Plugin、RackのEmpty、Loading、Fault表示を共通Componentで構成する

### UI受入シナリオ

1. Playを開く
2. Input DeviceとChannelを選ぶ
3. Input Meterの表示を確認する
4. BrowserからAmpliTubeを選ぶ
5. Rackへ追加する
6. Rack Deviceを選択する
7. Inspectorから利用可能なParameterを操作する
8. BypassとRemoveを実行する
9. Output Meter、Mute、Latency、Faultを確認する

このシナリオでは、既存機能へ接続できるUIだけを受入対象とする。VST3 EditorなどFrontendへActionが提供されていない操作は、Production UIの完了条件に含めない。

### 完了条件

- Input、Rack、Outputの関係を画面から理解できる
- Rack表示がFrontendへ渡されたDeviceと一致する
- 選択DeviceとInspectorの内容が一致する
- 表示された操作が既存Actionへ接続されている
- Empty、Loading、Faultから次の操作を判断できる
- 旧Playと同じ既存機能を新Playから利用できる

## 11. 作業5 — 旧Home・旧Play撤去

### 作業

- 起動後の正規WorkspaceをSession Shellへ切り替える
- HomeのSession操作をSession Menuへ統合する
- HomeのAudio操作をAudio StatusとAudio Settingsへ統合する
- Quick RecordをPlay Transportへ統合する
- `WorkspaceHome`、旧`WorkspacePlay`、専用Styleを削除する
- 旧UIだけが使用するFrontend StateとPropsを削除する
- Homeを前提とするNavigationとTestを更新する

### 完了条件

- Homeを経由せずSession、Audio、Recovery、Playへ到達できる
- 新旧Playを選ぶ入口がない
- 旧Homeと旧Playへ到達できない
- 削除したComponentを参照するImport、Style、Testがない

## 12. 作業6 — Recording / Inbox UI

### 作業

- Play Transportに録音中、経過、停止、保存結果を表示する
- BrowserにInboxとRecording一覧を表示する
- Raw、Processed、MIDI、録音条件を一つのRecordingとしてまとめて表示する
- Recording選択時にInspectorへIdentity、Metadata、Provenance、Stateを表示する
- Rename、Tag、Note、Promote、Archive、Deleteを新UIへ接続する
- Preview対象をTransportに表示する
- Rack Save / LoadをBrowserへ統合する
- 録音中断と保存失敗を共通状態Componentで表示する

### 完了条件

- 録音中であることと保存結果が分かる
- RecordingがInboxへ表示される
- BrowserとInspectorの選択対象が一致する
- 既存のRecording操作を新UIから行える
- ブラウザ標準Promptに依存するMetadata編集が残っていない

## 13. 作業7 — Design UI

### 作業

- Source、Tool Rail、Design Canvas、ResultのLayoutを作る
- 利用可能なToolだけをTool Railに表示する
- RecordingとAssetをSourceとして表示する
- Sample Pad MappingをMap Toolとして再構成する
- 既存Range操作をWaveform上のEdit UIへ接続する
- Analyze、Reference Compare、ChangeSetをSourceと対応付けて表示する
- Stereo Left / Right処理をChannel Splitとして表示する
- Preview対象と位置をTransportへ表示する
- Source、Pad、ResultをInspectorへ接続する
- 数式生成など機能が存在しないToolはPreviewでUI契約だけを確認し、Productionには表示しない

### 完了条件

- Designを開いたときにSource、Tool、Resultの関係が分かる
- 利用できないToolがProduction UIに表示されない
- 選択対象とInspectorの内容が一致する
- 既存のSample、Analyze、Compare、Split操作を新UIから利用できる
- Tool固有画面が共通Layoutと状態Componentを使用している

## 14. 作業8 — Arrange UI

### 作業

- Track HeaderとTrack別Laneを同じ時間軸に配置する
- BrowserからRecordingとAudio Assetを配置するUIを作る
- Clipの選択、移動、Resize、Split、DuplicateをPointer操作へ接続する
- TrackとClipの値をInspectorへ表示する
- Gain、Pan、Fade、Loop、Mute、Soloを直接操作とInspectorから変更できるようにする
- Timelineの再生対象と位置をTransportに表示する
- MIDI編集をPiano Rollとして再構成する
- Render ActionとJob状態をArrangeから表示する
- Tempo、拍子、Marker、AutomationはActionと値が利用可能になるまでProductionに表示しない

### 完了条件

- TrackとClipの所属、位置、長さが視覚的に分かる
- 既存Clip編集ActionをTimelineから利用できる
- TimelineとInspectorの表示値が一致する
- Timelineの再生対象と位置がTransportから分かる
- 利用できないTimeline機能が操作可能に見えない

## 15. 作業9 — 全体調整と最終整理

### 作業

- Play、Design、Arrangeの密度、余白、文字、Panel幅を調整する
- Empty、Working、Muted、Recoverable、Failedの表現を統一する
- Missing Dependency、Recovery、Background Jobを共通Componentへ統合する
- Keyboard Shortcut、Focus順序、Screen Reader Labelを確認する
- 狭いWindow、長い名称、空一覧、大量一覧を確認する
- 旧Route、旧Component、旧CSS、固定Fixture、重複Stateを削除する
- Production BuildにPreview専用UIや固定データが含まれていないことを確認する
- Section 5のすべての処分項目を完了へ更新する

### 完了条件

- Play、Design、Arrangeが同じVisual Languageを使用している
- 共通操作の位置と挙動がWorkspace間で一致する
- Keyboardだけで主要UIを移動・操作できる
- 代表的なWindow幅で操作が欠けない
- Production UIにPlaceholderと固定データがない
- 到達不能な旧UIコードと専用Styleがない
- 既存UI処分表に未処理項目がない

## 16. Pull Requestの分割

一つのPull Requestに複数Workspaceの置換を混ぜない。次の単位を基本とする。

1. 誤認表示の削除
2. Design Token
3. 共通入力Component
4. 共通状態Component
5. Session Shell Layout
6. Global BarとSession Menu
7. Browser Frame
8. Inspector Frame
9. Transport Frame
10. Play Input / Output
11. Play Rack / Inspector
12. Play切替と旧Home・旧Play削除
13. Recording / Inbox
14. Design LayoutとTool単位の置換
15. Arrange Layoutと操作単位の置換
16. Recovery、Accessibility、最終削除

各Pull Requestは、表示変更、Interaction Test、必要な既存Callback接続を同時に含める。Componentの外観だけを追加してProductionへ未接続のまま残さない。

## 17. 検証

### 各Component

- Default、Hover、Focus、Selected、Disabled
- Empty、Loading、Error
- 長い名称、値なし、大量項目
- Keyboard操作

### 各Workspace

- Browser、Workspace、InspectorのSelection一致
- Panel開閉と幅変更
- Transportの再生対象表示
- 既存Actionへの接続
- 未接続操作が表示されていないこと

### 全体

- Production Build
- Component TestとInteraction Test
- 代表的なWindow幅での目視確認
- Production UIに固定Fixtureが含まれないこと
- 旧UIへ到達できないこと
- `git diff --check`
