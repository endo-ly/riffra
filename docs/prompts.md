# ドメインモデル再設計・移行

## 目的

Riffraの現行実装を、`docs/` 配下に定義された新しい製品設計に適合するドメインモデルへ移行する。

現在の実装は旧設計を基礎としており、以下の問題を持つ。

- `Sample`、`Analyze`、`Separate` が独立したWorkspaceになっている
- Sessionが多数の制作状態を直接抱えている
- ClipやSample Padがファイルパスを直接参照している
- Recording、Library、Timelineなどがそれぞれ独自の素材表現を持っている
- Rackの「現在使用中の状態」と「保存して再利用する定義」が区別されていない
- ドメイン上の制作ルールがRustとTypeScriptの双方へ分散している

今後のApplication層整備とUI統合の基盤となるドメインモデルをRust側に構築し、現行データと既存機能を新しいモデルへ移行する。

完了後は、新しい機能を旧モデルへ追加しない状態にする。

---

# 1. 設計方針

## 1.1 RustをドメインのAuthorityとする

主要なドメインモデルとドメインルールはRust側に実装する。

TypeScript側には以下のみを置く。

- Rustとの通信に使用するDTO
- UI表示用のView Model
- UI固有の一時状態
- 編集途中のDraft State

以下のようなドメイン判断をTypeScript側へ実装しない。

- ClipをTrackへ追加できるか
- AssetをArrangeへ配置できるか
- RecordingCaptureを次の状態へ遷移できるか
- DesignToolを現在のWorkspaceで利用できるか
- RackDefinitionを変更した結果をどう保存するか
- Asset参照が有効か

同じドメインルールをRustとTypeScriptへ重複実装しない。

---

## 1.2 C++ Audio Runtimeの設計は変更しない

`riffra-audio.exe` は引き続きAudio Runtimeとして扱う。

C++側へ以下の概念を導入しない。

- CreativeSession
- Asset
- Library
- Project
- Provenance
- RecordingCapture

C++へ渡す時点でRust側がAsset等を解決し、音声処理に必要な具体値へ変換する。

```text
CreativeSession / Asset
        ↓
Rust Domain / Application
        ↓
path / range / plugin state / MIDI / audio parameters
        ↓
riffra-audio.exe
```

本PhaseでC++側の内部アーキテクチャを再設計しない。

新しいRust側モデルとの接続に必要なインターフェース変更のみ行う。

---

# 2. 新しいドメイン構造

以下を中心モデルとして実装する。

```text
CreativeSession
Asset
RecordingCapture
RackInstance
RackDefinition
Provenance
```

Background Jobそのものは本Phaseで独立したDomain Aggregateにしない。

Analysis、Separation、Renderは既存構造を維持し、生成結果をAssetとして扱えるモデルまで整備する。

Application層そのものの本格的な再構築は後でで行う。

---

# 3. CreativeSession

## 3.1 CreativeSessionを制作状態の中心とする

現在の`ScratchSession`を新しい`CreativeSession`へ移行する。

Scratch Sessionと保存済みProjectで別の制作モデルを作らない。

どちらも`CreativeSession`を使用する。

CreativeSessionは以下を保持する。

```text
CreativeSession
├─ id
├─ workspace
├─ designContext
├─ playState
├─ arrangement
└─ sessionSettings
```

CreativeSessionは以下を保持しない。

- 音声ファイル本体
- MIDIファイル本体
- Asset検索用Libraryデータ
- Assetのファイルパスを直接持つClip
- Recordingの実ファイル管理
- Background Jobの実行状態そのもの

---

## 3.2 Workspaceを4種類へ変更する

新しいWorkspaceは以下の4種類に固定する。

```rust
Home
Play
Design
Arrange
```

旧Workspaceの以下3種類を削除する。

```text
Sample
Analyze
Separate
```

これらは`DesignTool`として定義する。

```rust
Sample
Analyze
Separate
```

CreativeSessionには以下を保持する。

```text
workspace: Workspace
design_context: DesignContext
```

`DesignContext`には最低限以下を持たせる。

```text
active_tool: DesignTool
target_asset_id: Option<AssetId>
```

`DesignTool`は`Workspace::Design`と独立したトップレベルWorkspaceとして扱わない。

---

# 4. Arrangement

Arrangeの制作状態を`Arrangement`として明示的に分離する。

```text
CreativeSession
└─ Arrangement
   ├─ Tracks
   ├─ AudioClips
   └─ MidiClips
```

現在CreativeSession直下に存在するTimeline、Track、Clip関連状態をArrangement配下へ移行する。

## 4.1 AudioClip

Audio Clipは以下を持つ。

```text
id
track_id
asset_id
position
source_range
gain
pan
fade
loop_settings
```

`assetPath`を削除する。

