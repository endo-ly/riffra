# 現行UI評価

## 1. 評価基準

UIを次の四区分で評価する。

| 判定             | 意味                                                             |
| ---------------- | ---------------------------------------------------------------- |
| 実動確認済み     | Native実機または自動回帰で主要経路が確認されている               |
| 結線済み         | UIから処理へ接続されているが、UX改善または実機確認が残る         |
| 表示のみ         | 固定値、固定サンプル、操作のないボタンなど、制作状態を変更しない |
| 非表示・統合候補 | 現時点の製品理解を妨げるため、実装まで隠すか別UIへ統合すべきもの |

「結線済み」は完成を意味しない。処理が呼び出せても、利用者が目的を理解できない、実状態を誤認する、失敗後の行動が分からない、実機確認がない場合は完成したUIとして扱わない。

## 2. UI棚卸し

### 2.1 共通シェル

| UI                                 | 判定         | 評価                                                                  |
| ---------------------------------- | ------------ | --------------------------------------------------------------------- |
| Session名と自動保存表示            | 結線済み     | 表示と名称変更は動くが、名称変更がブラウザ標準Promptである            |
| Undo / Redo                        | 実動確認済み | Session変更を取り消し、やり直せる                                     |
| Home / Play / Arrange / Designタブ | 結線済み     | 切替は動くが、Homeを三制作領域と同列に置いている                      |
| Command Palette                    | 表示のみ     | Workspace切替以外のAction、Asset、Settings検索とキーボード移動がない  |
| Audio Engine表示                   | 結線済み     | 状態は実データだが、ボタンとしての操作がない                          |
| Emergency Mute                     | 実動確認済み | 全画面から実行できる                                                  |
| Background Job表示とCancel         | 実動確認済み | Scan、Analysis、Separation、Renderの状態を表示する                    |
| Missing Dependencies               | 結線済み     | Relink、Ignore、Plugin無効化は動くが、パス手入力中心である            |
| Focus Mode                         | 結線済み     | 表示整理は動くが、Inspectorの閉じる記号がFocus Mode開始を意味している |

### 2.2 LibraryとInbox

| UI                      | 判定         | 評価                                                                                             |
| ----------------------- | ------------ | ------------------------------------------------------------------------------------------------ |
| 横断Asset検索           | 結線済み     | 検索、選択、関連Asset、Preview、Edit、Analyzeへ接続されているが、検索しないとAssetを閲覧できない |
| Plugins                 | 実動確認済み | VST3一覧、絞込み、ラックへの読込みが動く                                                         |
| Racks                   | 結線済み     | 保存済みRackを読めるが、保存先指定がPromptである                                                 |
| Recordings / Inbox      | 実動確認済み | Preview、Analyze、Rename、Tag、Promote、Archive、Delete、重複検出がある                          |
| Presets                 | 表示のみ     | 固有の一覧と操作がない                                                                           |
| Samples                 | 表示のみ     | 固有の一覧と操作がない                                                                           |
| MIDI                    | 表示のみ     | 固有の一覧と操作がない                                                                           |
| Projects                | 表示のみ     | 固有の一覧と操作がない                                                                           |
| References              | 表示のみ     | 固有の一覧と操作がない                                                                           |
| Library上部の追加ボタン | 表示のみ     | 操作が接続されていない                                                                           |
| Inbox固定ボタン         | 結線済み     | Recordingsセクションを開く                                                                       |

### 2.3 Inspector

| UI                                       | 判定             | 評価                                                                |
| ---------------------------------------- | ---------------- | ------------------------------------------------------------------- |
| Plugin名、Vendor、Load、Bypass状態       | 結線済み         | 一部は実データを表示する                                            |
| Session更新時刻とData Root               | 結線済み         | SessionとBootstrap状態を表示する                                    |
| Input Mono、Gain、Safe                   | 表示のみ         | 選択機器や実測値と連動しない固定表示である                          |
| Provenance                               | 表示のみ         | 選択したAssetやClipの由来を辿る表示になっていない                   |
| Clip、Track、Recording、Asset、Padの編集 | 非表示・統合候補 | 各画面内の個別フォームへ分散し、Inspectorが選択対象を所有していない |

### 2.4 Transport

| UI                | 判定         | 評価                                                |
| ----------------- | ------------ | --------------------------------------------------- |
| Play / Stop       | 結線済み     | Preview経路と接続されているが、再生対象が明確でない |
| Record            | 実動確認済み | 原音、加工音、録音情報の保存へ接続されている        |
| Loop              | 結線済み     | Session設定は変わるが、範囲指定がない               |
| Master Gain       | 実動確認済み | SessionとNative音量へ反映する                       |
| IN / OUT Meter    | 結線済み     | Nativeピーク値を表示する                            |
| Previous Position | 表示のみ     | 操作が接続されていない                              |
| 小節、拍、時刻    | 表示のみ     | 固定値である                                        |
| BPMと拍子         | 表示のみ     | ボタンの外観だが固定値であり編集できない            |

