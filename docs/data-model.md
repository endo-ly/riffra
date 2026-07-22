# Riffra データモデル

## スコープ

本書はRiffraのドメインエンティティを正準化し、TypeScript / Rust / C++ の3言語での定義場所と対応関係を示す。3言語で同じエンティティを別々に定義するため、ズレを検知するための単一の参照元として機能する。

### 書くこと

- エンティティのカタログと役割
- 各エンティティの3言語での定義場所（ファイルパス）
- 言語間対応の規則（serde・命名・欠落扱い・不透明データ）
- 守るべき不変条件と制約
- スキーマ進化の方針

### 書かないこと

- 各エンティティのフィールド全件・型の全列挙
- 各フィールドのJSONキー名
- 派生型・内部表現・実装詳細
- 個別のバリデーションロジック

詳細は各言語のコードを真実源とする。層構造の全体像は `architecture.md §2` を参照。

---

## 1. アーキテクチャ上の位置づけ

データモデルは3層にまたがり、各層が異なる責務で同じエンティティを扱う。

| 層                  | 責務                         | データモデル上の役割                                 |
| ------------------- | ---------------------------- | ---------------------------------------------------- |
| TypeScript (UI)     | 表示と操作                   | セッション状態の表示とユーザー操作の発行             |
| Rust (Application)  | 正準状態・永続化・検証       | ドメインロジックの真実源・自動保存・アセット索引     |
| C++ (Audio Runtime) | リアルタイム音声・MIDI・VST3 | 音声処理に必要な最小状態。ドメインロジックは持たない |

Rustが正準化するエンティティをC++が独自に再定義しない。C++側はJSON Linesメッセージで受ける必要な状態だけを持ち、永続化やドメイン規則は持たない。同じデータを複数層で独立して持たず、所有権と参照方向を明示する（`architecture.md §11`）。

---

## 2. エンティティカタログ

各エンティティの正準定義場所を示す。C++欄が「—」のものはRust境界で完結し、C++側に現れない。

### 2.1 制作状態（CreativeSession 中心）

| エンティティ                      | 役割                                                                                                             | TS                  | Rust                             | C++                                               |
| --------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------- | -------------------------------- | ------------------------------------------------- |
| CreativeSession                   | 制作中の状態の正準モデル。ワークスペース、デザイン文脈、Play状態、アレンジ、ラック、スナップショット、設定を所有 | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                                 |
| Workspace                         | 4つの固定制作領域（home / play / design / arrange）                                                              | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                                 |
| DesignTool                        | Design workspace内の3つのツール（sample / analyze / separate）                                                   | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                                 |
| DesignContext                     | Design workspaceの現在の対象（active_tool + target_asset_id）                                                    | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                                 |
| PlayState                         | Play側のライブ状態（sample_instrument）                                                                          | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                                 |
| SampleInstrumentState / SamplePad | MIDIキーにマップされたサンプルスライスのパッドセット                                                             | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | `native/audio-engine/src/Main.cpp`（MidiMonitor） |
| SessionSettings                   | master / loop / countIn / note / AI関連設定                                                                      | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                                 |
| SessionSnapshot                   | A/B比較用のラック+マスタ状態キャプチャ                                                                           | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                                 |
| AiChangeSet                       | AI提案の変更履歴エントリ                                                                                         | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                                 |

### 2.2 アレンジメント

| エンティティ                   | 役割                                                                 | TS                  | Rust                             | C++                                        |
| ------------------------------ | -------------------------------------------------------------------- | ------------------- | -------------------------------- | ------------------------------------------ |
| Arrangement                    | revision・timebase・loopRange・Track・Clipを所有する正準タイムライン | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | `TimelineEngine::PreparedTimeline`（派生） |
| ProjectTimebase / TimelineTick | PPQ 960の音楽時間と単一テンポ・拍子                                  | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | `TimelineEngine::PreparedTimeline`（派生） |
| FrameRange / FrameDuration     | 半開区間のSource Frame範囲とSample Rate付き実時間                    | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | `TimelineEngine::Clip`（派生）             |
| TimelineLoopRange              | 有効状態を含む音楽時間上のLoop範囲                                   | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | `TimelineEngine::PreparedTimeline`（派生） |
| Track / TrackKind              | audio / instrumentのタイムライントラック                             | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | Runtime Snapshotでmix値へ解決              |
| AudioClip / AudioClipPatch     | AssetId、開始Tick、Source Frame範囲、実時間長を持つ非破壊編集        | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | `native/audio-engine/src/TimelineEngine.h` |
| MidiClip / MidiNote            | Tickだけで位置と長さを保持するMIDIクリップとノート                   | `src/lib/domain.ts` | `src-tauri/src/session/model.rs` | —                                          |

