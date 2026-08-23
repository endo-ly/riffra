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

Riffraは1つのTauriシェルプロセスと複数の子プロセスで構成される。リアルタイム音声は常にサイドカーが担当し、Tauriプロセスは音声コールバックやプラグインコードを実行しない。

```text
┌─────────────────────────────────────────────────────────────────┐
│ Tauri シェル                                                     │
│ ┌──────────────────────┐   ┌──────────────────────────────────┐ │
│ │ WebView (React)      │   │ Rust バックエンド                │ │
│ │ 表示・操作・表示状態  │◀──▶│ Core / Desktop Adapter /       │ │
│ │ NativeApi 経由で指令 │   │ ランタイム調整 / 永続化 / Jobs   │ │
│ └──────────────────────┘   └──────────────────────────────────┘ │
└──────┬──────────────────────────┬───────────────────────────────┘
       │ JSON Lines (stdin/stdout)
       │ 1つのコマンド + 1行の応答
┌──────▼────────────────┐  ┌──────▼──────────────┐  ┌──────▼────────┐
│ riffra-audio          │  │ riffra-plugin-scan  │  │ riffra-render │
│ リアルタイム音声       │  │ VST3スキャン        │  │ -worker       │
│ JUCE / ASIO / WASAPI  │  │ (起動時・再スキャン) │  │ オフライン    │
│ 投影・演奏・録音・監視 │  └─────────────────────┘  │ レンダリング  │
│ VST3 ホスティング     │                            └──────────────┘
└───────────────────────┘
```

| プロセス             | 役割                                                       | 所有状態                                    |
| -------------------- | ---------------------------------------------------------- | ------------------------------------------- |
| Tauri シェル         | アプリ全体の監督。UI、Core接続、永続化、ジョブ管理         | CreativeSession（Core内）、SQLite索引、設定 |
| riffra-audio         | リアルタイム音声。デバイス、VST3グラフ、演奏・録音・監視   | ランタイムグラフ（投影される一時状態のみ）  |
| riffra-plugin-scan   | VST3の列挙・検証（`--probe` 系と分離された専用起動モード） | なし                                        |
| riffra-render-worker | タイムラインのオフラインレンダリング                       | なし                                        |

Tauriシェルはセーフモード（§7）で起動するとサイドカーの起動を省略し、外部デバイス・プラグインを一切触らない。

---

## 3. レイヤー構成

依存は上位から下位への一方向。下位層は上位層を知らない。

```text
React フロントエンド
  ├─ 状態: CreativeSession を保持・描画する
  ├─ 編集: Feature別の NativeApi capability 経由で Tauri 命令を呼ぶ
  ├─ app: bootstrap / アプリ全体のComposition / グローバルなRuntime lifecycle
  ├─ features: 機能ごとの状態・操作・UI・テスト（arrange、audio、library、plugins、project、recording、transport）
  ├─ shared: Feature所有を持たない共通UI・utility（Toast、ContextMenu、audio meters など）
  ├─ native: ReactとTauriの境界（NativeApi capability 定義・invoke実装・FakeNativeApi）
  └─ model: src/model/generated（Rust の ts-rs 出力を gen-barrel.js で束ねた型）

Tauri 命令層 (src-tauri/src/**/commands.rs)
  ├─ 受け取った命令を Desktop Adapter へ委譲
  └─ 実行モード: run_blocking（重い操作は spawn_blocking）で async ワーカーを塞がない

Desktop Adapter (apps/desktop/src-tauri/src)
  ├─ riffra-host / AudioSupervisor / RuntimeReconciler を Core の Port に接続
  └─ ファイル・音声・ジョブを伴うホスト固有の調整を担当

riffra-host（crates/riffra-host）: Desktop / CLI 共通のOS境界
  ├─ SessionStore / Asset Repository / Project package
  ├─ WAV metadata / MIDI SMF parser
  └─ DataRootLease（プロセス間の排他所有）

riffra-core（crates/riffra-core）: プラットフォーム非依存のApplication / Domain / Ports
  ├─ domain: CreativeSession / Arrangement / Recording / Asset / Rack
  ├─ application: Session / Arrangement / Recording / Rack / Transport / History
  ├─ ports: SessionStorage / RuntimeProjection / RenderRuntime
  ├─ AppCore: 正準状態、コミット順序、履歴、投影sequence
  ├─ validate_and_normalize（コミット前に正準化）
  └─ Tauri・WebView・OS統合を含まない

CLI ホスト（apps/cli）
  ├─ riffra-host のDataRootLease / SessionStoreを取得する
  ├─ AppCore と SessionStorage Port を直接利用する
  ├─ ワンショット引数と対話型 JSON Lines を同じ Dispatcher へ渡す
  └─ Tauri・React・Desktop Adapter・Audio Runtimeに依存しない

Attached CLI（apps/cli --attach）
  ├─ Desktopが公開する制御エンドポイントを検出して接続する
  ├─ DesktopのAppCore / SessionStore / DataRootLeaseを開かない
  └─ Desktop Control Routerを通じてDesktop Adapterへ要求を渡す

永続化・外部境界
  ├─ riffra-host: SessionStore / Asset Repository / Project package / file parsers
  ├─ SessionStore: scratch/current.json + generations（§6）
  ├─ ライブラリ索引: SQLite リードモデル（§8）
  └─ ランタイム境界: AudioSupervisor → riffra-audio サイドカー（§5）
```

