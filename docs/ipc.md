# Riffra IPC 契約

## 1. スコープ

本書はRiffraのIPC境界とその契約を正準化する。「どうやり取りするか」を示し、「何がやり取りされるか」の詳細は各言語のコードを真実源とする。

### 書くこと

- IPC境界の全体像と使い分け基準
- Tauri命令のカタログ（領域ごとの分類と責務、実行モード）
- NativeApi TS契約とTauri命令との対応規則
- サイドカー JSON Lines プロトコルの構造と規則（音声・レンダー・プローブ）
- CLI JSON Lines プロトコルの現在の境界
- 境界ごとのエラー・状態遷移の契約
- 権限・ケイパビリティ設定

### 書かないこと

- 各Tauri命令の引数・戻り値の詳細（code参照）
- サイドカーコマンドの全シグネチャ（code参照）
- 各メッセージの全フィールド（code参照）

層構造の全体像は `architecture.md`、エンティティの定義は `data-model.md` を参照。

---

## 2. 境界の全体像

```text
┌────────────────────────────── WebView ──────────────────────────────┐
│ React（src/native/native-api.ts）                                    │
└────┬───────────────────────┬───────────────────────┬───────────────┘
     │ A: Tauri 命令          │ B: イベント購読        │
     │ invoke 系             │ listen(7種)           │
┌────▼───────────────────────▼───────────────────────▼───────────────┐
│ Rust バックエンド（src-tauri）                                          │
│ 命令層 → Desktop Adapter → riffra-core / RuntimeReconciler / 永続化    │
└────┬───────────────┬───────────────┬────────────────┬───────────────┘
     │ C: JSON Lines  │ D: JSON 1行   │ E: JSON 1行     │
     │ stdin/stdout   │ stdin/stdout  │ stdout         │
┌────▼───────┐  ┌─────▼────────┐  ┌──▼──────────┐  ┌───▼───────────┐
│riffra-audio│  │riffra-render │  │riffra-audio │  │riffra-plugin- │
│ --serve    │  │ -worker      │  │ --probe系   │  │ scan          │
│ 常駐・音声  │  │ レンダ要求1  │  │ デバイス列挙│  │ VST3スキャン   │
└────────────┘  │ 回ごとに起動  │  └─────────────┘  └───────────────┘
                └──────────────┘
```

| 境界 | 方向                         | 方式                                    | 用途                                                     |
| ---- | ---------------------------- | --------------------------------------- | -------------------------------------------------------- |
| A    | WebView → Rust               | `invoke`（Tauri command）               | 一切の操作・編集・照会                                   |
| B    | Rust → WebView               | Tauri event                             | 音声状態・メーター・トランスポート・ランタイム回復の通知 |
| C    | Rust ↔ riffra-audio          | 子プロセスの stdin/stdout（JSON Lines） | 投影・演奏・録音・MIDI・プレビュー・デバイス制御         |
| D    | Rust → riffra-render-worker  | 子プロセスの stdin/stdout（JSON 1行）   | オフラインレンダリング（1要求1プロセス）                 |
| E    | Rust ↔ riffra-audio（probe） | 子プロセスの stdout（JSON 1行）/ 引数   | デバイス・チャンネル列挙、VST3スキャン                   |
| F    | Host ↔ riffra-cli            | stdin/stdout（JSON Lines、対話モード）  | セッションの最小操作を外部Hostから実行                   |

使い分け基準: 常時稼働で低レイテンシが必要な音声経路は C、完了まで数秒〜数分かかるバッチは D、UI の都度起動が不要な一括列挙は E、GUIを持たない外部Hostからの最小セッション操作は F、それ以外のデスクトップ操作は A。

---

## 3. 境界 A: Tauri 命令（WebView → Rust）

### 3.1 実行モード

命令は責務に応じて3つの実行モードを使い分ける。すべて `spawn_blocking` で async ワーカーを塞がない。

