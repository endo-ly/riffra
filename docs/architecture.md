# Riffra アーキテクチャ

## 1. ドキュメントの目的とスコープ

本書はRiffraのシステム構造と主要機構を記述する。「どの層が何の正準状態を持ち、どのように整合するか」を対象とする。個々の型の詳細は `data-model.md`、境界の契約は `ipc.md`、画面の設計は `ui-ux-design/arrange-screen.md` を参照する。

### 書くこと

- プロセス構成（Tauriシェルとサイドカー群）
- レイヤー構成と依存方向
- セッション正準化、ランタイム投影、永続化・回復、セーフモード、ライブラリ索引、バックグラウンドジョブの各機構
- ドメインが守る不変条件

### 書かないこと

- 個別のTauri命令・IPCプロトコルの詳細（`ipc.md`）
- エンティティのフィールド定義（`data-model.md`、code参照）
- 画面の操作仕様（`ui-ux-design/`）

---

## 2. プロセス構成

```text
┌─────────────────────────────────────────────────────────────────┐
│ Tauri シェル                                                     │
│ ┌──────────────────────┐   ┌──────────────────────────────────┐ │
│ │ WebView (React)      │   │ Rust バックエンド                │ │
│ │ 表示・操作・表示状態  │◀──▶│ HostConnectionManager / adapter │ │
│ │ NativeApi 経由で指令 │   │ Embedded / Attached Host       │ │
│ └──────────────────────┘   └──────────────────────────────────┘ │
└──────┬──────────────────────────┬───────────────────────────────┘
       │ JSON Lines (stdin/stdout)
       │ 1つのコマンド + 1行の応答
┌───────────────────────┐  ┌─────────────────────┐  ┌─────────────────┐
│ riffra-audio          │  │ riffra-plugin-scan  │  │ riffra-render   │
│ リアルタイム音声       │  │ VST3スキャン        │  │ オフライン      │
│ JUCE / ASIO / WASAPI  │  │ (起動時・再スキャン) │  │ レンダリング    │
│ 投影・演奏・録音・監視 │  │                     │  │                 │
│ VST3 ホスティング     │  │                     │  │                 │
└───────────────────────┘  └─────────────────────┘  └─────────────────┘
```

| プロセス            | 役割                                                                | 所有状態                                   |
| ------------------- | ------------------------------------------------------------------- | ------------------------------------------ |
| Tauri シェル        | DesktopのUI、Host接続、Tauri event bridge                           | UI接続、window、dialog、Host選択           |
| `riffra serve` Host | GUIなしの正準状態、履歴、ローカルControl、Runtime投影を監督         | DataRootLease、AppCore、Runtime状態        |
| riffra-audio        | リアルタイム音声。デバイス、Built-in / VST3グラフ、演奏・録音・監視 | ランタイムグラフ（投影される一時状態のみ） |
| riffra-plugin-scan  | VST3の列挙・検証（`--probe` 系と分離された専用起動モード）          | なし                                       |
| riffra-render       | タイムラインのオフラインレンダリング                                | なし                                       |

- リアルタイム音声は常にサイドカーが担当し、Host プロセスは音声コールバックやプラグインコードを実行しない
- GUI を使わない構成では `riffra serve` が共有 `DawHost` をフォアグラウンドで起動する
- セーフモードの扱いは §7 を参照

---

## 3. レイヤー構成

依存は上位から下位への一方向。下位層は上位層を知らない。たとえば riffra-core は Tauri の存在を知らないため、Desktop と CLI のどちらからも使い回せる。

