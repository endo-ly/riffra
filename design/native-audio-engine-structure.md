# ネイティブ音声エンジンのディレクトリ構成と責務

`native/audio-engine` の各ディレクトリが何を持つか、依存の向き、CMake ターゲットとの対応を定める。2〜5章が完成形の構成、6章が構成の変更に伴って移す責務、7章がテスト、8章がビルド構成、9章が文書、10章以降が作業の進め方と守る条件である。

## 1. 目的

- ファイルの置き場所を、内容から1つに決められるようにする。同じ概念を複数のディレクトリに分けない。
- ディレクトリ間の依存を一方向に保つ。下の層が上の層を include しない。
- ディレクトリと CMake のライブラリを1対1に対応させる。「このファイルはどのライブラリに入るか」をディレクトリから決められるようにする。
- 3つのサイドカーの入口を同じ場所・同じ形に揃える。
- `tests/` を `src/` と同じ構成にする。テストファイルの主語と置き場所を一致させる。

この作業では、外から見える振る舞い（命令・応答・イベントの型と内容、安全チェーンの順序と数値、テレメトリの周期、ライフサイクルの上限時間、サイドカーの実行ファイル名と起動引数）を変えない。例外は 6.4 の録音停止の重複エラーだけである。

## 2. トップレベル

```text
native/audio-engine/
├── CMakeLists.txt
├── build.ps1 / build.sh
├── .clang-format / .clang-tidy / tsan.supp
├── README.md
├── src/          製品コード（3章）
├── sidecars/     3つのサイドカーの main（4章）
└── tests/        GTest と、実プラグイン経路の検証（7章）
```

`tools/` は廃止する。中身はすべて製品のサイドカーで、開発用ツールではないためである。

## 3. `src/` の層と依存の向き

### 3.1 ディレクトリ一覧

下から順に並べる。

| 層  | ディレクトリ   | 責務                                                                                               |
| --- | -------------- | -------------------------------------------------------------------------------------------------- |
| 0   | `concurrency/` | 実時間スレッド間の受け渡し部品。ドメインを知らない                                                 |
| 0   | `contract/`    | サイドカー境界の型・定数と、JSON との相互変換・検証。他のディレクトリに依存しない                  |
| 1   | `protocol/`    | JSON Lines の入出力（stdout の予約、出力レーン、要求への応答）                                     |
| 1   | `automation/`  | 準備済みオートメーションレーンと、その実時間評価                                                   |
| 2   | `plugins/`     | VST3 の読み込み・処理・状態・パラメータ・エディタ、エフェクト列                                    |
| 3   | `instruments/` | 楽器の実行時抽象（VST3 / Sonalloy）                                                                |
| 3   | `recording/`   | 録音の取り込みと、録音データの書き出し・確定                                                       |
| 4   | `timeline/`    | 準備済みアレンジグラフの構築・公開・再生・ミックス                                                 |
| 5   | `render/`      | オフライン書き出し                                                                                 |
| 5   | `preview/`     | Project の外で鳴らす音（サンプル試聴、テイク比較、内蔵楽器プレビュー、プラグイン試奏、簡易シンセ） |
| 6   | `pipeline/`    | オーディオコールバック内の描画順序と安全チェーン、計測、アレンジ録音の制御                         |
| 7   | `device/`      | 物理 I/O の境界（オーディオデバイスと MIDI 入力の探索・開閉・切替、MIDI の振り分け）               |
| 8   | `app/`         | `--serve` の制御層（寿命管理、命令の受付と処理、状態投影、ライフサイクル実行器）                   |

### 3.2 依存してよい先

| ディレクトリ  | 依存してよい先                                                                 |
| ------------- | ------------------------------------------------------------------------------ |
| `concurrency` | なし                                                                           |
| `contract`    | なし                                                                           |
| `protocol`    | `contract`                                                                     |
| `automation`  | なし                                                                           |
| `plugins`     | `concurrency`、`contract`、`automation`                                        |
| `instruments` | `concurrency`、`contract`、`plugins`                                           |
| `recording`   | `concurrency`、`contract`                                                      |
| `timeline`    | `concurrency`、`contract`、`automation`、`plugins`、`instruments`、`recording` |
| `render`      | `contract`、`timeline`                                                         |
| `preview`     | `concurrency`、`contract`、`plugins`、`instruments`                            |
| `pipeline`    | `concurrency`、`contract`、`recording`、`timeline`、`preview`                  |
| `device`      | `contract`、`timeline`、`preview`、`pipeline`                                  |
| `app`         | すべて                                                                         |
| `sidecars/*`  | すべて                                                                         |

- 表にない向きの include を作らない。前方宣言で済むときも、向きはこの表に従う。
- `protocol` は `plugins` に依存しない（6.6）。
- `contract` は `preview` などの実行側に依存しない（6.1）。

### 3.3 include の書き方

