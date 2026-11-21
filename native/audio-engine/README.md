# Riffra オーディオエンジン

リアルタイム音声を担当する C++ / JUCE 製のサイドカー群。Tauri プロセスと Rust Host は音声コールバックやサードパーティのプラグインコードを直接実行せず、すべての発音・録音・監視はこのエンジンに委ねる。Rust 側は JSON Lines で命令を送り、状態とメーターを受け取る。

- 言語・基盤: C++20、JUCE 9、CMake
- 内蔵音源: Sonalloy C API（`CMakeLists.txt` の既定バージョンに追従）とビルトインリソースバンドル
- 通信: 子プロセスの stdin / stdout 上の JSON Lines（1 コマンド = 1 行、1 応答 = 1 行）。stdout はプロトコル専用で、VST3 をロードするモードでは起動時にプロセス自身の出力は stderr へ切り離す

関連する設計は [`docs/architecture.md`](../../docs/architecture.md) と [`docs/audio-engine.md`](../../docs/audio-engine.md)、通信契約の正本は [`docs/ipc.md`](../../docs/ipc.md) を参照。この README はネイティブ側の構成と振る舞いを日本語でまとめたものである。

## 実行形態

3 つの音声サイドカー、Sonalloy CLI、2 つの静的ライブラリから構成される。

| 成果物               | 種別           | 役割                                                                                                                                               |
| -------------------- | -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `riffra-audio`       | サイドカー     | リアルタイム音声の本体。デバイスを開き、タイムライングラフを再生し、録音・プレビュー・MIDI を扱う                                                  |
| `riffra-plugin-scan` | サイドカー     | VST3 の列挙と読み込み検証。スキャン結果を型タグ付き JSON Lines で返す                                                                              |
| `riffra-render`      | サイドカー     | オフライン書き出し専用ワーカー。stdin の 1 行要求から WAV を生成する                                                                               |
| `sonalloy`           | CLI            | Instrument の作成・検証・Inspect・Render・Auditionを提供する同梱コマンド                                                                           |
| `riffra-engine-core` | 静的ライブラリ | デバイス非依存の再生基盤（タイムライン、プラグイン、楽器、録音、プレビュー、レンダー）。`riffra-render` と `riffra-serve-core`、テストで共有する   |
| `riffra-serve-core`  | 静的ライブラリ | デバイス接続側の基盤（パイプライン、デバイス制御、命令配送、ライフサイクル実行器）。`riffra-engine-core` の上に積み、`riffra-audio` とテストが使う |

`riffra-audio` の起動モードは 3 つある。

| 起動引数                                                                                                                                                   | 動作                                                                                                                                           |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `--probe`                                                                                                                                                  | 音声ストリームを開かずに、プラットフォームのオーディオドライバとデバイスを列挙して 1 行の JSON で返す。Windows は ASIO / WASAPI、Linux は ALSA |
| `--probe-channels --audio-driver <driver> [--input-device ...] [--output-device ...]`                                                                      | 指定デバイスのチャンネル構成を返す                                                                                                             |
| `--serve [--parent-pid ...] [--audio-driver ...] [--input-device ...] [--input-channel ...] [--output-device ...] [--sample-rate ...] [--buffer-size ...]` | 指定デバイスを `EngineTransition` ミュート状態で開き、stdin から JSON コマンドを 1 行ずつ受け付ける常用モード                                  |

`riffra-render` は起動ごとに 1 件の `renderTimelineOffline` 要求を stdin から読み、WAV を書き出して `offlineRenderComplete` か構造化エラーを 1 行で返す。

- プラグインの生成と状態適用はメインスレッド（JUCE メッセージスレッド）で行い、書き出しは別スレッドで実行する。一部の VST3 プラグインは準備をメッセージスレッド経由で完了させるため、書き出し中もメインスレッドはメッセージを処理し続ける
- プラグインは VST3 のオフライン処理モードで準備する
- ディスクストリーミング型サンプラーなど、生成後も読み込みを続けて準備完了を通知しないプラグインがあるため、VST3 プラグインを含むグラフは書き出し前に無音を実時間の速さで 2 秒処理し、その処理で進んだプラグインの状態をリセットしてから書き出す

## 全体構造

`--serve` 実行時のデータの流れは次の通り。