```text
React フロントエンド
  ├─ 状態: CreativeSession を保持・描画する
  ├─ 編集: 機能別の窓口（NativeApi capability）経由で Tauri 命令を呼ぶ
  ├─ app: 起動処理（bootstrap）/ アプリ全体の組み立て（Composition）/ 全体のRuntime寿命管理
  ├─ features: 機能ごとの状態・操作・UI・テスト（arrange、audio、library、plugins、project、recording、transport）
  ├─ shared: 機能に属さない共通UI・汎用部品（Toast、ContextMenu、audio meters など）
  ├─ native: ReactとTauriの境界（窓口の定義・invoke実装・テスト用の偽装 FakeNativeApi）
  └─ model: src/model/generated（Rust の ts-rs 出力を gen-barrel.js で束ねた型）

Tauri 命令層 (src-tauri/src/**/commands.rs)
  └─ 受け取った命令を共有の処理役（Host service）へ委譲する（実行モードは ipc.md §3.1）

Desktop adapter (apps/desktop/src-tauri/src)
  ├─ Tauri の命令・通知・窓（command / event / window）との境界を担当
  ├─ 接続状態を管理する（HostConnectionManager。内蔵 Embedded ／別プロセス Attached ／未接続 Disconnected）
  ├─ 内蔵構成では進行役本体（riffra-runtime::DawHost）を所有し、別プロセス構成では接続用具（LocalHostClient）を利用
  └─ 現在Hostの操作・起動情報・通知（operation、bootstrap、event）をWebViewへ接続

riffra-runtime（crates/riffra-runtime）: Desktop / Headless Host が共有するlive Runtime基盤
  ├─ Host本体・設定・利用権（DawHost / HostConfig / DataRootLease）を含むHostの構成
  ├─ 音声の監督（AudioSupervisor）/ 内蔵音源の実行基盤（Instrument Runtime）/ 正準と再生用複製の突き合わせ（RuntimeReconciler）/ 再生順の整理（Transport ordering）
  ├─ 同梱内蔵音源の一覧（Built-in instrument catalog。起動時の組み立て元から渡す）
  ├─ 書き出し子プロセス（`riffra-render` executable）の起動・制御口（adapter）
  ├─ 出来事の受付（HostEventSink）/ 配信所（HostEventHub）/ 起動情報（Host bootstrap）
  └─ 別プロセスからの操作要求（command connection）と出来事購読（events connection）の受付（Local Control Server）

riffra-control（crates/riffra-control）: current-user Local Host接続基盤
  ├─ Hostの名乗り（identity）/ 接続先情報（endpoint descriptor）/ 一覧（Local Host Registry）
  ├─ 接続用具（LocalHostClient）/ 要求と応答のやり取り / 出来事の流れ（event stream）
  └─ プロセス間通信路（Named Pipe / Unix Domain Socket）での区切り（framing）と権限の境界

riffra-host（crates/riffra-host）: Desktop / CLI 共通のOS境界
  ├─ Projectの出し入れ（ProjectStore）/ Project単位の楽曲保存（SessionStore）/ 素材置き場（Asset Repository）/ 可搬形式（Project package）
  ├─ WAVの付帯情報とMIDIファイル（SMF）の読み取り
  └─ 多重起動を防ぐ利用権（DataRootLease）

riffra-core（crates/riffra-core）: プラットフォーム非依存のApplication / Domain / Ports
  ├─ 楽曲データの形（domain: CreativeSession / Arrangement / Recording / Asset / Rack）
  ├─ 操作の手順（application: Session / Arrangement / Recording / Rack / Transport / History）
  ├─ 外部（保存・音声・書き出し）との接続口（ports: SessionStorage / RuntimeProjection / RenderRuntime）
  ├─ 正準・順番・履歴・投影順の中枢（AppCore）
  ├─ 保存前の検査と整形（validate_and_normalize）
  └─ Tauri・WebView・OS統合を含まない

CLI ホスト（apps/cli）
  ├─ 利用権・Project保存・楽曲保存（DataRootLease / ProjectStore / SessionStore）を取得する
  ├─ 決まりごと（AppCore）と保存口（SessionStorage Port）を直接利用する
  ├─ 一回きりの起動引数も対話入力も同じ振り分け（Dispatcher）へ渡す
  └─ ファイル編集専用の Standalone と、進行役を起動する serve の二つの使い方を持つ

Attached CLI（apps/cli --attach）
  ├─ 起動中の Host の操作受付口（制御エンドポイント）を見つけてつなぐ
  ├─ 接続先の決まりごと・保存・利用権（AppCore / SessionStore / DataRootLease）は直接開かず、借りて使う
  └─ 操作受付（Host Control Server）経由で正準操作を頼む

永続化・外部境界
  ├─ riffra-host: Project保存 / 楽曲保存 / 素材置き場 / 可搬形式 / ファイル読み取り部品（ProjectStore / SessionStore / Asset Repository / Project package / file parsers）
  ├─ ProjectStore: workspace.json + projects/<project-id>（§6）
  ├─ SessionStore: projects/<project-id>/session.json + generations（§6）
  ├─ ライブラリ索引: SQLite リードモデル（§8）
  └─ ランタイム境界: 音声の監督 → 音声サイドカー（AudioSupervisor → riffra-audio。§5）
```

