# Riffra データモデル

## 1. スコープ

本書はRiffraのドメインエンティティと、RustからTypeScript・C++へ渡す際の対応関係を示す。正準定義はRustに置き、各境界で必要な投影がどのモデルに由来するかを確認できるようにする。

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

詳細は各言語のコードを真実源とする。層構造の全体像は `architecture.md`、境界の契約は `ipc.md` を参照。

---

## 2. 型定義の場所

| エンティティ群                                  | 正準定義（Rust）                                                         | TypeScript                                            | C++ ミラー                                              |
| ----------------------------------------------- | ------------------------------------------------------------------------ | ----------------------------------------------------- | ------------------------------------------------------- |
| セッション / アレンジ / クリップ / 録音レコード | `crates/riffra-core/src/domain/`                                         | `apps/desktop/src/model/generated/*.ts`（ts-rs 生成） | `native/audio-engine`（ランタイム投影に必要な部分のみ） |
| 素材（Asset / Provenance）                      | `crates/riffra-core/src/domain/asset/`                                   | 同上                                                  | —                                                       |
| ラック（RackDevice / Macro）                    | `crates/riffra-core/src/domain/rack/`                                    | 同上                                                  | `native/audio-engine`（グラフ構築）                     |
| 録音キャプチャ / ドロップアウト                 | `apps/desktop/src-tauri/src/recording/model.rs`                          | 同上                                                  | `native/audio-engine`（録音制御）                       |
| 録音の read model                               | `apps/desktop/src-tauri/src/recording/repository.rs`（`RecordingAsset`） | 同上                                                  | —                                                       |
| バックグラウンドジョブ                          | `apps/desktop/src-tauri/src/jobs.rs`                                     | 同上                                                  | —                                                       |
| オーディオ / デバイス状態                       | `apps/desktop/src-tauri/src/model.rs` ほか                               | 同上                                                  | `native/audio-engine`                                   |

TypeScript は `npm run gen:types`（cargo test による ts-rs 出力 → `scripts/gen-barrel.js` のバレル生成）で常に Rust から再生成される。手書きの型は追加しない。

---

## 3. 言語間対応の規則

| 規則               | 内容                                                                                                                                                                  |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| serde 直列化       | `rename_all = "camelCase"`（enum は `"lowercase"`）                                                                                                                   |
| 不透明 ID 型       | `TimelineTick`・`Marker.tick` などは `#[ts(type = "number")]`。`AssetId` は `string & { readonly __brand: 'AssetId' }`（直列化はプレーン文字列 `<-> asset:<UUIDv7>`） |
| 省略可能フィールド | `skip_serializing_if = "Option::is_none"` + `#[ts(optional)]` を対で使用                                                                                              |
| 型の欠落           | ts-rs で生成できない型（`serde_json::Map` の `parameters` 等）は該当フィールドを `#[ts(skip)]` せず、生成側の扱いに従う                                               |
| C++ ミラー         | セッション全体はコピーしない。投影（グラフ・パラメータ・演奏・録音）に必要なスライスだけを別プロトコルで渡す（`ipc.md` のサイドカー契約）                             |
| 正当性の基準       | 永続化される正準表現は常に Rust の `CreativeSession`。TS は表示・編集のための投影、C++ は実行のための投影                                                             |

---

## 4. エンティティカタログ

### 4.1 セッションと設定

| エンティティ      | 役割                                                                                             |
| ----------------- | ------------------------------------------------------------------------------------------------ |
| `CreativeSession` | 永続化される制作状態の単一の正準モデル                                                           |
| `SessionSettings` | マスターゲイン、ループ、カウントイン、メトロノーム、ノートなど、構造ではないセッション全体の設定 |

### 4.2 アレンジ（時間軸）

