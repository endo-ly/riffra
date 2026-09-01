# Riffra IPC 契約

## 1. スコープ

本書はRiffraのIPC境界とその契約を正準化する。「どうやり取りするか」を示し、「何がやり取りされるか」の詳細は各言語のコードを真実源とする。

### 書くこと

- IPC境界の全体像と使い分け基準
- Tauri命令のカタログ（領域ごとの分類と責務、実行モード）
- NativeApi TS契約とTauri命令との対応規則
- サイドカー JSON Lines プロトコルの構造と規則（音声・レンダー・プローブ）
- CLIとRiffra Host制御のプロトコル境界
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
     │ invoke 系             │ listen(8種)           │
┌────▼───────────────────────▼───────────────────────▼───────────────┐
│ Rust バックエンド（src-tauri）                                          │
│ 命令層 → Host Adapter → riffra-core / RuntimeReconciler / 永続化       │
└────┬───────────────┬───────────────┬──────────────────────────────┘
     │ C: JSON Lines  │ D: JSON 1行   │ E: JSON 1行
     │ stdin/stdout   │ stdin/stdout  │ stdout
┌────▼───────┐  ┌─────▼────────┐  ┌──▼──────────┐
│riffra-audio│  │riffra-render │  │riffra-audio │
│ --serve    │  │ -worker      │  │ --probe系   │
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
| D    | Rust → riffra-render-worker                   | 子プロセスの stdin/stdout（JSON 1行）                   | オフラインレンダリング（1要求1プロセス）                 |
| E    | Rust ↔ riffra-audio（probe）                  | 子プロセスの stdout（JSON 1行）/ 引数                   | デバイス・チャンネル列挙、VST3スキャン                   |
| F    | 外部クライアント ↔ Riffra Host Control Server | Windows Named Pipe / Unix Domain Socket（長さ付きJSON） | Hostの正準状態・Runtime操作とHost event購読              |

低レイテンシの音声処理はC、時間のかかるバッチはD、デバイスやプラグインの列挙はEを使う。起動中のRiffra Hostを外部クライアントから操作する経路がFで、WebViewからの操作はAを使う。Standalone CLIの標準入出力はFを使わない。

---

## 3. 境界 A: Tauri 命令（WebView → Rust）

### 3.1 実行モード

命令は責務に応じて3つの実行モードを使い分ける。すべて `spawn_blocking` で async ワーカーを塞がない。

| モード                              | 挙動                                                                                                | 使う命令                                           |
| ----------------------------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| `run_blocking`                      | Host command gate を取得してから実行（正準セッション操作と保存を直列化）                            | 楽曲編集・ライブラリ操作・素材操作の大半           |
| `run_blocking_without_command_gate` | ゲートなしで blocking 実行。読み取り専用処理と、VSTライフサイクル中にホストゲートを保持できない処理 | プローブ、スキャン、録音一覧、プラグイン接続など   |
| `run_runtime_control`               | ゲートなし。永続セッションを決して変更しない音声制御（snapshot の読み取りだけ）                     | play / stop / seek、MIDI送信、プレビュー、ミュート |

### 3.2 命令カタログ

領域ごとに代表を示す。全命令は `src-tauri/src/**/commands.rs` と `lib.rs` の `invoke_handler` が真実源。

**起動・全体（lib.rs / startup.rs / audio_preferences.rs）**

| 命令                                                                   | 責務                                                   |
| ---------------------------------------------------------------------- | ------------------------------------------------------ |
| `get_bootstrap_state`                                                  | CanonicalState・セーフモード・回復候補の初期状態を返す |
| `get_audio_status`                                                     | 音声状態の照会                                         |
| `probe_audio_devices` / `probe_device_channels`                        | オーディオデバイス・チャンネル列挙（境界E経由）        |
| `set_emergency_mute` / `set_master_gain_db` / `preview_master_gain_db` | 安全制御とマスターゲイン                               |
| `recover_audio_device` / `retry_startup_runtime`                       | デバイス回復・スタートアップ再試行                     |
| `restore_recovery_generation`                                          | 世代からの回復                                         |

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

