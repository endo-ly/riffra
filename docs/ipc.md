# Riffra IPC 契約

## 1. スコープ

本書は Riffra の IPC 境界とその契約を正準化する。「どうやり取りするか」を示し、「何がやり取りされるか」の詳細は各言語のコードを真実源とする。

### 書くこと

- IPC 境界の全体像と使い分け基準
- Tauri 命令のカタログ（領域ごとの分類と責務、実行モード）
- NativeApi TS 契約と Tauri 命令との対応規則
- サイドカー JSON Lines プロトコルの構造と規則（音声・レンダー・プローブ）
- CLI と Riffra Host 制御のプロトコル境界
- 境界ごとのエラー・状態遷移の契約
- 権限・ケイパビリティ設定

### 書かないこと

- 各 Tauri 命令の引数・戻り値の詳細（code 参照）
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

---

## 3. 境界 A: Tauri 命令（WebView → Rust）

### 3.1 実行モード

命令は責務に応じて 3 つの実行モードを使い分ける。すべて `spawn_blocking` 経由で async ワーカーから分離して実行する。

| モード                              | 挙動                                                                                                                | 使う命令                                           |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| `run_blocking`                      | Host command gate を取得してから実行（正準セッション操作と保存を直列化）                                            | 楽曲編集・ライブラリ操作・素材操作の大半           |
| `run_blocking_without_command_gate` | ゲートなしで blocking 実行。読み取り専用処理と、VSTライフサイクル中にホストゲートを保持できない処理                 | プローブ、スキャン、録音一覧、プラグイン接続など   |
| `run_runtime_control`               | Runtime側は永続セッションを変更しないsnapshot読み取りで実行。Project-bound requestはHost入口でProject切替と排他する | play / stop / seek、MIDI送信、プレビュー、ミュート |

### 3.2 命令カタログ

領域ごとに代表を示す。全命令は `src-tauri/src/**/commands.rs` と `lib.rs` の `invoke_handler` が真実源。

**起動・全体（lib.rs / startup.rs / audio_preferences.rs）**

| 命令                                                                                                 | 責務                                                                                              |
| ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `get_bootstrap_state`                                                                                | CanonicalState・ProjectState・Built-in instrument catalog・セーフモード・回復候補の初期状態を返す |
| `get_audio_status`                                                                                   | 音声状態の照会                                                                                    |
| `probe_audio_devices` / `probe_device_channels`                                                      | オーディオデバイス・チャンネル列挙（境界E経由）                                                   |
| `set_emergency_mute` / `reset_feedback_protection` / `set_master_gain_db` / `preview_master_gain_db` | 安全制御とマスターゲイン                                                                          |
| `recover_audio_device` / `retry_startup_runtime`                                                     | デバイス回復・スタートアップ再試行                                                                |
| `restore_recovery_generation`                                                                        | 世代からの回復                                                                                    |

**セッション・アレンジ（session/commands/）**

