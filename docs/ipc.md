# Riffra IPC 契約

## 1. スコープ

本書は Riffra の IPC 境界とその契約を正準化する。「どうやり取りするか」を示し、「何がやり取りされるか」の詳細は各言語のコードを真実源とする。

### 書くこと

- IPC 境界の全体像と使い分け基準
- Tauri 命令の構成と Control Command の定義・性質
- NativeApi TS 契約と Tauri 命令との対応規則
- サイドカー JSON Lines プロトコルの構造と規則（音声・レンダー・プローブ）
- CLI と Riffra Host 制御のプロトコル境界
- 境界ごとのエラー・状態遷移の契約
- 権限・ケイパビリティ設定

### 書かないこと

- 各 Control Command の Params・結果の詳細（`riffra-runtime` の `api` モジュール参照）
- サイドカーコマンドの全シグネチャ（code 参照）
- 各メッセージの全フィールド（code 参照）

層構造の全体像は `architecture.md`、エンティティの定義は `data-model.md` を参照。

---

## 2. 境界の全体像

```text
┌────────────────────────────── WebView ──────────────────────────────┐
│ React（src/native/）                                                 │
└────┬───────────────────────┬───────────────────────┬───────────────┘
      │ A: Tauri 命令          │ B: イベント購読        │
      │ invoke 系             │ listen 系            │
┌────▼───────────────────────▼───────────────────────▼───────────────┐
│ Rust バックエンド（src-tauri）                                          │
│ 命令層 → Host Adapter → riffra-core / RuntimeReconciler / 永続化       │
└────┬───────────────┬───────────────┬──────────────────────────────┘
      │ C: JSON Lines  │ D: JSON 1行   │ E: JSON 1行
      │ stdin/stdout   │ stdin/stdout  │ stdout
┌────▼───────┐  ┌─────▼────────┐  ┌──▼──────────┐
│riffra-audio│  │riffra-render │  │riffra-audio │
│ --serve    │  │ オフライン    │  │ --probe系   │
│ 常駐・音声  │  │ レンダ要求1  │  │ デバイス列挙│
└────────────┘  │ 回ごとに起動  │  └─────────────┘
                └──────────────┘
```

```text
外部クライアント（`riffra --attach`）
        │ F: Named Pipe / Unix Domain Socket
        ├─ command connection
        └─ events connection
        ▼
Riffra Host Control Server → HostEventHub → Host state / Core
```

| 境界 | 方向                                          | 方式                                                    | 用途                                                     |
| ---- | --------------------------------------------- | ------------------------------------------------------- | -------------------------------------------------------- |
| A    | WebView → Rust                                | `invoke`（Tauri command）                               | 一切の操作・編集・照会                                   |
| B    | Rust → WebView                                | Tauri event                                             | 音声状態・メーター・トランスポート・ランタイム回復の通知 |
| C    | Rust ↔ riffra-audio                           | 子プロセスの stdin/stdout（JSON Lines）                 | 投影・演奏・録音・MIDI・プレビュー・デバイス制御         |
| D    | Runtime → riffra-render                       | 子プロセスの stdin/stdout（JSON 1行）                   | オフラインレンダリング（1要求1プロセス）                 |
| E    | Rust ↔ riffra-audio（probe）                  | 子プロセスの stdout（JSON 1行）/ 引数                   | デバイス・チャンネル列挙、VST3スキャン                   |
| F    | 外部クライアント ↔ Riffra Host Control Server | Windows Named Pipe / Unix Domain Socket（長さ付きJSON） | Hostの正準状態・Runtime操作とHost event購読              |

使い分け: 低遅延の音声は C、時間のかかる一括処理は D、列挙は E、WebView 操作は A。F の利用者は Host 外部操作に限る。

ストリームの予約: C・D・E の子プロセスは stdout をプロトコル専用とする。VST3 をロードする起動（`riffra-audio --serve`、`riffra-render`、`riffra-plugin-scan`）では、起動時にプロセス自身の出力を切り離して stderr へ回し、第三者コードの出力がプロトコル行に混ざらないようにする。切り離しに失敗した場合はプロトコルが成立しないため、プラグインをロードせずに起動を失敗させる。Rust 側は stderr をプロトコルとして解釈しない。

---

## 3. 境界 A: Tauri 命令（WebView → Rust）

### 3.1 命令の構成

WebView から Host への操作は、すべて `dispatch_control` 1 本で送る。Tauri 命令として個別に持つのは、Desktop shell が所有する処理だけである。

| 命令                                                                                | 責務                                                                                    |
| ----------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| `dispatch_control`                                                                  | Control Command（§3.2）を 1 件デコードし、接続中の Host で実行して結果の `value` を返す |
| `get_bootstrap_state`                                                               | CanonicalState・ProjectState・プラグイン一覧・セーフモード・回復候補の初期状態を返す    |
| `get_host_connection_state` / `list_local_hosts` / `switch_host` / `reconnect_host` | Host の選択・切替・再接続                                                               |
| `import_midi_bytes`                                                                 | ドロップされた MIDI バイト列を一時ファイルへ書き出し、`asset.import-midi` で取り込む    |
| `render_timeline`                                                                   | `render.start` でジョブを開始し、完了まで `job.get` で待って結果を返す                  |