### 2.5 Home

| UI                                  | 判定             | 評価                                                 |
| ----------------------------------- | ---------------- | ---------------------------------------------------- |
| Playへ                              | 実動確認済み     | Playへ移動する                                       |
| Quick Record                        | 実動確認済み     | 録音を開始、停止する                                 |
| Export / Import Manifest            | 結線済み         | Project入出力は動くが、ファイル選択体験がない        |
| Safe ModeとRecovery世代             | 実動確認済み     | 障害時の継続と世代選択を提供する                     |
| Recover Audio Device                | 実動確認済み     | Audio sidecarの復旧へ接続されている                  |
| Driver、Device、Sample Rate、Buffer | 実動確認済み     | 実機へ適用されるが、選択ごとに即時切替が走る         |
| Device一覧Refresh                   | 結線済み         | 動作するがDriver Pickerと情報が重複する              |
| Count-in                            | 実動確認済み     | Sessionへ保存し、録音開始へ反映する                  |
| 前回の状態                          | 表示のみ         | Session名以外の波形と展開操作が固定表示である        |
| 最近の制作資産                      | 表示のみ         | 固定サンプルであり、LibraryやAssetへ接続されていない |
| Startup Volume Meter                | 表示のみ         | 値とMeterが固定されている                            |
| ENGINE NEXT                         | 非表示・統合候補 | 現在の実装状態を正しく説明しない内部的なラベルである |

### 2.6 Play

| UI                         | 判定             | 評価                                                        |
| -------------------------- | ---------------- | ----------------------------------------------------------- |
| A/B Snapshot               | 結線済み         | CaptureとRecallは動くが、空Slotと保存先の意味が不明確である |
| Inputカード                | 表示のみ         | Input名、Mono、Gain、Meterが固定されている                  |
| Plugin Load                | 実動確認済み     | LibraryからVST3を読み込む                                   |
| Plugin Bypass / Remove     | 実動確認済み     | Native RackとSessionへ反映する                              |
| Add Device                 | 表示のみ         | 操作が接続されていない                                      |
| Outputカード               | 表示のみ         | Meterが固定されている                                       |
| Save / Load Rack           | 結線済み         | 保存と読込みは動くが、保存先PromptとLibrary移動に依存する   |
| Common Parameter View      | 結線済み         | Parameter変更へ接続されているが、実Pluginでの確認が残る     |
| Pluginネイティブ画面       | 非表示・統合候補 | 構想上の中核機能だが、開閉UIとWindow管理がない              |
| Macro                      | 結線済み         | 値変更とParameter Mappingへ接続されている                   |
| Session Note               | 結線済み         | 保存されるが、入力のたびに永続化処理を行う                  |
| Input / Outputルーティング | 非表示・統合候補 | Play内になく、Homeの詳細設定まで戻る必要がある              |

Rackが空のときに検出済みPluginの先頭三件をRack Deviceとして表示するため、検出されたPluginと実際に読み込まれたPluginが同じ見た目になる。複数カードのBypassとRemoveも単一のRuntime Plugin操作を共有しており、表示と音声経路が一致しない可能性がある。

### 2.7 Design

| UI                     | 判定             | 評価                                                                         |
| ---------------------- | ---------------- | ---------------------------------------------------------------------------- |
| Design内の道具切替     | 非表示・統合候補 | Sample、Analyze、Separateを明示的に選ぶナビゲーションがない                  |
| Sample Pad Mapping     | 結線済み         | RecordingからPadを作り、範囲、Gain、Loop、Previewを操作できる                |
| MIDI Device / Monitor  | 結線済み         | 機器操作は接続されているが、Sample編集の中に置かれている                     |
| Analyze                | 結線済み         | Waveform、Level、Dynamics、Spectrum、Phaseを表示する                         |
| Reference Compare      | 結線済み         | 分析比較とPreviewへ接続されるが、Analyzeの基本目的より強く表示される         |
| AI Context / ChangeSet | 結線済み         | 実装はローカルの比較とGain適用であり、一般的なAI機能に見える名称が過大である |
| Separate               | 結線済み         | 実装内容は音源分離ではなくStereo Left / Right分割である                      |
| 数式と信号生成         | 非表示・統合候補 | 構想上の主要機能だがUIがない                                                 |
| 波形編集               | 非表示・統合候補 | Padの範囲入力以外に編集UIがない                                              |