**プラグイン（plugins/commands.rs）**: `scan_vst3_folder`、`start_scan_job`、`open_track_plugin_editor`。エディタからのstate / parameter変更はHost内のpersistence coordinatorがイベントをcoalesceしてCanonical stateへ保存する。

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

**演奏・トランスポート（session/transport.rs / runtime）**: `play_timeline`、`stop_timeline`、`seek_timeline`、`go_to_start_timeline`、`send_midi_to_track`、`panic_midi_track`、`enable_midi_listening`、`disable_midi_listening`

### 3.3 エラー規約

- 全命令は `Result<T, String>` を返す。失敗は人間可読な説明文字列となり、`dataSafe` 相当の保証（音声・保存データは安全）はメッセージに含める
- セーフモード中の音声系・プラグイン系命令は明示エラーを返す（`architecture.md §7`）
- 制作状態を変更する命令の応答に含まれる `CanonicalState` は「その操作を含む最新の正準状態」であり、UI は `canonical.session` を表示状態へ反映する

### 3.4 UI呼び出しの順序

制作状態を変更する命令の順序はCoreとHost command gateが所有する。応答に含まれる `CanonicalState` はCoreの確定順序を表すため、フロントエンド独自の直列化、時刻比較、セッション全体のマージは行わない。

連続操作で中間値を送る意味がない制御は、同じ対象への要求を集約して最後の値を送る。集約された要求を待つ呼び出し元には、同じ確定応答を返す。

`invokeHostOrFallback` は非ネイティブ環境（ブラウザプレビュー・スモークテスト）でフォールバック値を返す。ネイティブ実行時は実害のないフォールバックをせず、失敗はそのまま reject する。

---

## 4. 境界 B: シェル → WebView イベント

| イベント                    | ペイロード                | 意味                                                                                                                    |
| --------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `runtime-startup-finished`  | `{ succeeded }`           | スタートアップ時のランタイム初期化完了（セーフモードでは即通知）                                                        |
| `audio-status`              | `AudioStatus`             | 音声状態の変更（ready / muted / starting / faulted / offline、緊急ミュート、フィードバック検知、Preview再生中かどうか） |
| `audio-meters`              | `AudioMeters`             | 入力・出力ピーク、無効サンプル数（高頻度）                                                                              |
| `transport-status`          | `TransportStatus`         | トランスポート状態（再生位置・再生中フラグ）                                                                            |
| `runtime-projection-status` | `RuntimeProjectionStatus` | 非同期のランタイム投影状態（queued / preparing / active / failed）                                                      |
| `runtime-restarted`         | `{ generation }`          | サイドカー再起動（世代番号）。RustがCoreの最新スナップショットを再投影する                                              |
| `canonical-state-changed`   | `CanonicalState`          | GUI以外のHost操作を含む正準セッション、シーケンス、履歴の変更                                                           |

購読は全て `src/native/api/events.ts` の `listen` ラッパを経由する。イベントは Rust が正準状態に基づいて発行する投影通知であり、UI はこれを表示の更新にのみ使う（これは楽曲編集の入力経路ではない）。
プラグインエディタ由来のstate / parameter変更はHostEventHubの内部subscriberが受け取り、Host内でCanonical stateへ保存するため、WebViewイベントとしては公開しない。

---

## 5. 境界 C: 音声サイドカー（riffra-audio）

### 5.1 接続とフレーミング

- 起動: `riffra-audio.exe --serve`。`riffra-runtime::AudioSupervisor`が起動を待ち（`SIDECAR_READY_TIMEOUT`）、起動ごとに世代番号を採番する
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
| プレビュー          | `previewSample`、`stopPreview`、`stopPreviewForKey`                                         |
| テイク比較          | `startTakeComparison`、`switchTakeComparisonVariant`、`stopTakeComparison`                  |
| MIDI                | `enableMidiListening`、`disableMidiListening`、`sendTrackMidi`、`panicTrackMidi`            |

