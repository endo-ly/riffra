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

Desktop版RiffraはTauriシェルプロセスと複数の子プロセスで構成される。GUIを使わない場合は、`riffra serve` が共有 `riffra-runtime::DawHost` をフォアグラウンドで起動する。どちらの構成でもリアルタイム音声はサイドカーが担当し、Hostプロセスは音声コールバックやプラグインコードを実行しない。

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
  ├─ 受け取った命令を共有Host serviceへ委譲
  └─ 実行モード: run_blocking（重い操作は spawn_blocking）で async ワーカーを塞がない

Desktop adapter (apps/desktop/src-tauri/src)
  ├─ Tauri command / event / windowの境界を担当
  ├─ HostConnectionManagerでEmbedded / Attached / Disconnectedを管理
  ├─ Embeddedではriffra-runtime::DawHostを所有し、AttachedではLocalHostClientを利用
  └─ 現在Hostのoperation、bootstrap、eventをWebViewへ接続

riffra-runtime（crates/riffra-runtime）: Desktop / Headless Host が共有するlive Runtime基盤
  ├─ DawHost / HostConfig / DataRootLeaseを含むHost composition
  ├─ AudioSupervisor / Instrument Runtime / RuntimeReconciler / Transport ordering
  ├─ 同梱Built-in instrument catalog（composition rootから注入）
  ├─ Offline render process adapter（`riffra-render` executable）
  ├─ HostEventSink / HostEventHub / Host bootstrap
  └─ Local Control Server（command connection / events connection）

riffra-control（crates/riffra-control）: current-user Local Host接続基盤
  ├─ Host identity / endpoint descriptor / Local Host Registry
  ├─ LocalHostClient / command request-response / event stream
  └─ Named Pipe / Unix Domain Socketのframingと権限境界

riffra-host（crates/riffra-host）: Desktop / CLI 共通のOS境界
  ├─ ProjectStore / Project-scoped SessionStore / Asset Repository / Project package
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
  ├─ riffra-host のDataRootLease / ProjectStore / SessionStoreを取得する
  ├─ AppCore と SessionStorage Port を直接利用する
  ├─ ワンショット引数と対話型 JSON Lines を同じ Dispatcher へ渡す
  └─ 永続編集だけを行うStandaloneモードと、DawHostを起動するserveモードを持つ

Attached CLI（apps/cli --attach）
  ├─ 起動中のRiffra Hostが公開する制御エンドポイントを検出して接続する
  ├─ 接続先のAppCore / SessionStore / DataRootLeaseを開かない
  └─ Host Control Serverを通じて正準操作を要求する

永続化・外部境界
  ├─ riffra-host: ProjectStore / SessionStore / Asset Repository / Project package / file parsers
  ├─ ProjectStore: workspace.json + projects/<project-id>（§6）
  ├─ SessionStore: projects/<project-id>/session.json + generations（§6）
  ├─ ライブラリ索引: SQLite リードモデル（§8）
  └─ ランタイム境界: AudioSupervisor → riffra-audio サイドカー（§5）