制作状態を変更する命令はCoreのApplication層を通り、確定した`CanonicalState`が同じ順序でフロントエンドへ返る。選択やパネル状態などの表示状態はCreativeSessionとは分離してフロントエンドが保持する。

### Desktopの外部制御

Windows Desktopは、起動中のアプリを外部Hostから操作する制御経路を持つ。Attached CLIはこの経路を介してDesktop Adapterへ要求を送り、DesktopのCoreとRuntimeを共有する。

Standalone CLIは自分のDataRootLease、SessionStore、`AppCore<()>`で動く独立Hostである。Attached CLIはこれらを開かず、Desktopが保持するCore、Undo/Redo履歴、正準シーケンス、Audio RuntimeをGUIと共有する。

---

## 4. 制作状態とコミット

### 4.1 単一の正準状態

CreativeSession（`riffra-core/src/domain/session`）が、アレンジ、クリップ、テイク、トラック、ラック、設定など、永続化される制作状態の正準モデルである。`AppCore`はCreativeSessionと正準シーケンス、Undo/Redo履歴を一体の状態として管理し、フロントエンドと音声サイドカーはその投影を扱う。

### 4.2 Core操作境界

`AppCore` は主要な制作操作の入口であり、正準状態、操作順序、Undo/Redo履歴、ランタイム投影の順序を一体として管理する。ホストはPortを実装して永続化や音声ランタイムへ接続するが、正準状態の採否は決めない。DesktopとCLIは同じ編集規則と永続化規則を共有する。

### 4.3 コミットパイプライン

`AppCore` と `riffra-core::application` が正準操作の保存境界を定める。

1. Application操作が現在の正準状態から更新候補を作る
2. Domainが不変条件を検証し、正準化する
3. SessionStorage Portを通じて更新候補を永続化する
4. 保存成功後に正準状態を交換し、履歴と投影順序を更新する
5. Desktop Adapterがライブラリ索引の更新と音声ランタイムへの投影を要求する

検証または永続化に失敗した更新候補は正準状態にも履歴にも反映されない。

録音の後処理など、開始から確定まで時間が空く操作は、自身が所有する変更だけを最新の正準状態へ適用する。処理中に確定した別の編集は維持される。

---

## 5. ランタイム投影とトランスポート

リアルタイム音声グラフは正準状態の**投影（projection）**である。正準セッションが変わると、投影だけが再構築される。

### 5.1 投影プロトコル

Coreの `RuntimeProjection` Portは、正準スナップショットとその確定順序をホストの音声ランタイムへ渡す契約を定める。Desktopでは `RuntimeReconciler` がサイドカーとの接続を担う。