| 領域               | 命令                                                                                                                                                                                                                                                                                                                                                                                                    |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| タイムラインレンジ | `update_timeline_loop_range`、`update_timeline_punch_range`、`update_arrangement_timebase`                                                                                                                                                                                                                                                                                                              |
| トラック           | `add_track`、`duplicate_track`、`remove_track`、`reorder_track`、`update_track`、`set_track_audio_input`、`set_track_midi_input`、`list_built_in_instruments`、`set_track_built_in_instrument`、`set_track_vst3_instrument`、`clear_track_instrument`、`set_track_device_bypassed`、`set_track_device_parameter`                                                                                        |
| クリップ           | `add_audio_clip_to_arrangement`、`add_midi_clip_to_arrangement`、`create_midi_clip`、`update_audio_clip`、`update_midi_clip`、`move_audio_clips`、`move_midi_clips`、`trim_audio_clip`、`trim_midi_clip`、`split_audio_clip`、`split_midi_clip`、`crossfade_audio_clips`、`duplicate_audio_clip`、`duplicate_midi_clip`、`remove_timeline_clips`、`paste_timeline_clips`、`set_audio_clip_take_variant` |
| ノート             | `add_midi_note`、`insert_midi_notes`、`update_midi_note`、`update_midi_notes`、`remove_midi_note`、`remove_midi_notes`、`duplicate_midi_notes`、`quantize_midi_notes`                                                                                                                                                                                                                                   |
| オートメーション   | `set_track_automation`                                                                                                                                                                                                                                                                                                                                                                                  |
| マーカー           | `add_marker`、`update_marker`、`remove_marker`                                                                                                                                                                                                                                                                                                                                                          |
| 設定               | `update_session_settings`                                                                                                                                                                                                                                                                                                                                                                               |
| 履歴               | `undo_session`、`redo_session`、`get_history_state`                                                                                                                                                                                                                                                                                                                                                     |
| Project            | `list_projects`、`create_project`、`open_project`、`rename_project`、`export_project`、`import_project`                                                                                                                                                                                                                                                                                                 |
| 素材入出力         | `import_midi_file`、`import_midi_bytes`                                                                                                                                                                                                                                                                                                                                                                 |
| 欠落依存           | `get_missing_dependencies`、`relink_missing_dependency`、`disable_missing_plugin`、`replace_missing_track_plugin`                                                                                                                                                                                                                                                                                       |

- `open_project` は同一 DataRoot 内の Project container を直接切り替える
- 外部 package を扱うのは `import_project` と `export_project` のみ。Import は既存 Project を残したまま新規 container を Active にし、Export の出力先は指定 path のみとする

**プラグイン（plugins/commands.rs）**: `scan_vst3_folder`、`start_scan_job`、`open_track_plugin_editor`。エディタ由来の state / parameter 変更は Host 内の coalesce と正準保存で完結する。

**録音（recording/commands.rs）**

| 領域           | 命令                                                                                                                                                |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| 録音制御       | `start_arrange_recording`、`stop_arrange_recording`、`record_another_take`                                                                          |
| テイク         | `activate_take`、`place_take_as_separate_clip`、`start_take_comparison`、`switch_take_comparison_variant`、`stop_take_comparison`                   |
| キャプチャ管理 | `list_recordings`、`rename_recording`、`archive_recording`、`promote_recording`、`tag_recording`、`delete_recording`、`detect_duplicate_recordings` |

**素材・ライブラリ（asset / library / analysis / render / plugins commands）**

| 領域       | 命令                                                               |
| ---------- | ------------------------------------------------------------------ |
| ライブラリ | `search_library`、`related_library_assets`、`update_library_asset` |
| プレビュー | `preview_asset`、`stop_preview`                                    |
| 解析       | `analyze_asset`（同期）                                            |
| レンダー   | `render_timeline`                                                  |

**ランタイム投影**: `get_runtime_projection_status`、`retry_runtime_projection`

**演奏・トランスポート（session/transport.rs / runtime）**: `play_timeline`、`stop_timeline`、`seek_timeline`、`go_to_start_timeline`、`send_midi_to_track`、`set_live_midi_target`、`panic_midi_track`、`enable_midi_listening`、`disable_midi_listening`

### 3.3 エラー規約

- 失敗は `NativeCommandError` として `code`、`message`、`details` を返す。`message` は表示用、`code` と `details` は機械判定用であり、UI はメッセージ文字列を解析しない
- Native 音声エラーは `kind`、`operation`、`details` を保ったまま `NativeAudioError`、Host の `ProtocolError`、Tauri の `NativeCommandError` へ渡す。境界ごとに情報を文字列へ潰さない
- セーフモード中の音声系・プラグイン系命令は明示エラーを返す（`architecture.md §7`）
- 要求された操作に失敗した場合は、現在の状態を保ったままエラーと状態を返す
- 制作状態を変更する命令の応答に含まれる `CanonicalState` は「その操作を含む最新の正準状態」であり、UI は `canonical.session` を表示状態へ反映する

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