### 2.8 Arrange

| UI                               | 判定             | 評価                                                                                             |
| -------------------------------- | ---------------- | ------------------------------------------------------------------------------------------------ |
| Track追加、Gain、Pan、Mute、Solo | 結線済み         | Sessionへ反映するが、数値入力中心である                                                          |
| Recordingの配置                  | 結線済み         | 非破壊Audio Clip作成へ接続されている                                                             |
| Timeline表示                     | 結線済み         | Clip位置と長さを表示するが、選択、Drag、Resizeがない                                             |
| Clip編集                         | 結線済み         | Track、位置、長さ、Source範囲、Gain、Fade、Pan、Loop、Duplicate、Split、Mute、Removeを操作できる |
| MIDI Import / Edit / Export      | 結線済み         | 処理へ接続されているが、Piano Rollは表示だけで編集は数値欄から行う                               |
| WAV / Stem Render                | 結線済み         | Offline Renderへ接続されている                                                                   |
| Tempo、拍子、Marker、Loop Region | 非表示・統合候補 | 未実装である                                                                                     |
| Automation                       | 非表示・統合候補 | 未実装である                                                                                     |
| Track別Lane                      | 非表示・統合候補 | 単一LaneのためTrack構造が視覚化されない                                                          |

## 3. 現状の問題点

### 3.1 実状態を信頼できない

固定Meter、固定位置、架空の最近の制作資産、動作しないボタンが実動UIと同じ外観で表示される。利用者は、表示が現在の制作状態なのか、将来像の見本なのかを判断できない。

UIに表示する状態は次のいずれかに限定する。

- 実データと操作可能な機能
- 未接続であることと理由が明確な無効状態
- 次に行える操作を示す正直なEmpty State

### 3.2 最重要フローが画面をまたいで分断されている

「入力を選ぶ、VST3を挿す、加工音を聞く」というPlayの中核フローが、Homeの機器設定、LibraryのPlugin一覧、PlayのRackへ分断されている。Plugin画面も存在しないため、利用者が音を出すまでの順序を画面から理解できない。

### 3.3 Homeの役割が曖昧である

Homeは入口であるにもかかわらず、Play、Design、Arrangeと同じ最上位タブにあり、Library、Inspector、Transportも常時表示される。開始、継続、復旧、設定の入口と、制作中の操作領域が分離されていない。

### 3.4 Designの機能構造が見えない

Sample、Analyze、Separateへの明示的な入口がない。Designへ移動して何ができるかを理解できず、Libraryの操作や保存済み`activeTool`に依存する。波形編集、信号生成、音源化、分析、参照比較、分離を一つのDesign領域で扱うという構想が、画面構造として表現されていない。

### 3.5 Libraryが未完成機能を強く表示する

八カテゴリを同じ強さで表示する一方、固有UIがあるのはPlugins、Racks、Recordingsである。未完成カテゴリは製品の機能に見えるため、利用者の期待と実装をずらす。

### 3.6 Inspectorが選択対象を所有しない

Asset、Recording、Plugin、Rack、Track、Clip、Padの選択が統一されていない。編集フォームが各画面へ分散し、InspectorはSessionとPluginの固定情報に留まる。

### 3.7 Transportが時間と再生対象を表現しない

位置、BPM、拍子が固定され、Previous Positionは動かない。Playの入力監視、Asset Preview、ArrangeのTimeline再生が同じTransportに集約されているが、現在の再生対象が分からない。

### 3.8 制作操作が管理フォームになっている

ArrangeのClip編集、Track Mix、MIDI編集、Rack保存、Asset Metadata編集が数値入力とPromptへ偏っている。音と時間を直接操作するDrag、Resize、Inline編集、選択、比較が不足している。

### 3.9 複雑さの開示順が逆転している

Audio Driver詳細、AI Context、Reference比較、Stem Render、Missing Dependencyなどの高度な情報が、入力、Plugin、録音という基本操作と同じ視覚強度で並ぶ。初めて音を出す利用者と、詳細を調整する利用者の導線が分かれていない。

### 3.10 用語と表示言語が利用目的を説明しない

日本語と英語、製品用語と実装用語が混在する。`ENGINE NEXT`、`CHANNEL SPLIT FALLBACK`、`Asset Memory`のような内部状態が、利用者の目的より前に出ている。

### 3.11 UIの完成感と検証状態が一致しない

MIDI、Common Parameter View、Arrange、Sample、AIなどは完成した画面に見える一方、挙動確認では実機未確認の項目が多い。画面の視覚的な完成度を、機能の完成証拠として扱わない構造が必要である。
