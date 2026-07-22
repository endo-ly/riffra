# Riffra IPC 契約

## スコープ

本書はRiffraの3つのIPC境界とその契約を正準化する。「どうやり取りするか」を示し、「何がやり取りされるか」の詳細は各言語のコードを真実源とする。

### 書くこと

- 3つのIPC境界の全体像と使い分け基準
- Tauri命令のカタログ（featureモジュール単位の分類と責務）
- NativeApi TS契約とTauri命令との対応規則
- sidecar JSON Linesプロトコルの構造と規則
- 境界ごとのエラー・状態遷移の契約
- 権限・ケイパビリティ設定

### 書かないこと

- 各Tauri命令の引数・戻り値の詳細（code参照）
- sidecarコマンドの全シグネチャ（code参照）
- 各メッセージの全フィールド（code参照）

---

## 1. IPC境界の全体像

Riffraは3言語を3つのIPC境界で繋ぐ。各境界は異なる性質を持ち、処理の性質に応じて使い分ける。

```
┌──────────────┐  invoke   ┌──────────────┐  stdin    ┌──────────────┐
│ React (TS)   │ ────────→ │ Rust         │ ────────→ │ C++ sidecar  │
│              │           │ (Tauri)      │           │ (JUCE)       │
│ UI・操作     │ ←──────── │ アプリ状態   │ ←──────── │ オーディオ   │
└──────────────┘  結果     └──────────────┘  stdout   └──────────────┘
                                  ↑ JSON Lines（イベント通知・状態・メーター）
```

| 境界       | 方式              | 性質                         | 担当                                     |
| ---------- | ----------------- | ---------------------------- | ---------------------------------------- |
| TS → Rust  | Tauri `invoke`    | 同期RPC風・JSON              | UI操作の副作用・永続化・ビジネスロジック |
| Rust → C++ | stdin JSON Lines  | 要求-応答（`requestId`照合） | リアルタイム音声の制御指令               |
| C++ → Rust | stdout JSON Lines | 非同期イベント通知           | 状態変化・メーター・エラー               |

### 使い分けの基準

- **Tauri命令**: UIから起こるすべての副作用。ファイルIO・永続化・バックグラウンドジョブ・ビジネスロジック
- **sidecar**: リアルタイム性が重要な処理。JUCEオーディオスレッド・VST3ホスティング・デバイス管理・録音ストリーム・低レイテンシMIDI

Tauri命令の一部は「runtime（sidecar）への反映」と「session（永続化）への反映」を両方行う（例: プラグインロード・マスターゲイン設定・サンプルパッド操作）。この二段階の同期はApplication層のOperationsが担保し、片方だけを成功扱いにしない。詳細は `architecture.md §4`。

---

## 2. Tauri 命令のカタログ

Tauri命令はRustの `#[tauri::command]` で定義し、`src-tauri/src/lib.rs` の `generate_handler!` マクロで一括登録する。真実源は各featureモジュールの `commands.rs`。以下はfeatureモジュール別の代表命令。

### 2.1 トップレベル（`lib.rs`）

`get_bootstrap_state` / `export_scratch_session` / `get_background_job` / `cancel_background_job` / `probe_audio_devices` / `probe_midi_devices` / `get_audio_status` / `preview_master_gain_db` / `set_emergency_mute` / `recover_audio_device` / `open_midi_input` / `close_midi_input` / `stop_preview` / `stop_preview_for_key`

### 2.2 Rack Application Operations（`rack/commands.rs`）

`load_plugin_into_rack` / `clear_plugin_from_rack` / `open_plugin_editor` / `set_rack_plugin_bypassed` / `set_rack_plugin_parameter` / `set_rack_macro_value` / `map_rack_macro` / `restore_current_rack` / `capture_snapshot` / `recall_snapshot` / `save_rack_definition` / `list_rack_definitions` / `load_rack_definition_asset`