- 同じディレクトリのヘッダは名前だけで書く（`#include "PluginRack.h"`）。
- 別のディレクトリのヘッダは `src/` からのパスで書く（`#include "contract/ExecutionGraph.h"`）。
- `../` は使わない。今ある3か所（`src/timeline/TimelineTimebase.h`、`src/audio/InstrumentPreviewSession.{h,cpp}`）と `src/app/commands/*.cpp` の `../AudioCommandDispatcher.h` は、`app/AudioCommandDispatcher.h` のような `src/` 起点の形に直す。
- テストも同じ規則に従う。テストのターゲットに `tests/` を include パスとして加え、`#include "support/TestSupport.h"` のように書く（`../support/` を使わない）。
- ファイルを移したら、移した先で不要になった include を消す。`src/app/AudioEngine.cpp` は現状でも `plugins/FaultInjection.h` と、使っていない標準ヘッダ（`<deque>`、`<set>`、`<unordered_map>` など）を include している。

## 4. `sidecars/`

| パス                                      | 実行ファイル         | 旧パス                                       |
| ----------------------------------------- | -------------------- | -------------------------------------------- |
| `sidecars/audio/AudioMain.cpp`            | `riffra-audio`       | `src/app/AudioMain.cpp`                      |
| `sidecars/render/RenderMain.cpp`          | `riffra-render`      | `tools/render/RenderMain.cpp`                |
| `sidecars/plugin-scan/PluginScanMain.cpp` | `riffra-plugin-scan` | `tools/plugin-scanner/PluginScannerMain.cpp` |

- ディレクトリ名は実行ファイル名から `riffra-` を除いたものにする。
- `main` のファイルは起動引数の解釈と、`src/` の部品の呼び出しだけを持つ。`AudioMain.cpp` の `--probe` / `--probe-channels` / `--serve` の分岐はここに残す。

## 5. 各ディレクトリの中身

「旧」の列は現在のパスである。空欄は移動しないもの。

### 5.1 `concurrency/`

移動なし。`BoundedMpmcQueue.h`、`RealtimeCommandQueue.h`、`RetireQueue.h`、`SeqLockFrame.h`。

### 5.2 `contract/`

| ファイル                        | 内容                                                       | 旧                                                                               |
| ------------------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `ContractReader.{h,cpp}`        | 宣言済みのキー集合を強制する JSON 読み取り                 |                                                                                  |
| `ExecutionGraph.h`              | 実行グラフの型                                             |                                                                                  |
| `ExecutionGraphDecoder.{h,cpp}` | 実行グラフ・スナップショット・書き出し要求の decode と検証 |                                                                                  |
| `LoadProgress.h`                | 読み込みの段階と進捗の型                                   |                                                                                  |
| `InstrumentPreview.h`           | 内蔵楽器プレビューの型と上限値・検証関数                   | `audio/InstrumentPreviewContract.h` と `audio/InstrumentPreviewSession.h` の一部 |
| `SidecarCommands.{h,cpp}`       | 命令の型と decode                                          |                                                                                  |
| `SidecarMessages.{h,cpp}`       | 応答・イベントの型と encode                                |                                                                                  |

- `MixerGraphValidation.h` は廃止する。`validateMixerGraph` は `ExecutionGraphDecoder.cpp` の無名名前空間へ移す（6.2）。
- `ExecutionGraph.h` の `sameEffectTopology` / `sameInstrumentTopology` / `sameInstrumentRuntime` は `timeline/TimelineSnapshotBuilder.h` へ移す（6.3）。`contract/` には型と、境界での変換・検証だけを置く。

### 5.3 `protocol/`

移動なし。`AudioProtocol.{h,cpp}`、`CommandResponder.{h,cpp}`、`OutputQueue.h`、`ProtocolChannel.{h,cpp}`。`AudioProtocol.cpp` は `plugins/FaultInjection.h` を include しない（6.6）。

### 5.4 `automation/`（新設）

| ファイル              | 内容                                                            | 旧                             |
| --------------------- | --------------------------------------------------------------- | ------------------------------ |
| `AutomationRuntime.h` | `AutomationRuntime`、`AutomationPreviewValue`、前方専用カーソル | `timeline/AutomationRuntime.h` |
| `MixAutomation.h`     | ゲインとパンのサンプル精度の評価（`MixAutomationCursor`）       | `timeline/MixAutomation.h`     |

`plugins/PluginRack.h` が `timeline/AutomationRuntime.h` を include していて、`plugins` → `timeline` の逆流ができている。オートメーションの評価はトラックのミックスとプラグインのパラメータの両方が使う部品なので、どちらにも属さない層に置く。

### 5.5 `plugins/`

| ファイル                                    | 内容                                                   | 旧  |
| ------------------------------------------- | ------------------------------------------------------ | --- |
| `PluginRack.{h,cpp}`、`PluginRackState.cpp` | 単体プラグインの読み込み・処理・状態・パラメータ・MIDI |     |
| `PluginChain.{h,cpp}`                       | トラック内のエフェクト列                               |     |
| `PluginEditorHost.{h,cpp}`                  | エディタの表示と、状態・パラメータ変化の送出           |     |
| `PluginTransportPosition.h`                 | プラグインへ渡すトランスポート位置                     |     |
| `FaultInjection.{h,cpp}`                    | プラグインのライフサイクル境界に挿入するテスト用の障害 |     |

`RuntimeLifecycleExecutor` は `app/` へ移す（6.7）。

### 5.6 `instruments/`

移動なし。`InstrumentRuntime.h`、`SonalloyInstrumentRuntime.{h,cpp}`、`Vst3InstrumentRuntime.{h,cpp}`。