| エンティティ                                                                                 | 役割                                                                                                                                                                                                                                     |
| -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TimelineTick`                                                                               | 時間軸の基本単位。`TIMELINE_PPQ = 960`：1拍を960分割                                                                                                                                                                                     |
| `MusicalPosition` / `MusicalDuration` / `MusicalOffset` / `MusicalPitch` / `MusicalNoteName` | CLIやGUIなどの制作操作が使う音楽表現。小節・拍、全音符を1とする有理数、音名を表し、CoreでTimelineTickまたはMIDI pitchへ変換する。音名は入力した臨時記号の表記を保持し、double accidentalにも対応する。ノートの正準状態としては保存しない |
| `ProjectTimebase`                                                                            | 拍子・テンポ・ppq からなる音楽クロック。ルーラー・スナップ・MIDI・トランスポートが共有                                                                                                                                                   |
| `FrameRange` / `FrameDuration`                                                               | ソース素材のフレーム範囲／持続時間（サンプルレートを併せ持つ）                                                                                                                                                                           |
| `TimelineLoopRange` / `TimelinePunchRange`                                                   | ループ区間（無効化しても端点保持）とパンチ録音区間                                                                                                                                                                                       |
| `Track`                                                                                      | Audio / Instrument の2種類。ゲイン・パン・ミュート・ソロ・アーム・モニタリング・入力ルート・インストゥルメント・トラックラックを保持                                                                                                     |
| `AudioClip`                                                                                  | 非破壊オーディオクリップ。`asset_id` + `source_range` + `timeline_duration`、ゲイン・パン・フェード・ループ・ミュート。録音テイクへの関連（recording_take_id）と`take_variant`（raw/processed）を持つ                                    |
| `MidiClip`                                                                                   | 非破壊 MIDI クリップ。`MidiNote`（ピアノロール編集対象）と `MidiEvent`（CC/ピッチベンド/チャンネルプレッシャーを忠実保持）を持つ。ノートとイベントはそれぞれ最大200,000件。`asset_id` は任意（セッション内で完結する MIDI は持たない）   |
| `AudioClipPatch` / `MidiClipPatch`                                                           | 部分更新。None のフィールドは現値を維持                                                                                                                                                                                                  |
| `AutomationLane` / `AutomationPoint`                                                         | トラックミックスパラメータ（volume / pan）のタイムライン制御データ                                                                                                                                                                       |
| `Marker`                                                                                     | ルーラー表示用の名前付き位置情報（音声処理には影響しない）                                                                                                                                                                               |
| `TimelineRegion`                                                                             | セクション種別を持たない、自由な名前付き時間範囲。重複・入れ子・同名を許可し、音声処理やHarmonyなどの所有権は持たない                                                                                                                    |
| `HarmonyChord`                                                                               | コード記号または明示音集合を解決した和声。name、任意のroot / bass、octaveを持たないtonesを保持し、third-party parserの型は公開しない                                                                                                     |
| `HarmonyEvent`                                                                               | 和声コンテキストを表すArrangement全体の時間軸イベント。TimelineRegion、Track、MidiClipを所有せず、重複・入れ子・gapを許可する                                                                                                            |
| `PhrasePattern` / `PhraseNote` / `PhrasePlacement`                                           | 半音差による相対フレーズと配置を表す操作値。複数placement・repeatへ展開した後は正準セッションへ保存しない                                                                                                                                |
| `RhythmPattern` / `RhythmStep`                                                               | Harmony realizationへ渡す反復リズム操作値。任意長、offset、duration、velocityを持ち、正準セッションへ保存しない                                                                                                                          |
| `Arrangement`                                                                                | 上記のすべてを束ねるアレンジのルート。revision（編集のたびに単調増加）、timebase、tracks、clips、automation、markers、regions、harmony_events、録音レコード群を持つ                                                                      |

### 4.3 録音

| エンティティ             | 役割                                                                                                                                                                                                                            |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RecordingSessionRecord` | 録音の試行グループ。track_slots（トラックごとのアクティブテイクとタイムラインクリップ）と pass_ids を持つ                                                                                                                       |
| `RecordingPassRecord`    | 録音範囲を1回通ったパス。ordinal、位置・長さ、部分開始/終了フラグ、そのパスのテイクID列                                                                                                                                         |
| `RecordingTakeRecord`    | 1パスが生んだトラック単位の成果物。`raw_audio` / `processed_audio`（`TakeAudioSource`：asset_id + サンプル範囲 + テール + サンプルレート）と`midi_asset_id`を持つ                                                               |
| `AudioTakeVariant`       | `raw` / `processed`。AudioClip がどちらの音源を使うか。片方が欠けていれば `preferred_audio_source` がフォールバック                                                                                                             |
| `RecordingCapture`       | 録音イベントそのもの（工程）。状態遷移 `recording → completing → completed \| recoverable \| failed` を唯一の遷移行列で定義。開始時点のセッション文脈（デバイス・マスター・アーム済みトラック）を保存。生成物は Asset ID で参照 |
| `DropoutInformation`     | 録音中のドロップアウト診断（書き込みサンプル数、欠落ブロック、欠落サンプル、ドロップアウト区間。raw/processed 別）                                                                                                              |
| `RecordingAsset`         | UI 用 read model。`recordings/inbox` のマニフェストから組み立て、回復（recoverable）時の表示・復旧操作を担う。永続ドメインとしては使用しない                                                                                    |