| モード                              | 挙動                                                                                                | 使う命令                                           |
| ----------------------------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| `run_blocking`                      | Desktop command gate を取得してから実行（正準セッション操作と保存を直列化）                         | 楽曲編集・ライブラリ操作・素材操作の大半           |
| `run_blocking_without_command_gate` | ゲートなしで blocking 実行。読み取り専用処理と、VSTライフサイクル中にホストゲートを保持できない処理 | プローブ、スキャン、録音一覧、プラグイン接続など   |
| `run_runtime_control`               | ゲートなし。永続セッションを決して変更しない音声制御（snapshot の読み取りだけ）                     | play / stop / seek、MIDI送信、プレビュー、ミュート |

### 3.2 命令カタログ

領域ごとに代表を示す。全命令は `src-tauri/src/**/commands.rs` と `lib.rs` の `invoke_handler` が真実源。

**起動・全体（lib.rs / startup.rs / audio_preferences.rs）**

| 命令                                                                   | 責務                                                    |
| ---------------------------------------------------------------------- | ------------------------------------------------------- |
| `get_bootstrap_state`                                                  | CreativeSession・セーフモード・回復候補の初期状態を返す |
| `get_audio_status` / `get_runtime_projection_status`                   | 音声状態・ランタイム投影状態の照会                      |
| `probe_audio_devices` / `probe_device_channels`                        | オーディオデバイス・チャンネル列挙（境界E経由）         |
| `set_emergency_mute` / `set_master_gain_db` / `preview_master_gain_db` | 安全制御とマスターゲイン                                |
| `recover_audio_device` / `retry_startup_runtime`                       | デバイス回復・スタートアップ再試行                      |
| `restore_recovery_generation`                                          | 世代からの回復                                          |
| `run_native_probe`（内部）                                             | probe サイドカーの直列実行コーディネータ                |

**セッション・アレンジ（session/commands/）**

| 領域               | 命令                                                                                                                                                                                                                                                                                                                                                                                                    |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| タイムラインレンジ | `update_timeline_loop_range`、`update_timeline_punch_range`、`update_arrangement_timebase`                                                                                                                                                                                                                                                                                                              |
| トラック           | `add_track`、`duplicate_track`、`remove_track`、`reorder_track`、`update_track`、`set_track_audio_input`、`set_track_midi_input`、`set_track_instrument`、`clear_track_instrument`、`set_track_device_bypassed`、`set_track_device_parameter`                                                                                                                                                           |
| クリップ           | `add_audio_clip_to_arrangement`、`add_midi_clip_to_arrangement`、`create_midi_clip`、`update_audio_clip`、`update_midi_clip`、`move_audio_clips`、`move_midi_clips`、`trim_audio_clip`、`trim_midi_clip`、`split_audio_clip`、`split_midi_clip`、`crossfade_audio_clips`、`duplicate_audio_clip`、`duplicate_midi_clip`、`remove_timeline_clips`、`paste_timeline_clips`、`set_audio_clip_take_variant` |
| ノート             | `add_midi_note`、`insert_midi_notes`、`update_midi_note`、`update_midi_notes`、`remove_midi_note`、`remove_midi_notes`、`duplicate_midi_notes`、`quantize_midi_notes`                                                                                                                                                                                                                                   |
| オートメーション   | `set_track_automation`                                                                                                                                                                                                                                                                                                                                                                                  |
| マーカー           | `add_marker`、`update_marker`、`remove_marker`                                                                                                                                                                                                                                                                                                                                                          |
| 設定               | `update_session_settings`                                                                                                                                                                                                                                                                                                                                                                               |
| 履歴               | `undo_session`、`redo_session`、`get_history_state`                                                                                                                                                                                                                                                                                                                                                     |
| セッション入出力   | `export_scratch_session`、`import_scratch_session`、`import_midi_file`、`import_midi_bytes`                                                                                                                                                                                                                                                                                                             |
| 欠落依存           | `get_missing_dependencies`、`relink_missing_dependency`、`disable_missing_plugin`、`replace_missing_track_plugin`                                                                                                                                                                                                                                                                                       |