- 順序の所有者は Core と Host command gate。フロントエンドは応答の `CanonicalState` を確定順序として受け入れる
- 中間値を捨ててよい連続制御は集約して最終値のみ送信する。集約された待機者には同一の確定応答を返す
- `invokeHostOrFallback` は非ネイティブ環境（ブラウザプレビュー・スモークテスト）専用のフォールバック。ネイティブ実行時は失敗をそのまま reject する

---

## 4. 境界 B: シェル → WebView イベント

| イベント                    | ペイロード                          | 意味                                                                                        |
| --------------------------- | ----------------------------------- | ------------------------------------------------------------------------------------------- |
| `runtime-startup-finished`  | `{ succeeded }`                     | スタートアップ時のランタイム初期化完了（セーフモードでは即通知）                            |
| `audio-status`              | `AudioStatus`                       | デバイス、コールバック、安全ミュート理由、MIDI、Preview、音声診断の状態                     |
| `audio-meters`              | `AudioMeters`                       | 入力・出力ピーク、無効サンプル数、ミュート理由（高頻度）                                    |
| `transport-status`          | `TransportStatus`                   | トランスポート状態（`stopped` / `starting` / `playing`、再生位置）                          |
| `runtime-projection-status` | `RuntimeProjectionStatus`           | 非同期のランタイム投影状態と世代・音声環境 revision（queued / preparing / active / failed） |
| `runtime-restarted`         | `{ generation }`                    | サイドカー再起動（世代番号）。RustがCoreの最新スナップショットを再投影する                  |
| `canonical-state-changed`   | `CanonicalState`                    | GUI以外のHost操作を含む正準セッション、シーケンス、履歴の変更                               |
| `recording-finalized`       | `{ directory, succeeded, message }` | Native処理後の録音Asset登録とArrangement確定の完了結果                                      |
| `project-state-changed`     | `ProjectState`                      | Projectの作成・改名・Importによる一覧の変更                                                 |
| `project-activated`         | `ProjectActivationResult`           | Project切替の完了。Active Projectの一覧、CanonicalState、RecoveryStateを一括で通知する      |

- 購読は `src/native/api/events.ts` のラッパ経由。用途は表示更新に限る
- エディタ由来の state / parameter 変更は Host 内の正準保存で完結する

---

## 5. 境界 C: 音声サイドカー（riffra-audio）

### 5.1 接続とフレーミング

- 起動: `riffra-audio --serve`。`AudioSupervisor` が起動を待ち（`SIDECAR_READY_TIMEOUT`）、起動ごとに世代番号を採番する
- 送受信: JSON Lines（1 コマンド = stdin 1 行、1 応答 = stdout 1 行）
- 相関: `command_bus.rs` が `requestId`（原子カウンタ）を付与し、応答は同一 ID を返す。`Condvar` で待機側へ配送する
- 期限: 通常は `COMMAND_ACK_TIMEOUT`、`prepareTimelineSnapshot` は `TIMELINE_PREPARE_TIMEOUT`。期限切れは失敗報告で確定する

### 5.2 コマンド分類

| 分類                | コマンド                                                                                                                          |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| 状態照会            | `status`、`meterStatus`                                                                                                           |
| 投影                | `prepareTimelineSnapshot`、`commitTimelineSnapshot`、`discardTimelineSnapshot`                                                    |
| トランスポート      | `playTimeline`、`stopTimeline`、`seekTimeline`                                                                                    |
| デバイス・安全      | `recoverAudioDevice`、`setAudioDriver`、`setEmergencyMute`、`setFeedbackProtection`、`setEngineTransitionMute`、`setMasterGainDb` |
| トラック/プラグイン | `setTrackDeviceBypassed`、`setTrackDeviceParameter`、`openTrackPluginEditor`                                                      |
| 録音                | `startArrangeRecording`、`stopArrangeRecording`                                                                                   |
| プレビュー          | `previewSample`、`stopPreview`、`stopPreviewForKey`                                                                               |
| テイク比較          | `startTakeComparison`、`switchTakeComparisonVariant`、`stopTakeComparison`                                                        |
| MIDI                | `enableMidiListening`、`disableMidiListening`、`setLiveMidiTarget`、`sendTrackMidi`、`panicTrackMidi`                             |
| トランスポート準備  | `setTransportStarting`                                                                                                            |