### 5.7 `recording/`

移動なし。`ArrangeRecordingSession.{h,cpp}`、`ArrangementCaptureSink.h`、`RecordingCaptureRuntime.{h,cpp}`、`RecordingSession.{h,cpp}`。

`recording/` は `timeline/` より下の層である（`TimelineEngine.h` と `TrackRuntime.h` が取り込み先と取り込み状態を include する）。`TimelineEngine` を操作する `RecordingController` をここに置くと `recording` ↔ `timeline` の循環になる。そのため `RecordingController` は `pipeline/` に置く（5.11）。

### 5.8 `timeline/`

| ファイル                                                                     | 内容                                                   | 旧                                        |
| ---------------------------------------------------------------------------- | ------------------------------------------------------ | ----------------------------------------- |
| `TimelineEngine.{h,cpp}`                                                     | グラフの公開、トランスポート、実時間命令、状態の公開   |                                           |
| `TimelineEngineDevices.cpp`                                                  | トラックデバイス（プラグイン・楽器）の照会と操作       |                                           |
| `TimelineEngineMidi.cpp`                                                     | ライブ MIDI・対象指定 MIDI の振り分け                  | `TimelineEngineDevices.cpp` の一部（6.8） |
| `TimelineEngineRecording.cpp`                                                | 録音窓、カウントイン、録音の確定とオフライン処理       |                                           |
| `TimelineEngineMix.cpp`                                                      | ブロックごとのミックス（`mix` とその下請け）           | `TimelineEngineRender.cpp`（6.8）         |
| `TimelineSnapshotBuilder.{h,cpp}`                                            | スナップショットから準備済みグラフを組み立てる         |                                           |
| `PreparedTimeline.h`                                                         | 準備済みグラフ                                         |                                           |
| `ArrangementGraph.{h,cpp}`                                                   | 入出力経路・MIDI 経路・補償遅延・取り込み範囲の計算    |                                           |
| `ControlGraphRegistry.{h,cpp}`                                               | 制御側のグラフ登録とトラックのキー                     |                                           |
| `MixerRuntime.h`、`MixRouteDelay.h`                                          | バス・リターン・マスターの準備済みミックス             |                                           |
| `TrackRuntime.h`                                                             | トラック単位の DSP 状態                                |                                           |
| `TrackProcessingPool.{h,cpp}`                                                | トラック処理の並列実行                                 |                                           |
| `MidiScheduler.{h,cpp}`、`MidiNoteTransform.h`、`MidiSourceRegistry.{h,cpp}` | タイムライン MIDI の事前展開・ノート変換・入力元の索引 |                                           |
| `MetronomeClicks.{h,cpp}`                                                    | メトロノームのクリック                                 |                                           |
| `TimelineReadAheadSource.h`                                                  | 音声クリップの先読み                                   |                                           |
| `TimelineTimebase.h`                                                         | tick と sample の換算                                  |                                           |
| `RealtimeCommand.h`、`RealtimeFrame.h`                                       | 実時間状態の所有者への命令と、所有者が公開する状態     |                                           |

`AutomationRuntime.h` と `MixAutomation.h` は `automation/` へ移す（5.4）。

### 5.9 `render/`

移動なし。`OfflineRenderer.{h,cpp}`。

### 5.10 `preview/`（新設）

| ファイル                           | 内容                                                   | 旧                                       |
| ---------------------------------- | ------------------------------------------------------ | ---------------------------------------- |
| `PreviewEngine.{h,cpp}`            | サンプル試聴のボイス、テイク比較のボイス、簡易シンセ   | `audio/PreviewEngine.{h,cpp}`            |
| `InstrumentPreviewSession.{h,cpp}` | 内蔵楽器プレビューの準備済み実行時と MIDI スケジュール | `audio/InstrumentPreviewSession.{h,cpp}` |
| `PluginAudition.{h,cpp}`           | Project の外での VST3 の試奏                           | `audio/PluginAudition.{h,cpp}`           |

`instrument_preview::barPositionForFrame` は実行時の計算なので、`contract/InstrumentPreview.h` ではなく `InstrumentPreviewSession.h` に置く。

### 5.11 `pipeline/`（新設。旧 `audio/` の残り）

| ファイル                      | 内容                                                 | 旧                                  |
| ----------------------------- | ---------------------------------------------------- | ----------------------------------- |
| `AudioRenderPipeline.{h,cpp}` | コールバック内の描画順序と安全チェーン、ミュート理由 | `audio/AudioRenderPipeline.{h,cpp}` |
| `AudioSafetyDsp.h`            | DC 除去とフィードバック検知                          | `audio/AudioSafetyDsp.h`            |
| `AudioMetrics.{h,cpp}`        | ピーク・リミッター診断・コールバック計測             | `audio/AudioMetrics.{h,cpp}`        |
| `RecordingController.{h,cpp}` | アレンジ録音の開始・停止・確定の受け渡し（6.4）      | `app/RecordingController.{h,cpp}`   |

`audio/` は廃止する。音声エンジンの中で「audio」はどのディレクトリにも当てはまり、置き場所を決める手がかりにならないためである。`RecordingController` は `AudioRenderPipeline` が所有し、パイプラインの境界での録音制御を担う。現状、`audio/AudioRenderPipeline.h` が `app/RecordingController.h` を include していて、`pipeline` → `app` の逆流になっている。