**プラグイン（plugins/commands.rs）**: `scan_vst3_folder`、`start_scan_job`、`open_track_plugin_editor`、`persist_track_plugin_state`、`persist_track_plugin_parameter`

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
| 解析       | `analyze_asset`                                                    |
| レンダー   | `render_timeline`                                                  |

**ランタイム回復**: `retry_runtime_projection`

**演奏・トランスポート（session/transport.rs / runtime）**: `play_timeline`、`stop_timeline`、`seek_timeline`、`go_to_start_timeline`、`send_midi_to_track`、`panic_midi_track`、`enable_midi_listening`、`disable_midi_listening`

### 3.3 エラー規約

- 全命令は `Result<T, String>` を返す。失敗は人間可読な説明文字列となり、`dataSafe` 相当の保証（音声・保存データは安全）はメッセージに含める
- セーフモード中の音声系・プラグイン系命令は明示エラーを返す（`architecture.md §7`）
- 制作状態を変更する命令が返す CreativeSession は「その操作を含む最新の正準セッション」であり、UI はそれを表示状態へ反映する

### 3.4 UI呼び出しの順序

制作状態を変更する命令の順序はCoreとDesktop command gateが所有する。応答はCoreの確定順序でCreativeSessionへ反映されるため、フロントエンド独自の直列化、時刻比較、セッション全体のマージは行わない。

連続操作で中間値を送る意味がない制御は、同じ対象への要求を集約して最後の値を送る。集約された要求を待つ呼び出し元には、同じ確定応答を返す。

`invokeOrFallback` は非ネイティブ環境（ブラウザプレビュー・スモークテスト）でフォールバック値を返す。ネイティブ実行時は実害のないフォールバックをせず、失敗はそのまま reject する。

---

## 4. 境界 B: シェル → WebView イベント

| イベント                         | ペイロード                            | 意味                                                                                             |
| -------------------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `runtime-startup-finished`       | `{ succeeded }`                       | スタートアップ時のランタイム初期化完了（セーフモードでは即通知）                                 |
| `audio-status`                   | `AudioStatus`                         | 音声状態の変更（ready / muted / starting / faulted / offline、緊急ミュート、フィードバック検知） |
| `audio-meters`                   | `AudioMeters`                         | 入力・出力ピーク、無効サンプル数（高頻度）                                                       |
| `transport-status`               | `TransportStatus`                     | トランスポート状態（再生位置・再生中フラグ）                                                     |
| `runtime-restarted`              | `{ generation }`                      | サイドカー再起動（世代番号）。RustがCoreの最新スナップショットを再投影する                       |
| `track-plugin-state-changed`     | `{ trackId, deviceId, ... }`          | プラグイン状態（ロード・バイパス）の変化                                                         |
| `track-plugin-parameter-changed` | `{ trackId, deviceId, index, value }` | プラグインパラメータの変化                                                                       |

購読は全て `src/native/api/events.ts` の `listen` ラッパを経由する。イベントは Rust が正準状態に基づいて発行する投影通知であり、UI はこれを表示の更新にのみ使う（これは楽曲編集の入力経路ではない）。

---

## 5. 境界 C: 音声サイドカー（riffra-audio）

### 5.1 接続とフレーミング

- 起動: `riffra-audio.exe --serve`。Tauri 側は起動を待ち（`SIDECAR_READY_TIMEOUT`）、起動ごとに世代番号を採番する
- 送受信: Rust は **1コマンド = 1行のJSON** を stdin に書き、サイドカーは **1行のJSON** で応答する（JSON Lines）
- 相関: コマンドバス（`command_bus.rs`）が各コマンドに `requestId`（原子カウンタ）を付与する。応答は同一 `requestId` を返し、`Condvar` で待機側へ届く
- タイムアウト: 通常コマンドは `COMMAND_ACK_TIMEOUT`。投影の `prepareTimelineSnapshot` は `TIMELINE_PREPARE_TIMEOUT` に制限（遅いVSTはセッション操作をブロックしない）

### 5.2 コマンド分類

