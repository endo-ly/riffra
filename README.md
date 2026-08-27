# Riffra

**AIと人間が同じセッションで曲を作る**DAW。楽器や声の録音から素材を育て、時間軸上で音楽として組み立てる。楽曲・素材・制作の文脈はすべて手元のディスクに留まり、クラウドには出ない。

実現しているのは、制作の操作と状態をGUIの奥に閉じない設計である。正準状態・命令契約・イベント同期を外部に開き、Desktop GUIとCLIが同一の正準状態を共有する。GUIで作りかけた曲をエージェントがCLI経由で組み続き、逆にCLIで組んだ楽曲をGUIで開いて仕上げる、といった分担がそのまま動く。ヘッドレスでの楽曲制作・再生・レンダリングまで可能。

- **AIと共同できるセッション** — 正準状態と命令契約を共有するGUIとCLI。エージェントはヘッドレスでセッションを編集・再生・レンダリングできる
- **文脈が失われない素材** — 録音は機器・ラック・録音条件・由来とセットで残る。「あの音、どう作ったっけ」を人間に残すと同時に、AIが素材を理解して作業するための機械可読な文脈になる
- **手元で完結するデータ** — セッション・素材・ライブラリはすべてローカルディスクに保存され、ネットワーク非依存。AIが作品に触れても中身は外に出ない

詳細な製品構想は [docs/CONCEPT.md](docs/CONCEPT.md) を参照。

## 全体像

```text
┌───────────────────────────────────────────────┐
│  Desktop Tauri シェル（1プロセス）              │
│  React フロントエンド ── Tauri IPC ── Rust      │
│  HostConnectionManager ── Embedded / Attached   │
└──────┬──────────────────────┬─────────────────┘
       │ JSON Lines (stdin/stdout)
┌──────▼──────────┐   ┌───────▼─────────┐  ┌──────▼────────┐
│ riffra-audio    │   │ riffra-plugin-  │  │ riffra-render │
│ リアルタイム音声 │   │ scan            │  │ -worker       │
│ (C++ / JUCE)    │   │ VST3 スキャン    │  │ オフライン    │
└─────────────────┘   └─────────────────┘  │ レンダリング   │
                                           └───────────────┘
```

GUIを使わない場合は `riffra serve` が `riffra-runtime::DawHost` を起動する。
DesktopとHeadless Hostは同じ正準状態・投影・ローカル制御契約を共有し、起動中のHostは
`<data_root>/control/host.json` に接続情報を公開し、current-user Local Host Registryへ
instanceごとのentryを登録する。
DesktopはHostConnectionManagerを介して自身のEmbedded Hostまたは別プロセスのAttached Hostへ接続する。

- リアルタイム音声は常に **riffra-audio サイドカー**（C++ / JUCE）が担当し、Tauri プロセスは音声コールバックやプラグインコードを実行しない
- セッション・素材・由来の正準状態は **riffra-core**（Rust）が保持し、WebView と サイドカーは契約された型と命令のみで接続される
- データはすべてローカルディスクに保存される（ネットワーク非依存）

## リポジトリ構成

| パス                             | 内容                                                                             |
| -------------------------------- | -------------------------------------------------------------------------------- |
| `apps/desktop/`                  | Tauri デスクトップアプリ（React フロントエンド + `src-tauri` Rust バックエンド） |
| `apps/cli/`                      | Standalone編集、`riffra serve`、Hostへの`--attach`を提供するCLI                  |
| `crates/riffra-core/`            | Application / Domain / Ports（Session / Asset / Rack / 履歴）                    |
| `crates/riffra-host/`            | SessionStore、Asset、Project、制作ファイル解析、DataRoot所有                     |
| `crates/riffra-runtime/`         | Desktop と Headless Host が共有するRuntime型・投影・ローカル制御の基盤           |
| `crates/riffra-render-worker/`   | オフラインレンダリングの子プロセスバイナリ                                       |
| `native/audio-engine/`           | リアルタイム音声エンジンのサイドカー（C++ / JUCE）                               |
| `scripts/`                       | 型生成（`gen-barrel.js`）などの開発スクリプト                                    |
| `docs/`                          | 設計・調整ドキュメント                                                           |
| `.agent/skills/riffra-headless/` | AI エージェントが CLI でヘッドレス操作するためのスキル                           |