### 5.3 応答とエラー

- 成功応答: `{"type":"audioStatus","requestId":N, ...}`（状態スナップショット）または `{"type":"audioMeters","requestId":N, ...}`
- 失敗応答: `{"type":"error","requestId":N,"scope":"...","message":"...","dataSafe":true}`。`scope` は `audioDevice` / `plugin` / `recording`、未指定は `protocol`。`dataSafe` は「保存済みデータは無事」の宣言
- ack 待ちの間も状態イベントは流れ続ける。応答が届かない場合、Rust はタイムアウト後に世代跨ぎの再試行・再起動判断を行う（`recovery.rs`）

### 5.4 サイドカー → Rust イベント

| type                                                      | 内容                                                                                                 |
| --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `audioStatus`                                             | 状態・デバイス・録音・MIDI・Preview再生状態の要約（Rust は `AudioStatus` へ正規化して境界Bへ転送）   |
| `audioMeters`                                             | ピーク・無効サンプル・緊急ミュート・フィードバック検知。Preview状態の変化は `audioStatus` として通知 |
| `transportStatus`                                         | トランスポート状態の変化                                                                             |
| `trackPluginStateChanged` / `trackPluginParameterChanged` | エディタ操作等によるプラグイン状態の変化                                                             |
| `keepAlive`                                               | 生存確認（Rustは無視）                                                                               |
| `error`                                                   | scope 付き失敗通知                                                                                   |

フィードバック検知（`feedbackSuspected`）は緊急ミュートと連動し、原因表示のために Rust 側の `MuteCause` と突き合わせられる。

---

## 6. 境界 D: レンダーワーカー（riffra-render-worker）

- 起動: `render_timeline` 命令のたびに、Composition Rootから渡された `RuntimeBinaries` のレンダーワーカー実行ファイルを1回起動する。DesktopとHeadlessで同じ配置規則を使う
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

プローブは共有RuntimeのProbe Coordinatorを通して直列に起動し、コーディネータの待機とプロセス実行の双方にタイムアウトを適用する。タイムアウト・異常終了は「デバイス状態は変更されていない」ことを明示して失敗する。プローブ専用の起動なので通常の音声セッション（`--serve`）には影響を与えない。

---

## 8. 境界 F: Local ClientとRiffra Host制御

`riffra`にはStandalone、serve、Attachedの三つの実行モードがある。DesktopのHostConnectionManagerも同じ`riffra-control::ControlCommand`とLocalHostClientを利用するため、EmbeddedとAttachedの制作操作は同じHost command境界を通る。

| モード     | 状態の所有者                                         | 要求の経路                                         |
| ---------- | ---------------------------------------------------- | -------------------------------------------------- |
| Standalone | CLIの`DataRootLease`、`SessionStore`、`AppCore<()>`  | CoreとHostを直接利用                               |
| serve      | `DawHost`のDataRootLease、`AppCore<AudioSupervisor>` | Host Control Serverを公開                          |
| Attached   | 接続先HostのCore、履歴、Runtime、Asset DB            | Host Control Serverへ接続                          |
| Desktop    | Embedded DawHost、または選択したAttached Host        | in-process dispatchまたはHost Control Serverへ接続 |

同じDataRootを別のHostが所有している場合、Standalone CLIと`serve`は起動に失敗し、DesktopはそのHostへ接続する。

起動中のHostは、接続情報を`<data_root>/control/host.json`へ公開し、同じユーザーのregistryへも登録する。接続先が分からない場合はregistryから候補を探し、各候補へ接続して`host.status`を確認する。

候補を削除するのは、そのプロセスが存在しないか、接続先が登録内容と異なるHostであると確定したときだけである。一時的に接続できないだけなら、一覧から外すのみで登録は残す。

Helloでは接続の役割を明示する。

```json
{"type":"hello","role":"command"}
{"type":"hello","role":"events"}
```

接続には二種類ある。