ディスク上の構成は `recordings/inbox|archive|library/<take>/`（manifest.json + raw/processed WAV + midi.json）。再生・編集に使うのはキャプチャから登録された正準 Asset であり、テイクディレクトリはリカバリ用の記録に留まる。

### 4.4 素材（Asset）

| エンティティ                         | 役割                                                                                                                                |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| `AssetId`                            | `asset:<UUIDv7>` のみ有効な、プロセス跨ぎで一意なID                                                                                 |
| `Asset`                              | 正準の制作素材。id、kind、コンテンツの場所（content_location）、作成・更新時刻、provenance、管理メタデータ（tag / note / favorite） |
| `AssetKind`                          | `audio` / `midi`                                                                                                                    |
| `Provenance` / `ProvenanceOperation` | 素材がどう生まれたか。operation（recorded / processed / rendered / imported）と source_asset_ids（消費した素材）、parameters        |
| 生成規則                             | `register`（新規IDを mint）と `derive`（source から派生物を mint）。コンテンツ変更は決して既存IDを上書きしない                      |

### 4.5 ラック

| エンティティ   | 役割                                                                                                                                                                                               |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RackInstance` | Trackで現在使われている信号チェーン（devices + macros）                                                                                                                                            |
| `RackDevice`   | チェーンの1スロット。`input` / `plugin` / `utility` / `output`。パス、バイパス、ゲイン、パラメータ値、プラグイン状態データ（不透明文字列）、欠落プラグインのプレースホルダ（disabled_placeholder） |
| `RackMacro`    | パラメータに割り当てる名前付きマクロコントロール                                                                                                                                                   |

### 4.6 バックグラウンドジョブ

| エンティティ          | 役割                                                                                     |
| --------------------- | ---------------------------------------------------------------------------------------- |
| `JobKind`             | `scan`。ジョブの種別は結果ペイロードの型を固定する判別子                                 |
| `JobState`            | `queued → running → cancelling → cancelled \| completed \| failed`（終端からは戻らない） |
| `BackgroundJobStatus` | IPC 境界の typed view。kind がタグとなり result の形状を決定                             |

---

## 5. エンティティ関係

```mermaid
flowchart TD
    CS[CreativeSession] --> AR[Arrangement]
    AR --> TR[Track]
    AR --> AC[AudioClip]
    AR --> MC[MidiClip]
    AR --> AU[AutomationLane]
    AR --> RG[TimelineRegion]
    AR --> HE[HarmonyEvent]
    AR --> RS[RecordingSessionRecord]
    RS --> RP[RecordingPassRecord]
    RP --> RT[RecordingTakeRecord]
    RT -->|raw/processed source| AS[Asset]
    AC -->|asset_id| AS
    MC -->|任意 asset_id| AS
    TR --> RI[RackInstance]
    RI --> RD[RackDevice]
    CS --> SE[SessionSettings]
    RC[RecordingCapture] -->|生成物| AS
    RC -->|ドロップアウト診断| DI[DropoutInformation]