- 制作状態を変更する命令は Core の Application 層を通り、確定した `CanonicalState` が同じ順序でフロントエンドへ返る
- 起動中 Host の外部制御と実行モード別の状態所有は `ipc.md` §8（境界F）を参照

---

## 4. 制作状態とコミット

### 4.1 単一の正準状態

- 正準モデルは CreativeSession（`riffra-core/src/domain/session`）。アレンジ、クリップ、テイク、トラック、ラック、設定など永続化される制作状態を一体として表す
- `AppCore` は CreativeSession・正準シーケンス・Undo/Redo 履歴を一体の状態として管理する
- フロントエンドと音声サイドカーが扱うのは正準の投影のみである

### 4.2 Core操作境界

- `AppCore` は主要な制作操作の入口であり、正準状態・操作順序・Undo/Redo 履歴・ランタイム投影の順序を一体として管理する
- ホストは Port を実装して永続化や音声ランタイムへ接続する。正準状態の採否は決めない
- Desktop と CLI は同じ編集規則と永続化規則を共有する

### 4.3 コミットパイプライン

`AppCore` と `riffra-core::application` が正準操作の保存境界を定める。

1. Application操作が現在の正準状態から更新候補を作る
2. Domainが不変条件を検証し、正準化する
3. SessionStorage Portを通じて更新候補を永続化する
4. 保存成功後に正準状態を交換し、履歴と投影順序を更新する
5. 共有Runtime serviceがライブラリ索引の更新と音声ランタイムへの投影を要求する

検証または永続化に失敗した更新候補は破棄し、正準状態にも履歴にも反映しない。

開始から確定まで時間が空く操作（録音の後処理など）は、自身の変更のみを最新の正準状態へ適用する。処理中に確定した別の編集は維持される。

---

## 5. ランタイム投影とトランスポート

リアルタイム音声グラフは正準状態の**投影（projection）**である。正準セッションが変わると、投影だけが再構築される。

### 5.1 投影プロトコル

Core の `RuntimeProjection` Port は、正準スナップショットとその確定順序をホストの音声ランタイムへ渡す契約を定める。`riffra-runtime` の `RuntimeReconciler` がサイドカーとの接続、最新投影の採用、Transport ordering を担う。

| 操作                                  | 意味                                                                              |
| ------------------------------------- | --------------------------------------------------------------------------------- |
| `prepare_timeline_snapshot(snapshot)` | 投影候補をサイドカーへ渡して事前構築（VST読み込み・グラフ構築）。まだ再生されない |
| `commit_timeline_snapshot()`          | 準備済みの投影を現役グラフへ昇格                                                  |
| `discard_timeline_snapshot()`         | 準備済みの候補を破棄                                                              |

再生状態の所有分担は次の通り。

| 所有者                           | 範囲                                                                                                               |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `TrackRuntime`（Track ごと）     | Instrument Runtime、Effect Chain、MIDI Scheduler、ライブ MIDI 状態、Automation、PDC 用バッファ、録音キャプチャ状態 |
| Device Runtime（TrackRuntime内） | プラグインインスタンス。投影間で再利用される単位                                                                   |
| `TimelineEngine`                 | グラフの公開、処理順序、Transport、ループ、クロックの調停                                                          |

- Arrangement の MIDI と Play Surface / 外部 MIDI の入力は同じ Instrument Runtime へ合流する。ライブ入力専用の音源やエフェクト経路は設けない