全命令は `lib.rs` の `invoke_handler` が真実源。命令は `spawn_blocking` で async ワーカーから分離して実行する。

### 3.2 Control Command

Host に頼める操作（Control Command）は、`riffra-runtime` の `api` モジュールにある表（`api/table.rs`）で 1 回だけ定義する。表の 1 行は、命令名・Params 型・結果型・性質を持つ。Standalone dispatcher、Live Host、CLI、Desktop はすべてこの表から生成された型を使い、命令名の文字列リストを別に持たない。

| 性質     | 値                                        | 意味                                                                                                                                  |
| -------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| scope    | `host`                                    | `expectedProjectId` を要求しない                                                                                                      |
|          | `project`                                 | Active Project に紐づき、`expectedProjectId` を要求する。Live Host では Projectの書き込み権を取る                                     |
|          | `project(long)`                           | 準備時は確定スナップショットを読み、コミット直前に書き込み権とProject ID・sequenceを照合する長時間処理（VST3 のロードを伴う編集など） |
| executor | `canonical`（`read` / `mutation(batch)`） | 正準状態を読む・変える。Standalone でも実行でき、`batch` の命令は `session.apply` に含められる                                        |
|          | `project`                                 | Project container の一覧・作成・切替・Import / Export                                                                                 |
|          | `runtime`                                 | Live Host の Runtime を要する。Standalone は `runtimeUnavailable` を返す                                                              |

- 要求は最初に `ControlCommand::decode` で型付きの命令になる。未知の命令名、未知の Params キー、型違いは `invalidRequest` になり、`details` に JSON Pointer 形式の `path`、配列要素の `index`、小さな値の `value` が付く
- 実行側は executor ごとの命令を網羅的に `match` する。表に命令を足して実装を書き忘れるとコンパイルエラーになる
- 正準状態を変える命令は `arrangementMutation`（`session.apply` は `batchMutation`）を返す。結果の `type` は `ControlOutput` の variant 名である
- TypeScript の `ControlCommand`（`{ command, params }` の union）と `ControlCommandResults`（命令名 → 結果型）は Rust から生成する（`npm run gen:types`）

### 3.3 エラー規約

- 失敗は `NativeCommandError` として `code`、`message`、`details` を返す。`message` は表示用、`code` と `details` は機械判定用であり、UI はメッセージ文字列を解析しない
- Native 音声エラーは `kind`、`operation`、`details` を保ったまま `NativeAudioError`、Host の `ProtocolError`、Tauri の `NativeCommandError` へ渡す。境界ごとに情報を文字列へ潰さない
- セーフモード中の音声系・プラグイン系命令は明示エラーを返す（`architecture.md §7`）
- 要求された操作に失敗した場合は、現在の状態を保ったままエラーと状態を返す
- 制作状態を変更する命令の応答に含まれる `CanonicalState` は、その操作自身が確定したスナップショットである。`projectId`、`sequence`、セッション、履歴を一体として返す

```json
{
  "code": "commandFailed",
  "message": "requested audio device was rejected",
  "details": {
    "domain": "nativeAudio",
    "kind": "deviceRejected",
    "operation": "audio.setDriver",
    "details": { "driver": "ASIO", "device": "Unavailable" }
  }
}
```

### 3.4 UI 呼び出しの順序

- 確定順序はCoreとHostのProject書き込み権が管理する。フロントエンドは状態を反映する入口で `CanonicalState.projectId` が現在のActive Projectと一致し、sequenceが受け入れ済みの値より新しい場合だけ採用する
- 中間値を捨ててよい連続制御は `dispatchLatestControl` で集約して最終値のみ送信する。集約された待機者には同一の確定応答を返す
- `dispatchControlOrFallback` は非ネイティブ環境（ブラウザプレビュー・スモークテスト）専用のフォールバック。ネイティブ実行時は失敗をそのまま reject する

---

## 4. 境界 B: シェル → WebView イベント

DesktopのActive Projectは初期bootstrapと`ProjectActivationResult`で確定する。`ProjectState`の一覧応答と名称更新イベントは一覧情報を更新し、現在のActive Project IDを維持する。

| イベント                    | ペイロード                | 意味                                                                                                                          |
| --------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `runtime-startup-finished`  | `RuntimeStartupFinished`  | スタートアップ時のランタイム初期化完了（セーフモードでは即通知）                                                              |
| `audio-status`              | `AudioStatus`             | デバイス、コールバック、安全ミュート理由、MIDI、Preview、音声診断の状態                                                       |
| `audio-meters`              | `AudioMeterFrame`         | Project ID、入力・出力ピーク、無効サンプル数、ミュート理由（高頻度）                                                          |
| `transport-status`          | `TransportStatus`         | Transport の状態と再生位置、audio clock、現役グラフの sample rate、適用済み命令番号、録音状態、armed track、instrument faults |
| `runtime-projection-status` | `RuntimeProjectionStatus` | 非同期のランタイム投影状態、現役投影の診断、エラーコード、世代・音声環境 revision（queued / preparing / active / failed）     |
| `runtime-restarted`         | `RuntimeRestarted`        | サイドカー再起動（世代番号）。RustがCoreの最新スナップショットを再投影する                                                    |
| `canonical-state-changed`   | `CanonicalState`          | GUI以外のHost操作を含む正準セッション、シーケンス、履歴の変更                                                                 |
| `recording-finalized`       | `RecordingFinalized`      | Native処理後の録音Asset登録とArrangement確定の完了結果                                                                        |
| `project-state-changed`     | `ProjectState`            | Projectの作成・改名・Importによる一覧の変更                                                                                   |
| `project-activated`         | `ProjectActivationResult` | Project切替の完了。Active Projectの一覧、CanonicalState、RecoveryStateを一括で通知する                                        |