| 操作                                  | 意味                                                                              |
| ------------------------------------- | --------------------------------------------------------------------------------- |
| `prepare_timeline_snapshot(snapshot)` | 投影候補をサイドカーへ渡して事前構築（VST読み込み・グラフ構築）。まだ再生されない |
| `commit_timeline_snapshot()`          | 準備済みの投影を現役グラフへ昇格                                                  |
| `discard_timeline_snapshot()`         | 準備済みの候補を破棄                                                              |

### 5.2 投影の整合性

各投影要求にはCoreが採番した確定順序と、診断に使うセッションrevisionが付く。Desktopは投影要求を直列に処理し、より新しい要求が到着した場合は古い準備結果を有効化しない。準備中は現在のグラフを維持し、準備と有効化が完了した投影だけを再生に使う。失敗時の再試行やサイドカー再起動も最新の正準スナップショットを起点に行う。

### 5.3 トランスポート

Core ApplicationがPlay / Stop要求の順序と、再生に必要な投影が有効かどうかを判断する。Desktop Adapterは決定済みの要求を音声ランタイムへ伝える。古い要求や古い投影完了は現在の再生状態を上書きせず、投影が準備できていない場合は再生を待機または停止する。

---

## 6. 永続化と回復

### 6.1 ディスクレイアウト

```text
<data_root>/
├─ scratch/
│  ├─ current.json          # 現行セッション（正準の保存先）
│  └─ generations/          # 世代スナップショット {ms}-{pid}.json（最大20件）
├─ library/riffra.db        # ライブラリ索引（SQLite リードモデル）
├─ recordings/
│  ├─ inbox/                # 録音キャプチャ（録音直後のテイク置き場）
│  ├─ archive/              # アーカイブ済みテイク
│  └─ library/              # ライブラリへ昇格済みテイク
├─ assets/
│  └─ imports/              # 外部ファイルのインポート先（register で登録）
└─ exports/
   └─ render-{ms}/          # レンダリング出力（timeline.wav + render.json）
```

### 6.2 アトミック保存と世代管理

`riffra-host::SessionStore` は、クラッシュしても current.json が「完全な旧内容か完全な新内容」のどちらかになるよう保存する。

1. 現在の current.json を generations/ へコピー
2. 新内容を `.tmp` へ書き、`sync_all`（fsync）
3. `MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)`（Windows）または `rename` で置換
4. 古い世代を20件を超えて削除

保存前にスクラッチ領域の空き容量を検証し、容量不足では保存を拒否する。保存はプロセス内のグローバルロックで直列化される。

### 6.3 ロードと回復

`load_or_create()` は以下の順で解決する。

1. current.json を読み、`deserialize_session` → `validate_and_normalize` → **アセット参照検証** を通れば採用
2. 破損・参照不正なら generations/ を新しい順に読み、**スキーマ検証に通る最新世代** を `recovered_from_generation: true` として採用
3. 世代も無い場合は新規セッションを作成して保存

破損した current.json は**決して上書きしない**（唯一の回復手段を壊さない）。起動時に世代回復が発生した場合は `recovery_candidates()` が世代ファイルからメタデータだけを軽量に読み、ユーザー選択の `restore_generation()` が指定世代を正準状態として復元・保存する。

### 6.4 参照整合

保存・ロードの両方で `asset::validate_session_references` が実行され、セッションが参照する全アセットIDが登録済みであることを保証する。コンテンツファイルの欠落は許容（MissingDependency としてUIに列挙）だが、**未登録のアセットIDを含むセッションは保存・ロードを拒否**する。

### 6.5 DataRootの所有

DesktopとStandalone CLIは起動時に `riffra-host::DataRootLease` を取得し、ホストの生存期間中保持する。Attached CLIはDataRootを開かず、Desktopへ制御要求だけを送る。

排他にはロックファイルではなくOSのファイルロックを使うため、異常終了後に残ったファイルは新しいホストの起動を妨げない。同じDataRootを別プロセスが開いている場合は、明示的な使用中エラーを返す。

---

## 7. セーフモード

