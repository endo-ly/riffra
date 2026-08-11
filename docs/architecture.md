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
│ │ 表示・操作・楽曲編集  │◀──▶│ コマンド受付 / Session Actor /  │ │
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

| プロセス             | 役割                                                         | 所有状態                                             |
| -------------------- | ------------------------------------------------------------ | ---------------------------------------------------- |
| Tauri シェル         | アプリ全体の監督。UI、セッション正準状態、永続化、ジョブ管理 | CreativeSession（正準）、SQLite ライブラリ索引、設定 |
| riffra-audio         | リアルタイム音声。デバイス、VST3グラフ、演奏・録音・監視     | ランタイムグラフ（投影される一時状態のみ）           |
| riffra-plugin-scan   | VST3の列挙・検証（`--probe` 系と分離された専用起動モード）   | なし                                                 |
| riffra-render-worker | タイムラインのオフラインレンダリング                         | なし                                                 |

Tauriシェルはセーフモード（§7）で起動するとサイドカーの起動を省略し、外部デバイス・プラグインを一切触らない。

---

## 3. レイヤー構成

依存は上位から下位への一方向。下位層は上位層を知らない。

```text
React フロントエンド
  ├─ 状態: 保持しない。CreativeSession を表示・編集するだけ
  ├─ 編集: NativeApi 経由で Tauri 命令を呼ぶ（フロントエンドは楽曲を直接変えない）
  └─ 型: src/lib/generated（Rust の ts-rs 出力を gen-barrel.js で束ねたもの）

Tauri 命令層 (src-tauri/src/**/commands.rs)
  ├─ 受け取った命令を Application 層へ委譲
  └─ 実行モード: run_blocking（重い操作は spawn_blocking）で async ワーカーを塞がない

Application 層 (src-tauri/src/session, asset, recording, analysis, separation, render, plugins)
  ├─ 衝突検知付きで 正準 CreativeSession へ操作を適用
  └─ SessionContext（audio / runtime / session_actor / data_root / session / safe_mode）を介して依存を注入

riffra-core（crates/riffra-core）: プラットフォーム非依存のドメイン
  ├─ CreativeSession / Asset / Rack / AppCore / AudioRuntime(port)
  ├─ validate_and_normalize（ロード・保存前に正準化）
  └─ Tauri・WebView・OS統合を含まない

永続化・外部境界
  ├─ SessionStore: scratch/current.json + generations（§6）
  ├─ ライブラリ索引: SQLite リードモデル（§8）
  └─ ランタイム境界: AudioSupervisor → riffra-audio サイドカー（§5）
```

フロントエンドは「編集の瞬間に戻った CreativeSession を受け取って差し替える」方式のみで状態を変更する。直接的な楽曲構造の書き換えや、バックエンドを経ない永続化は行わない。

---

## 4. セッション正準化

### 4.1 単一の正準状態

CreativeSession（`src-tauri/src/session` + `riffra-core`）が、楽曲の全ての編集可能な状態（アレンジ、クリップ、テイク、トラック、ラック、設定、スナップショット）の単一の正準状態である。WebView・サイドカーはすべて投影であり、正準状態は Rust プロセス内の `Mutex<CreativeSession>` にのみ存在する。

### 4.2 Session Actor

`session/actor.rs` は正準状態への操作順序と投影の整合を担う。

- **operation_gate（Mutex）**: 正準セッション操作（コマンド）を直列化する。VST準備などの遅い処理はガードの外で行われるため、遅いプラグインがセッション操作を長時間ブロックしない
- **projection_version（AtomicU64 による seqlock）**: 偶数値が「安定した投影バージョン（sequence × 2）」、奇数値が「コミット境界での一時状態」。`capture_projection` はセッションと sequence のペアを一貫したまま非ブロッキングで取得し、コミット境界の短い交換を挟んで再試行する

### 4.3 コミットパイプライン

`session/commit.rs` が正準操作の保存境界を定める。

1. `validate_and_normalize()` — ドメイン不変条件の検証と正準化
2. `updatedAtMs` を単調増加に補正（`next_session_update_timestamp`）
3. `SessionStore::save()` — 世代保存 + アトミック置換（§6）
4. `publish_session()` — Actor の `begin_commit` / セッション交換 / `mark_committed` の間で、in-memory と投影の順序を一貫させる。保存中にユーザーがワークスペースを切り替えていた場合は最新の workspace が勝つ
5. `queue_session_index()` — ライブラリ索引への同期を非ブロッキングで投入（§8）

長時間ジョブ（録音の後処理など）は `commit_merged_session()` を使い、ベース時点から最新セッションへの**操作所有部分のみのマージ**をコミット境界で行う。これにより、ジョブ実行中にユーザーが行った無関係な編集が上書きされない。

---

## 5. ランタイム投影とトランスポート

リアルタイム音声グラフは正準状態の**投影（projection）**である。正準セッションが変わると、投影だけが再構築される。

### 5.1 投影プロトコル

`runtime/ports.rs` が 3 つのプリミティブを定める。実装は `native_audio/` の AudioSupervisor がサイドカーへのコマンドとして担う。

| 操作                                  | 意味                                                                              |
| ------------------------------------- | --------------------------------------------------------------------------------- |
| `prepare_timeline_snapshot(snapshot)` | 投影候補をサイドカーへ渡して事前構築（VST読み込み・グラフ構築）。まだ再生されない |
| `commit_timeline_snapshot()`          | 準備済みの投影を現役グラフへ昇格                                                  |
| `discard_timeline_snapshot()`         | 準備済みの候補を破棄                                                              |

### 5.2 Projection Coordinator

`runtime/projection_coordinator.rs` は専用ワーカースレッド（`riffra-runtime-projection`）を持ち、投影要求を**最新優先（latest-wins）**で直列化する。