`audio-meters` は Runtime 投影が属する `projectId`、`outputPeakLeft` / `outputPeakRight`、`trackMeters`（Track ID、左右Peak/RMS）を含む。Desktop は現在の Active Project と `projectId` が一致する frame だけを採用し、Project 切替後に旧 Project の値を描画状態へ戻さない。既存の低頻度 `audio-status` が届いても、高頻度メーターの Track データを消去しない。

Desktopのイベントゲートは現在の接続世代のイベントだけをWebViewへ送る。接続切替後に旧世代のイベントは届かない。フックはこの保証を前提に購読し、コンポーネント破棄後のコールバックだけを自身で止める。購読は `src/native/api/events.ts` のラッパを使い、用途は表示更新に限る

- エディタ由来の state / parameter 変更は Host 内の正準保存で完結する

---

## 5. 境界 C: 音声サイドカー（riffra-audio）

### 5.1 接続とフレーミング

- 起動: `riffra-audio --serve`。`AudioSupervisor` が起動ごとに世代番号を採番し、`ready` イベント（§5.3）を `SIDECAR_READY_TIMEOUT` まで待つ
- 送受信: JSON Lines（1 命令 = stdin 1 行、1 メッセージ = stdout 1 行）。stderr は診断ログ専用で、プロトコルには使わない
- 型の真実源: Rust は `crates/riffra-runtime/src/audio/wire/`、C++ は `SidecarCommands` / `SidecarMessages`。両者は `contracts/sidecar/` の契約フィクスチャで固定する（`test-strategy.md §5`）
- 厳格性: 両方向とも欠落キー・未知キー・型違い・範囲外の値を拒否する。値の欠けうるフィールドはキーを必ず持ち、値に `null` を使う。既定値での補完はしない

### 5.2 封筒

```jsonc
// Rust → C++: requestId（1 以上の整数）と命令本体
{"requestId": 42, "command": {"type": "seekTimeline", "tick": 960}}

// C++ → Rust: kind で応答・失敗・イベントを区別する
{"kind": "response", "requestId": 42, "response": {"type": "transportAccepted", "commandSequence": 42}}
{"kind": "event", "event": {"type": "transportStatus", "appliedCommandSequence": 42, ...}}
{"kind": "error", "requestId": 42, "error": {"kind": "...", "message": "...", "operation": "...", "details": {...}}}
{"kind": "event", "event": {"type": "audioMeters", ...}}
```

- `response` と `error` は必ず `requestId` を持ち、`event` は持たない。Transport の応答はキューへの受け付けを示し、状態は独立した `transportStatus` event で送る。要求に紐づかない失敗は `fault` イベントで送る
- 1 つの要求には `response` か `error` を**ちょうど 1 回**返す。C++ の `CommandResponder` が要求ごとに 1 つ作られ、非同期処理へ move で渡る。応答しないまま破棄されると `noResponse` の `error` を送る
- 命令ごとに応答の型は 1 つに決まる（`SidecarCommand::expected_response`）。`command_bus.rs` は `requestId`（原子カウンタ）で要求を相関し、期待と異なる型の応答はその要求をプロトコルエラーで即座に失敗させ、Host の状態へ反映しない。中身を持たない完了応答も命令の分類ごとに別の型とし、取り違えを型の不一致として検出する
- 期限: 通常は `COMMAND_ACK_TIMEOUT`、デバイス操作は `AUDIO_DEVICE_COMMAND_TIMEOUT`、`prepareTimelineSnapshot` は `TIMELINE_PREPARE_TIMEOUT` と呼び出し側の残り時間の短い方、`waitForTimelineIdle` は呼び出し側の残り時間を使う。期限切れは失敗報告で確定する

### 5.3 起動とプロトコル版

- サイドカーはデバイスの接続後、最初のメッセージとして `ready` イベント（`protocolVersion` と初期 `AudioStatus`）を 1 回だけ送る。版は両言語の `SIDECAR_PROTOCOL_VERSION`（`3`）で一致させる
- Rust は `ready` を受けたときだけその世代を ready にする。版が異なる場合はその世代を起動失敗とする。`ready` の前にプロセスが終了した場合も起動失敗である
- 版の確認は `ready` に一本化し、個々の命令は版を持たない

### 5.4 命令と応答