### 2.3 Session Application Operations（`session/commands.rs`）

`save_scratch_session` / `restore_recovery_generation` / `import_scratch_session` / `create_sample_pad` / `update_sample_pad` / `remove_sample_pad` / `add_audio_clip_to_arrangement` / `update_audio_clip` / `trim_audio_clip` / `split_audio_clip` / `duplicate_audio_clip` / `move_audio_clips` / `paste_audio_clips` / `crossfade_audio_clips` / `remove_audio_clip` / `remove_audio_clips` / `add_track` / `update_track` / `remove_track` / `duplicate_track` / `reorder_track` / `update_timeline_loop_range` / `sync_arrangement_runtime` / `play_timeline` / `stop_timeline` / `seek_timeline` / `open_asset_in_design` / `switch_workspace` / `update_session_settings` / `apply_ai_suggestion` / `set_master_gain_db` / `relink_missing_dependency` / `disable_missing_plugin` / `get_missing_dependencies`

### 2.4 Audio Preferences（`audio_preferences.rs`）

`set_audio_driver` — アプリケーション設定（CreativeSession外）とランタイムの同時更新

### 2.5 Asset Application Operations（`asset/commands.rs`）

`preview_asset`

### 2.6 Recording Application Operations（`recording/commands.rs`）

`list_recordings` / `rename_recording` / `delete_recording` / `archive_recording` / `promote_recording` / `tag_recording` / `detect_duplicate_recordings` / `start_recording` / `stop_recording`

### 2.7 Library Read Model（`library/commands.rs`）

`search_library` / `update_library_asset` / `related_library_assets`

### 2.8 Background Jobs（各featureの `commands.rs`）

- Analysis: `start_analysis_job` / `analyze_asset`
- Separation: `start_separation_job` / `list_separations`
- Render: `render_timeline`
- Plugins: `scan_vst3_folder` / `start_scan_job`

### 2.9 エラー型

Tauri命令の戻り値は `Result<T, String>`。文字列は利用者向けの表示メッセージ。構造化エラーは `src-tauri/src/errors.rs` の `DomainError` enum で定義し、`Display` 経由で小文字のメッセージに変換する。

```
DomainError::InvalidAssetId(String)
DomainError::InvalidProvenance(String)
DomainError::InvalidClip(String)
DomainError::UnknownTrack(String)
DomainError::InvalidRecordingTransition { from: String, to: String }
```

新しい失敗区分が必要な場合は構造化エラーへ追加し、文字列結合で表現しない。

---

## 3. NativeApi TS契約

`src/native/native-api.ts` の `NativeApi` interface がTS側の単一窓口。コンポーネントは `useApp` フック経由でこのAPIを呼び、直接 `@tauri-apps/api/core` の `invoke` は呼ばない。

### 3.1 対応規則

- `NativeApi` の各メソッドは1つのTauri命令と1:1で対応する
- メソッド名はcamelCase、対応するTauri命令名はsnake_case（例: `loadPluginIntoRack()` ↔ `load_plugin_into_rack`）
- 戻り値が `CreativeSession` と `AudioStatus` の組の場合、Rust側はタプル `[CreativeSession, AudioStatus]` を返し、TS側でオブジェクト `{ session, audio }` に詰め替える
- 真実源は `src/native/native-api.ts`（契約）と `src/native/native.ts`（invoke 実装）

### 3.2 ブラウザプレビュー時の振る舞い

`native.ts` は `invoke` が失敗した場合、安全な既定値へフォールバックする（`getAudioStatus` → offline状態、`searchLibrary` → 空配列、等）。これはブラウザプレビューでTauri runtimeが無い環境を救済するためだが、**本番パスでは存在しない成功経路を作らない**規則は維持する。フォールバックは常に「機能が利用できない」状態を示す。

### 3.3 差替え実装

`src/native/native-api-fake.ts` の `FakeNativeApi` がテスト用の差替え。以下の規則を守る:

- **本番に存在しない成功経路を作らない**。フォールト状態・保存失敗・ロールバック失敗等のエラーシナリオを再現する
- 決定的な振る舞い（カウンターベースのID・固定状態）でテストの再現性を保証する
- 呼出履歴を追跡し、アサーションに使えるようにする

詳細は `docs/test-strategy.md` を参照。

---

## 4. sidecar JSON Lines プロトコル

C++ sidecar（`riffra-audio.exe`）はRustプロセスの子プロセスで、stdin/stdoutでJSON Lines（1行=1JSON）をやり取りする。Rust側の `AudioSupervisor`（`src-tauri/src/native_audio.rs`）が単一のオーケストレータであり、C++側のエントリポイントは `native/audio-engine/src/Main.cpp`。

### 4.1 メッセージの基本構造

Rust → C++（要求）。stdinへ1行で書き込む:

```
{ "type": "<command>", "requestId": <number>, ...params }
```

C++ → Rust（応答・イベント）。stdoutへ1行で出力:

```
{ "type": "<messageKind>", "requestId"?: <number>, ...payload }
```

- `type`: メッセージ種別（camelCase）
- `requestId`: 数値。Rust側が `AtomicU64` で1から連続発行し、`Condvar` で3秒タイムアウト付きで応答を待機する。イベント通知では省略可
- C++側は `thread_local` の `currentRequestId` に保持し、応答行に `requestId` を付与する

### 4.2 メッセージ種別（C++ → Rust）

通常稼働時は次の5種類を使用する。フィールド詳細はcodeを真実源とする。

| type              | 役割                                                                                     | 送信契機                  |
| ----------------- | ---------------------------------------------------------------------------------------- | ------------------------- |
| `audioStatus`     | 実行時オーディオ状態（state・deviceInfo・recording・plugin概要・meters・midi）           | 状態変化時・コマンド応答  |
| `audioMeters`     | メーター値のみ（inputPeak・outputPeak・invalidSamples・feedbackSuspected）。高頻度・軽量 | 定期的                    |
| `error`           | エラー通知（scope・message・dataSafe）                                                   | エラー発生時              |
| `timelineAck`     | Timeline Snapshotの準備完了revision・適用時刻・利用不能Clip                              | Snapshotコマンド応答      |
| `transportStatus` | Engine ClockとTimeline位置、再生状態、revision、不連続通知                               | 状態変化時・20 Hz定期送信 |

起動時だけ `audioDeviceProbe` メッセージを別途 stdout に出力する（`--probe` モード、または `--serve` 起動直後のプロービング）。これは `audioStatus` とは別のプロトコルで、Rust側の `parse_midi_probe` 等で処理される。

### 4.3 コマンドカタログ（Rust → C++）

真実源は `src-tauri/src/native_audio.rs` の `AudioSupervisor` 各メソッドと、`Main.cpp` のディスパッチ部。C++側で処理される `type` の一覧:

- **状態照会**: `status` / `meterStatus`
- **オーディオ設定**: `setEmergencyMute` / `setMasterGainDb` / `setAudioDriver` / `recoverAudioDevice`
- **プラグイン**: `loadPlugin` / `clearPlugin` / `setPluginBypassed` / `openPluginEditor` / `setPluginParameter` / `setPluginState` / `pluginParameterStatus`
- **録音**: `startRecording` / `stopRecording`
- **プレビュー**: `previewSample` / `stopPreview` / `stopPreviewForKey`
- **タイムライン**: `loadTimelineSnapshot` / `playTimeline` / `stopTimeline` / `seekTimeline`
- **MIDI・サンプルパッド**: `openMidiInput` / `closeMidiInput` / `configureSamplePads` / `probeMidiDevices` / `sendMidi`
- **シャットダウン**: `shutdown`

命名はすべてcamelCase。Rust側の `AudioSupervisor` メソッドが対応する。未対応の `type` は C++側で `protocol` スコープのエラーになる。