```text
stdin (JSON Lines)
  → AudioCommandDispatcher → 各コマンド群 → 所有者（デバイス / タイムライン / 録音 / MIDI …）
  → AudioRenderPipeline::processBlock（オーディオコールバック）
      → TimelineEngine::mix（グラフ再生）
      → PreviewEngine（試聴ボイス）
      → 安全チェーン（ミュート / ゲイン / DC除去 / リミッター）
  → stdout（応答・状態・メーター・イベントの JSON Lines）
```

所有の考え方は「状態遷移はその状態の所有者だけが行い、呼び出し側は所有者をまたいで内部状態に触らない」である。

| 所有者                   | 責務                                                                                                                                                                   |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AudioEngine`            | `--serve` 全体の寿命管理。長寿命オブジェクトの保持、状態・メーターの定期配信、親プロセス監視                                                                           |
| `AudioCommandDispatcher` | stdin の命令読み取りと振り分け、命令をまたぐ編集状態（トラックのエディタ、試奏、テイク比較、タイムライン操作の進行）の所有                                             |
| `AudioDeviceController`  | 稼働中の JUCE デバイスの開閉・復旧・切替と遷移状態。探索だけなら `AudioDeviceService` が担当する                                                                       |
| `AudioRenderPipeline`    | デバイスコールバック内の描画順序と安全チェーン（メーター、プレビュー、録音制御、ミュート理由、ゲイン、リミッター）                                                     |
| `TimelineEngine`         | 準備済みアレンジグラフ、トランスポート時計、録音窓、グラフ公開。計時と発音の中核                                                                                       |
| 録音クラス群             | `RecordingController` が `pipeline/` で停止ワーカーとパイプライン境界の録音制御を持ち、`recording/` の各セッションが断片・manifest・オフライン確定を持つ               |
| プラグインクラス群       | `PluginRack` / `PluginChain` が処理と状態を持ち、`PluginEditorHost` がエディタと状態送出を持つ。サードパーティ呼び出しの直列化は `app/RuntimeLifecycleExecutor` が担う |
| MIDI クラス群            | `MidiInputService` が物理 MIDI 入力の開閉を持ち、`MidiScheduler` がタイムラインイベントをコールバック内のサンプル位置へ展開する                                        |

## ディレクトリ構成

```text
native/audio-engine/
├── CMakeLists.txt
├── build.ps1 / build.sh
├── .clang-format / .clang-tidy / tsan.supp
├── README.md
├── src/          製品コード（下表）
├── sidecars/     3つのサイドカーの main
└── tests/        GTest と、実プラグイン経路の検証
```

### `src/`

下から順に並べる。下の層が上の層を include しない。同じディレクトリのヘッダは名前だけで、別のディレクトリのヘッダは `src/` からのパスで include する。

| 層  | ディレクトリ   | 責務                                                                                               | 主な構成要素                                                                                                                                                                                                                                                                                                                                                                                           |
| --- | -------------- | -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 0   | `concurrency/` | 実時間スレッド間の受け渡し部品。ドメインを知らない                                                 | `BoundedMpmcQueue`、`RealtimeCommandQueue`、`RetireQueue`、`SeqLockFrame`                                                                                                                                                                                                                                                                                                                              |
| 0   | `contract/`    | サイドカー境界の型・定数と、JSON との相互変換・検証。他のディレクトリに依存しない                  | `ContractReader`、`ExecutionGraph`、`ExecutionGraphDecoder`、`LoadProgress`、`InstrumentPreview`、`SidecarCommands`、`SidecarMessages`                                                                                                                                                                                                                                                                 |
| 1   | `protocol/`    | JSON Lines の入出力（stdout の予約、出力レーン、要求への応答）                                     | `AudioProtocol`（制御・状態・テレメトリの出力）、`CommandResponder`、`OutputQueue`、`ProtocolChannel`（プロトコル用 stdout の予約と切り離し）                                                                                                                                                                                                                                                          |
| 1   | `automation/`  | 準備済みオートメーションレーンと、その実時間評価                                                   | `AutomationRuntime`（レーンと前方専用カーソル）、`MixAutomation`（ゲインとパンのサンプル精度評価）                                                                                                                                                                                                                                                                                                     |
| 2   | `plugins/`     | VST3 の読み込み・処理・状態・パラメータ・エディタ、エフェクト列                                    | `PluginRack`、`PluginChain`、`PluginEditorHost`、`PluginTransportPosition`、`FaultInjection`                                                                                                                                                                                                                                                                                                           |
| 3   | `instruments/` | 楽器の実行時抽象（VST3 / Sonalloy）                                                                | `InstrumentRuntime`、`Vst3InstrumentRuntime`、`SonalloyInstrumentRuntime`                                                                                                                                                                                                                                                                                                                              |
| 3   | `recording/`   | 録音の取り込みと、録音データの書き出し・確定                                                       | `RecordingSession`（raw / processed の二重書きと manifest）、`ArrangeRecordingSession`、`RecordingCaptureRuntime`、`ArrangementCaptureSink`                                                                                                                                                                                                                                                            |
| 4   | `timeline/`    | 準備済みアレンジグラフの構築・公開・再生・ミックス                                                 | `TimelineEngine`（機能別に `TimelineEngine{Devices,Midi,Mix,Recording}.cpp` へ分割）、`TimelineSnapshotBuilder`、`PreparedTimeline`、`ArrangementGraph`、`ControlGraphRegistry`、`MixerRuntime` / `MixRouteDelay`、`TrackRuntime`、`TrackProcessingPool`、`MidiScheduler`、`MidiSourceRegistry`、`MetronomeClicks`、`TimelineReadAheadSource`、`TimelineTimebase`、`RealtimeCommand` / `RealtimeFrame` |
| 5   | `render/`      | オフライン書き出し                                                                                 | `OfflineRenderer`                                                                                                                                                                                                                                                                                                                                                                                      |
| 5   | `preview/`     | Project の外で鳴らす音（サンプル試聴、テイク比較、内蔵楽器プレビュー、プラグイン試奏、簡易シンセ） | `PreviewEngine`、`InstrumentPreviewSession`、`PluginAudition`                                                                                                                                                                                                                                                                                                                                          |
| 6   | `pipeline/`    | オーディオコールバック内の描画順序と安全チェーン、計測、アレンジ録音の制御                         | `AudioRenderPipeline`（描画順序の正本）、`AudioSafetyDsp`（DC ブロッカーとフィードバック検知器）、`AudioMetrics`（ピーク・リミッター診断・コールバック計測）、`RecordingController`（停止ワーカーと録音の確定受け渡し）                                                                                                                                                                                |
| 7   | `device/`      | 物理 I/O の境界（オーディオデバイスと MIDI 入力の探索・開閉・切替、MIDI の振り分け）               | `AudioDeviceController`、`AudioDeviceService`、`AudioDeviceCallback`、`AudioConfiguration`、`MidiInputService` / `MidiMonitor`                                                                                                                                                                                                                                                                         |
| 8   | `app/`         | `--serve` の制御層（寿命管理、命令の受付と処理、状態投影、ライフサイクル実行器）                   | `AudioEngine`（寿命管理）、`AudioCommandDispatcher` + `commands/`（機能別の命令処理）、`AudioStatusBuilder`（状態・メーター投影）、`RuntimeLifecycleExecutor`（サードパーティ呼び出しの直列実行と番犬）                                                                                                                                                                                                |

依存の向きは層の順に限る。`concurrency/` と `contract/` は他のディレクトリに依存しない。`protocol/` は `contract/`、`plugins/` は `concurrency/` / `contract/` / `automation/`、`instruments/` は `plugins/` までを使う。`timeline/` は `automation/` / `plugins/` / `instruments/` / `recording/` を、`render/` は `contract/` / `timeline/` を、`preview/` は `plugins/` / `instruments/` を使う。`pipeline/` は `timeline/` / `preview/` / `recording/`、`device/` は `pipeline/` / `timeline/` / `preview/` を使い、`app/` と `sidecars/` はすべてを使える。`contract/` と `protocol/` は実行側のヘッダに依存せず、`plugins/` は `timeline/` に依存しない。

### `sidecars/`

| パス                                      | 実行ファイル         | 責務                                                                                      |
| ----------------------------------------- | -------------------- | ----------------------------------------------------------------------------------------- |
| `sidecars/audio/AudioMain.cpp`            | `riffra-audio`       | 起動引数（`--probe` / `--probe-channels` / `--serve`）の解釈と、`app/AudioEngine` の起動  |
| `sidecars/render/RenderMain.cpp`          | `riffra-render`      | stdin の 1 行要求を `OfflineRenderer` に渡し、結果か構造化エラーを 1 行で返す             |
| `sidecars/plugin-scan/PluginScanMain.cpp` | `riffra-plugin-scan` | VST3 バンドルの記述列挙と、`PluginRack` と同じ読み込み経路による検証（`--validate-load`） |

### `tests/`

製品構成に対応する GTest 群（`riffra-audio-tests`）と、実プラグイン経路の検証手段を持つ。テストファイルは `tests/<ディレクトリ>/<対象>Test.cpp` とする。

| ディレクトリ         | 内容                                                                                                                                |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `tests/app/`         | 命令振り分け、状態投影、サードパーティ呼び出しの直列実行                                                                            |
| `tests/automation/`  | オートメーションの評価                                                                                                              |
| `tests/concurrency/` | 有界キューとフレーム交換                                                                                                            |
| `tests/contract/`    | 契約の decode / encode とフィクスチャ                                                                                               |
| `tests/device/`      | デバイス制御、デバイス探索、物理 MIDI 入力                                                                                          |
| `tests/instruments/` | 内蔵楽器ランタイム（ベンチマーク含む）                                                                                              |
| `tests/pipeline/`    | 描画パイプラインと安全 DSP、計測                                                                                                    |
| `tests/plugins/`     | ラック・チェイン・エディタ。実 VST テスト（`RealVstRuntimeTest`）はテスト用 VST3 と `riffra-plugin-scan --validate-load` を併用する |
| `tests/preview/`     | サンプル試聴、内蔵楽器プレビュー、プラグイン試奏                                                                                    |
| `tests/protocol/`    | プロトコル出力キューと要求応答                                                                                                      |
| `tests/recording/`   | 録音セッション                                                                                                                      |
| `tests/render/`      | オフライン書き出し                                                                                                                  |
| `tests/support/`     | 複数のディレクトリが使うテスト支援と、テスト用 VST3 のソース                                                                        |
| `tests/timeline/`    | グラフ・自動化・スナップショット・再生・録音・デバイス・MIDI・ミックス                                                              |

## 通信契約

詳細なコマンド表と応答形式の正本は [`docs/ipc.md`](../../docs/ipc.md) の「境界 C・D・E」を参照。ここでは分類だけを示す。

| 分類                  | コマンド例                                                                                                                        |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| 状態照会              | `status`、`meterStatus`                                                                                                           |
| 投影                  | `prepareTimelineSnapshot`、`commitTimelineSnapshot`、`discardTimelineSnapshot`、`waitForTimelineIdle`                             |
| トランスポート        | `playTimeline`、`stopTimeline`、`seekTimeline`、`setTransportStarting`                                                            |
| デバイス・安全        | `recoverAudioDevice`、`setAudioDriver`、`setEmergencyMute`、`setFeedbackProtection`、`setEngineTransitionMute`、`setMasterGainDb` |
| トラック / プラグイン | `setTrackDeviceBypassed`、`setTrackDeviceParameter`、`openTrackPluginEditor`                                                      |
| 録音                  | `startArrangeRecording`、`stopArrangeRecording`                                                                                   |
| プレビュー            | `previewSample`、`previewInstrument`、`stopPreview`、`stopInstrumentPreview`、`stopPreviewForKey`                                 |
| テイク比較            | `startTakeComparison`、`switchTakeComparisonVariant`、`stopTakeComparison`                                                        |
| MIDI                  | `enableMidiListening`、`disableMidiListening`、`sendTrackMidi`、`panicTrackMidi`                                                  |

成功応答は状態（`audioStatus`）、メーター（`audioMeters`）、または操作固有のACK（`timelineIdleAck`、`trackMixAck`、`midiAck`）を返し、失敗応答は `type: "error"` に `kind`、`message`、`operation`、オブジェクト値の `details` を持つ。`audioMeters` は Runtime 投影の `projectId` と、Master / Track のメーター値を含む。状態変化の通知（`transportStatus`、`recordingComplete`、`trackPluginStateChanged` など）は応答とは別に流れる。コマンド名・応答型・エラー形式・状態項目・メーター項目・ミュート理由の所有則は互換性として保つ。

## スレッドとリアルタイム制約

| スレッド                        | 担う処理                                                                                                         |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| デバイスライフサイクル / 制御側 | デバイスの開始・停止・復旧と、命令読み取りが行う設定・復旧呼び出し                                               |
| JUCE メッセージスレッド         | JUCE メッセージの処理と、`RuntimeLifecycleExecutor` が回すサードパーティのライフサイクル処理                     |
| オーディオコールバック          | `audioDeviceIOCallbackWithContext` → `AudioRenderPipeline::processBlock` → `TimelineEngine` の混合と安全チェーン |
| 命令読み取り                    | stdin を 1 行ずつ読み `AudioCommandDispatcher` を呼ぶ。重い VST 処理は直接呼ばず実行器へ積む                     |
| 実行器ワーカーと番犬            | プラグイン・タイムライン処理の直列実行と期限監視。期限超過はプロセス終了による回復へつなげる                     |
| MIDI コールバック               | デバイス MIDI を受け、有界・非阻止でプレビューとタイムラインへ渡す                                               |
| 状態・監視                      | メーターとトランスポートの定期配信（50 ms）、MIDI 装置の変化確認と親プロセス監視（1 s）                          |

オーディオコールバックから到達するコードは、次の規則をすべて守る。

- 確保しない
- 阻止待機しない
- デバイスの開閉をしない
- プラグインの生成・破棄をしない
- ファイル入出力しない
- JSON と stdout を扱わない

グラフ・バッファ・プラグイン実体・録音セッションは制御側で事前に準備し、コールバックは用意済みの状態交換と有界テレメトリだけを行う。

## 安全チェーン

出力段の保護は小さく監査しやすい構成に絞っている。順序は `AudioRenderPipeline::processBlock` が正本である。

- 所有者別ミュート（`UserEmergency` / `EngineTransition` / `DeviceFault` / `FeedbackProtection`）。各所有者は自分の bit だけを解除する
- デバイス・正準グラフの両方が整うまで `EngineTransition` を保ち、整ってから解除する。遷移解除時は 50 ms のフェードインをかける
- 非有限サンプルの検出と除去、マスターゲイン（既定 0 dB）の適用
- DC オフセット除去、リミッター処理、最終上限 0.98 による硬性制限
- ソフトウェア監視中の持続するニアピーク入力に対するフィードバック検知。検知中は `FeedbackProtection` がかかり、`feedbackSuspected` として報告される
- 診断の報告: リミッター前ピーク、リミッターのゲイン低減量、硬性制限サンプル数、コールバック超過、無効サンプル数、グラフ診断

## ビルドと検証

前提は CMake 3.22 以上、C++20 に対応する toolchain、Rust / Cargo、Node.js である。`npm install` は不要で、このディレクトリ単体でビルドできる。

```powershell
# Windows
.\build.ps1 -Configuration Debug
```

```bash
# macOS / Linux
./build.sh Debug
```

ラッパーは configure、音声サイドカーと Sonalloy CLI の build、CTest、`cmake --install`（Tauri 用とヘッドレス用の配置）までを一括で行う。主な選択肢は次の通り。

| 構成             | ネイティブ                           | Sonalloy の Cargo プロファイル | ヘッドレス配置先  |
| ---------------- | ------------------------------------ | ------------------------------ | ----------------- |
| `Debug`          | 記号つき検査用                       | `dev`                          | `target/debug/`   |
| `RelWithDebInfo` | 最適化つき記号あり（常用の開発構成） | `release`                      | `target/debug/`   |
| `Release`        | 配布用                               | `release`                      | `target/release/` |

- 既定の Visual Studio 生成子は `Visual Studio 17 2022` と `x64`。`-Generator` / `-Architecture` で変更できる
- `-SidecarsOnly`（`build.sh` は `SIDECARS_ONLY=1`）でテスト用ターゲットを除く音声サイドカーと Sonalloy CLI だけを build する。開発起動の ensure 処理もこの形態を使う
- クロスコンパイル時は `-DRIFFRA_TARGET_TRIPLE=<triple>` を CMake に渡す。既定はホストに従う
- 配置先: Tauri 用は `apps/desktop/src-tauri/binaries/<名前>-<triple>[.exe]`、ヘッドレス用は接尾辞なし複製。`RIFFRA_HEADLESS_BINARIES_DESTINATION` で変更できる
- ビルトイン音源バンドルも同時に配置される（Tauri 用 `apps/desktop/src-tauri/resources/instruments/builtin/` とヘッドレス用 `riffra-resources/instruments/builtin/`、`RIFFRA_HEADLESS_RESOURCES_DESTINATION` で変更可）。配置先以外のバンドルを使う場合は `RIFFRA_BUILTIN_INSTRUMENTS_ROOT` に `instruments/builtin` の場所を指定する

個別に実行する場合は次の通り。

```powershell
cmake -S native/audio-engine -B native/audio-engine/build
cmake --build native/audio-engine/build --config Debug --parallel
ctest --test-dir native/audio-engine/build -C Debug --output-on-failure
```

C++ の整形は `clang-format`、静的解析は `.clang-tidy` に従う。変更時の互換条件（命令・応答・安全チェーンの数値・テレメトリ周期・ライフサイクルの上限時間など）を崩す変更はしない。