| 分類                | 命令                                                                                                                                  | 応答                                                                |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| 状態照会            | `status`                                                                                                                              | `audioStatus`                                                       |
| デバイス・安全      | `recoverAudioDevice`、`setAudioDriver`、`setEmergencyMute`、`setFeedbackProtection`、`setEngineTransitionMute`、`previewMasterGainDb` | `audioStatus`                                                       |
| 投影                | `prepareTimelineSnapshot`、`commitTimelineSnapshot`、`discardTimelineSnapshot`、`waitForTimelineIdle`                                 | `timelineAck`（prepare / commit / discard）、`timelineIdleAck`      |
| トランスポート      | `setTransportStarting`、`playTimeline`、`stopTimeline`、`seekTimeline`                                                                | `transportAccepted`                                                 |
| 録音                | `startArrangeRecording`、`stopArrangeRecording`                                                                                       | `audioStatus`                                                       |
| MIDI                | `enableMidiListening`、`disableMidiListening`、`setLiveMidiTarget`                                                                    | `audioStatus`                                                       |
|                     | `sendTrackMidi`、`panicTrackMidi`                                                                                                     | `midiAck`                                                           |
| トラック/プラグイン | `setTrackMix`                                                                                                                         | `trackMixAck`                                                       |
|                     | `setTrackDeviceBypassed`、`setTrackDeviceParameter`、`setTrackPluginState`、`openTrackPluginEditor`                                   | `trackDeviceAck`                                                    |
|                     | `getTrackDeviceStatus`、`getTrackDeviceParameters`、`getTrackDevicePrograms`                                                          | `trackDeviceStatus`、`trackDeviceParameters`、`trackDevicePrograms` |
|                     | `getTrackPluginState`、`setTrackDeviceProgram`                                                                                        | `trackPluginState`、`trackDeviceProgramChanged`                     |
| プレビュー          | `previewSample`、`previewInstrument`、`stopPreview`、`stopInstrumentPreview`                                                          | `audioStatus`                                                       |
| テイク比較          | `startTakeComparison`、`switchTakeComparisonVariant`、`stopTakeComparison`                                                            | `audioStatus`                                                       |

トランスポート、録音、MIDI 送信、グラフの公開はリアルタイム命令キューを通る。再生中は音声スレッドが次のブロック先頭で適用し、デバイス停止中は制御側が適用する（`architecture.md §5.6`）。`transportAccepted` は命令の受け付けと `commandSequence` を返し、適用済みの命令番号は `transportStatus.appliedCommandSequence` で分かる。

`setTrackMix` はアクティブな Track Runtime の Gain / Pan を一時的に更新する。`trackMixAck` は値の Canonical commit を意味しない。

`prepareTimelineSnapshot` は `TimelineSnapshot` を受け取り、C++ の厳格デコーダが契約違反を `kind: timelineContract` として返し、グラフの準備処理へ進めない。投影の置換には prepare / commit / discard を使う。

正準 Master Gain は `ExecutionGraph.masterGainDb` に含まれ、現役グラフの commit 時に出力へ適用される。`previewMasterGainDb` は一時プレビューのみを更新し、確定値は Host の `setMasterGainDb` が正準設定を更新して投影する。

MIDI 系の意味づけは次の通り。

| 命令                               | 意味                                                                                                                                                         |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `setLiveMidiTarget`                | Play Surface のフォーカスを Runtime-only 状態に設定する。`trackId: null` で解除する。対象トラックは同一 Track Runtime を使い、トラック間補償遅延のみ迂回する |
| `sendTrackMidi` / `panicTrackMidi` | 要求内の Track ID へライブ MIDI を直接送る                                                                                                                   |
| `setFeedbackProtection`            | フィードバック保護の切替。解除は `active: false` で行う                                                                                                      |

### 5.5 エラー

`error` と `fault` は共通の形（`kind`、`message`、`operation`、`details`）を持つ。`kind` は分類、`operation` は失敗した操作、`details` は機械可読の追加情報である。

| `kind`             | 意味                                                              |
| ------------------ | ----------------------------------------------------------------- |
| `invalidCommand`   | `requestId` は読めたが命令をデコードできない                      |
| `protocol`         | `requestId` も読めない行を受けた（`fault` で通知）                |
| `noResponse`       | 命令の処理が応答せずに終わった                                    |
| `timelineBusy`     | 投影の置換が進行中で受け付けられない。Rust は再試行可能として扱う |
| `timelineContract` | `TimelineSnapshot` の契約違反                                     |
| その他             | 各領域の失敗（`deviceLost`、`invalidAudioConfiguration` など）    |

Rust は解釈できない行を捨てない。`tracing` に記録し、`AudioStatus.diagnostics.protocolErrors` を加算し、行から `requestId` が読めればその要求を即座に失敗させる。プロセスは止めない。

### 5.6 デバイス切替（`setAudioDriver`）

Native が切替と旧デバイスへの復元を 1 トランザクションで行う。

```text
Host: EngineTransition を有効化
  → Native: デバイス切替を試行
    → 成功: 正準グラフを新環境へ投影 → 遷移ミュート解除
    → 拒否＋復元成功（details.restoredPreviousDevice: true）: 旧環境へ再投影 → 遷移ミュート解除
    → 復元失敗: deviceLost 扱い
```