Track削除は、そのTrackに属するClip参照をArrangementから同時に削除する。Clipが参照していたAsset本体は削除しない。Trackの並べ替えではClipのTrack所有関係を変更しない。

### 2.3 ラック

| エンティティ   | 役割                                                                  | TS                            | Rust                          | C++                                    |
| -------------- | --------------------------------------------------------------------- | ----------------------------- | ----------------------------- | -------------------------------------- |
| RackInstance   | セッションで使用中のライブララック全体                                | `src/lib/domain.ts`           | `src-tauri/src/rack/model.rs` | —                                      |
| RackDevice     | ラック内の1スロット（input / plugin / utility / output）              | `src/lib/domain.ts`           | `src-tauri/src/rack/model.rs` | `native/audio-engine/src/PluginRack.h` |
| RackMacro      | 1操作を複数パラメータへ割り当てるマクロ                               | `src/lib/domain.ts`           | `src-tauri/src/rack/model.rs` | —                                      |
| RackDefinition | RackInstanceの保存済み形態。AssetKind=RackDefinitionのAssetとして保存 | `src/lib/domain.ts`（含まず） | `src-tauri/src/rack/model.rs` | —                                      |
| DeviceKind     | ラックスロットの役割（input / plugin / utility / output）             | `src/lib/domain.ts`           | `src-tauri/src/rack/model.rs` | —                                      |

### 2.4 録音

| エンティティ           | 役割                                                                  | TS                                         | Rust                               | C++                                          |
| ---------------------- | --------------------------------------------------------------------- | ------------------------------------------ | ---------------------------------- | -------------------------------------------- |
| RecordingCapture       | 1回の録音イベントと生成物参照。状態遷移を所有                         | `src/lib/domain.ts`（RecordingCaptureDto） | `src-tauri/src/recording/model.rs` | `native/audio-engine/src/RecordingSession.h` |
| RecordingCaptureStatus | 録音状態（recording / completing / completed / recoverable / failed） | `src/lib/domain.ts`                        | `src-tauri/src/recording/model.rs` | —                                            |
| DropoutInformation     | 録音中のドロップアウト診断情報                                        | `src/lib/domain.ts`                        | `src-tauri/src/recording/model.rs` | —                                            |

### 2.5 素材・資産

| エンティティ        | 役割                                                                                     | TS                                  | Rust                           | C++ |
| ------------------- | ---------------------------------------------------------------------------------------- | ----------------------------------- | ------------------------------ | --- |
| Asset               | 再利用可能な素材の正準モデル。ID・種類・コンテンツ場所・由来を所有                       | `src/lib/domain.ts`（LibraryAsset） | `src-tauri/src/asset/model.rs` | —   |
| AssetId             | `asset:<UUIDv7>` 形式のみの識別子。newtype                                               | `src/lib/domain.ts`（string）       | `src-tauri/src/asset/model.rs` | —   |
| AssetKind           | 素材種別（audio / midi / sample / rackDefinition / generationDefinition）                | `src/lib/domain.ts`                 | `src-tauri/src/asset/model.rs` | —   |
| Provenance          | 素材の由来関係（source_asset_ids + operation + parameters）                              | —                                   | `src-tauri/src/asset/model.rs` | —   |
| ProvenanceOperation | 由来操作（recorded / processed / sampled / separated / rendered / generated / imported） | —                                   | `src-tauri/src/asset/model.rs` | —   |

### 2.6 ランタイム状態（C++起点・Rust経由でTSへ伝播）

| エンティティ                                           | 役割                                                  | TS                  | Rust                     | C++                                                                |
| ------------------------------------------------------ | ----------------------------------------------------- | ------------------- | ------------------------ | ------------------------------------------------------------------ |
| AudioStatus                                            | オーディオランタイムの状態・メーター・MIDI状況        | `src/lib/domain.ts` | `src-tauri/src/model.rs` | `native/audio-engine/src/Main.cpp`（currentStatus）                |
| AudioState                                             | 5状態（offline / starting / ready / muted / faulted） | `src/lib/domain.ts` | `src-tauri/src/model.rs` | `native/audio-engine/src/Main.cpp`                                 |
| PluginStatus                                           | ロード済みプラグインのパラメータ・状態                | `src/lib/domain.ts` | `src-tauri/src/model.rs` | `native/audio-engine/src/PluginRack.h`（status / parameterStatus） |
| PluginParameter                                        | プラグインの単一パラメータ                            | `src/lib/domain.ts` | `src-tauri/src/model.rs` | `native/audio-engine/src/PluginRack.h`                             |
| RecordingStatus                                        | 録音中の進捗・サンプル数・欠落情報                    | `src/lib/domain.ts` | `src-tauri/src/model.rs` | `native/audio-engine/src/RecordingSession.h`（status）             |
| AudioDeviceProbe                                       | オーディオ/MIDIデバイスのprobing結果                  | `src/lib/domain.ts` | `src-tauri/src/model.rs` | `native/audio-engine/src/Main.cpp`（probeAudioDevices）            |
| AudioDriverInfo / AudioAccessMode / AudioDevicePairing | ドライバ情報とアクセス特性                            | `src/lib/domain.ts` | `src-tauri/src/model.rs` | `native/audio-engine/src/Main.cpp`                                 |
| MidiProbe                                              | MIDIデバイスのprobing結果                             | `src/lib/domain.ts` | `src-tauri/src/model.rs` | —                                                                  |
| BootstrapState / RecoveryCandidate                     | 起動時状態・復旧候補                                  | `src/lib/domain.ts` | `src-tauri/src/model.rs` | —                                                                  |
| TransportStatus                                        | Engine Clock、Timeline位置、revision、不連続通知      | `src/lib/domain.ts` | JSON event relay         | `native/audio-engine/src/TimelineEngine.cpp`                       |