MIDI 系の意味づけは次の通り。

| コマンド                           | 意味                                                                                                                             |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `setLiveMidiTarget`                | Play Surface のフォーカスを Runtime-only 状態に設定する。対象トラックは同一 Track Runtime を使い、トラック間補償遅延のみ迂回する |
| `sendTrackMidi` / `panicTrackMidi` | 要求内の Track ID へライブ MIDI を直接送る                                                                                       |
| `setFeedbackProtection`            | フィードバック保護の切替。解除は `active: false` で行う                                                                          |

### 5.3 応答形式

```jsonc
// 成功: 状態かメーターのいずれか
{"type": "audioStatus", "requestId": N, ...}
{"type": "audioMeters", "requestId": N, ...}
// 失敗: 構造化エラー
{"type": "error", "requestId": N, "kind": "...", "message": "...", "operation": "...", "details": {...}}
```

- `kind` は分類、`operation` は失敗した操作、`details` は機械可読の追加情報
- ack 待ちの間も状態イベントは流れ続ける

### 5.4 デバイス切替（`setAudioDriver`）

Native が切替と旧デバイスへの復元を 1 トランザクションで行う。

```text
Host: EngineTransition を有効化
  → Native: デバイス切替を試行
    → 成功: 正準グラフを新環境へ投影 → 遷移ミュート解除
    → 拒否＋復元成功（details.restoredPreviousDevice: true）: 旧環境へ再投影 → 遷移ミュート解除
    → 復元失敗: deviceLost 扱い
```

### 5.5 録音フロー

```text
stopArrangeRecording → Raw 確定＋Transport 停止 → recording.processing: true で即応答
  → グラフ外で Processed をブロック単位に逐次生成する
  → recordingComplete → Asset 登録＋Arrangement 確定 → recording-finalized（境界 B）
```

- 失敗時は Raw あり → `recoverable`、Raw なし → `failed` へ必ず確定してから失敗を通知する
- `processing` 中の新規録音と Project 切替は排他する
- 進捗停滞（ブロック・VST 処理境界が一定時間停止）の場合のみ Native を終了して Rust の復旧経路へ移す

### 5.6 再生の非同期

- Play の投影準備は非同期に行い、`transportStatus: starting` と `runtime-projection-status` で進捗を通知する
- Stop は保留中の Play を取り消す

### 5.7 サイドカー → Rust イベント

| type                                                      | 内容                                                                                                                       |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `audioStatus`                                             | 状態・デバイス・録音・MIDI・Preview・ミュート理由・コールバック診断の要約（Rust は `AudioStatus` へ正規化して境界Bへ転送） |
| `audioMeters`                                             | ピーク・リミッター診断・無効サンプル・ミュート理由・フィードバック検知。Preview状態の変化は `audioStatus` として通知       |
| `transportStatus`                                         | `stopped` / `starting` / `playing` と再生位置の変化                                                                        |
| `recordingComplete`                                       | NativeのRaw / Processed / MIDI出力の確定結果。`directory`、`success`、失敗時の`message`を持つ                              |
| `trackPluginStateChanged` / `trackPluginParameterChanged` | エディタ操作等によるプラグイン状態の変化                                                                                   |
| `keepAlive`                                               | 生存確認（Rustは無視）                                                                                                     |
| `error`                                                   | `kind`、`message`、`operation`、`details` を持つ構造化失敗通知                                                             |

- `feedbackSuspected` は `FeedbackProtection` のミュート理由と連動する
- ミュート理由は Native の bitmask を正本とし、所有者（ユーザー・遷移・障害・保護）ごとに解除する。Rust が保持するのはユーザー緊急ミュートの意図のみとする

---

## 6. 境界 D: オフラインレンダリング（riffra-render）