AudioClipからファイルパスを直接参照しない。

---

## 4.2 TrackとClipのルール

Rust側で以下を保証する。

- Clipは存在するTrackに所属する
- Clipの開始位置は負数にならない
- source rangeの開始位置は負数にならない
- source rangeの終了位置は開始位置より後である
- AudioClipは`AssetId`を参照する
- Clip追加時に参照Assetの存在を検証する

UIからSession JSONを直接書き換えることで、これらの制約を回避できる構造にしない。

---

# 5. Asset

## 5.1 Assetを制作素材の正規ドメインモデルとする

以下の共通モデルを実装する。

```text
Asset
├─ id: AssetId
├─ kind: AssetKind
├─ name
├─ content_location
├─ created_at
├─ updated_at
└─ provenance
```

扱う`AssetKind`は以下とする。

```text
Audio
MIDI
Sample
RackDefinition
GenerationDefinition
```

既存機能から生成される素材は、このいずれかのAssetとして表現する。

新しいAsset種別を将来追加できるenum構造にする。

---

## 5.2 Asset ID

Asset IDはRiffra全体で一意とする。

Project単位でID空間を分けない。

新規Asset生成時には新しいAsset IDを発行する。

Project Export / Importを行ってもAsset IDを維持する。

---

## 5.3 Assetの正本

本Phaseでは新しいAsset Manifest形式を導入しない。

既存の以下を活用する。

- 実ファイル
- SQLiteの既存Asset情報
- 現在の保存ディレクトリ構造

ただしドメイン上は`Asset`を正規モデルとする。

`LibraryAsset`を制作ドメインの正規モデルとして使用しない。

既存のSQLite LibraryはAsset検索用のRead Modelとして位置付ける。

```text
Asset Domain Model
        ↓ index
SQLite Library
```

Library固有のモデルをSessionやArrangementの参照先として使用しない。

---

# 6. AssetのImmutableルール

Assetの制作内容は原則Immutableとする。

以下を変更した結果は新しいAssetとして保存する。

- Audioの音声内容
- MIDI内容
- Sample定義
- RackDefinition内容
- GenerationDefinition内容

変更後は新しいAsset IDを発行する。

以下の管理情報は同じAsset IDのまま変更可能とする。

- name
- tags
- note
- favorite等のLibrary管理情報

既存Assetの制作内容を上書きして、同じAsset IDの意味を変更しない。

---

# 7. Provenance

Assetの生成元を表現する`Provenance`を実装する。

最低限以下を保持する。

```text
source_asset_ids
operation
parameters
```

`operation`は文字列を自由入力する形式にせず、既知の処理種別をenumで表現する。

最低限以下を定義する。

```text
Recorded
Processed
Sampled
Separated
Rendered
Generated
```

例:

```text
Audio Asset A
    ↓ Sampled
Sample Asset B
```

```text
Audio Asset A
    ↓ Separated
Audio Asset B
```

複数Assetから生成されるRenderについては、`source_asset_ids`へ複数IDを保持する。

本PhaseではProvenance専用Graph Engineを実装しない。

Assetから直接生成元Asset IDを辿れる構造とする。

---

# 8. RecordingCapture

## 8.1 録音処理と録音結果を分離する

既存のRecording Manifestを`RecordingCapture`の永続表現として発展させる。

`RecordingCapture`は「一回の録音」という出来事を表す。

録音結果そのものはAssetとして扱う。

```text
RecordingCapture
        ↓ produces
Raw Audio Asset
Processed Audio Asset
MIDI Asset
```

---

## 8.2 RecordingCaptureの状態

以下の状態を定義する。

```text
Recording
Completing
Completed
Recoverable
Failed
```

許可する状態遷移をRust側で定義する。

```text
Recording
├─→ Completing
├─→ Recoverable
└─→ Failed

Completing
├─→ Completed
├─→ Recoverable
└─→ Failed
```

`Completed`、`Recoverable`、`Failed`から`Recording`へ戻す遷移は許可しない。

不正な状態遷移はエラーとして拒否する。

---

## 8.3 RecordingCaptureのデータ

最低限以下を保持する。

```text
capture_id
session_id
status
started_at
completed_at
sample_rate
input_device
rack_snapshot
raw_audio_asset_id
processed_audio_asset_id
midi_asset_id
dropout_information
```

存在しない成果物については対応するAsset IDを`None`とする。

Partial Recording Recoveryの既存機能はこのモデルへ接続する。

Recoveryによって復旧された成果物もAssetとして登録する。

---

# 9. Rack

Rackの現在状態と再利用可能な保存物を分離する。

## 9.1 RackInstance

CreativeSessionで現在使用しているRackを`RackInstance`とする。

以下を保持する。

- Device順序
- Plugin State
- Parameter
- Bypass
- Utility設定
- 現在の実行状態に必要な設定