---

## 3. 言語間対応の規則

### 3.1 命名規則

3言語は層ごとに命名規則を使い分け、シリアライズ境界で変換する。

| 境界       | Rust内部   | ワイヤー形式  | TS内部    |
| ---------- | ---------- | ------------- | --------- |
| TS ↔ Rust  | snake_case | **camelCase** | camelCase |
| Rust ↔ C++ | snake_case | **camelCase** | —         |

- Rust側の公開データ型には `#[serde(rename_all = "camelCase")]` を付与し、ワイヤー形式をcamelCaseに統一する
- 1語の列挙型は `#[serde(rename_all = "lowercase")]`（例: Workspace, AudioState, DeviceKind, RecordingCaptureStatus, ProvenanceOperation）
- 複数語の列挙型は `#[serde(rename_all = "camelCase")]`（例: AssetKind, AudioAccessMode, AudioDevicePairing）

### 3.2 欠落フィールドの扱い

- Rust側の `Option<T>` は `#[serde(default, skip_serializing_if = "Option::is_none")]` で `None` 時に省略する
- 既定値で受けるフィールドには `#[serde(default)]` を付与する
- 既定 `false` のboolは `#[serde(skip_serializing_if = "std::ops::Not::not")]` で省略する
- TS側は optional (`field?:`) または `null` 許容で受け、明示的な `null` チェックを前提とする
- 後方互換のためのフォールバックは作らない（`AGENT.md` 規約）。新形式へ一直線に置き換える

### 3.3 不透明なデータ

プラグイン状態（`stateData`）はC++ランタイムだけが意味を知るbase64文字列。Rust/TS側では解釈せず、サイズ上限（4 MiB）と存在可否だけを検証して運ぶ。

### 3.4 newtype と透過シリアライズ

`AssetId` は `#[serde(transparent)]` のnewtypeで定義し、TS側では単なる文字列として扱う。ラッパー構造がワイヤー形式に現れない。

### 3.5 ID体系

- `AssetId`: `asset:<UUIDv7>` のみ正準。レガシー形式（`asset:<millis>-<counter>` 等）は拒否
- `SessionId`: 空文字列禁止
- その他のID（Track / AudioClip / SamplePad 等）: 各エンティティのcode上の要件に従う

---

## 4. 不変条件と制約

`CreativeSession::validate_and_normalize()`（`session/model.rs`）が保存・読込時に強制する。各エンティティが常に満たす条件を以下に示す。

### 4.1 由来と素材

- アセットのコンテンツは不変。変更時は新しいIDを発行し、由来情報で元资产と紐付ける
- `Asset::register` は常に新しいIDを発行する。`Asset::derive` は派生専用で元资产を変更しない
- 由来情報（Provenance）は不変。生成物・処理内容・元素材の関係を常に辿れる
- `Processed / Sampled / Separated / Rendered / Generated` の操作は源素材を必須とする。源なしはドメインエラー
- 管理メタデータ（name, tag, note, favorite）のみ可変。`Asset::update_metadata` はID・kind・content・provenanceを保持する

### 4.2 コレクションサイズ上限

| 対象                        | 上限                                                             |
| --------------------------- | ---------------------------------------------------------------- |
| RackInstance.devices        | 256 / ラック                                                     |
| RackInstance.macros         | 64 / ラック                                                      |
| RackDevice.parameter_values | 512 / デバイス                                                   |
| SessionSnapshot.rack        | 256 / スナップショット                                           |
| snapshots                   | 16 / セッション                                                  |
| Arrangement.tracks          | 128 / アレンジ（空を許容。最初のAudio Asset配置時にTrackを作成） |
| Arrangement.audio_clips     | 512 / アレンジ                                                   |
| Arrangement.midi_clips      | 256 / アレンジ                                                   |
| MidiClip.notes              | 200,000 / クリップ                                               |
| SampleInstrumentState.pads  | 128 / セッション                                                 |
| ai_context                  | 16 / セッション                                                  |
| ai_history                  | 128 / セッション                                                 |