依存関係は npm workspace（`@riffra/desktop`）と Cargo workspace（`riffra-core` / `riffra-host` / `riffra-runtime` / `riffra-cli` / `riffra-render-worker` / デスクトップバイナリ）で管理する。

## 技術スタック

| 層               | 技術                                                          |
| ---------------- | ------------------------------------------------------------- |
| シェル           | Tauri 2                                                       |
| フロントエンド   | React 19 + TypeScript + Vite                                  |
| バックエンド     | Rust（edition 2024）                                          |
| リアルタイム音声 | C++ / JUCE サイドカー                                         |
| 永続化           | SQLite（ライブラリ索引）、JSON（セッション）、WAV（録音素材） |

## 開発

### 前提条件

- Node.js と npm
- Rust toolchain（`Cargo.toml` の `rust-version` を確認）
- ネイティブ音声エンジンのビルド済みサイドカー（`native/audio-engine/` 参照。ビルドはDesktop用に `apps/desktop/src-tauri/binaries/`、Headless用に `target/debug/` または `target/release/` へ配置される）

### コマンド

```powershell
npm install            # npm workspace の依存関係を導入
npm run dev            # Vite のみでフロントエンド開発（ブラウザ）
npm run dev:tauri      # Tauri アプリ全体を起動（ネイティブ音声を利用する場合はこちら）
npm run test            # フロントエンドの全テスト

npm run gen:types      # Rust 定義から TS 型を再生成（ts-rs + gen-barrel.js）
npm run check          # 型チェック + ビルド + テスト

npm run lint           # ESLint + Stylelint
npm run lint:css       # Stylelint（z-index は tokens.css のレイヤートークンを使用）
npm run typecheck      # tsc

cargo run -p riffra-cli -- --data-root ./riffra-data session get
cargo run -p riffra-cli -- --data-root ./riffra-data --interactive
cargo run -p riffra-cli -- --data-root ./riffra-data serve --safe-mode
cargo run -p riffra-cli -- --data-root ./riffra-data --attach session get

# Native audio engineを使うLive Host
./native/audio-engine/build.sh Debug
cargo run -p riffra-cli -- --data-root ./riffra-data serve
```

`serve` はフォアグラウンドでHostを保持し、起動診断を標準エラーへ出力する。
`--attach` は既存Hostへ接続し、DataRootを直接開かない。
ヘッドレス操作のコマンドカタログと運用手順は `.agent/skills/riffra-headless/` を参照。

## ドキュメント

| ドキュメント                                                                       | 内容                                                                                        |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| [docs/CONCEPT.md](docs/CONCEPT.md)                                                 | 製品構想（価値、二つの制作領域、利用の形）                                                  |
| [docs/architecture.md](docs/architecture.md)                                       | システム構造と主要機構（セッション正準化、整合性、保存）                                    |
| [docs/data-model.md](docs/data-model.md)                                           | ドメインエンティティの正準カタログと不変条件                                                |
| [docs/ipc.md](docs/ipc.md)                                                         | IPC 境界の契約（Tauri 命令 / イベント / サイドカープロトコル）                              |
| [docs/ui-ux-design/application-layout.md](docs/ui-ux-design/application-layout.md) | 共通画面構造（Global Control Bar / Left Column / Main Canvas / Detail Area / Play Surface） |
| [docs/ui-ux-design/arrange-screen.md](docs/ui-ux-design/arrange-screen.md)         | Arrange 画面の設計（レイアウト・操作・ショートカット）                                      |
| [docs/test-strategy.md](docs/test-strategy.md)                                     | テスト戦略                                                                                  |