既存Session内のRackは`RackInstance`へ移行する。

---

## 9.2 RackDefinition

保存して再利用可能なRackを`RackDefinition`とする。

`RackDefinition`は`AssetKind::RackDefinition`のAssetとして保存する。

```text
RackDefinition Asset
        ↓ load
RackInstance
```

RackDefinitionから生成したRackInstanceを編集しても、元のRackDefinition Assetを変更しない。

編集後のRackを保存する場合は、新しいRackDefinition Assetを生成し、新しいAsset IDを発行する。

RackDefinitionの上書き保存は実装しない。

---

# 10. Project

Projectを独立したDomain Aggregateとして新設しない。

制作状態の正規モデルは`CreativeSession`とする。

既存Project保存機能は以下の組み合わせを保存する仕組みとして扱う。

```text
CreativeSession
+
参照Asset情報
+
Project Manifest
```

Project Export時には、CreativeSessionから参照されるAssetを収集する。

Project Import時にはAsset IDを維持する。

同一Asset IDが既に存在する場合は、既存Assetとの同一性を確認する。

同じIDで異なる制作内容を持つAssetを無条件に上書きしない。

この衝突処理のUI改善は本Phaseでは行わないが、データ破壊につながる上書きは拒否する。

---

# 11. Library

現在のSQLite Libraryを維持する。

LibraryはAsset検索用Read Modelとして扱う。

```text
Asset
  ↓ indexing
LibraryEntry
```

既存の`LibraryAsset`をSession、Clip、RecordingCapture等から直接参照しない。

既存の`asset_relations`は、新しいProvenanceモデルとの整合を取る。

Asset登録時にはLibrary Indexも更新する。

Library Index更新に失敗した場合でもAsset本体を削除しない。

LibraryからAsset情報を再取得できない場合は、Asset本体の存在を優先する。

---

# 12. Rustのコード構造

Rust側に`domain`領域を作成する。

最低限以下の責務を分離する。

```text
domain/
├─ session
├─ asset
├─ recording
└─ rack
```

実際のモジュール名は上記に合わせる。

Domainモデルを`src-tauri/src/lib.rs`へ直接追加しない。

Tauri command内に新しいDomain Ruleを実装しない。

あとででApplication層を整理するため、本Phaseでは既存commandから新しいDomain Modelを呼び出す形で接続する。

---

# 13. TypeScript側の変更

現在の`src/lib/domain.ts`に存在する旧Domain Modelをそのまま維持しない。

RustからUIへ公開するDTOとして整理する。

以下を変更する。

```text
Workspace
```

を、

```text
home
play
design
arrange
```

へ変更する。

以下を追加する。

```text
DesignTool
AssetId
AssetSummaryDto
DesignContextDto
```

Timeline Clipの`assetPath`を削除し、`assetId`へ変更する。

TypeScript側でRustと同じDomain Entityのメソッドや状態遷移ロジックを再実装しない。

---

# 14. Session Format v2

Session Format Versionを更新する。

旧形式をv1、新形式をv2として扱う。

以下のMigrationを実装する。

```text
v1
↓
migrate_v1_to_v2
↓
v2
```

---

## 14.1 Workspace Migration

以下の変換を行う。

```text
home
→ workspace = home

play
→ workspace = play

arrange
→ workspace = arrange

sample
→ workspace = design
→ designTool = sample

analyze
→ workspace = design
→ designTool = analyze

separate
→ workspace = design
→ designTool = separate
```

---

## 14.2 Clip Migration

旧Clipの`assetPath`ごとに対応Assetを解決する。

同一ファイルを参照する既存Assetが存在する場合、そのAsset IDを使用する。

対応Assetが存在しない場合、新しいAudio Assetを登録する。

その後、

```text
assetPath
```

を削除し、

```text
assetId
```

へ置換する。

---

## 14.3 Sample Migration

ファイルパスを直接保持しているSample PadについてもClipと同じ規則でAudio Assetへ変換する。

Sample自体を独立した制作物として保存しているものは`Sample Asset`へ移行する。

---

## 14.4 Rack Migration

旧SessionのRackを`RackInstance`へそのまま移行する。

以下を失わない。

- Plugin ID
- Plugin State
- Parameter
- Bypass
- Device順序
- Utility設定

既存Rackを自動的にRackDefinition Assetへ変換しない。

RackDefinition Assetは明示的にLibraryへ保存されたRackのみ対象とする。

---

# 15. Migrationの安全性

Migrationでは以下を必須とする。

- v1データを上書きする前に既存Backup機構を実行する
- Migration失敗時はv2として保存しない
- Migration失敗時に元Sessionを変更しない
- 未知のFormat Versionをv1またはv2として推測しない
- Unsupported VersionとCorrupt Dataを区別する
- Recovery Generationの既存機能を維持する