### 4.3 数値範囲

| 対象                                                            | 範囲                    |
| --------------------------------------------------------------- | ----------------------- |
| Track / AudioClip / SamplePad / RackDevice / AiChangeSet ゲイン | -90.0 ~ 24.0 dB         |
| SessionSettings.master_db / SessionSnapshot.master_db           | -90.0 ~ 0.0 dB          |
| Track.pan / AudioClip.pan                                       | -1.0 ~ 1.0              |
| RackDevice.parameter_values / PluginParameter.value             | 0.0 ~ 1.0               |
| RackMacro.value                                                 | 0.0 ~ 1.0               |
| SessionSettings.count_in_beats                                  | 0 ~ 8                   |
| MIDI ノート番号                                                 | 0-127                   |
| MIDI ベロシティ                                                 | 1-127（着信時クランプ） |
| MIDI チャンネル                                                 | 1-16                    |
| RackDevice.state_data                                           | 最大 4,000,000 バイト   |

### 4.4 文字列長上限

| 対象                                 | 上限        |
| ------------------------------------ | ----------- |
| SessionSettings.note                 | 16,384 文字 |
| SessionSnapshot.description          | 16,384 文字 |
| AiChangeSet.reason / expected_effect | 4,096 文字  |
| AiChangeSet.risk                     | 256 文字    |
| SessionSettings.ai_context 各項目    | 64 文字     |

### 4.5 列挙値の制約

- `ai_permission`: `Explain` / `Suggest` / `Apply` のいずれか
- `ai_context` 各項目: 固定識別子集合 `{selectedRack, parameterList, analysis, selectedClip, project, userNote, snapshot, previewAudio, errorLog}` のいずれか。重複排除される

### 4.6 状態遷移の不変

`RecordingCapture` は次の遷移のみ許可する。終端状態（Completed / Recoverable / Failed）からRecordingへの逆遷移は禁止する。

```
Recording → Completing
Recording → Recoverable
Recording → Failed
Completing → Completed
Completing → Recoverable
Completing → Failed
```

### 4.7 非破壊編集の規則

- `FrameRange`は半開区間`[start, end)`であり、Sentinel値を使わない
- AudioClipの開始位置は`start_tick`、Source範囲はFrame、再生長とFadeはSample Rate付きFrame数で保存する
- 非Loop AudioClipの再生長はSource範囲長と一致し、Loop AudioClipの再生長はSource範囲長以上とする
- AudioClipのフェード長は再生長を超えない（クランプ）
- TrimはAsset本体を変更せず、Source範囲と開始Tickを一つの編集として更新する
- Split後の両ClipとDuplicateは同じAssetIdを参照し、Source Assetを複製しない
- Arrangement編集が確定するたびにrevisionを単調増加させる
- スナップショットの `master_db` は -90..0 にクランプ（セッション master と異なり 24 dB 上限なし）
- MIDIノートの`duration_ticks`は最低1

### 4.8 ランタイム制約

- ラックは **アクティブなプラグインデバイスを1つまで** サポートする（`RackDefinition::runtime_supported`）。2つ以上のアクティブプラグインを含む保存済みRackDefinitionはロード時に拒否される
- `disabled_placeholder = true` のプラグインデバイスは「アクティブ」 counts に含まれない
- 緊急ミュートはランタイムfault中常に `true`
- フィードバック検出: 250ms以上の持続的な高ピークで発動する

---

## 5. スキーマ変更

### 5.1 方針

- `validate_and_normalize()` が現在の正準スキーマを強制する
- セッションにバージョン番号を持たせず、保存・読込は常に現在のスキーマだけを扱う
- 後方互換のためのフォールバック・旧形式読替・退避は作らない（`AGENT.md` 規約）
- AssetId のみ: 旧形式（`asset:<millis>-<counter>`）も `from_normalized` で拒否される

### 5.2 変更時の手順

エンティティの追加・変更時:

1. 本書 §2 のカタログを更新（エンティティ・定義場所・不変条件）
2. 3言語すべての該当ファイルを更新する
3. `validate_and_normalize()` に新しい不変条件を追加し、単体テストで検証する

### 5.3 拡張ポイント

現在のスキーマとして以下を許容する:

- 新しい `AssetKind` バリアントの追加
- 新しい `ProvenanceOperation` バリアントの追加
- 既存必須フィールドのoptional化（既定値付き）

旧形式の読替・マイグレーションは作らない。スキーマ変更後は現在の構造だけを正とする。