### 5.12 `device/`

| ファイル                        | 内容                                                            | 旧                              |
| ------------------------------- | --------------------------------------------------------------- | ------------------------------- |
| `AudioConfiguration.h`          | 要求するドライバとデバイスの設定                                |                                 |
| `AudioDeviceCallback.{h,cpp}`   | JUCE のデバイスコールバックからパイプラインへの適配             |                                 |
| `AudioDeviceController.{h,cpp}` | 稼働中デバイスの開閉・復旧・切替と遷移状態                      |                                 |
| `AudioDeviceService.{h,cpp}`    | デバイスの探索と設定値の組み立て                                |                                 |
| `MidiInputService.{h,cpp}`      | MIDI 入力の開閉・集合変化の検知と、`MidiMonitor` による振り分け | `midi/MidiInputService.{h,cpp}` |

`midi/` は廃止する。物理 MIDI 入力はオーディオデバイスと同じく `juce_audio_devices` に依存する物理 I/O の境界であり、1クラスのためにディレクトリを分けない。タイムライン MIDI（`MidiScheduler` など）は `timeline/` のまま置く。

### 5.13 `app/`

| ファイル                           | 内容                                                                         | 旧                                         |
| ---------------------------------- | ---------------------------------------------------------------------------- | ------------------------------------------ |
| `AudioEngine.{h,cpp}`              | `--serve` の寿命管理、長寿命オブジェクトの組み立て、定期配信と監視のスレッド |                                            |
| `AudioCommandDispatcher.{h,cpp}`   | 命令の読み取りと振り分け、命令をまたぐ状態の所有（6.5）                      |                                            |
| `commands/*.cpp`                   | 機能別の命令処理                                                             |                                            |
| `AudioStatusBuilder.{h,cpp}`       | 状態・メーター・トランスポートの投影                                         |                                            |
| `RuntimeLifecycleExecutor.{h,cpp}` | サードパーティ呼び出しの直列実行と番犬                                       | `plugins/RuntimeLifecycleExecutor.{h,cpp}` |

`AudioMain.cpp` は `sidecars/audio/` へ移す（4章）。

## 6. 構成の変更に伴って移す責務

ファイルの移動だけでは依存の逆流や所有の重複が残る箇所を、ここで直す。

### 6.1 内蔵楽器プレビューの型を `contract/` に置く

- 現状、`contract/SidecarCommands.h` が `audio/InstrumentPreviewSession.h` を、`SidecarCommands.cpp` が `audio/InstrumentPreviewContract.h` を include している。命令の型 `InstrumentPreviewSpec` が実行側のヘッダで定義されているためである。
- `InstrumentPreviewTimeSignature`、`InstrumentPreviewNote`、`InstrumentPreviewSpec` と、`InstrumentPreviewContract.h` の上限値・検証関数（`isValidTempo` など）を `contract/InstrumentPreview.h` に置く。`InstrumentPreviewContract.h` は削除する。
- 検証を decode とセッションのどちらで行うかは変えない。

### 6.2 実行グラフの検証を decode の1か所にする

- `validateMixerGraph` は、`decodeExecutionGraph`（`ExecutionGraphDecoder.cpp`）と `TimelineSnapshotBuilder::build` の両方で呼ばれている。製品のグラフはどれも decode を通る（`prepareTimelineSnapshot` の命令と `riffra-render` の要求）ので、builder 側の呼び出しは同じ検証の重複である。
- builder 側の呼び出しを削除し、関数は `ExecutionGraphDecoder.cpp` の無名名前空間へ移す。`MixerGraphValidation.h` は削除する。
- 「C++ は安全に実行できない投影を拒否してよい」という境界の規則は、この1か所で満たす。

### 6.3 デバイスの再利用判定を `timeline/` に置く

- `sameEffectTopology` / `sameInstrumentTopology` / `sameInstrumentRuntime` は、準備中のグラフが前のグラフのプラグイン実体を再利用できるかの判定である。製品で使うのは `TimelineSnapshotBuilder.cpp` だけである。
- `contract/ExecutionGraph.h` から `timeline/TimelineSnapshotBuilder.h` へ移す。`tests/timeline/TimelineDeviceTest.cpp` の直接の呼び出しは、include 先を変えてそのまま残す。

### 6.4 録音の停止を `RecordingController` に一本化する

現状は、録音停止の進行中を表す状態が2つある。

- `RecordingController::stopInProgress`：`requestStop` から `completeStop` の終わりまで
- `AudioCommandDispatcher::recordingStopScheduled`：停止命令の受付から、停止後の状態イベントを書き終えるまで

後者は、ディスパッチャが持つ停止用ワーカースレッド（`recordingStopMutex`、`recordingStopWake`、`recordingStopFinished`、`pendingRecordingStop`、`recordingStopWorker`、`reserveRecordingStop`、`releaseRecordingStopReservation`、`queueRecordingStop`、`recordingStopWorkerLoop`）に付いている。

次の形にする。

- 停止用ワーカースレッドと「停止の進行中」の状態は `RecordingController` が持つ。状態は1つにする。
  - 停止の要求から、停止の適用と完了の通知までを1つの区間として扱う。