### 5.7 録音フロー

```text
stopArrangeRecording → 停止を realtime 命令として受理し、受理時点の audioStatus で即応答
  → realtime 状態の所有者が停止を適用（Raw 確定＋Transport 停止）→ audioStatus イベント（recording.processing: true）
  → グラフ外で Processed をブロック単位に逐次生成する
  → recordingComplete → Asset 登録＋Arrangement 確定 → recording-finalized（境界 B）
```

- 応答は停止の受理を表し、適用前の `recording.active: true` を含みうる。適用後の状態は `audioStatus` イベントで届く
- カウントイン中の停止は録音を取り消す。応答は `recording.cancelled: true` を持ち、確定処理は行わない
- 失敗時は Raw あり → `recoverable`、Raw なし → `failed` へ必ず確定してから失敗を通知する
- `processing` 中の新規録音と Project 切替は排他する
- 進捗停滞（ブロック・VST 処理境界が一定時間停止）の場合のみ Native を終了して Rust の復旧経路へ移す

### 5.8 再生の非同期

- Play の投影準備は非同期に行い、`transportStatus: starting` と `runtime-projection-status` で進捗を通知する
- Stop は保留中の Play を取り消す

### 5.9 イベントと出力レーン

C++ の出力は 3 つのレーンに分かれる。`control` は順序を保つバリアで、書き込み時に滞留中の `telemetry` を捨てる。`state` はキーごとに最新の 1 件へ合流する。`telemetry` は損失を許す。

| type                                                      | レーン      | 内容                                                                                                                   |
| --------------------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------- |
| `ready`                                                   | `control`   | 起動完了、プロトコル版、初期状態                                                                                       |
| `fault`                                                   | `control`   | 要求に紐づかない構造化失敗（§5.5）                                                                                     |
| `recordingComplete`                                       | `control`   | Native の Raw / Processed / MIDI 出力の確定結果。`directory`、`success`、失敗時の `message` を持つ                     |
| `audioStatus`                                             | `state`     | 状態・デバイス・録音・MIDI・Preview・ミュート理由・コールバック診断。Rust は `AudioStatus` へ写像して境界 B へ転送する |
| `trackPluginStateChanged` / `trackPluginParameterChanged` | `state`     | エディタ操作等によるプラグイン状態の変化。キーはデバイス（パラメータ変化はデバイスとパラメータ番号）                   |
| `audioMeters`                                             | `telemetry` | ピーク・リミッター診断・無効サンプル・フィードバック検知・Track Meter                                                  |
| `transportStatus`                                         | `telemetry` | Transport の状態と再生位置、録音状態、適用済み命令番号、現役グラフの instrument faults。投影診断は別イベントで通知する |

`response` と `error` は `control` レーンを通る。

`transportStatus` は `state`（`stopped` / `starting` / `playing` / `faulted`）、`revision`、`timelineTick`、`timelineSample`、`audioClockSample`、`sampleRate`、`appliedCommandSequence`、`recordingPhase`、`recordingStartTick`、`recordingPassOrdinal`、`armedTrackIds`、`instrumentFaults`、`clockGeneration`、`discontinuity` を持つ。`timelineSample` は保留中の Seek 先を含み、`revision` と `sampleRate` は現役グラフがない場合に `null` となる。`instrumentFaults` は現役グラフの同じ診断情報を `AudioStatus.diagnostics.instrumentFaults` と共有する。

`audioMeters` の Track Meter は左右別 Peak / RMS、Master は左右別の最終出力 Peak を持つ。Native の Meter thread が約 50 ms ごとに発行し、Rust は Runtime 投影の `projectId` を付けて `audio-meters` Host event（`AudioMeterFrame`）へ写像する。

`feedbackSuspected` は `FeedbackProtection` のミュート理由と連動する

- ミュート理由は Native の bitmask を正本とし、所有者（ユーザー・遷移・障害・保護）ごとに解除する。Rust が保持するのはユーザー緊急ミュートの意図のみとする
- 未解決クリップと欠落デバイスは Native の Transport 通知に含めず、現役グラフの `ProjectionDiagnostics` として `RuntimeProjectionStatus.activeDiagnostics` から通知する

---

## 6. 境界 D: オフラインレンダリング（riffra-render）

| 項目 | 内容                                                                                                                                                                                                                                                                                            |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 起動 | `render_timeline` 命令ごとに `riffra-runtime::render` が `RuntimeBinaries` の executable を 1 プロセス起動する。配置規則は Desktop / Headless 共通                                                                                                                                              |
| 要求 | stdin へ JSON 1 行を書いて閉じる。`renderTimelineOffline` + `protocolVersion: 3` + `request`（`graph` / `destination` / `startTick` / `endTick` / `sampleRate` / `blockSize` / `normalize`）。Master Gain は `graph.masterGainDb` に含む                                                        |
| 応答 | stdout へ JSON 1 行。成功は `offlineRenderComplete`（`frames`、`sampleRate`）、失敗は `error`（`operation: renderTimelineOffline`。`kind` は要求の契約違反 `renderContract`、版の不一致 `protocol`、レンダー失敗 `renderRejected`）。形は `contracts/sidecar/messages/render.*.json` で固定する |
| 異常 | プロセス異常終了・応答の不一致・デコードできない応答はエラー扱いとし、部分的な WAV は破棄する。失敗時のエラーには stdout / stderr の末尾を抜粋として含める                                                                                                                                      |
| 分担 | 計画（範囲・出力先 `renders/render-{ms}/timeline.wav`・manifest）はシェル側で組み立て、ワーカーは実行のみ                                                                                                                                                                                       |

