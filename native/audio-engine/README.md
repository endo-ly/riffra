# Riffra オーディオエンジン

リアルタイム音声を担当する C++ / JUCE 製のサイドカー群。Tauri プロセスと Rust Host は音声コールバックやサードパーティのプラグインコードを直接実行せず、すべての発音・録音・監視はこのエンジンに委ねる。Rust 側は JSON Lines で命令を送り、状態とメーターを受け取る。

- 言語・基盤: C++20、JUCE 9、CMake
- 内蔵音源: Sonalloy C API（`CMakeLists.txt` の既定バージョンに追従）とビルトインリソースバンドル
- 通信: 子プロセスの stdin / stdout 上の JSON Lines（1 コマンド = 1 行、1 応答 = 1 行）

関連する全体設計は [`docs/architecture.md`](../../docs/architecture.md)、通信契約の正本は [`docs/ipc.md`](../../docs/ipc.md) を参照。この README はネイティブ側の構成と振る舞いを日本語でまとめたものである。

## 実行形態

3 つのサイドカーと 2 つの静的ライブラリから構成される。

| 成果物               | 種別           | 役割                                                                                                               |
| -------------------- | -------------- | ------------------------------------------------------------------------------------------------------------------ |
| `riffra-audio`       | サイドカー     | リアルタイム音声の本体。デバイスを開き、タイムライングラフを再生し、録音・プレビュー・MIDI を扱う                  |
| `riffra-plugin-scan` | サイドカー     | VST3 の列挙と読み込み検証。スキャン結果を型タグ付き JSON Lines で返す                                              |
| `riffra-render`      | サイドカー     | オフライン書き出し専用ワーカー。stdin の 1 行要求から WAV を生成する                                               |
| `riffra-render-core` | 静的ライブラリ | デバイス非依存の再生基盤（タイムライン、プラグイン、録音、レンダー）。`riffra-audio` と `riffra-render` で共有する |
| `riffra-audio-core`  | 静的ライブラリ | デバイス接続側の基盤（命令配送、デバイス制御、パイプライン）。`riffra-render-core` の上に積む                      |

`riffra-audio` の起動モードは 3 つある。

| 起動引数                                                                                                                                                   | 動作                                                                                                                                           |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `--probe`                                                                                                                                                  | 音声ストリームを開かずに、プラットフォームのオーディオドライバとデバイスを列挙して 1 行の JSON で返す。Windows は ASIO / WASAPI、Linux は ALSA |
| `--probe-channels --audio-driver <driver> [--input-device ...] [--output-device ...]`                                                                      | 指定デバイスのチャンネル構成を返す                                                                                                             |
| `--serve [--parent-pid ...] [--audio-driver ...] [--input-device ...] [--input-channel ...] [--output-device ...] [--sample-rate ...] [--buffer-size ...]` | 指定デバイスを `EngineTransition` ミュート状態で開き、stdin から JSON コマンドを 1 行ずつ受け付ける常用モード                                  |

`riffra-render` は起動ごとに 1 件の `renderTimelineOffline` 要求を stdin から読み、WAV を書き出して `offlineRenderComplete` か構造化エラーを 1 行で返す。

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

| 所有者                  | 責務                                                                                                                                                    |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `AudioEngine`           | `--serve` 全体の寿命管理。命令配送、状態・メーターの定期配信、親プロセス監視、長寿命オブジェクトの保持                                                  |
| `AudioDeviceController` | 稼働中の JUCE デバイスの開閉・復旧・切替と遷移状態。探索だけなら `AudioDeviceService` が担当する                                                        |
| `AudioRenderPipeline`   | デバイスコールバック内の描画順序と安全チェーン（メーター、プレビュー、録音制御、ミュート理由、ゲイン、リミッター）                                      |
| `TimelineEngine`        | 準備済みアレンジグラフ、トランスポート時計、録音窓、グラフ公開。計時と発音の中核                                                                        |
| 録音クラス群            | `RecordingController` がパイプライン境界の録音制御を持ち、各セッションが断片・manifest・オフライン確定を持つ                                            |
| プラグインクラス群      | `PluginRack` / `PluginChain` が処理と状態を持ち、`RuntimeLifecycleExecutor` / `PluginEditorHost` がサードパーティのライフサイクルとエディタを直列化する |
| MIDI クラス群           | `MidiInputService` が物理 MIDI 入力の開閉を持ち、`MidiScheduler` がタイムラインイベントをコールバック内のサンプル位置へ展開する                         |

## ディレクトリ構成