音声の基本経路は次のとおりである。

```text
Audio Track
Timeline Audio ──┐
Live Audio ────────────────────────────┴─ Pre-FX → Effect Chain → Track output compensation → master mix

Instrument Track
Timeline MIDI ──┐
Live MIDI ──────┴─ Instrument Runtime → Effect Chain → Track output compensation → master mix
```

MIDI とライブ入力の扱いは次の通り。

- MIDI イベントは投影時に時系列へ整列し、再生時はカーソル周辺の必要範囲のみ取り出す。ループは事前展開せず、境界の Note Off と次周の Note On を同一スケジューラで処理する
- Timeline と Live は同一 Track DSP を同一時間文脈で通り、Track 出力にソース固有の準備済み PDC を適用する。Play Surface で選択された Instrument Track のみ、遅延バッファの更新を続けながらトラック間補償遅延を迂回する
- ライブ MIDI は固定容量のキューで受け、超過分は破棄して診断値へ記録する
- Audio Track の入力監視は、その Track の Effect Chain を一度だけ通る

録音キャプチャの扱いは次の通り。

- リアルタイム中は入力の Raw テイクのみ保存する
- 停止時は短いグラフ境界でキャプチャ終了と Rack 状態を確定し、Transport 停止後にグラフ外で Processed Variant を生成する
- 生成は正準 Rack 状態から一時的な Effect Chain を構築し、ブロック単位で書き出す。録音時間に比例する作業用バッファも、録音専用の常設 Effect Chain も使わない

### 5.2 投影の整合性

投影要求の識別子は `canonicalSequence`、`sessionRevision`、サイドカーの `runtimeGeneration`、デバイス環境の `audioEnvironmentRevision` の組である。Host はこの組を一致条件に投影を直列処理する。準備中は現在のグラフを維持し、準備と有効化が完了した投影のみ再生に使う。

Play / Stop の順序は次の通り。

```text
Play（準備済み）→ 直ちに開始
Play（準備中） → TransportStatus: starting を表示して投影完了を待つ
Stop → 保留中の Play より優先。後続の準備完了でも自動再生しない
投影失敗 → Play 意図を解除。再試行は呼び出し側の明示要求でのみ行う
再起動・環境変更後 → 最新の正準スナップショットを新しい識別子で投影する
```

### 5.3 トランスポート

- Host Runtime が Play / Stop 要求の順序と、再生に必要な投影の有効性を判断する。Transport executor は決定済みの要求を音声ランタイムへ伝える
- 反映対象は最新の決定済み要求のみとし、古い要求や古い投影完了で現在の再生状態を変えない。投影が未準備の場合は `starting` を経由する
- 音声デバイスの状態、投影グラフの状態、トランスポートの状態は別々に通知する

### 5.4 デバイス、安全状態、診断

デバイスの有効化とグラフ投影は別の操作である。

```text
setAudioDriver → 要求された設定（ドライバ・デバイス・サンプルレート・バッファサイズ）を Native 側で有効化 → 応答
  → 有効化失敗: 以前のデバイスを復元する
  → 有効化後の投影失敗: デバイスは戻さず、EngineTransition 保持のまま投影失敗を報告する
  → 要求拒否＋復元成功: 以前の音声環境へ正準グラフを再投影してから遷移を完了する
```

- 有効化する設定は明示されたものに限り、別の設定への置き換えは行わない

安全ミュートは Native の atomic bitmask で所有者別に管理する（ユーザー操作・エンジン遷移・デバイス障害・フィードバック保護は独立）。状態通知の分担は次の通り。

| 通知                      | 表す状態                     |
| ------------------------- | ---------------------------- |
| `AudioStatus`             | デバイスとコールバックの状態 |
| `RuntimeProjectionStatus` | グラフ投影の状態             |
| `TransportStatus`         | 停止・準備中・再生中の状態   |