| 項目 | 内容                                                                                                                                                                                                |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 起動 | `render_timeline` 命令ごとに `riffra-runtime::render` が `RuntimeBinaries` の executable を 1 プロセス起動する。配置規則は Desktop / Headless 共通                                                  |
| 要求 | stdin へ JSON 1 行を書いて閉じる。`renderTimelineOffline` + `protocolVersion: 1` + `snapshot` / `destination` / `startTick` / `endTick` / `sampleRate` / `blockSize` / `masterGainDb` / `normalize` |
| 応答 | stdout へ JSON 1 行。成功は `offlineRenderComplete`、失敗は `error`（`kind: renderRejected`、`operation: renderTimelineOffline`）                                                                   |
| 異常 | プロセス異常終了・応答不一致はエラー扱いとし、部分的な WAV は破棄する                                                                                                                               |
| 分担 | 計画（範囲・出力先 `renders/render-{ms}/timeline.wav`・manifest）はシェル側で組み立て、ワーカーは実行のみ                                                                                           |

---

## 7. 境界 E: デバイスプローブとプラグインスキャン

| 起動引数                                              | 応答                                     | 用途                                                            |
| ----------------------------------------------------- | ---------------------------------------- | --------------------------------------------------------------- |
| `riffra-audio --probe`                                | `{"type":"audioDeviceProbe", ...}` を1行 | ASIO/WASAPI のドライバ・デバイスの列挙専用                      |
| `riffra-audio --probe-channels <driver> <device> ...` | `{"type":"deviceChannels", ...}`         | 指定デバイスのチャンネル構成                                    |
| `riffra-plugin-scan <args>`                           | 型タグ付き JSON Lines                    | VST3 の列挙・検証（スキャン結果は `ScanReport` としてジョブ化） |

- 直列化: 共有 Runtime の Probe Coordinator 経由。待機と実行の双方にタイムアウトを適用する
- 失敗時: 「デバイス状態は変更されていない」ことを明示して失敗する。プローブ専用起動であり、実行中の `--serve` セッションとは独立する
- 失敗も `kind` / `operation` / `details` 付きの構造化応答として扱う

---

## 8. 境界 F: Local ClientとRiffra Host制御

`riffra`にはStandalone、serve、Attachedの三つの実行モードがある。DesktopのHostConnectionManagerも同じ`riffra-control::ControlCommand`とLocalHostClientを利用するため、EmbeddedとAttachedの制作操作は同じHost command境界を通る。

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

- 成功応答は対応する正準シーケンスを含む。`session.get` は `result.type: "session"`、投影連動の変更は `result.type: "arrangementMutation"`（`canonical` + `projection`、sequence 一致）、その他は固有型を使う
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
| Rack state             | `plugin catalog list`、`instrument builtin list/set`、`plugin instrument/effect`、`plugin scan/scan-start`、`instrument clear`、`effect remove/reorder`、`device bypass`、`device inspect`、`device parameter list/get/set`、`plugin preset list/get/set`、`plugin state get/set`                                                                 |
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

`src/native/api/` は Tauri 命令をドメイン用語の capability interface へ写像する。コマンド名・引数名の知識は capability 層に集約し、各 Feature は必要な capability だけに依存する。低レベル API の import は ESLint で `src/native/` 配下に限定する。

- Host 所有の method は `invokeHost` を使う。開始時と応答時の connection generation が一致した応答のみ成功とする。bootstrap・Host 切替・Reconnect は現在 generation を更新する
- Window・dialog・Host selector など Desktop shell 所有の method は通常の `invoke` を使う。再同期範囲は Host 所有の method に限る
- 制作状態を変更する method は `CanonicalState` を含む結果を返す。起動時は履歴可否を Core の HistoryState で判定する
- 音声系 method は `AudioStatus` を返し、状態遷移と再試行は Audio 設定 Feature に集約する
- 失敗は `NativeCommandError` の `code` と `details` で分岐する。Native の `kind` / `operation` は `details` 内の `nativeAudio` 情報から参照する
- テストでは `native-api-fake.ts` を注入し、呼び出し記録・設定済み応答と失敗・イベント発火だけを扱う。制作規則・履歴・validation は Core のテストが担う