| 種類    | 用途                                                |
| ------- | --------------------------------------------------- |
| command | 要求と応答を運ぶ。Desktopは要求ごとに開く           |
| events  | `HostEventFrame { event, payload }`を運ぶ。長く保つ |

Desktopは要求ごとにcommand接続を開くため、時間のかかる要求の実行中でもTransport操作や緊急ミュートを並行に処理できる。応答にはタイムアウトを設け、応答しないHostで処理が止まり続けないようにする。

Hostのイベント配信では、meterやtransport statusなど最新値があれば足りる通知を最新値で上書きする。重要な通知は押し出されず、待ち行列が溢れても接続を切らない。

外部Hostとの初期同期では、イベント接続を確立してから`host.bootstrap`を取得する。一覧表示は軽量な`host.info`を使い、`host.bootstrap`は接続対象に選ぶときだけ使う。

接続先の変更は、新しい接続とbootstrapを準備してから現在のHostを交換し、交換後は世代を更新して旧Host由来の遅延イベントや応答を破棄する。録音中の切替は拒否する。外部Hostが終了した場合はDisconnectedとし、最後のDataRootとinstanceIdを保持して`host.json`から再接続する。

### 8.1 起動とフレーミング

ワンショットは階層化された引数で1つの操作を実行する。

```bash
riffra --data-root ./data session get
riffra --data-root ./data track add --name Bass --kind instrument
riffra --data-root ./data serve --safe-mode
riffra --data-root ./data --attach session get
```

対話モードは標準入力の1行を1要求として読み、標準出力へ1行の応答を書いてflushする。空行は無視する。

```bash
riffra --data-root ./data --interactive
riffra --data-root ./data --attach --interactive
```

StandaloneとAttachedのinteractive要求は`command`と`params`を持つ。`requestId`は応答へそのまま返され、`expectedSequence`を指定した要求は正準シーケンスが一致するときだけ実行される。

```json
{
  "requestId": "42",
  "command": "track.add",
  "expectedSequence": 18,
  "params": { "name": "Bass", "kind": "instrument" }
}
```

Attachedでは、CLIが標準入力の各行をHostのローカルエンドポイントのフレームへ変換して送る。Hostは1接続内の要求を受けた順に処理し、CLIは応答を標準出力へ1行ずつflushする。フレームは8 MiB以下のUTF-8 JSONでなければならない。

### 8.2 応答とエラー

成功応答には、結果が対応する正準シーケンスを含める。HostとDesktopが共有するControl protocolでは、`session.get`は`result.type: "session"`と`CreativeSession`を返す。Rack、欠落依存、Undo/RedoなどRuntime投影と結び付くアレンジ変更は`result.type: "arrangementMutation"`とし、`result.value.canonical`が正準状態、`result.value.projection`が投影結果を表す。この場合は`result.value.canonical.sequence`と応答の`sequence`が一致する。その他の変更は各コマンド固有の結果型を使う。

読み取りコマンドは1つの`CanonicalState`スナップショットから結果と応答シーケンスを構築する。応答を組み立てる途中で別の正準状態を読み直さない。

```json
{
  "requestId": "42",
  "ok": true,
  "sequence": 12,
  "result": { "type": "session", "value": {} }
}
```

| エラーコード         | 発生条件                                         |
| -------------------- | ------------------------------------------------ |
| `invalidRequest`     | 入力形式、`params`、型、未知のコマンドが不正     |
| `commandFailed`      | Core、Host、保存処理が失敗                       |
| `conflict`           | `expectedSequence`が現在の正準シーケンスと不一致 |
| `hostUnavailable`    | Attached CLIがHostへ接続できない                 |
| `runtimeUnavailable` | Safe Mode中など、要求されたRuntimeを利用できない |

機械判定にはエラーコードを使い、message文字列を解析しない。

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

Standaloneの`undo`と`redo`はそのプロセス内の履歴を使う。serveの`undo`と`redo`はHostの履歴を使い、Attachedの要求は接続先Hostの履歴を共有する。

### 8.3 制作操作