---

# 16. 既存機能の接続

新しいDomain Modelへ以下の既存機能を接続する。

- Session load/save
- Autosave
- Recovery
- Recording
- Recording recovery
- Library indexing
- Timeline
- MIDI Clip
- Sample Pad
- Rack
- Plugin State
- Project import/export

これらが旧Domain Modelを正規モデルとして使用していない状態にする。

旧形式を読み込むためのMigrationコードは残す。

通常実行経路で旧`ScratchSession`、旧Workspace、`assetPath`ベースのClipを生成しない。

---

# 17. テスト

## 17.1 Domain Test

以下をRust単体テストで検証する。

### CreativeSession

- Workspaceが4種類のみである
- DesignToolを保持できる
- Designの対象AssetをAsset IDで保持できる
- ArrangementがTrackとClipを管理する

### Arrangement

- 存在しないTrackへClipを追加できない
- 不正なsource rangeを持つClipを追加できない
- AudioClipがAsset IDを保持する

### RecordingCapture

定義したすべての正常状態遷移をテストする。

定義していない状態遷移が拒否されることをテストする。

### Rack

- RackDefinitionからRackInstanceを生成できる
- RackInstance変更後もRackDefinitionが変更されない
- 保存時に新しいRackDefinition Asset IDが発行される

### Asset

- 新規Asset生成時にAsset IDが発行される
- 制作内容の変更時に同じAsset IDを再利用しない
- Metadata更新ではAsset IDを維持する
- Provenanceから生成元Asset IDを取得できる

---

## 17.2 Migration Test

実際のv1形式を表現するFixtureをRepository内に追加する。

以下をテストする。

```text
v1 fixture
↓
migrate_v1_to_v2
↓
serialize
↓
deserialize
↓
v2 domain model
```

検証対象は以下とする。

- Workspace
- DesignTool
- Rack
- Plugin State
- Track
- Audio Clip
- MIDI Clip
- Sample
- Asset参照

Migration前後で制作内容が失われていないことをAssertする。

---

## 17.3 Regression Test

既存テストを維持し、以下の機能を壊さない。

- Application起動
- Scratch Session開始
- Session保存
- Autosave
- Recovery
- Audio Device
- Emergency Mute
- VST3 Plugin Load
- Plugin State復元
- Recording
- Partial Recording Recovery
- MIDI
- Render
- Project Import / Export

---

# 18. 削除対象

Migrationと互換処理の実装完了後、通常実行経路から以下を削除する。

- `Sample` Workspace
- `Analyze` Workspace
- `Separate` Workspace
- Audio Clipの`assetPath`
- 新規データ生成に使用される旧Session Model
- SessionからLibrary固有モデルへの直接依存
- TypeScript側の重複Domain Rule

Migration専用の旧形式型は残す。

旧形式型を新規Session生成や通常更新処理に使用しない。

---

# 19. このPhaseで実装しないもの

以下は変更しない。

- Play / Design / ArrangeのUI統合
- `useApp`全体のApplication層分割
- Tauri command全体の再構築
- Signal Generator
- Asset Manifest方式への永続化変更
- Asset Versioning
- Event Sourcing
- CQRSフレームワーク
- C++ Audio Runtimeの内部再設計

---

# 完了条件

以下をすべて満たした時点で完了とする。

1. Rust側に新しいDomain Modelが存在する
2. `CreativeSession`が制作状態の正規モデルになっている
3. Workspaceが`Home / Play / Design / Arrange`の4種類になっている
4. `Sample / Analyze / Separate`が`DesignTool`になっている
5. Arrangeの状態が`Arrangement`として整理されている
6. Audio Clipが`assetPath`ではなく`AssetId`を参照している
7. Assetが制作素材の正規ドメインモデルになっている
8. Asset IDがRiffra全体で一意になっている
9. Assetの制作内容がImmutableとして扱われている
10. RecordingCaptureと録音結果Assetが分離されている
11. RecordingCaptureの状態遷移がRustで制御されている
12. RackInstanceとRackDefinitionが分離されている
13. RackDefinitionがAssetとして扱われている
14. ProvenanceによってAssetの生成元を追跡できる
15. LibraryがAsset検索用Read Modelとして扱われている
16. Rustが主要なDomain RuleのAuthorityになっている
17. v1 Sessionからv2 SessionへMigrationできる
18. Migration失敗時に既存データを破壊しない
19. 通常実行経路が旧Session Modelへ依存していない
20. 通常実行経路で`assetPath`を持つ新しいClipが生成されない
21. 既存の主要機能がRegression Testを通過する
22. `npm run verify`が成功する
23. Rustの全テストが成功する

以降のApplication層整備とUI統合を新しいDomain Modelのみを基準として実施できる状態にする。
