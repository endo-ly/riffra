# Riffra 挙動確認・課題管理表

この表は、[製品挙動要件](./behavior-requirements.md) に定義した各要求を、Nativeアプリで確認し、修正し、再確認するための作業表です。要求の意味や期待結果は要件文書に置き、ここでは判定と証跡だけを管理します。

## 判定

| 判定 | 意味 |
| --- | --- |
| 未確認 | 条件から結果までNativeアプリで確認していない |
| 不成立 | Native操作で期待結果と異なることを確認した |
| 修正中 | 原因を特定し、コードまたはデータを修正している |
| 再確認待ち | 修正と自動回帰は済んだが、Native再操作が残っている |
| 適合 | Native再操作、画面・音声・保存データの確認、自動回帰が完了 |
| 保留 | 外部Device、ライセンス、仕様判断などの決定待ち |

## 優先度

- **P0**: 録音・Project・Audioの消失、危険な音声出力、主要導線の停止につながる挙動
- **P1**: 主要機能の状態不整合、操作結果が画面や保存データへ反映されない挙動
- **P2**: 表示、ショートカット、境界条件、使い勝手

## 管理表

| ID | 優先度 | 対象挙動 | 判定 | 根拠・修正 | 再確認手順 |
| --- | --- | --- | --- | --- | --- |
| `G-001` | P0 | Scratch Sessionを即時に開く | 適合 | 2026-07-13 Cold startで作成ダイアログなしにUntitled Scratchが開き、current.jsonのSession IDと自動保存を確認 | 初回データなし環境の回帰時に再確認 |
| `G-002` | P0 | 単一ウィンドウで状態を共有する | 再確認待ち | 既知の修正後。Native再確認を未完了 | Workspace往復でSession/Selection/Transport/Muteを照合 |
| `G-003` | P0 | 起動時は安全にMuteされる | 未確認 | 2026-07-13 Cold start、再起動、Driver変更後にMUTED表示とemergencyMuted=trueを確認。Recover後の再MuteとFade-inは未確認 | Recover device後にAudio status、出力、Fade-inを照合 |
| `G-004` | P0 | Emergency Muteを全Workspaceから実行できる | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `G-005` | P0 | 失敗範囲を限定する | 再確認待ち | 既知の修正後。Native再確認を未完了 | 失敗操作で影響範囲と復旧導線を確認 |
| `G-006` | P1 | Empty/Loading/Errorを区別する | 再確認待ち | 既知の修正後。Native再確認を未完了 | Empty/Loading/Errorの表示と次操作を確認 |
| `G-007` | P1 | Focus Modeは表示だけを整理する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `FLOW-001` | P0 | 起動して演奏する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `FLOW-002` | P0 | 音色候補を比較する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `FLOW-003` | P0 | 思いつきを録る | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `FLOW-004` | P0 | 録音をArrangeへ進める | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `FLOW-005` | P1 | AudioからSampleを作る | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `FLOW-006` | P1 | AI案を確認して適用する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `FLOW-007` | P0 | 障害から復旧する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AUD-001` | P0 | Driverを列挙する | 適合 | 2026-07-13 Native HomeでWindows Audio 3/7、Exclusive 3/7、Low Latency 3/7、DirectSound 4/8、ASIO 8/8を確認 | Device構成変更時に再列挙 |
| `AUD-002` | P0 | Sample Rate/Bufferを適用する | 適合 | 2026-07-13 Native releaseでHome下部へ到達し、Exclusive切替、48 kHz/480 samplesの実効値、Session保存、再起動後の保持を確認。未対応の64 samples要求は実効480 samplesへ戻り、理由をStatusへ表示 | 別Audio Interface接続時に対応Rate/Bufferでも回帰確認 |
| `AUD-003` | P0 | Input/OutputをMeter表示する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AUD-004` | P0 | Device切断を安全に処理する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AUD-005` | P1 | Plugin/ParallelのLatencyを補償する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AUD-006` | P0 | 異常値とFeedbackを保護する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `MIDI-001` | P0 | MIDI Portを検出・開閉する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `MIDI-002` | P1 | MIDIイベントを処理する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `MIDI-003` | P0 | Panicで全Noteを止める | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PLG-001` | P0 | VST3 Scanを隔離実行する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PLG-002` | P1 | Plugin Browserを検索する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PLG-003` | P0 | PluginをLoad/Bypass/Removeする | 適合 | 2026-07-13 Native releaseでMarshallをLoadし、Play/Inspector/Nativeが一致。Bypass表示、Remove後のRack Empty、再起動後の空Rackを確認 | 回帰時に別VST3でも同じ操作を実行 |
| `PLG-004` | P1 | Common Parameter Viewを操作する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PLG-005` | P0 | Plugin Stateを完全保存・復元する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `RACK-001` | P1 | Serial/Parallel Signal Flowを表示する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `RACK-002` | P1 | Rackを編集・再利用する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `RACK-003` | P1 | Macroを複数ParameterへMapする | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `RACK-004` | P1 | Snapshotを複数保存・比較する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `RACK-005` | P2 | Tone Explorationを安全に行う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `RACK-006` | P1 | Freeze/Render fallbackを作る | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `REC-001` | P0 | Raw/Processedを同時録音する | 再確認待ち | 既知の修正後。Native再確認を未完了 | 既存Inboxのcompleted、sample数、Placeを確認 |
| `REC-002` | P1 | Count-in/Pre-roll/Punch/Loopを扱う | 再確認待ち | 既知の修正後。Native再確認を未完了 | 4 beat Count-inの実時間と録音範囲を確認 |
| `REC-003` | P1 | Take Groupを管理する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `REC-004` | P0 | 録音異常時に取得済みデータを保全する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `ARR-001` | P0 | Audio/MIDIを非破壊配置する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `ARR-002` | P1 | Track/Mixerを共有する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `ARR-003` | P1 | MIDI Clipを編集する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `ARR-004` | P1 | Tempo/Marker/Loop Regionを扱う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `ARR-005` | P2 | Automationを編集する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `EXP-001` | P0 | Master/Track/Stemを書き出す | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `EXP-002` | P1 | DAW handoffを行う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SMP-001` | P0 | AudioをSampleへ非破壊Importする | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SMP-002` | P1 | Sample範囲・Loop・Envelopeを編集する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SMP-003` | P1 | Drum Pad/Kitを保存する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SMP-004` | P1 | Keyboard Instrumentを作る | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SMP-005` | P1 | Internal Synthを使う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SMP-006` | P1 | Utility Deviceを使う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `ANL-001` | P0 | WAVを解析する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `ANL-002` | P1 | Referenceを比較する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `ANL-003` | P1 | Referenceを同期Previewする | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SEP-001` | P0 | Separation Jobを実行する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SEP-002` | P0 | Separation Resultを非破壊保存する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AI-001` | P0 | Explainは変更しない | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AI-002` | P0 | SuggestはChangeSetだけを作る | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AI-003` | P0 | Applyは確認済みChangeSetだけを適用する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AI-004` | P0 | Contextを制御する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `AI-005` | P0 | Providerと外部送信を明示する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `LIB-001` | P1 | Asset種別を横断検索する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `LIB-002` | P1 | Metadataを後から編集する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `LIB-003` | P0 | Inboxを保全領域として使う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `LIB-004` | P1 | Related Assetを辿る | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PRJ-001` | P0 | Project内容を完全に保存する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PRJ-002` | P0 | Auto Saveを世代管理する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PRJ-003` | P1 | Version/Undo/Redoを操作する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PRJ-004` | P0 | Missing File/Pluginでも開く | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PRJ-005` | P1 | Portable Packageを作る | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `PRJ-006` | P1 | Format Migrationを安全に行う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `REL-001` | P0 | Plugin障害を隔離・復旧する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `REL-002` | P0 | Audio sidecarを孤立させない | 適合 | 親PID監視の回帰確認に加え、2026-07-13にriffra.exeとriffra-audio.exeの起動後、Native終了から4秒以内に両Processと録音lockが消えることを確認 | 異常終了・再起動の回帰時にProcessとlockを再照合 |
| `REL-003` | P0 | Background Jobをキャンセル・再開する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `REL-004` | P0 | Disk障害を扱う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SEC-001` | P0 | Local Firstを守る | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SEC-002` | P0 | Credentialを平文保存しない | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `SEC-003` | P1 | Logを安全に扱う | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `WIN-001` | P1 | DPI/複数モニターで使える | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `WIN-002` | P1 | Settingsを明示的に保存する | 適合 | 2026-07-13 Native releaseでDriver、実効Sample Rate/Bufferをcurrent.jsonへ保存し、再起動後もExclusive・48 kHz・480 samplesが選択されることを確認 | 他設定項目を追加した際に同じ再起動確認を実施 |
| `Q-001` | P0 | Startupを測定する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `Q-002` | P0 | UIをAudioより優先しない | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `Q-003` | P0 | 長時間安定性を確認する | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `DONE-001` | P0 | Product-level DoDを満たす | 未確認 | Native操作未確認 | 条件→操作→期待結果をNativeで確認 |
| `DONE-002` | P0 | 失敗状態を残さない | 再確認待ち | 既知の修正後。Native再確認を未完了 | 録音消失、無音成功、manifest既定値化、孤立sidecarを横断確認 |

## 1件の確認で残すもの

- NativeアプリのBuildまたはCommit
- Workspace、操作順、入力Device、Driver、Sample Rate、Buffer、Plugin
- 期待結果と実際の結果
- 画面上の状態、Audio status、生成ファイル、manifest、Session差分
- 原因、修正ファイル、回帰テスト
- 修正後に同じ操作を再実行した日時と結果

## 更新ルール

1. まず要件文書の該当IDを読み、条件・操作・期待結果を固定する
2. Nativeアプリで確認し、期待結果と異なればこの行を `不成立` にする
3. 原因と修正を記録し、自動回帰を実行する
4. 新しいNativeビルドで同じ操作を再実行する
5. 画面・音声・保存データが一致した場合だけ `適合` にする

P0が残っている間は、新しい見た目の改善や機能追加を優先しない。仕様にない重要な挙動が見つかった場合は、先に要件文書へIDを追加してから、この表に行を追加する。