- `RecordingController` に、停止を受け付けて非同期に完了させる操作を置く。完了したら、停止の完了通知（成否とエラー文字列）を呼ぶ。
  - 通知は `setFinalizationDispatcher` と同じく、設定で受け取る。`RecordingController` はディスパッチャより先に（`AudioRenderPipeline` のメンバとして）作られるためである。
  - ディスパッチャはこの通知で `FaultSpec` と状態イベントを書く。`RecordingController` は `protocol/` に依存しない。
- ディスパッチャの `waitForBackgroundWork` は、`RecordingController` の「停止の完了を待つ」操作に置き換える。
- 停止の重複を拒否するエラーは、`RecordingController` の `kind: "recording"`、`message: "The previous recording stop is still pending."` に揃える。
  - 現状ディスパッチャが返している `kind: "recordingBusy"` は使わない。
  - Rust 側・契約フィクスチャ・`docs/` のどれも `recordingBusy` を参照していないことは確認済みである。
- `PendingRecordingStop` は `RecordingController` の内部型にする。
- 呼び出し元のない `RecordingController::takePendingFinalization()` を削除する。

デバイスを失ったときに録音を止める判断も、1か所にまとめる。

- 現状は次の2か所で同じ停止をしている。
  - `AudioDeviceController::changeListenerCallback` が `renderPipeline.recording().stop()` を直接呼ぶ
  - `AudioEngine` が `AudioDeviceController` に渡すデバイス停止ハンドラも、同じ停止を `callAsync` で呼ぶ
- `device/` はデバイスの状態の変化を通知するだけにし、録音を止めるかどうかは `AudioEngine`（組み立て側）が通知を受けて決める。
- `AudioDeviceController` から `recording()` の呼び出しを削除する。
- `AudioEngine` が `setDeviceLossHandler` で渡すハンドラの中で、状態イベントを書く前に録音を止める。今と同じスレッド・同じ順序（停止してから状態を書く）になる。
- デバイス停止ハンドラ（`callAsync` での停止）は今のまま `AudioEngine` に残す。

録音の確定処理（`AudioEngine::serve` の `publishRecordingCompletion` と、確定の受け渡しの設定）は `app/` に残す。ライフサイクル実行器とイベントの出力を使う、組み立て側の処理だからである。

### 6.5 命令をまたぐ状態をディスパッチャが持つ

現状、次の状態は `AudioEngine` のメンバとして宣言され、参照で `AudioCommandDispatcher::Context` に渡されている。

- `trackPluginEditor`、`pluginEditorOwner`、`trackPluginEditorDeviceId`
- `auditionEditor`
- `comparisonRaw`、`comparisonProcessed`
- `timelineOperationRunning`

`AudioEngine` 自身は、これらを終了時の後片付け以外では使っていない。読み書きはすべて `commands/*.cpp` が行っている。

- これらを `AudioCommandDispatcher` の private メンバにする。`Context` には、`AudioEngine` が所有する長寿命の部品（フォーマット管理、タイムライン、パイプライン、デバイス制御、MIDI 入力、ライフサイクル実行器）だけを残す。
- 終了時の後片付け（トラックのエディタを閉じる、試奏を閉じる、`timelineOperationRunning` を下ろす）は、ディスパッチャの1つの操作にまとめる。`AudioEngine` はライフサイクル実行器の上でそれを呼ぶ。今の `closePluginAudition()` はこの操作に統合する。

### 6.6 障害挿入とプロトコルの循環を断つ

- 現状は次の循環がある。
  - `protocol/AudioProtocol.cpp` が制御出力のたびに `FaultInjection::stdoutFlood()` を呼ぶ
  - `plugins/FaultInjection.cpp` が `protocol/ProtocolChannel.h` の `writeProtocolLine` を使う
- `stdoutFlood` を `FaultInjection` から削除し、同じ処理を `AudioProtocol.cpp` の無名名前空間に置く。
  - 処理の中身：`RIFFRA_FAULT_MODE=stdoutFlood` のとき、プロセスで一度だけ 256 KiB の行を `writeProtocolLine` で書く。
- `FaultInjection` はプラグインのライフサイクル境界（`FaultStage`）だけを扱う。ヘッダのコメントから `stdoutFlood` の記述を除き、`AudioProtocol.h`（または `AudioProtocol.cpp`）に移す。

### 6.7 ライフサイクル実行器を `app/` に置く

`RuntimeLifecycleExecutor` は、プラグインに限らず、グラフの読み込みと録音の確定も直列に実行する `--serve` のスレッド境界である。`plugins/` のどのクラスにも依存しておらず、使っているのは `app/` だけである。`app/` へ移す。

### 6.8 `TimelineEngine` の分割ファイルを中身に合わせる

- `TimelineEngineRender.cpp` の中身は、オーディオコールバックごとのミックス（`mix`、`mixActiveGraph`、`processTracks` など）である。`render/`（オフライン書き出し）と名前が重なるため、`TimelineEngineMix.cpp` にする。
- `TimelineEngineDevices.cpp` には、デバイスの操作と並んでライブ MIDI の振り分けが入っている。次の関数を `TimelineEngineMidi.cpp` へ移す。
  - `enqueueLiveMidi`、`enqueueTargetedMidi`、`routeLiveMidi`、`setLiveMidiTarget`
  - `playTargetedMidi`、`panicTargetedMidi`、`playSurfaceTrackKey`、`midiSourceIndex`