`--safe-mode` フラグまたは `RIFFRA_SAFE_MODE` 環境変数（`1` / `true` / `yes` / `on`）で起動すると、サイドカーの起動・デバイスアクセス・プラグイン読み込みをすべて省略する。このとき `AudioSupervisor` はオフライン実装として生成され、外部デバイス・MIDI・プラグインから隔離される。

- 初期化は「セッション読込 + ライブラリ索引」のみで完了し、`BootstrapState.safeMode: true` がUIに通知される
- 音声・録音・再生・プレビュー・VST3読込系の命令はセーフモードではエラーとして無効化される。オフライン解析・書き出し・ライブラリ操作は通常モードと同一に利用できる
- 外部デバイスやプラグインが原因のハングを切り分けるための診断手段であり、データの読み書きは通常モードと同一

フラグ判定は `--safe-mode` の明示のみを認識し、他の起動引数（`--serve` など）からセーフモードを推測しない。

---

## 8. ライブラリ索引（リードモデル）

ライブラリは SQLite の**読み取り専用モデル**であり、正準状態は常にセッションと Assets である。

- 素材（Asset）、録音（Recording Session/Pass/Take）、セッション内容の全文検索用の眺めを提供する
- セッション保存のたびに `library::index::queue()` が索引更新を非ブロッキングで投入する。index.rs は**最新1件だけを残す結合キュー**（latest-wins）で駆動され、連続保存時もワーカーの実行を追い越さない
- UI はライブラリの検索・一覧をこのモデルからのみ読む

---

## 9. バックグラウンドジョブ

時間のかかる処理（VST3スキャン）は JobRegistry（`jobs.rs`）のジョブとして実行される。

- **種類**: `Scan`。`kind` が結果ペイロードの型を固定する（`BackgroundJobStatus` は tagged union）
- **状態遷移**: `Queued → Running → Cancelling → Cancelled | Completed | Failed`。終端状態から `Running` には戻らない
- ジョブは `progress` / `message` 付きでUIへ状態が配信され、ID重複を避けた登録とクエリで操作される
- レンダリングは別経路: `OfflineRenderRequest`（riffra-core のポート）を `riffra-render-worker` 子プロセスが処理する

---

## 10. ドメイン不変条件

riffra-core が `validate_and_normalize` と各モジュールで強制する不変条件。

| 対象           | 不変条件                                                                                                                                    |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| AssetId        | `asset:<UUIDv7>` の形式のみ有効。旧形式や任意文字列は拒否                                                                                   |
| 素材コンテンツ | 生成済み素材のコンテンツは**不変**。内容変更は新しい Asset を mint する。変更できるのは管理メタデータ（name / tag / note）のみ              |
| 参照整合       | セッションが参照する AssetId は必ず登録済みでなければならない（§6.4）                                                                       |
| セッション     | ロード・保存前に `validate_and_normalize` を必ず通過。master gain などの安全限界は正準化でクランプされる                                    |
| 更新順序       | Coreが制作状態の更新と投影を同じ確定順序で管理し、Desktop Adapterがその順序を保ってUIとランタイムへ渡す                                     |
| ランタイム     | 現役の投影グラフはセッションに保存されない。投影はいつでも破棄・再構築できる一時状態                                                        |
| 安全           | サイドカーは緊急ミュート、起動時低ゲイン、非有限値拒否、DCブロック、音響フィードバック検知を安全チェーンとして持つ（`native/audio-engine`） |

---

## 11. Presentationの責務

フロントエンドはCreativeSessionを描画し、ユーザー操作をFeature別NativeApi capabilityの命令へ変換する。制作状態を変更する命令の応答はCoreの確定順序で適用されるため、フロントエンドはセッション同士の競合解決、部分マージ、全体mutation queueを持たない。

選択、パネル幅、ズーム、ダイアログはPresentation Stateであり、CreativeSessionとは別に管理する。Undo/Redoの可否はCoreが返す履歴状態を表示し、ランタイム投影の構築や再試行はDesktop Adapterへ委ねる。
