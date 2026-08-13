# Riffra

**練習から曲までを一つの作業空間で繋ぐ**ローカルファーストの音楽制作ワークベンチ。楽器や声を入力して音を作り、その音を録音・素材として残し、時間軸上で音楽として組み立てる。どの段階でも、音を作った瞬間の設定と文脈（機器、ラック、録音条件、由来）が失われないことを最優先にする。

詳細な製品構想は [docs/CONCEPT.md](docs/CONCEPT.md) を参照。

## 全体像

```text
┌───────────────────────────────────────────────┐
│  Tauri シェル（1プロセス）                      │
│  React フロントエンド ── Tauri IPC ── Rust      │
│  バックエンド（Core / Desktop Adapter）          │
└──────┬──────────────────────┬─────────────────┘
       │ JSON Lines (stdin/stdout)
┌──────▼──────────┐   ┌───────▼─────────┐  ┌──────▼────────┐
│ riffra-audio    │   │ riffra-plugin-  │  │ riffra-render │
│ リアルタイム音声 │   │ scan            │  │ -worker       │
│ (C++ / JUCE)    │   │ VST3 スキャン    │  │ オフライン    │
└─────────────────┘   └─────────────────┘  │ レンダリング   │
                                           └───────────────┘
```

- リアルタイム音声は常に **riffra-audio サイドカー**（C++ / JUCE）が担当し、Tauri プロセスは音声コールバックやプラグインコードを実行しない
- セッション・素材・由来の正準状態は **riffra-core**（Rust）が保持し、WebView と サイドカーは契約された型と命令のみで接続される
- データはすべてローカルディスクに保存される（ネットワーク非依存）

## リポジトリ構成

| パス                           | 内容                                                                             |
| ------------------------------ | -------------------------------------------------------------------------------- |
| `apps/desktop/`                | Tauri デスクトップアプリ（React フロントエンド + `src-tauri` Rust バックエンド） |
| `apps/cli/`                    | `riffra-core` を直接利用するワンショット／JSON Lines CLI ホスト                  |
| `crates/riffra-core/`          | Application / Domain / Ports（Session / Asset / Rack / 履歴）                    |
| `crates/riffra-render-worker/` | オフラインレンダリングの子プロセスバイナリ                                       |
| `native/audio-engine/`         | リアルタイム音声エンジンのサイドカー（C++ / JUCE）                               |
| `scripts/`                     | 型生成（`gen-barrel.js`）、検証（`verify.mjs`）などの開発スクリプト              |
| `docs/`                        | 設計・調整ドキュメント                                                           |

依存関係は npm workspace（`@riffra/desktop`）と Cargo workspace（`riffra-core` / `riffra-cli` / `riffra-render-worker` / デスクトップバイナリ）で管理する。

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
- ネイティブ音声エンジンのビルド済みサイドカー（`native/audio-engine/` 参照。ビルドは `apps/desktop/src-tauri/binaries/` へ配置される）

### コマンド

```powershell
npm install            # npm workspace の依存関係を導入
npm run dev            # Vite のみでフロントエンド開発（ブラウザ）
npm run dev:tauri      # Tauri アプリ全体を起動（ネイティブ音声を利用する場合はこちら）

npm run gen:types      # Rust 定義から TS 型を再生成（ts-rs + gen-barrel.js）
npm run check          # 型チェック + ビルド + テスト
npm run verify         # ルートの一括検証（--native でネイティブビルドを含む）

npm run lint           # ESLint
npm run typecheck      # tsc

cargo run -p riffra-cli -- --session ./project.json get-session
cargo run -p riffra-cli -- --interactive --session ./project.json
```

### 検証

- フロントエンド: Vitest + Testing Library（`apps/desktop/src` 配下に配置）
- Rust: `cargo test`（各 crate）
- ネイティブ: CMake + CTest（`native/audio-engine`）
- 一括検証: `npm run verify`

## ドキュメント

| ドキュメント                                                               | 内容                                                           |
| -------------------------------------------------------------------------- | -------------------------------------------------------------- |
| [docs/CONCEPT.md](docs/CONCEPT.md)                                         | 製品構想（価値、二つの制作領域、利用の形）                     |
| [docs/architecture.md](docs/architecture.md)                               | システム構造と主要機構（セッション正準化、整合性、保存）       |
| [docs/data-model.md](docs/data-model.md)                                   | ドメインエンティティの正準カタログと不変条件                   |
| [docs/ipc.md](docs/ipc.md)                                                 | IPC 境界の契約（Tauri 命令 / イベント / サイドカープロトコル） |
| [docs/ui-ux-design/arrange-screen.md](docs/ui-ux-design/arrange-screen.md) | Arrange 画面の設計（レイアウト・操作・ショートカット）         |
| [docs/test-strategy.md](docs/test-strategy.md)                             | テスト戦略                                                     |
| [docs/headless-linux.md](docs/headless-linux.md)                           | ヘッドレス Linux でのビルド・実行                              |