Audio Status の診断値は、コールバック計測（回数・平均/最大処理時間・オーバーラン）、出力診断（準備前ピーク・リミッターのゲインリダクション・最終ハードクリップ数）、ライブ MIDI のドロップ数、規模（Track / Runtime / Plugin 数・最大レイテンシ）、投影時間、音声環境 revision を含む。これらは障害の推測材料ではなく、同じ世代の音声処理状態を確認するための値である。

フィードバック保護の検知中は `FeedbackProtection` ミュートを保持する。解除は安全確認後の明示リセット操作で行い、他の所有者のミュートは維持する。

---

## 6. 永続化と回復

- Project は DataRoot 内の制作単位であり、正準状態を `projects/<project-id>/session.json` に保持する
- Project 切替は Project 一覧の操作のみで行う
- `.riffra` は Project の portable package であり、Import / Export 時のみ扱う。正準そのものではない
- 音声 Render の結果は Project package とは別に `renders/` へ保存する

### 6.1 ディスクレイアウト

```text
<data_root>/
├─ workspace.json            # Active Project の識別
├─ .riffra.lock              # DataRoot の排他所有
├─ projects/
│  └─ <project-id>/          # UUID形式のProject container
│     ├─ session.json        # Projectの現行CreativeSession
│     └─ generations/        # 世代スナップショット（最大20件）
├─ library/riffra.db        # ライブラリ索引（SQLite リードモデル）
├─ recordings/
│  ├─ inbox/                # 録音キャプチャ（録音直後のテイク置き場）
│  ├─ archive/              # アーカイブ済みテイク
│  └─ library/              # ライブラリへ昇格済みテイク
├─ assets/
│  └─ imports/              # 外部ファイルのインポート先（register で登録）
└─ renders/
   └─ render-{ms}/          # レンダリング出力（timeline.wav + render.json）
```

### 6.2 アトミック保存と世代管理

`riffra-host::SessionStore` は、Projectごとに、クラッシュしても `session.json` が「完全な旧内容か完全な新内容」のどちらかになるよう保存する。

1. 現在の `session.json` を同じProjectの `generations/` へコピー
2. 新内容を `.tmp` へ書き、`sync_all`（fsync）
3. `MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)`（Windows）または `rename` で置換
4. 古い世代を20件を超えて削除

保存前に Project 領域の空き容量を検証し、容量不足時は保存を拒否する。保存はプロセス内のグローバルロックで直列化する。

### 6.3 ロードと回復

`ProjectStore` はDataRootの初期化時に最初のProjectを作成するか、`workspace.json` のActive Projectを選ぶ。各Projectの `SessionStore` は以下の順で解決する。

1. `session.json` を読み、`deserialize_session` → `validate_and_normalize` → **アセット参照検証** を通れば採用
2. 破損・参照不正なら同じProjectの `generations/` を新しい順に読み、**スキーマ検証に通る最新世代** を `recovered_from_generation: true` として採用
3. 新規DataRootにProjectが無い場合だけ、空のCreativeSessionを作成して保存

破損した `session.json` は上書きしない（唯一の回復手段のため）。世代回復の手順は次の通り。

- `recovery_candidates()` が世代ファイルからメタデータのみ軽量に読み、一覧として提示する
- ユーザー選択の `restore_generation()` が指定世代を正準状態として復元・保存する
- Active Project 以外の読込不能 Project も一覧に残し、読込エラー付きで表示する

### 6.4 参照整合

保存・ロードの両方で `asset::validate_session_references` が実行され、セッションが参照する全アセットIDが登録済みであることを保証する。コンテンツファイルの欠落は MissingDependency として UI に列挙して継続するが、未登録のアセットIDを含むセッションの保存・ロードは拒否する。

### 6.5 DataRootの所有

- Desktop Embedded Host の既定 DataRoot はユーザーの `Music/Riffra` 配下である。Standalone CLI と `riffra serve` は指定された DataRoot を使う。Attached mode の Desktop と Attached CLI は接続先 Host の DataRoot を利用する（直接開かない）
- ローカル Host は起動時に `riffra-host::DataRootLease` を取得し、生存期間中保持する
- 排他にはロックファイルではなく OS のファイルロックを使うため、異常終了後の残骸があっても新規ホストは起動できる
- 使用中の DataRoot を別プロセスが開いた場合は明示的な使用中エラーを返す