通常の `audioStatus` にプラグインのパラメータ一覧とstateDataは含めない。パラメータ一覧は `pluginParameterStatus` の応答で取得し、プラグイン状態はSessionからランタイムへ復元するときだけ渡す。Masterのドラッグ中は `preview_master_gain_db` がAudio Runtimeだけを更新し、操作確定時に `set_master_gain_db` がSessionへ保存する。

Timeline Snapshotは`protocolVersion: 1`とArrangement revisionを持つ。RustはAssetIdを解決済みパスとSource Frame情報へ変換し、利用不能AssetはSnapshotから除外して`unavailableClipIds`へ残す。C++はファイルopen、read-ahead、Sample Rate補正、作業バッファ確保をコマンドスレッドで完了してから交換する。Audio CallbackはファイルI/O・JSON解析・メモリ確保を行わない。

`transportStatus.timelineSample`はseekやloopで不連続になり得る。`audioClockSample`はAudio Callbackごとに単調増加する。UIは最新イベントをanchorとして`requestAnimationFrame`で表示だけを補間し、補間値を正準状態へ書き戻さない。

### 4.4 状態遷移

C++側は `audioStatus.state` として `faulted` / `muted` / `ready` のいずれかを出力する。Rust側はこれに加えて `starting`（起動中）と `offline`（未接続・不明状態のフォールバック）を追加する。

```
                 ┌──────────┐
   起動 ─────→   │ starting │
                 └────┬─────┘
                      ↓ 初回 status 受信
                 ┌──────────┐
                 │  muted   │  ←─── ラック復元中・復旧後の安全状態
                 └────┬─────┘
                      │ Master設定とラック復元に成功
                      ↓
                 ┌──────────┐  デバイス消失  ┌──────────┐
                 │  ready   │ ────────────→ │ faulted  │
                 │          │ ←──────────── │          │
                 └──────────┘   復旧コマンド └──────────┘

                 ┌──────────┐
                 │ offline  │  sidecar未接続・状態不明時のRust側フォールバック
                 └──────────┘
```

- 起動直後は `starting` → `muted` と進み、Master設定とラック復元の完了後に自動で `ready` へ遷移する
- `faulted` はC++側の `SafetyAudioCallback::setDeviceFaulted(true)` が検出時に出力される
- `recoverAudioDevice` コマンドで `faulted` → `muted` へ復旧する。復旧できない場合はsidecar再起動を試みる
- 不明な状態文字列はRust側で `offline` にフォールバックする

### 4.5 エラー表現

エラーは `error` タイプで通知する。C++側の `makeError()` が生成する:

```
{ "type": "error", "scope": "<category>", "message": "<user message>", "dataSafe": true }
```

`scope` でエラーの影響範囲を分類する。Rust側の `render_native_error` は `scope == "audioDevice"` のみ `faulted` 状態へ遷移させ、それ以外はコマンド失敗（`message` 更新）扱いにする。

| scope         | 影響                            | C++側の主な発生元                             |
| ------------- | ------------------------------- | --------------------------------------------- |
| `audioDevice` | **fault状態へ遷移する**         | デバイス切断・ドライバ切替失敗・復旧失敗      |
| `plugin`      | コマンド失敗。fault状態にしない | プラグインのパラメータ/状態設定失敗           |
| `recording`   | コマンド失敗                    | 録音開始/停止/ディレクトリ失敗                |
| `midi`        | コマンド失敗                    | MIDI入力オープン/クローズ失敗                 |
| `preview`     | コマンド失敗                    | サンプルプレビュー失敗                        |
| `protocol`    | コマンド失敗                    | JSONパース失敗・未対応コマンド                |
| `arguments`   | 起動時失敗                      | CLI引数エラー（`--probe` / `--serve` 起動時） |

`dataSafe` は保存済みデータが保全されているかを示す。現在のC++実装は常に `true` を送信し、保存データがsidecarの異常で失われないことを表明する。