```text
native/audio-engine/
├── CMakeLists.txt      # JUCE / Sonalloy / GTest の取得、5 ターゲットの定義、sidecar の install 規則
├── build.ps1 / build.sh # configure → build → CTest → install の一括実行ラッパー
├── src/                # 製品コード（下表）
├── tools/              # plugin-scan / render のエントリーポイント
└── tests/              # GTest 群とテスト用 VST プラグイン（製品構成に対応）
```

### `src/`

| ディレクトリ                | 責務                                                                       | 主な構成要素                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| --------------------------- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/app/`                  | `--serve` の制御側。命令の受付・振り分けと、状態投影・録音制御の取りまとめ | `AudioEngine`（寿命管理）、`AudioCommandDispatcher` + `commands/`（機能別コマンド群: デバイス / タイムライン / トラックデバイス / 録音 / プレビュー / MIDI）、`CommandRouting`（コマンド名→分類）、`AudioStatusBuilder`（状態・メーター投影）、`RecordingController`（アレンジ録音の開始・停止・確定受け渡し）                                                                                                                                                                                                                        |
| `src/protocol/`             | JSON Lines 入出力の共通部品                                                | `writeJson`（制御・状態・テレメトリの出力）、`makeError`（構造化エラー）、`parseMidiBytes`、要求 ID とドロップ計数                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `src/device/`               | オーディオデバイスの境界                                                   | `AudioDeviceController`（開閉・復旧・切替・遷移）、`AudioDeviceService`（探索と設定値の組み立て）、`AudioDeviceCallback`（JUCE コールバックからパイプラインへの薄い適配）、`AudioConfiguration`（起動・切替設定）                                                                                                                                                                                                                                                                                                                     |
| `src/audio/`                | コールバック内の描画と安全処理                                             | `AudioRenderPipeline`（描画順序の正本）、`AudioSafetyDsp`（DC ブロッカーとフィードバック検知器）、`PreviewEngine`（試聴ボイスと簡易シンセ）、`AudioMetrics`（ピーク・リミッター診断・コールバック計測）                                                                                                                                                                                                                                                                                                                               |
| `src/timeline/`             | アレンジ再生の中核                                                         | `TimelineEngine`（スナップショットの二重化公開・トランスポート・録音窓・`mix`、機能別に `TimelineEngine{Devices,Recording,Render}.cpp` へ分割）、`TimelineSnapshotBuilder`（適用前の検証と事前構築）、`ArrangementGraph`（入出力経路・MIDI 経路・補償遅延・取り込み範囲の純粋計算）、`MidiScheduler`（MIDI の事前展開とブロック内配置）、`TrackRuntime`（トラック単位の DSP 状態・遅延補償・自動化・取り込み）、`AutomationRuntime`（自動化レーンと前方専用カーソル）、`TimelineTimebase`（tick⇔sample 換算）、`instruments/`（下記） |
| `src/timeline/instruments/` | 楽器の実行時抽象                                                           | `InstrumentRuntime`（VST3 / 内蔵共通の再生 IF）、`Vst3InstrumentRuntime`（`PluginRack` 背負いの VST3 楽器）、`SonalloyInstrumentRuntime`（Sonalloy C API 駆動の内蔵楽器）                                                                                                                                                                                                                                                                                                                                                             |
| `src/plugins/`              | エフェクトと VST ライフサイクル                                            | `PluginRack`（単体プラグインの読み込み・処理・状態・MIDI 受付）、`PluginChain`（トラック内エフェクト列）、`RuntimeLifecycleExecutor`（サードパーティ呼び出しの直列実行と番犬）、`PluginEditorHost`（エディタ表示と状態・パラメータ変化の送出）、`FaultInjection`（環境変数によるテスト用障害挿入）                                                                                                                                                                                                                                    |
| `src/midi/`                 | 物理 MIDI 入力の境界                                                       | `MidiInputService`（デバイス開閉・再開・集合変化検知）、`MidiMonitor`（JUCE MIDI スレッドからプレビューとタイムラインへの非阻止転送）                                                                                                                                                                                                                                                                                                                                                                                                 |
| `src/recording/`            | 録音データの書き出しと確定                                                 | `RecordingSession`（raw / processed の二重書きと manifest）、`ArrangeRecordingSession`（アレンジ録音の確定単位）、`RecordingCaptureRuntime`（リアルタイム取り込み）、`ArrangementCaptureSink`（取り込み先 IF）                                                                                                                                                                                                                                                                                                                        |
| `src/render/`               | オフライン書き出し                                                         | `OfflineRenderer`（スナップショットと tick 範囲から WAV を生成。`riffra-render` と録音後処理で共有）                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `src/concurrency/`          | 実時間スレッド間の受け渡し                                                 | `BoundedMpmcQueue`（確保なし・待機なしの固定容量 MPMC キュー。MIDI などの有界転送の土台）                                                                                                                                                                                                                                                                                                                                                                                                                                             |

### `tools/`

| ディレクトリ            | 責務                                                                                                                         |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `tools/plugin-scanner/` | `riffra-plugin-scan` の本体。VST3 バンドルの記述列挙と、`PluginRack` と同じ読み込み経路による検証（`--validate-load`）を行う |
| `tools/render/`         | `riffra-render` の本体。stdin の 1 行要求を `OfflineRenderer` に渡し、結果か構造化エラーを 1 行で返す                        |

### `tests/`

製品構成に対応する GTest 群（`riffra-audio-tests`）と、実プラグイン経路の検証手段を持つ。

| ディレクトリ         | 内容                                                                                                                                                                                      |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tests/app/`         | 命令振り分けの回帰                                                                                                                                                                        |
| `tests/protocol/`    | プロトコル部品                                                                                                                                                                            |
| `tests/device/`      | デバイス制御                                                                                                                                                                              |
| `tests/audio/`       | 描画パイプラインと安全 DSP                                                                                                                                                                |
| `tests/timeline/`    | グラフ・自動化・スナップショット・再生・録音・デバイス・MIDI・内蔵楽器（ベンチマーク含む）                                                                                                |
| `tests/recording/`   | 録音セッション                                                                                                                                                                            |
| `tests/plugins/`     | ラック・チェイン・ライフサイクル実行器。実 VST テスト（`RealVstRuntimeTest`）はテスト用 VST3（`tests/support/` の effect / instrument）と `riffra-plugin-scan --validate-load` を併用する |
| `tests/concurrency/` | 有界キュー                                                                                                                                                                                |