```

制作状態を変更する命令はCoreのApplication層を通り、確定した`CanonicalState`が同じ順序でフロントエンドへ返る。選択やパネル状態などの表示状態はCreativeSessionとは分離してフロントエンドが保持する。

### Hostの外部制御

起動中のHostは接続情報を公開し、外部クライアントから操作できる。DesktopはHostConnectionManagerで、自分のEmbedded Hostか別プロセスのHostへ接続し、Host Selectorから切り替える。接続・イベント・切替の詳細は `ipc.md` の境界Fに譲る。

Standalone CLIは自分のDataRootLease、SessionStore、`AppCore<()>`で動く独立した永続編集モードである。`riffra serve`は自分のDataRootLease、`AppCore<AudioSupervisor>`、Undo/Redo履歴、正準シーケンス、Audio Runtimeを保持する。Attached CLIはこれらを開かず、接続先Hostの状態を利用する。

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
5. 共有Runtime serviceがライブラリ索引の更新と音声ランタイムへの投影を要求する

検証または永続化に失敗した更新候補は正準状態にも履歴にも反映されない。

録音の後処理など、開始から確定まで時間が空く操作は、自身が所有する変更だけを最新の正準状態へ適用する。処理中に確定した別の編集は維持される。

---

## 5. ランタイム投影とトランスポート

リアルタイム音声グラフは正準状態の**投影（projection）**である。正準セッションが変わると、投影だけが再構築される。

### 5.1 投影プロトコル

Coreの `RuntimeProjection` Portは、正準スナップショットとその確定順序をホストの音声ランタイムへ渡す契約を定める。`riffra-runtime` の `RuntimeReconciler` がサイドカーとの接続、最新投影の採用、Transport orderingを担う。

| 操作                                  | 意味                                                                              |
| ------------------------------------- | --------------------------------------------------------------------------------- |
| `prepare_timeline_snapshot(snapshot)` | 投影候補をサイドカーへ渡して事前構築（VST読み込み・グラフ構築）。まだ再生されない |
| `commit_timeline_snapshot()`          | 準備済みの投影を現役グラフへ昇格                                                  |
| `discard_timeline_snapshot()`         | 準備済みの候補を破棄                                                              |

### 5.2 投影の整合性

各投影要求にはCoreが採番した確定順序と、診断に使うセッションrevisionが付く。Hostは投影要求を直列に処理し、より新しい要求が到着した場合は古い準備結果を有効化しない。準備中は現在のグラフを維持し、準備と有効化が完了した投影だけを再生に使う。失敗時の再試行やサイドカー再起動も最新の正準スナップショットを起点に行う。

### 5.3 トランスポート

Core ApplicationがPlay / Stop要求の順序と、再生に必要な投影が有効かどうかを判断する。HostのTransport executorは決定済みの要求を音声ランタイムへ伝える。古い要求や古い投影完了は現在の再生状態を上書きせず、投影が準備できていない場合は再生を待機または停止する。

---

## 6. 永続化と回復

ProjectはRiffraがDataRoot内で管理する制作単位であり、正準状態を
`projects/<project-id>/session.json` に保持する。通常のProject切替はProject一覧から行い、
ファイルダイアログを使わない。`.riffra` はProjectのportable packageで、ユーザーが扱うのは
Import / Exportのときだけである。DataRoot内のcanonical Projectや作業中のSessionそのものではない。
音声Renderの結果はProject packageとは別に `renders/` へ保存する。

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

保存前にProject領域の空き容量を検証し、容量不足では保存を拒否する。保存はプロセス内のグローバルロックで直列化される。

### 6.3 ロードと回復

`ProjectStore` はDataRootの初期化時に最初のProjectを作成するか、`workspace.json` のActive Projectを選ぶ。各Projectの `SessionStore` は以下の順で解決する。

1. `session.json` を読み、`deserialize_session` → `validate_and_normalize` → **アセット参照検証** を通れば採用
2. 破損・参照不正なら同じProjectの `generations/` を新しい順に読み、**スキーマ検証に通る最新世代** を `recovered_from_generation: true` として採用
3. 新規DataRootにProjectが無い場合だけ、空のCreativeSessionを作成して保存

破損した `session.json` は**決して上書きしない**（唯一の回復手段を壊さない）。起動時に世代回復が発生した場合は `recovery_candidates()` が世代ファイルからメタデータだけを軽量に読み、ユーザー選択の `restore_generation()` が指定世代を正準状態として復元・保存する。Active Project以外の読込不能Projectも一覧から除外せず、読込エラーを持つProjectとして表示できる。

### 6.4 参照整合

保存・ロードの両方で `asset::validate_session_references` が実行され、セッションが参照する全アセットIDが登録済みであることを保証する。コンテンツファイルの欠落は許容（MissingDependency としてUIに列挙）だが、**未登録のアセットIDを含むセッションは保存・ロードを拒否**する。

### 6.5 DataRootの所有

WindowsのDesktop Embedded Hostは、既定でユーザーの `Music/Riffra` 配下をDataRootとして使用する。Standalone CLIと `riffra serve` は指定されたDataRootを使用し、Attached modeのDesktopとAttached CLIは接続先HostのDataRootを開かない。

これらのローカルHostは起動時に `riffra-host::DataRootLease` を取得し、ホストの生存期間中保持する。DesktopのAttached modeとAttached CLIはDataRootを開かず、接続先Hostへ制御要求だけを送る。

排他にはロックファイルではなくOSのファイルロックを使うため、異常終了後に残ったファイルは新しいホストの起動を妨げない。同じDataRootを別プロセスが開いている場合は、明示的な使用中エラーを返す。

---

## 7. セーフモード

`--safe-mode` フラグまたは `RIFFRA_SAFE_MODE` 環境変数（`1` / `true` / `yes` / `on`）で起動すると、サイドカーの起動・デバイスアクセス・プラグイン読み込みをすべて省略する。`riffra serve --safe-mode`でも同じ扱いとなり、`AudioSupervisor`はオフライン実装として生成される。

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
- レンダリングは別経路: `OfflineRenderRequest`（riffra-core のポート）を `riffra-runtime::render` が受け取り、`riffra-render` executableを子プロセスとして起動・制御する

---

## 10. ドメイン不変条件

riffra-core が `validate_and_normalize` と各モジュールで強制する不変条件。

| 対象           | 不変条件                                                                                                                                          |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| AssetId        | `asset:<UUIDv7>` の形式のみ有効                                                                                                                   |
| 素材コンテンツ | 生成済み素材のコンテンツは**不変**。内容変更は新しい Asset を mint する。変更できるのは管理メタデータ（name / tag / note）のみ                    |
| 参照整合       | セッションが参照する AssetId は必ず登録済みでなければならない（§6.4）                                                                             |
| セッション     | ロード・保存前に `validate_and_normalize` を必ず通過。master gain などの安全限界は正準化でクランプされる                                          |
| 更新順序       | Coreが制作状態の更新と投影を同じ確定順序で管理し、Host Adapterがその順序を保ってUIとランタイムへ渡す                                              |
| ランタイム     | 現役の投影グラフはセッションに保存されない。投影はいつでも破棄・再構築できる一時状態                                                              |
| 安全           | サイドカーは緊急ミュート、起動時のフェードイン、非有限値拒否、DCブロック、音響フィードバック検知を安全チェーンとして持つ（`native/audio-engine`） |

---

## 11. Presentationの責務

フロントエンドはCreativeSessionを描画し、ユーザー操作をFeature別NativeApi capabilityの命令へ変換する。制作状態を変更する命令の応答はCoreの確定順序で適用されるため、フロントエンドはセッション同士の競合解決、部分マージ、全体mutation queueを持たない。

選択、パネル幅、ズーム、ダイアログはPresentation Stateであり、CreativeSessionとは別に管理する。Undo/Redoの可否はCoreが返す履歴状態を表示し、ランタイム投影の構築や再試行はHostのRuntimeへ委ねる。