```

- 素材（Asset）はセッションの外に正準で存在し、セッションは ID で参照する
- 録音レコード（Session/Pass/Take）はアレンジに永続化され、テイクの音源は Asset を指す
- 録音キャプチャは `recordings/` 配下の一時的な記録であり、完了時に Asset が正準となる

---

## 6. 不変条件と正準化

`validate_and_normalize`（`CreativeSession`）と `normalize_fields`（`AudioClip`）が守る規則。ロードと保存の両方の境界で適用される。

| 対象           | ルール                                                                                                                                                                                     |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| session_id     | 空文字禁止。新規は `scratch-<ms>`                                                                                                                                                          |
| タイムベース   | `ppq` は常に `960`（`TIMELINE_PPQ`）。`bpm` は有限かつ `20.0..=400.0`。拍子の分母は `1/2/4/8/16/32`、分子は非ゼロ                                                                          |
| 音楽操作値     | `MusicalPosition` は1-originのbar/beatとbeat内の正規化分数、`MusicalDuration` は正の正規化分数、`MusicalPitch` は表記を保持したMIDI範囲内の音名。入力値はCoreで正準tick/pitchへ変換する    |
| TimelineRegion | idとnameは空文字禁止、`end_tick > start_tick`。Region同士の重複・入れ子・同名を許可し、セクション種別を固定しない                                                                          |
| HarmonyEvent   | idはArrangement内で一意、和声名とtonesは空文字・空集合を禁止、`end_tick > start_tick`。最大16,384件で、イベント同士の重複・入れ子・gapを許可し、`start_tick`・`end_tick`・id順に正規化する |
| ゲイン         | マスター `-90.0..=0.0`、クリップ・トラック・デバイス `-90.0..=24.0`。非有限値はエラー（マスター）または 0.0 へ正準化                                                                       |
| パン           | `-1.0..=1.0`、非有限値は 0.0                                                                                                                                                               |
| フェード       | fade_in / fade_out はタイムライン持続時間以下にクランプ                                                                                                                                    |
| カウントイン   | `0..=8` 拍                                                                                                                                                                                 |
| AssetId        | `asset:<UUIDv7>` のみ有効（旧形式・任意文字列は拒否）                                                                                                                                      |
| 素材コンテンツ | 不変。内容変更は新しい Asset を mint する。変更可は管理メタデータのみ                                                                                                                      |
| 参照整合       | セッションが参照する AssetId は登録済みでなければならない（未登録参照は保存・ロード拒否、`architecture.md §6.4`）                                                                          |
| 録音遷移       | `RecordingCapture` は定義済み遷移行列のみ許可。終端状態からは戻れない                                                                                                                      |
| 更新時刻       | `updated_at_ms` はコミット時に単調増加し、保存世代やライブラリ表示の更新時刻として使う                                                                                                     |

---

## 7. スキーマ進化の方針

| 方針         | 内容                                                                                                        |
| ------------ | ----------------------------------------------------------------------------------------------------------- |
| 現行スキーマ | `deserialize_session` は現行のCreativeSessionだけを受け入れる。対応しない形のデータは正準状態へ取り込まない |
| 世代回復     | 自動回復は読み込めない世代を飛ばし、手動復元も検証と正準化に成功した世代だけを正準状態へ取り込む            |
| 言語間の同期 | Rustを唯一の型定義元とする。TypeScriptは生成し、C++は投影プロトコルの検証テストで整合を保つ                 |