- 無名名前空間の補助関数は、使う側のファイルへ一緒に移す。

### 6.9 MIDI の振り分け先をコンストラクタで受け取る

- `MidiMonitor` の振り分け先は、受け取り方が揃っていない。
  - プレビューとタイムラインは `MidiInputService` のコンストラクタから `setPreviewEngine` / `setTimelineEngine` で設定している
  - 試奏だけは `AudioEngine` があとから `setAudition` で設定している
- `MidiInputService(PreviewEngine&, PluginAudition&, TimelineEngine&)` で3つとも受け取り、`MidiMonitor` も参照で持つ。3つの setter は削除する。

### 6.10 古いコメントを直す

- `TimelineSnapshotBuilder.h` の「Builds a runtime timeline graph from a JSON snapshot」は、実際には decode 済みの `TimelineSnapshotSpec` を受け取る。中身に合わせて直す。
- 移動・改名したファイルとクラスを参照しているコメントを、新しい名前に直す。

## 7. `tests/`

### 7.1 構成

`tests/` は `src/` と同じディレクトリ構成にする。テストファイルは `tests/<ディレクトリ>/<対象>Test.cpp` とし、テストスイート名は対象のクラス名に `Test` を付けたものにする。1つのファイルには1つの対象のテストだけを置く。

```text
tests/
├── app/          AudioCommandDispatcherTest、AudioStatusBuilderTest
├── automation/   AutomationRuntimeTest
├── concurrency/
├── contract/
├── device/       AudioDeviceControllerTest、AudioDeviceServiceTest、MidiInputServiceTest
├── instruments/  SonalloyInstrumentRuntimeTest、SonalloyInstrumentRuntimeBenchmarkTest
├── pipeline/     AudioMetricsTest、AudioRenderPipelineTest、AudioSafetyDspTest
├── plugins/
├── preview/      InstrumentPreviewSessionTest、PluginAuditionTest、PreviewEngineTest
├── protocol/
├── recording/
├── render/       OfflineRendererTest、RenderProtocolIsolationTest.cmake
├── timeline/
└── support/      複数のディレクトリが使うテスト支援と、テスト用 VST3
```

`app/`、`device/`、`pipeline/` などが `TimelineEngine` を持つテストを書くための支援は、`timeline/` ではなく `support/` に置く。

### 7.2 テストの移動

| 現在                                                                                                                                                                                                                                                            | 移動先                                                                                                                    |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `tests/audio/AudioMetricsTest.cpp`、`AudioRenderPipelineTest.cpp`、`AudioSafetyDspTest.cpp`                                                                                                                                                                     | `tests/pipeline/`                                                                                                         |
| `tests/audio/PreviewEngineTest.cpp`、`PluginAuditionTest.cpp`、`InstrumentPreviewSessionTest.cpp`                                                                                                                                                               | `tests/preview/`                                                                                                          |
| `InstrumentPreviewSessionTest.cpp` の `PreviewEngineTest` 3件                                                                                                                                                                                                   | `tests/preview/PreviewEngineTest.cpp`                                                                                     |
| `tests/protocol/AudioProtocolTest.cpp` の `AudioDeviceServiceTest.ReportsInitialMetersForTheActiveProject` と `KeepsProjectMeterEpochAndCumulativeDiagnosticsConsistent`                                                                                        | `tests/app/AudioStatusBuilderTest.cpp`（スイート名 `AudioStatusBuilderTest`。対象は `AudioStatusBuilder::currentMeters`） |
| 同 `AudioDeviceServiceTest.ReportsProbeFieldsRequiredByTheHost`                                                                                                                                                                                                 | `tests/device/AudioDeviceServiceTest.cpp`                                                                                 |
| 同 `MidiInputServiceTest`                                                                                                                                                                                                                                       | `tests/device/MidiInputServiceTest.cpp`                                                                                   |
| `tests/timeline/TimelineDeviceTest.cpp` の `AudioCommandDispatcherTest` 5件                                                                                                                                                                                     | `tests/app/AudioCommandDispatcherTest.cpp`                                                                                |
| `tests/timeline/TimelineRenderTest.cpp` の `TimelineReadAheadSourceTest` 4件                                                                                                                                                                                    | `tests/timeline/TimelineReadAheadSourceTest.cpp`                                                                          |
| 同 `MixerRoutingTest` 14件                                                                                                                                                                                                                                      | `tests/timeline/MixerRoutingTest.cpp`                                                                                     |
| 同 `RendersBuiltInAndAutomatedVst3InstrumentsThroughOfflineRenderer`、`OfflineRendererReturnsIdenticalFrameCountWithAndWithoutTerminalNotes`、`OfflineRendererReturnsIdenticalFrameCountWithAndWithoutTrackLatency`、`NormalizesAnOfflineRenderToTheTargetPeak` | `tests/render/OfflineRendererTest.cpp`（スイート名 `OfflineRendererTest`）                                                |
| `tests/timeline/TimelineRenderTest.cpp` の残り                                                                                                                                                                                                                  | `tests/timeline/TimelineMixTest.cpp`                                                                                      |
| `tests/timeline/AutomationRuntimeTest.cpp`                                                                                                                                                                                                                      | `tests/automation/`                                                                                                       |
| `tests/timeline/instruments/*`                                                                                                                                                                                                                                  | `tests/instruments/`                                                                                                      |
| `tests/timeline/TimelineTestSupport.h`、`tests/timeline/SonalloyTestSupport.h`                                                                                                                                                                                  | `tests/support/`                                                                                                          |
| `tests/contract/JsonTestSupport.h`                                                                                                                                                                                                                              | 移動なし（`contract/` のテストだけが使う）                                                                                |
| `tests/plugins/PluginRackTest.cpp` の `PluginEditorHostTest` 1件                                                                                                                                                                                                | `tests/plugins/PluginEditorHostTest.cpp`                                                                                  |