CLIは入力形式だけを解釈し、制作規則と正準化は `riffra-core::Application` に委譲する。

| 分類                   | コマンド                                                                                                                                                                                                                                                                                                                              |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Session / History      | `session inspect`、`session get`、`session settings update`、`history get`、`undo`、`redo`                                                                                                                                                                                                                                            |
| Track / Routing        | `track list`、`add`、`update`、`remove`、`duplicate`、`reorder`、`audio-input`、`midi-input`                                                                                                                                                                                                                                          |
| Audio Clip             | `audio-clip list`、`add-asset`、`update`、`move`、`trim`、`split`、`duplicate`、`crossfade`                                                                                                                                                                                                                                           |
| MIDI Clip / Note       | `midi-clip list`、`create`、`add-asset`、`update`、`move`、`trim`、`split`、`duplicate`、`midi-note add/insert/update/update-many/remove/remove-many/clear/quantize/transform/duplicate`                                                                                                                                              |
| Music Operations       | `music.midi-clip.create`、`music.note.insert`、`music.region.list`、`music.region.add`、`music.region.update`、`music.region.remove`、`music.harmony.resolve`、`music.harmony.list`、`music.harmony.insert`、`music.harmony.update`、`music.harmony.remove`、`music.harmony.realize`、`music.phrase.insert`                           |
| Timeline / Arrangement | `clip remove`、`clip paste`、`marker add/update/remove`、`timebase update`、`loop-range set`、`punch-range set`                                                                                                                                                                                                                       |
| Automation             | `automation set`、`automation clear`                                                                                                                                                                                                                                                                                                  |
| Asset / Project        | `asset import-midi`、`asset preview`、`project export`、`project import`                                                                                                                                                                                                                                                              |
| Rack state             | `plugin catalog list`、`plugin instrument/effect`、`plugin scan/scan-start`、`instrument clear`、`effect remove/reorder`、`device bypass/parameter-set`                                                                                                                                                                               |
| Runtime services       | `audio status/probe/channels-probe`、`audio driver get/set`、`audio recover/startup-retry`、`record start/another-take/stop/status/list/rename/archive/promote/tag/delete/duplicates`、`render start`、`job get/cancel`、`library search/asset-update/related`、`analysis start`、`missing list/relink/disable-plugin/replace-plugin` |

Live HostのControl Serverは、正準状態、履歴、Track、Runtime投影、Transport、Audio、Plugin、Recording、Render、Job、Library、Missing、Analysisを公開する。Safe ModeではRuntimeを必要とする操作が`runtimeUnavailable`になる。

`session inspect` は `CanonicalState` の1つのSnapshotから、Project設定、content end、範囲指定、History、Track/Clip/Region/Harmony/Markerの軽量な構造Projectionを返す。MIDI Note/Event、Automation Point、Plugin parameter、`stateData` は展開せず、件数に固定上限を設けない。`automationLaneCount` はTrack全体のLane数、`automationPointCount` は指定範囲に含まれるPoint数を表す。`--start` / `--end` の範囲は `[start, end)`、`--track-id` はTrack固有のClipとAutomationへ適用し、Region/Harmony/MarkerはArrangement全体の文脈として残る。

Agent向けCLIでは、Canonical Sessionを返す正準Mutationの成功応答を軽量な `mutation` receiptへ変換する。receiptは応答の `sequence`、Projection状態、構造Entity ID(Track、Clip、Region、Harmony、Marker、Automation Lane、Device)を含み、一部の直接Note操作では生成されたMIDI Note IDも含む。Canonical Session、MIDI Note/Eventの内容、Automation Point、Plugin parameter、`stateData` は含まない。後続操作に必要な最新状態は `session inspect` で取得する。DesktopとHost間の共有Control protocolではDesktop同期のためCanonical結果を維持する。

`track list` は軽量な `TrackSummary` 投影を返す。Track と device の識別情報・ミキサー情報は含むが、device の `parameterValues` は含まない。完全な device 状態が必要な場合は `session get` を使う。