---

## 7. セーフモード

起動条件は `--safe-mode` フラグまたは `RIFFRA_SAFE_MODE` 環境変数（`1` / `true` / `yes` / `on`）である。対象はサイドカーの起動・デバイスアクセス・プラグイン読み込みの省略であり、`riffra serve --safe-mode` も同じ扱いで `AudioSupervisor` をオフライン実装として生成する。

- 初期化は「セッション読込 + ライブラリ索引」のみで完了し、`BootstrapState.safeMode: true` がUIに通知される
- 音声・録音・再生・プレビュー・VST3読込系の命令はセーフモードではエラーとして無効化される。オフライン解析・書き出し・ライブラリ操作は通常モードと同一に利用できる
- 外部デバイスやプラグインが原因のハングを切り分けるための診断手段であり、データの読み書きは通常モードと同一

セーフモードの判定材料は `--safe-mode` の明示のみとし、他の起動引数（`--serve` など）からの推定は行わない。

---

## 8. ライブラリ索引（リードモデル）

ライブラリは SQLite の**読み取り専用モデル**であり、正準状態は常にセッションと Assets である。

- 素材（Asset）、録音（Recording Session/Pass/Take）、セッション内容の全文検索用の眺めを提供する
- 正準コミットごとに `library::index::refresh()` が索引を同期更新する。呼び出し側のコミット経路から直接呼ばれ、失敗時は警告に留める
- UI の検索・一覧の読み先はこのモデルに統一する

---

## 9. バックグラウンドジョブ

時間のかかる処理（VST3スキャン）は JobRegistry（`jobs.rs`）のジョブとして実行される。

- **種類**: `Scan`。`kind` が結果ペイロードの型を固定する（`BackgroundJobStatus` は tagged union）
- **状態遷移**: `Queued → Running → Cancelling → Cancelled | Completed | Failed`。終端状態は確定し、`Running` への復帰はない
- ジョブは `progress` / `message` 付きで UI へ状態を配信する。登録とクエリは ID で行う
- レンダリングは別経路: `OfflineRenderRequest`（riffra-core のポート）を `riffra-runtime::render` が受け取り、`riffra-render` executableを子プロセスとして起動・制御する

---

## 10. ドメイン不変条件

riffra-core が `validate_and_normalize` と各モジュールで強制する不変条件。

| 対象           | 不変条件                                                                                                                                              |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| AssetId        | `asset:<UUIDv7>` の形式のみ有効                                                                                                                       |
| 素材コンテンツ | 生成済み素材のコンテンツは**不変**。内容変更は新しい Asset を mint する。変更できるのは管理メタデータ（name / tag / note）のみ                        |
| 参照整合       | セッションが参照する AssetId は登録済みとする（§6.4）                                                                                                 |
| セッション     | ロード・保存前に `validate_and_normalize` を必ず通過。master gain などの安全限界は正準化でクランプされる                                              |
| 更新順序       | Coreが制作状態の更新と投影を同じ確定順序で管理し、Host Adapterがその順序を保ってUIとランタイムへ渡す                                                  |
| ランタイム     | 現役の投影グラフはセッションに保存されない。投影はいつでも破棄・再構築でき、識別子に世代と音声環境を含む                                              |
| 安全           | サイドカーは所有者別ミュート、起動時のフェードイン、非有限値拒否、DCブロック、音響フィードバック検知を安全チェーンとして持つ（`native/audio-engine`） |

---

## 11. Presentationの責務

- フロントエンドは CreativeSession を描画し、ユーザー操作を Feature 別 NativeApi capability の命令へ変換する。応答は Core の確定順序で適用する。競合解決・部分マージ・全体 mutation queue は Core / Host 側に集約する
- 選択、パネル幅、ズーム、ダイアログは Presentation State であり、CreativeSession とは別に管理する。Undo/Redo の可否は Core が返す履歴状態を表示し、ランタイム投影の構築や再試行は Host の Runtime へ委ねる