---

## 7. 境界 E: デバイスプローブとプラグインスキャン

| 起動引数                                              | 応答                                     | 用途                                                            |
| ----------------------------------------------------- | ---------------------------------------- | --------------------------------------------------------------- |
| `riffra-audio --probe`                                | `{"type":"audioDeviceProbe", ...}` を1行 | ASIO/WASAPI のドライバ・デバイスの列挙専用                      |
| `riffra-audio --probe-channels <driver> <device> ...` | `{"type":"deviceChannels", ...}`         | 指定デバイスのチャンネル構成                                    |
| `riffra-plugin-scan <args>`                           | 型タグ付き JSON Lines                    | VST3 の列挙・検証（スキャン結果は `ScanReport` としてジョブ化） |

- 直列化: 共有 Runtime の Probe Coordinator 経由。待機と実行の双方にタイムアウトを適用する
- 失敗時: 「デバイス状態は変更されていない」ことを明示して失敗する。プローブ専用起動であり、実行中の `--serve` セッションとは独立する
- 失敗は終了コードと stderr のメッセージで返し、stdout には成功時の結果だけを書く
- 厳格性: スキャン結果は stdout 全体を 1 件の JSON としてパースし、単一に収まらない出力は候補を隔離して失敗とする。失敗メッセージには stdout / stderr の抜粋を含める

---

## 8. 境界 F: Local ClientとRiffra Host制御

`riffra`にはStandalone、serve、Attachedの三つの実行モードがある。DesktopのHostConnectionManagerも同じControl Command（§3.2）とLocalHostClientを利用するため、EmbeddedとAttachedの制作操作は同じHost command境界を通る。CLIの一回実行と`--interactive`の各行も、送信前に`ControlCommand::decode`で型付きの命令にする。

| モード     | 状態の所有者                                         | 要求の経路                                         |
| ---------- | ---------------------------------------------------- | -------------------------------------------------- |
| Standalone | CLIの`DataRootLease`、`SessionStore`、`AppCore<()>`  | CoreとHostを直接利用                               |
| serve      | `DawHost`のDataRootLease、`AppCore<AudioSupervisor>` | Host Control Serverを公開                          |
| Attached   | 接続先HostのCore、履歴、Runtime、Asset DB            | Host Control Serverへ接続                          |
| Desktop    | Embedded DawHost、または選択したAttached Host        | in-process dispatchまたはHost Control Serverへ接続 |

- 排他: 同一 DataRoot の所有者は 1 Host のみ。Standalone / `serve` は所有者ありで起動失敗し、Desktop は接続へ回る
- 公開: 起動中 Host は `<data_root>/control/host.json` と current-user registry へ登録する。Attached CLI の探索先は registry のみとする
- 削除: プロセス不存在か別 Host 確定のときのみ登録を削除する。一時的到達不能は一覧から外すだけで登録は残す
- Hello では接続の役割を明示する

```json
{"type":"hello","role":"command"}
{"type":"hello","role":"events"}
```

| 種類    | 用途                                                |
| ------- | --------------------------------------------------- |
| command | 要求と応答を運ぶ。Desktopは要求ごとに開く           |
| events  | `HostEventFrame { event, payload }`を運ぶ。長く保つ |

- Desktop は要求ごとに command 接続を開くため、長時間要求の実行中も Transport 操作や緊急ミュートを並行処理できる。応答にはタイムアウトを設ける
- イベント配信では meter や transport status など最新値で足りる通知を上書き集約する。重要通知は必ず配送し、待ち行列が溢れても接続を維持する
- 初期同期はイベント接続の確立後に `host.bootstrap` を取得する。一覧表示は軽量な `host.info` を使い、`host.bootstrap` は接続確定時のみ使う
- Host一覧はRegistryの探索に成功したHostを基準とし、個別の`host.info`取得に失敗したHostを除外して他のHostの表示を継続する。Registry探索自体の失敗は一覧全体のエラーとする
- 切替は新接続と bootstrap の準備後に現 Host を交換し、世代を更新して旧 Host 由来の遅延を破棄する。切替は録音の完了後に行う。終了時は Disconnected とし、最終 DataRoot と instanceId を保持して再接続する。Project 切替は Host 切替と独立し、同一 Host 内の Active Project のみ変更する

### 8.1 起動とフレーミング

ワンショットは階層化された引数で1つの操作を実行する。