DesktopのTauri command境界が所有する機能と、Live HostのControl Serverが所有する機能は次のように分かれる。

| 操作群                                                 | Desktop / serve       | Standalone           |
| ------------------------------------------------------ | --------------------- | -------------------- |
| Runtime投影・トランスポート                            | HostのRuntime         | `runtimeUnavailable` |
| 音声状態・Live MIDI                                    | HostのAudio Runtime   | `runtimeUnavailable` |
| プラグイン一覧・VST音源/エフェクト・デバイスパラメータ | HostのPlugin Runtime  | `runtimeUnavailable` |
| 欠落依存                                               | HostのMissing service | `runtimeUnavailable` |
| 録音                                                   | HostのRecording       | `runtimeUnavailable` |
| レンダー・ジョブ                                       | HostのRenderWorker    | `runtimeUnavailable` |
| ライブラリ・解析・Asset preview                        | Hostのshared service  | `runtimeUnavailable` |

プラグインエディタのウィンドウ、ファイルダイアログ、ウィンドウ管理はDesktop shellに残る。プラグインエディタのopen command、録音、プレビュー、VSTスキャン、ライブラリmetadata、解析は現在HostのRuntime / shared serviceを使う。エディタから発生したplugin state / parameterの永続化はHost内のcoordinatorがCanonical commitを行い、Desktop WebViewの往復には依存しない。

`render start`は接続先Hostが所有する`RenderWorker`のジョブを開始し、ジョブIDを返す。音楽座標の部分Renderは`start` / `end`の`MusicalPosition`を受け取り、Runtime境界で既存のRender計画向けtickへ変換する。`trackId`との併用でTrack単位の部分Renderも指定できる。実行中の状態は`job get --id <id>`で取得し、`job cancel --id <id>`で停止を要求する。Attached CLIはRenderWorkerやその子プロセスを直接所有しない。

`expectedSequence` はMutationだけでなく `render.start`、`undo`、`redo` にも適用される。Inspect後に人間の編集が入った場合、対象Snapshotを別の状態でRenderしたり直前の別ユーザーの編集をUndoしたりしないようConflictで拒否する。RenderやUndoのConflictは自動再送せず、最新状態を再Inspectする。SequenceのRevision tokenは同じ `AppCore` の有効期間でだけ成立し、Standaloneのワンショットプロセス間では共有されない。

---

## 9. 権限・ケイパビリティ（`src-tauri/capabilities/default.json`）

メインウィンドウは最小ケイパビリティで構成する。

| 権限                                         | 内容                     |
| -------------------------------------------- | ------------------------ |
| `core:default` / `core:window:allow-destroy` | コア操作とウィンドウ破棄 |
| `dialog:default`                             | ファイルダイアログ       |

---

## 10. NativeApi と境界の対応規則

`src/native/native-api.ts` はTauri命令をドメイン用語のcapability interfaceへ写像する。各Featureは必要なcapabilityだけに依存し、Reactコンポーネントは`invoke`の文字列コマンド名・引数名を直接知らない。ESLintは低レベルのTauri command/event APIを`src/native/`以外からimportすることを禁止する。

- Host-owned methodは`invokeHost`を使い、開始時のconnection generationと応答時のgenerationが一致しなければ成功結果として扱わない。bootstrap、Host切替、Reconnectの結果は現在generationを更新する
- Window、dialog、Host selectorのようなDesktop shell-owned methodは通常の`invoke`を使い、Host切替による再同期対象にしない
- 制作状態を変更するメソッドは `CanonicalState` を含む結果を返す
- 起動時は `CanonicalState` を受け取り、履歴操作の可否はCoreのHistoryStateを参照する
- 音声系メソッドは `AudioStatus` を返し、Audio設定Featureが状態遷移と再試行を担う
- テストでは `native-api-fake.ts` を注入し、呼び出し記録、設定済み応答・失敗、イベント発火だけを扱う。制作規則、履歴、validationはCoreのテストが担う