- テストは移すだけで、中身（Arrange / Act / Assert）は変えない。6章の変更で呼び出し先が変わったところだけ直す。
- 次のテスト名は、CI の ThreadSanitizer の対象（`.github/workflows/ci.yml` の `ctest -R`）に含まれている。名前を変えない。
  - `TimelineEngineTest.KeepsCanonicalTrackStateWhenDeviceRuntimeIsReused`、`TimelineEngineTest.SendsEmergencyPanicToTheInstrumentRuntime`（`TimelineDeviceTest.cpp` に残る）
  - `AudioProtocolTest.ControlOutputDiscardsQueuedTelemetry`
  - `PreviewEngineTest.*`
  - そのほか、フィルタに書かれているすべての名前
- `TimelineEngineTest.StreamsALongLoopRecordingWithoutPreallocatingRam` と `SonalloyInstrumentRuntimeTest.CompilesAndPlaysEveryReleasedPreset` は、CMake が名前を指定して slow ラベルで登録している。名前を変えない。

## 8. ビルド構成（`CMakeLists.txt`）

### 8.1 ライブラリとディレクトリの対応

ライブラリは「`juce_audio_devices` に依存するか」で分ける。オフライン書き出しの `riffra-render` を、オーディオデバイス（Linux では ALSA）なしで動かすためである。

| ライブラリ           | 旧名                 | ディレクトリ                                                                                                                | 使う側                                       |
| -------------------- | -------------------- | --------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| `riffra-engine-core` | `riffra-render-core` | `concurrency`、`contract`、`protocol`、`automation`、`plugins`、`instruments`、`recording`、`timeline`、`render`、`preview` | `riffra-render`、`riffra-serve-core`、テスト |
| `riffra-serve-core`  | `riffra-audio-core`  | `pipeline`、`device`、`app`                                                                                                 | `riffra-audio`、テスト                       |

- 1つのディレクトリのソースは、すべて同じライブラリに入れる。
  - 現状は `protocol`、`contract`、`recording`、`plugins` が2つのライブラリに分かれている。
- `riffra-audio` のソースは `sidecars/audio/AudioMain.cpp` だけにする。
  - `app/AudioEngine.cpp`、`AudioCommandDispatcher.cpp`、`commands/*.cpp` は `riffra-serve-core` に入れる。
- `riffra-render` のソースは `sidecars/render/RenderMain.cpp` だけにする。
- `riffra-audio-tests` は `app/` のソースを直接コンパイルしない。`riffra-serve-core` をリンクする。
- `riffra-real-vst-runtime-test` と `riffra-audio-benchmarks` のリンク先を新しいライブラリ名に直す。
- `riffra_generate_core_juce_header` の呼び出し、`target_compile_definitions`、`target_include_directories`、MSVC の警告設定の対象を新しいライブラリ名に直す。
  - JUCE ヘッダの生成元（`riffra-engine-core` は `riffra-render`、`riffra-serve-core` は `riffra-audio`）は変えない。
- `target_sources` のソースは、ディレクトリごとにまとめてアルファベット順に並べる。

### 8.2 変えないもの

- 実行ファイル名、`juce_add_console_app` の製品名、install 規則、Tauri 用の配置名（`binaries/<名前>-<triple>`）
- `riffra-plugin-scan` が `riffra-engine-core` をリンクせず、必要なソース（`PluginRack.cpp`、`PluginRackState.cpp`、`FaultInjection.cpp`、`ProtocolChannel.cpp`、`SidecarMessages.cpp`）を直接コンパイルする構成
  - JUCE ヘッダの生成とリンクする JUCE モジュールが、ほかのターゲットと別だからである。
  - パスだけを新しい場所に直す。6.6 の変更で `FaultInjection.cpp` は `ProtocolChannel` を使わなくなるが、`PluginScanMain.cpp` が使うので `ProtocolChannel.cpp` は残す。
- テストの登録（`add_test`、`gtest_discover_tests` のフィルタ、ラベル、タイムアウト、環境変数）。パスだけを直す。

## 9. 文書

### 9.1 `native/audio-engine/README.md`

「全体構造」「ディレクトリ構成」の節を、この構成に合わせて書き直す。差分を足すのではなく、節の全体を完成形の説明として書き直す。

- 「実行形態」の表のライブラリ名と説明を、8.1 のとおりに直す。
- 「全体構造」の所有者の表：
  - 録音の行：`RecordingController` は `pipeline/`、各セッションは `recording/`
  - プラグインの行：`RuntimeLifecycleExecutor` を外し、`app/` の責務として書く