| 分類                | コマンド                                                                                    |
| ------------------- | ------------------------------------------------------------------------------------------- |
| 状態照会            | `status`、`meterStatus`                                                                     |
| 投影                | `prepareTimelineSnapshot`、`commitTimelineSnapshot`、`discardTimelineSnapshot`              |
| トランスポート      | `playTimeline`、`stopTimeline`、`seekTimeline`                                              |
| デバイス・安全      | `recoverAudioDevice`、`setAudioDriver`、`setEmergencyMute`、`setMasterGainDb`               |
| トラック/プラグイン | `setTrackDeviceBypassed`、`setTrackDeviceParameter`、`openTrackPluginEditor`                |
| 録音                | `startArrangeRecording`、`stopArrangeRecording`（raw/processed のパスとフレーム範囲を渡す） |
| プレビュー          | `previewSample`、`stopPreview`                                                              |
| テイク比較          | `startTakeComparison`、`switchTakeComparisonVariant`、`stopTakeComparison`                  |
| MIDI                | `enableMidiListening`、`disableMidiListening`、`sendTrackMidi`、`panicTrackMidi`            |

### 5.3 応答とエラー

- 成功応答: `{"type":"audioStatus","requestId":N, ...}`（状態スナップショット）または `{"type":"audioMeters","requestId":N, ...}`
- 失敗応答: `{"type":"error","requestId":N,"scope":"...","message":"...","dataSafe":true}`。`scope` は `audioDevice` / `plugin` / `recording`、未指定は `protocol`。`dataSafe` は「保存済みデータは無事」の宣言
- ack 待ちの間も状態イベントは流れ続ける。応答が届かない場合、Rust はタイムアウト後に世代跨ぎの再試行・再起動判断を行う（`recovery.rs`）

### 5.4 サイドカー → Rust イベント

| type                                                      | 内容                                                                               |
| --------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `audioStatus`                                             | 状態・デバイス・録音・MIDI の要約（Rust は `AudioStatus` へ正規化して境界Bへ転送） |
| `audioMeters`                                             | ピーク・無効サンプル・緊急ミュート・フィードバック検知                             |
| `transportStatus`                                         | トランスポート状態の変化                                                           |
| `trackPluginStateChanged` / `trackPluginParameterChanged` | エディタ操作等によるプラグイン状態の変化                                           |
| `keepAlive`                                               | 生存確認（Rustは無視）                                                             |
| `error`                                                   | scope 付き失敗通知                                                                 |

フィードバック検知（`feedbackSuspected`）は緊急ミュートと連動し、原因表示のために Rust 側の `MuteCause` と突き合わせられる。

---

## 6. 境界 D: レンダーワーカー（riffra-render-worker）

- 起動: `render_timeline` 命令のたびに `riffra-render` を1回起動する。実行ファイルは Tauri バイナリの隣（`RenderWorker::bundled`）
- 要求: stdin に JSON 1行（`{"type":"renderTimelineOffline","protocolVersion":1,"snapshot":...,"destination":...,"startTick":...,"endTick":...,"sampleRate":...,"blockSize":...,"masterGainDb":...,"normalize":...}`）を書いて stdin を閉じる
- 応答: stdout の JSON 1行。成功は `{"type":"offlineRenderComplete"}`、失敗は `{"type":"error","message":...}`
- プロセスが異常終了・応答タイプ不一致の場合はエラーとして扱う（部分的な WAV は残さない）
- レンダー計画（開始・終了ティック、レンジ解決、出力パス `export/render-{ms}/timeline.wav`、manifest）はシェル側で組み立て、ワーカーは計画の実行だけを担う

---

## 7. 境界 E: デバイスプローブとプラグインスキャン

| 起動引数                                              | 応答                                     | 用途                                                            |
| ----------------------------------------------------- | ---------------------------------------- | --------------------------------------------------------------- |
| `riffra-audio --probe`                                | `{"type":"audioDeviceProbe", ...}` を1行 | ASIO/WASAPI のドライバ・デバイスを列挙（ストリームを開かない）  |
| `riffra-audio --probe-channels <driver> <device> ...` | `{"type":"deviceChannels", ...}`         | 指定デバイスのチャンネル構成                                    |
| `riffra-plugin-scan <args>`                           | 型タグ付き JSON Lines                    | VST3 の列挙・検証（スキャン結果は `ScanReport` としてジョブ化） |