```bash
riffra --data-root ./data session get
riffra --data-root ./data project list
riffra --data-root ./data project create --name "New Song"
riffra --data-root ./data project open <project-id>
riffra --data-root ./data track add --name Bass --kind instrument
riffra --data-root ./data serve --safe-mode
riffra --attach session get
riffra host list
riffra --attach --host <instance-id> session get
```

- Attached は候補 1 件なら自動接続し、複数件なら `--host` による instanceId 明示を要求する。`host list` は registry 表示のみ行うローカル操作である
- 対話モードは標準入力 1 行を 1 要求とし、標準出力へ 1 行応答を flush する。空行は読み飛ばす

```bash
riffra --data-root ./data --interactive
riffra --attach --interactive
```

- 要求は `command` と `params` を持ち、`requestId` は応答へそのまま返す。`expectedSequence` 付き要求は正準シーケンス一致時のみ実行する
- Project-bound request には `expectedProjectId`（`project.list` / `host.bootstrap` で取得）を付ける。Live Host は `expectedProjectId` を必須とし、欠落・不一致は Conflict で返す。Standalone は自 Active Project で補完する

```json
{
  "requestId": "42",
  "command": "track.add",
  "expectedSequence": 18,
  "expectedProjectId": "550e8400-e29b-41d4-a716-446655440000",
  "params": { "name": "Bass", "kind": "instrument" }
}
```

- Attached では CLI が stdin 各行をフレームへ変換して送る。Host は 1 接続内を受信順に処理し、CLI は応答を 1 行ずつ flush する。フレームは 8 MiB 以下の UTF-8 JSON とする

### 8.2 応答とエラー

- 成功応答は対応する正準シーケンスを含む。`result.type` は命令ごとに表（§3.2）で決まる。`session.get` は `"session"`、正準状態の変更は `"arrangementMutation"`（`canonical` + `projection`、sequence 一致）、その他は固有型を使う
- 結果と sequence は同一 `CanonicalState` スナップショットから一貫して構築する

```json
{
  "requestId": "42",
  "ok": true,
  "sequence": 12,
  "result": { "type": "session", "value": {} }
}
```

| エラーコード         | 発生条件                                                                                     |
| -------------------- | -------------------------------------------------------------------------------------------- |
| `invalidRequest`     | 入力形式、`params`、型、未知のコマンド、Project-bound requestの`expectedProjectId`欠落が不正 |
| `commandFailed`      | Core、Host、保存処理が失敗                                                                   |
| `conflict`           | `expectedSequence`または`expectedProjectId`が現在の状態と不一致                              |
| `hostUnavailable`    | Attached CLIがHostへ接続できない                                                             |
| `runtimeUnavailable` | Safe Mode中など、要求されたRuntimeを利用できない                                             |

- 機械判定にはエラーコードを使い、message 文字列を解析しない

```json
{
  "requestId": "42",
  "ok": false,
  "sequence": 20,
  "error": {
    "code": "conflict",
    "message": "canonical state changed",
    "details": { "expectedSequence": 18, "currentSequence": 20 }
  }
}
```

- `undo` / `redo` の履歴は Standalone が自プロセス、serve / Attached が接続先 Host のものを共有する

`track.mix.preview` は Host Runtime command であり、Track ID と任意の `gainDb` / `pan` を受け取る。Host の現行 sequence を応答へ含めるが、Canonical sequence、履歴、保存、Project の世代を進めない。

### 8.3 制作操作

CLI は入力形式だけを解釈し、制作規則と正準化は `riffra-core::Application` に委譲する。

| 分類                   | コマンド                                                                                                                                                                                                                                                                                                                                          |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Session / History      | `session inspect`、`session get`、`session settings update`、`history get`、`undo`、`redo`                                                                                                                                                                                                                                                        |
| Track / Routing        | `track list`、`add`、`update`、`remove`、`duplicate`、`reorder`、`audio-input`、`midi-input`                                                                                                                                                                                                                                                      |
| Audio Clip             | `audio-clip list`、`add-asset`、`update`、`move`、`trim`、`split`、`duplicate`、`crossfade`                                                                                                                                                                                                                                                       |
| MIDI Clip / Note       | `midi-clip list`、`create`、`add-asset`、`update`、`move`、`trim`、`split`、`duplicate`、`midi-note add/insert/update/update-many/remove/remove-many/clear/quantize/transform/duplicate`                                                                                                                                                          |
| Music Operations       | `music.midi-clip.create/resize`、`music.note.list/get/insert/update/remove`、`music.region.list`、`music.region.add`、`music.region.update`、`music.region.remove`、`music.harmony.resolve`、`music.harmony.list`、`music.harmony.insert`、`music.harmony.update`、`music.harmony.remove`、`music.harmony.realize`、`music.phrase.insert`         |
| Timeline / Arrangement | `clip remove`、`clip paste`、`marker add/update/remove`、`timebase update`、`loop-range set`、`punch-range set`                                                                                                                                                                                                                                   |
| Automation             | `automation set`、`automation clear`                                                                                                                                                                                                                                                                                                              |
| Asset / Project        | `asset import-midi`、`asset preview`、`project list/create/open/rename/export/import`                                                                                                                                                                                                                                                             |
| Instrument / Effect    | `plugin catalog list`、`instrument list/apply`、`plugin instrument/effect`、`plugin scan/scan-start`、`instrument clear`、`effect remove/reorder`、`device bypass`、`device inspect`、`device parameter list/get/set`、`plugin preset list/get/set`、`plugin state get/set`                                                                       |
| Runtime services       | `audio status/diagnostics/probe/channels-probe`、`audio driver get/set`、`audio recover/startup-retry`、`record start/another-take/stop/status/list/rename/archive/promote/tag/delete/duplicates`、`render start`、`job get/cancel`、`library search/asset-update/related`、`analysis start`、`missing list/relink/disable-plugin/replace-plugin` |