- 「ディレクトリ構成」：
  - トップレベル（`sidecars/`）
  - `src/` の表：3.1 の順。層と依存の向きの要約も含める
  - `sidecars/` の表
  - `tests/` の表
- 現状の README には実在しないものが書かれているので、書き直しで消す。
  - 存在しない名前：`CommandRouting`、`parseMidiBytes`、`protocol/` の `makeError`（実際は `PluginScannerMain.cpp` のローカル関数）
  - 存在しない場所：`tests/app/` の記述（この作業で実在するようになる）、`timeline/` の下の `instruments/`
  - `contract/` が表にない
- 「通信契約」「スレッドとリアルタイム制約」「安全チェーン」「ビルドと検証」の節は、名前の変わった部分（`AudioRenderPipeline` の場所など）だけを確認して直す。

### 9.2 そのほか

- `docs/` には `native/audio-engine` の内部パスとクラスの場所を書いた箇所がない（確認済み）。変更は不要である。
- `.github/workflows/ci.yml` は `native/audio-engine/**` と `riffra-audio-tests` しか参照していない。7.2 のテスト名を守れば変更は不要である。

## 10. 実装の進め方（コミット分割）

1. `refactor(audio-engine): move sources into the layered directory layout`
   - 5章のファイル移動（`git mv`。6.7 の `RuntimeLifecycleExecutor` を含む）
   - 3.3 の include の書き方への統一
   - 4章の `sidecars/`
   - 8章の CMake
2. `refactor(audio-engine): keep contract and protocol independent of runtime code`
   - 6.1、6.2、6.3、6.6
3. `refactor(audio-engine): give recording stop and command state a single owner`
   - 6.4、6.5、6.9
4. `refactor(audio-engine): split TimelineEngine sources by responsibility`
   - 6.8、6.10
5. `test(audio-engine): mirror the source layout in tests`
   - 7章
6. `docs(audio-engine): describe the directory layout and ownership`
   - 9章

## 11. 守る条件

- 1章に書いた外から見える振る舞いを変えない。例外は 6.4 の停止の重複エラーの `kind` だけである。
- `git mv` で移動し、履歴を追えるようにする。移動と中身の変更が同じコミットに混ざるときも、移動したファイルの差分は include とパスの修正にとどめる。
- オーディオコールバックから到達するコードの制約（確保しない、待たない、デバイスの開閉・プラグインの生成破棄・ファイル入出力・JSON・stdout を扱わない）を崩さない。6.4 の停止ワーカーと 6.5 の状態は、制御側のスレッドだけで扱う。
- 破棄の順序を変えない。
  - `AudioEngine` のメンバの宣言順は、`commandDispatcher` がパイプラインとデバイス制御より先に破棄される今の順を保つ。
  - 6.5 でディスパッチャへ移すエディタは、これまでどおりライフサイクル実行器の上で閉じる。

## 12. 禁止事項

- 新しい抽象（インタフェース、基底クラス、レジストリ、イベントバス）を足さない。この作業は置き場所と所有者の整理である。
- 1ファイルだけのディレクトリを、この文書にあるもの以外に作らない。
- 移動のための転送ヘッダ（旧パスに置いて新パスを include するヘッダ）や、旧名の CMake ターゲットの別名を残さない。
- テストを削除しない。移動と、6章で呼び出し先が変わった箇所の修正だけを行う。
- 6章にない責務の変更をしない。見つけた問題は報告にとどめる。

## 13. 対象外

この作業では変えない。

- `TimelineEngine` の大きさ（ヘッダ 466 行、4つの分割ファイル）とクラスの分割
- 製品コードの `friend` 指定（`PluginRack` の `friend class TimelineEngine`、`PreviewEngine` / `AudioMetrics` の `friend class AudioRenderPipeline`、`TimelineEngine` の `friend class AudioRenderPipeline` / `TimelineSnapshotBuilder`）
- `AudioRenderPipeline` がプレビューの操作を中継しているメソッド（`startInstrumentPreview`、`isInstrumentPreviewing`、`isPreviewing`）
- 内蔵楽器プレビューの検証が、decode（`SidecarCommands.cpp`）とセッション（`InstrumentPreviewSession.cpp`）に分かれていること
- `AudioEngine::serve` の長さ（デバイスの初期化、監視・定期配信のスレッド、録音の確定、命令スレッド、終了手順が1つの関数にある）
- `riffra-plugin-scan` のビルド構成（8.2）

## 14. 完了条件

- `src/` のディレクトリ間の include が、3.2 の表から外れていない。
  - 確認例：`grep -rn '#include "' src` の結果をディレクトリごとに見る。
- `src/` と `tests/` に `../` の include がない。
- `src/audio/`、`src/midi/`、`tools/` がない。
- `tests/` のディレクトリ構成が `src/` と一致している（`support/` を除く）。
- 次が通る。
  - `.\native\audio-engine\build.ps1 -Configuration Debug`（Windows）または `./native/audio-engine/build.sh Debug`
  - 変更した C++ ファイルの `clang-format --style=file --dry-run --Werror`
- `native/audio-engine/README.md` の構成の説明が、実際のファイルと一致している。