プローブは排他コーディネータ（`run_native_probe`）を通して直列に起動し、タイムアウト・異常終了は「デバイス状態は変更されていない」ことを明示して失敗する。プローブ専用の起動なので通常の音声セッション（`--serve`）には影響を与えない。

---

## 8. 境界 F: CLI JSON Lines（`riffra-cli`）

`riffra-cli` の対話モードは、GUIを持たないHostからセッションの最小操作を実行するための境界である。CLIは `riffra-core::AppCore` とセッションファイルを直接利用し、デスクトップのTauri命令、音声サイドカー、レンダーワーカーは経由しない。

### 8.1 フレーミング

- 起動: `riffra-cli --interactive --session <path>`
- 送受信: stdinの1行を1要求として読み、stdoutへ1行の応答を書く
- 相関: 要求の `requestId` を応答へそのまま返す
- 応答: 成功は `ok: true` と `result`、失敗は `ok: false` と `error`
- 対話モードでは空行を無視し、要求ごとに応答をflushする

要求は `type` とコマンド固有のフィールドを同じ階層に置く。現在のCLIは `params` オブジェクトやイベントストリームを持たない。

```json
{"requestId":"1","type":"addTrack","name":"Bass","kind":"instrument"}
{"requestId":"2","type":"listTracks"}
{"requestId":"3","type":"updateSessionSettings","loopEnabled":true}
```

### 8.2 コマンドと応答

現在のコマンドは `getSession`、`listTracks`、`addTrack`、`removeTrack`、`updateSessionSettings`、`undo`、`redo` である。`undo` と `redo` は履歴がプロセス内に保持されるため、対話モードでのみ利用できる。

```json
{"requestId":"1","ok":true,"result":{}}
{"requestId":"4","ok":false,"error":{"code":"commandFailed","message":"..."}}
```

不正なJSONや未知のコマンドは `invalidRequest` または `commandFailed` として返す。イベント、ジョブ進捗、`protocolVersion`、共通Diagnostics形式は現在のCLI契約に含まれない。ワンショットモードはJSON Linesではなく、コマンドライン引数で1つの操作を実行してJSON結果を出力する。

---

## 9. 権限・ケイパビリティ（`src-tauri/capabilities/default.json`）

メインウィンドウは最小ケイパビリティで構成する。

| 権限                                           | 内容                                                                                                                                           |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `core:default` / `core:window:allow-destroy`   | コア操作とウィンドウ破棄                                                                                                                       |
| `dialog:default`                               | ファイルダイアログ                                                                                                                             |
| `shell:allow-spawn`                            | サイドカー起動のみ許可。`riffra-audio` は引数バリデータ `--(serve\|probe\|probe-channels)` で起動モードを限定、`riffra-plugin-scan` は引数自由 |
| `shell:allow-stdin-write` / `shell:allow-kill` | サイドカーへの標準入力書き込みと終了制御                                                                                                       |

---

## 10. NativeApi と境界の対応規則

`src/native/native-api.ts` はTauri命令をドメイン用語のcapability interfaceへ写像する。各Featureは必要なcapabilityだけに依存し、Reactコンポーネントは`invoke`の文字列コマンド名・引数名を直接知らない。ESLintは低レベルのTauri command/event APIを`src/native/`以外からimportすることを禁止する。

- 制作状態を変更するメソッドはCreativeSessionを返す
- 起動時はCreativeSessionを受け取り、履歴操作の可否はCoreのHistoryStateを参照する
- 音声系メソッドは `AudioStatus` を返し、Audio設定Featureが状態遷移と再試行を担う
- テストでは `native-api-fake.ts` を注入し、呼び出し記録、設定済み応答・失敗、イベント発火だけを扱う。制作規則、履歴、validationはCoreのテストが担う