軽量投影の約束: `session inspect`、`track list`、`device inspect`、`music.note.*` は構造把握用であり、本文の取得は `session get`、`plugin.state.get` に委譲する。`device inspect` の応答範囲は metadata と capability とする。

- Agent 向け CLI の正準 Mutation 応答は `mutation` receipt（sequence・投影状態・構造 ID のみ）へ変換する。Desktop 同期の共有プロトコルでは Canonical 結果を維持する
- `plugin state save` は結果をファイルへ保存し、標準出力には保存先のみ返す

Desktop の Tauri command 境界と Live Host の Control Server の機能分担は次の通り。

| 操作群                                                               | Desktop / serve                   | Standalone                                                                          |
| -------------------------------------------------------------------- | --------------------------------- | ----------------------------------------------------------------------------------- |
| Runtime投影・トランスポート                                          | HostのRuntime                     | `runtimeUnavailable`                                                                |
| 音声状態・Live MIDI                                                  | HostのAudio Runtime               | `runtimeUnavailable`                                                                |
| Built-in一覧・プラグイン一覧・VST音源/エフェクト・デバイスパラメータ | HostのInstrument / Plugin Runtime | Built-inの一覧・割り当てはCanonical編集として利用可能、その他は`runtimeUnavailable` |
| 欠落依存                                                             | HostのMissing service             | `runtimeUnavailable`                                                                |
| 録音                                                                 | HostのRecording                   | `runtimeUnavailable`                                                                |
| レンダー・ジョブ                                                     | HostのRenderWorker                | `runtimeUnavailable`                                                                |
| ライブラリ・解析・Asset preview                                      | Hostのshared service              | `runtimeUnavailable`                                                                |

- エディタ窓・ダイアログ・窓管理は Desktop shell に残る。open / 録音 / プレビュー / スキャン / 図書館・解析の実行は Host 側を使い、エディタ由来の永続化は Host 内 coordinator の commit で完結する
- `render start` は接続先 Host の `RenderWorker` にジョブ開始し ID を返す。部分 Render は音楽座標の `start` / `end` と任意の `trackId` で指定する。状態は `job get`、停止は `job cancel` で行う。Worker の所有者は接続先 Host とする
- `expectedSequence` は `render.start`、`undo`、`redo` にも適用する。Conflict 時は再 Inspect してやり直す。Revision token は同一 `AppCore` の有効期間内でのみ有効

---

## 9. 権限・ケイパビリティ（`src-tauri/capabilities/default.json`）

メインウィンドウは最小ケイパビリティで構成する。

| 権限                                         | 内容                     |
| -------------------------------------------- | ------------------------ |
| `core:default` / `core:window:allow-destroy` | コア操作とウィンドウ破棄 |
| `dialog:default`                             | ファイルダイアログ       |

---

## 10. NativeApi と境界の対応規則

`src/native/api/` は Control Command と Desktop 固有の Tauri 命令をドメイン用語の capability interface へ写像する。命令名・引数名の知識は capability 層に集約し、各 Feature は必要な capability だけに依存する。低レベル API の import は ESLint で `src/native/` 配下に限定する。

- Host 所有の method は `dispatchControl` 系で Control Command を送る。命令名と Params は生成された型で検査される。開始時と応答時の connection generation が一致した応答のみ成功とする。bootstrap・Host 切替・Reconnect は現在 generation を更新する
- Window・dialog・Host selector など Desktop shell 所有の method は通常の `invoke` を使う。再同期範囲は Host 所有の method に限る
- 制作状態を変更する method は `CanonicalState` を含む結果を返す。起動時は履歴可否を Core の HistoryState で判定する
- `previewTrackMix` と `previewMasterGainDb` は Runtime-only の一時プレビューであり、CanonicalStateを返すMutationではない。確定操作はそれぞれ `updateTrack` と `setMasterGainDb` を使い、結果は正準状態と投影結果を含む `ArrangementMutationResult` を返す
- 音声系 method は `AudioStatus` を返し、状態遷移と再試行は Audio 設定 Feature に集約する
- 失敗は `NativeCommandError` の `code` と `details` で分岐する。Native の `kind` / `operation` は `details` 内の `nativeAudio` 情報から参照する
- テストでは `native-api-fake.ts` を注入し、呼び出し記録・設定済み応答と失敗・イベント発火だけを扱う。制作規則・履歴・validation は Core のテストが担う