- 要求は `ProjectionKey`（更新時刻・世代）を持ち、古いキーの要求は `Superseded` として棄却される
- 準備失敗・タイムアウト時は再試行し、サイドカー再起動が必要な場合は recovery フックで次の世代を走らせる
- 準備中は既存の現役グラフが演奏を続けるため、編集中の音が途切れない
- コミット成功時に `on_activated` フックがトランスポート再開（再生中の継続再生）を試みる

### 5.3 トランスポート

`runtime/transport_controller.rs` / `transport_executor.rs` が Play / Stop の決定と実行を担う。トランスポート操作は投影操作とは別のポート（`TransportDriver`）に分離されており、トランスポートが投影の状態を誤って操作できない構造になっている。投影失敗時は結果に応じて再生を抑制・停止し、UI には「同期待ち」として遷移状態が通知される。

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
├─ separations/             # 音源分離の出力先
└─ exports/
   └─ render-{ms}/          # レンダリング出力（timeline.wav + render.json）
```

### 6.2 アトミック保存と世代管理

`storage.rs` の SessionStore は、クラッシュしても current.json が「完全な旧内容か完全な新内容」のどちらかになるよう保存する。

1. 現在の current.json を generations/ へコピー
2. 新内容を `.tmp` へ書き、`sync_all`（fsync）
3. `MoveFileExW(REPLACE_EXISTING|WRITE_THROUGH)`（Windows）または `rename` で置換
4. 古い世代を20件を超えて削除

保存前にスクラッチ領域の空き容量を検証し、容量不足では保存を拒否する（`EnsureStorageCapacity`）。保存はプロセス内のグローバルロックで直列化される。

### 6.3 ロードと回復

`load_or_create()` は以下の順で解決する。

1. current.json を読み、`deserialize_session` → `validate_and_normalize` → **アセット参照検証** を通れば採用
2. 破損・参照不正なら generations/ を新しい順に読み、**スキーマ検証に通る最新世代** を `recovered_from_generation: true` として採用
3. 世代も無い場合は新規セッションを作成して保存

破損した current.json は**決して上書きしない**（唯一の回復手段を壊さない）。起動時に世代回復が発生した場合は `recovery_candidates()` が世代ファイルからメタデータだけを軽量に読み、ユーザー選択の `restore_generation()` が指定世代を正準状態として復元・保存する。

### 6.4 参照整合

保存・ロードの両方で `asset::validate_session_references` が実行され、セッションが参照する全アセットIDが登録済みであることを保証する。コンテンツファイルの欠落は許容（MissingDependency としてUIに列挙）だが、**未登録のアセットIDを含むセッションは保存・ロードを拒否**する。

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
- セッション保存のたびに `queue_session_index()` が索引更新を非ブロッキングで投入する。index.rs は**最新1件だけを残す結合キュー**（latest-wins）で駆動され、連続保存時もワーカーの実行を追い越さない
- UI はライブラリの検索・一覧をこのモデルからのみ読む

---

## 9. バックグラウンドジョブ

時間のかかる処理（音声解析・分離・VST3スキャン）は JobRegistry（`jobs.rs`）のジョブとして実行される。

- **種類**: `Analysis` / `Separation` / `Scan`。`kind` が結果ペイロードの型を固定する（`BackgroundJobStatus` は tagged union）
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
| 更新順序       | updatedAtMs は単調増加に補正され、UI 境界での順序トークンとして機能する                                                                     |
| ランタイム     | 現役の投影グラフはセッションに保存されない。投影はいつでも破棄・再構築できる一時状態                                                        |
| 安全           | サイドカーは緊急ミュート、起動時低ゲイン、非有限値拒否、DCブロック、音響フィードバック検知を安全チェーンとして持つ（`native/audio-engine`） |

---

## 11. フロントエンドの構造

```text
apps/desktop/src/
├─ App.tsx               # ルート。bootstrap 後にワークスペース切替・パネル幅を管理
├─ native/
│  ├─ native-api.ts      # NativeApi: Tauri 命令の Promise 型インターフェース
│  ├─ native-api-fake.ts # テスト用フェイク実装
│  └─ invoke.ts          # invoke の共通ラッパ（イベント購読含む）
├─ lib/
│  ├─ generated/         # ts-rs 生成型（gen:types で再生成）
│  ├─ domain.ts          # 生成型のバレル再エクスポート
│  └─ audio-*.ts, arrange-*.ts  # 純粋ヘルパ（安全ロジック・ドラッグ・タイムライン変換）
├─ hooks/
│  ├─ useApp.ts          # bootstrap / セッション購読 / 編集の実行と戻り値の適用
│  ├─ useSession.ts      # セッションのローカル保持と setSession の差し替え
│  ├─ useAudio.ts        # AudioStatus の購読と状態遷移の再試行
│  ├─ arrange/           # 楽曲編集（useArrangeEditor / useClipInteractions ほか）
│  └─ runtime/           # useRuntimeSynchronization / useTransportController / useWorkspaceNavigation
└─ components/           # 画面コンポーネント
```

UI はセッションをローカル状態（`setSession`）で保持し、操作のたびに Rust の正準操作を呼び、返却された CreativeSession で置き換える。ランタイム投影との整合（revision / runtimeStatus の不一致検知、再試行）は `useApp` / `useRuntimeSynchronization` などの購読フックが担う。

## 12. 検証

- Rust 単体・結合: `cargo test`（各 crate。Session Actor / Storage / アプリ操作の回復・不変条件テストを含む）
- フロントエンド: Vitest + Testing Library
- ネイティブ: CMake + CTest
- 一括: `npm run verify`（`--native` でネイティブビルドを含む）