### 4.6 sidecarのライフサイクル

- 親プロセス（Rust）が `Drop` される際、`shutdown` コマンドを送ってから `kill` する
- C++側は `--parent-pid` 引数で親PIDを受け取り、親プロセス終了を検出したら自終了する（watchdog）
- Rust側はsidecarの`Terminated` / `Error` / `Stderr` イベントを検出すると `faulted` 状態へ遷移させる
- `sidecar_generation`（`AtomicU64`）で世代を管理し、古いイベントが新しい世代へ影響しないようにする
- `recoverAudioDevice` / `setAudioDriver` が行き詰まった場合、sidecarを再起動してから再送する

### 4.7 Safe Mode

`--safe-mode` フラグまたは `RIFFRA_SAFE_MODE` 環境変数で、sidecarを起動せず一部コマンドをブロックする。Application Operationsは Safe Mode を検知して runtime への指令をスキップし、session 永続化のみを行う。

- **隔離対象**: VST3発見・MIDI入力・ドライバ変更・ライブプレビュー・新規録音
- **許可対象**: プロジェクト開・ライブラリ・オフライン解析・レンダ・マニフェスト入出力

Safe Modeでも「失敗を成功として表示しない」契約は維持する。隔離された機能は利用不可状態として表示する。

---

## 5. 権限・ケイパビリティ

`src-tauri/capabilities/default.json` と `src-tauri/tauri.conf.json` で以下を設定する。

### 5.1 外部プロセス許可（`shell:allow-spawn`）

- `riffra-audio`: オーディオsidecar。`sidecar: true`、第1引数は `--(serve|probe)` の正規表現で検証
- `riffra-plugin-scan`: プラグインスキャンsidecar。`sidecar: true`、引数は任意

### 5.2 shell操作

- `shell:allow-stdin-write`（sidecarへのstdin書き込み）
- `shell:allow-kill`（sidecarプロセスの終了）

### 5.3 CSP

```
default-src 'self'
style-src 'self' 'unsafe-inline'
img-src 'self' asset: data:
connect-src ipc: http://ipc.localhost
```

### 5.4 ウィンドウ・バンドル

- 単一ウィンドウ（label: `main`）、既定 1440x900、最小 1000x700
- `bundle.active: false`（現在は配布ビルド無効）
- `bundle.externalBin`: `binaries/riffra-audio`、`binaries/riffra-plugin-scan`

---

## 6. 境界配置の基準

新しい処理をどの境界へ置くかの判断基準を示す。

| 処理の性質                               | 配置                                     |
| ---------------------------------------- | ---------------------------------------- |
| UIからの操作で永続化や検証を伴う         | Tauri命令（Rust）                        |
| ビジネスロジック・ドメイン規則           | Rust単体。単体テストで検証               |
| リアルタイム音声処理・オーディオスレッド | C++ sidecar                              |
| VST3ホスティング・デバイス直接操作       | C++ sidecar                              |
| バックグラウンドで走る重い処理           | Rustジョブ機構（別スレッド・別プロセス） |

### 守るべき規則

- 制作規則をTauri命令関数へ埋め込まない。Tauri命令は引数の変換とApplication Operationへの委譲に限定する（`architecture.md §4`）
- sidecarのオーディオコールバック内でファイルIO・JSON解析・SQLite・ネットワーク・大きいメモリー確保を行わない（`architecture.md §6`）。これらは別スレッド・別プロセスで行う
- C++側はドメインロジックを持たない。音声処理に必要な最小状態だけを持つ
- Tauri命令のエラーは構造化エラー（`DomainError`）を経由して文字列化する。文字列結合で分類を表現しない
- sidecarコマンドの成否は `audioStatus.state` を通じて表現され、Rust側が `audio_command_succeeded`（`state != Faulted && state != Offline`）で判定する。React側で再判定しない