## 通信契約

詳細なコマンド表と応答形式の正本は [`docs/ipc.md`](../../docs/ipc.md) の「境界 C・D・E」を参照。ここでは分類だけを示す。

| 分類                  | コマンド例                                                                                                                        |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| 状態照会              | `status`、`meterStatus`                                                                                                           |
| 投影                  | `prepareTimelineSnapshot`、`commitTimelineSnapshot`、`discardTimelineSnapshot`                                                    |
| トランスポート        | `playTimeline`、`stopTimeline`、`seekTimeline`、`setTransportStarting`                                                            |
| デバイス・安全        | `recoverAudioDevice`、`setAudioDriver`、`setEmergencyMute`、`setFeedbackProtection`、`setEngineTransitionMute`、`setMasterGainDb` |
| トラック / プラグイン | `setTrackDeviceBypassed`、`setTrackDeviceParameter`、`openTrackPluginEditor`                                                      |
| 録音                  | `startArrangeRecording`、`stopArrangeRecording`                                                                                   |
| プレビュー            | `previewSample`、`stopPreview`、`stopPreviewForKey`                                                                               |
| テイク比較            | `startTakeComparison`、`switchTakeComparisonVariant`、`stopTakeComparison`                                                        |
| MIDI                  | `enableMidiListening`、`disableMidiListening`、`sendTrackMidi`、`panicTrackMidi`                                                  |

成功応答は `audioStatus` か `audioMeters` を返し、失敗応答は `type: "error"` に `kind`、`message`、`operation`、オブジェクト値の `details` を持つ。状態変化の通知（`transportStatus`、`recordingComplete`、`trackPluginStateChanged` など）は応答とは別に流れる。コマンド名・応答型・エラー形式・状態項目・メーター項目・ミュート理由の所有則は互換性として保つ。

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

ラッパーは configure、3 sidecar の build、CTest、`cmake --install`（Tauri 用とヘッドレス用の配置）までを一括で行う。主な選択肢は次の通り。

| 構成             | ネイティブ                           | Sonalloy の Cargo プロファイル | ヘッドレス配置先  |
| ---------------- | ------------------------------------ | ------------------------------ | ----------------- |
| `Debug`          | 記号つき検査用                       | `dev`                          | `target/debug/`   |
| `RelWithDebInfo` | 最適化つき記号あり（常用の開発構成） | `release`                      | `target/debug/`   |
| `Release`        | 配布用                               | `release`                      | `target/release/` |

- 既定の Visual Studio 生成子は `Visual Studio 17 2022` と `x64`。`-Generator` / `-Architecture` で変更できる
- `-SidecarsOnly`（`build.sh` は `SIDECARS_ONLY=1`）でテスト用ターゲットを除く 3 sidecar だけを build する。開発起動の ensure 処理もこの形態を使う
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
