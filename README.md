# Riffra

Riffraは、演奏、音作り、録音、素材の整理、時間軸での構成を一つの作業空間につなぐ、ローカルファーストの音楽制作ワークベンチです。作った音だけでなく、使用した機器、ラック、録音条件、素材の由来も後から辿れる状態で残します。

デスクトップアプリは人が演奏と編集を行う入口です。CLIは同じ制作基盤を、スクリプトやAIエージェントから利用する入口です。両方の入口が同じ正準セッションと素材を扱います。

## 全体像

```text
┌──────────────────────────────────────────────────────────────┐
│ Tauri desktop                                                │
│ React UI ── Tauri IPC ── Rust Desktop Adapter               │
│                         └─ riffra-core / riffra-host         │
└───────────────┬────────────────────┬─────────────────────────┘
                │ JSON Lines         │ JSON Lines
        ┌───────▼────────┐   ┌──────▼─────────┐   ┌────────────▼──────┐
        │ riffra-audio   │   │ plugin scanner │   │ render worker      │
        │ real-time audio│   │ VST3 discovery │   │ offline rendering  │
        └────────────────┘   └────────────────┘   └───────────────────┘
```

- リアルタイム音声は `riffra-audio` サイドカーが担当します。Tauriプロセスは音声コールバックやプラグインコードを実行しません。
- セッション、素材、履歴の正準状態は `riffra-core` が保持します。
- ファイル保存、素材管理、プロジェクト入出力、制作ファイルの解析は `riffra-host` が担当します。
- データはローカルディスクに保存され、ネットワーク接続を中核機能の前提にしません。

## リポジトリ構成

| パス                           | 役割                                                                                     |
| ------------------------------ | ---------------------------------------------------------------------------------------- |
| `apps/desktop/`                | Tauriデスクトップアプリ。ReactフロントエンドとRustバックエンドで構成します。             |
| `apps/cli/`                    | `riffra-core` と `riffra-host` を使うCLIホストです。JSON Linesによる対話にも対応します。 |
| `crates/riffra-core/`          | ドメインモデル、制作操作、履歴、ポートを持つプラットフォーム非依存の中核です。           |
| `crates/riffra-host/`          | セッション保存、素材管理、プロジェクト入出力、WAV/MIDI解析、Data Rootの所有を扱います。  |
| `crates/riffra-control/`       | Desktopを外部から操作する制御プロトコルを定義します。                                    |
| `crates/riffra-render-worker/` | オフラインレンダリングを実行する子プロセスです。                                         |
| `native/audio-engine/`         | C++ / JUCEによるリアルタイム音声サイドカーです。                                         |
| `scripts/`                     | TypeScript型の生成など、開発時に使うスクリプトを置きます。                               |
| `docs/`                        | 製品構想、構造、境界、画面、検証方法を記載します。                                       |

依存関係はnpm workspaceとCargo workspaceで管理しています。Rustの型定義からTypeScriptの境界型を生成し、音声サイドカーへは実行に必要な投影だけを渡します。

## 技術スタック

| 層                   | 技術                                                 |
| -------------------- | ---------------------------------------------------- |
| デスクトップシェル   | Tauri 2                                              |
| フロントエンド       | React 19、TypeScript、Vite                           |
| アプリケーション中核 | Rust、edition 2024                                   |
| リアルタイム音声     | C++、JUCE                                            |
| 永続化               | SQLite（索引）、JSON（セッション）、WAV/MIDI（素材） |

## 開発を始める

### 前提

- Node.jsとnpm
- `Cargo.toml` の `rust-version` を満たすRust toolchain
- ネイティブ音声サイドカーのビルド環境

### よく使うコマンド

```bash
npm install
npm run dev             # ブラウザ上でフロントエンドを開発する
npm run dev:tauri      # Tauriアプリを起動する
npm run test
npm run gen:types      # Rustの定義からTypeScript型を生成する
npm run check          # 型チェック、ビルド、テスト
npm run lint

cargo run -p riffra-cli -- --data-root ./riffra-data session get
cargo run -p riffra-cli -- --data-root ./riffra-data --interactive
```

デスクトップアプリをネイティブ音声付きで起動する場合は、先に `native/audio-engine/` のサイドカーをビルドし、`apps/desktop/src-tauri/binaries/` へ配置します。手順は [ネイティブ音声エンジン](native/audio-engine/README.md) を参照してください。

## 文書の読み方

| 文書                                                    | 読む目的                                                        |
| ------------------------------------------------------- | --------------------------------------------------------------- |
| [製品構想](docs/CONCEPT.md)                             | Riffraが提供する制作体験と、機能を判断する原則を理解する        |
| [アーキテクチャ](docs/architecture.md)                  | プロセス、責務、正準状態、保存とランタイムの関係を理解する      |
| [データモデル](docs/data-model.md)                      | セッション、素材、録音、ラックの関係と不変条件を確認する        |
| [IPC契約](docs/ipc.md)                                  | WebView、Rust、サイドカー、外部CLIの境界を確認する              |
| [ヘッドレス環境](docs/headless-linux.md)                | LinuxでCLIを使う方法と、Desktopへ接続できる環境の範囲を確認する |
| [共通画面構造](docs/ui-ux-design/application-layout.md) | 画面を構成する領域と、それぞれの役割を確認する                  |
| [Arrange画面](docs/ui-ux-design/arrange-screen.md)      | 時間軸編集、MIDI編集、演奏、録音の操作仕様を確認する            |
| [テスト戦略](docs/test-strategy.md)                     | 変更に応じた検証範囲を決める                                    |

`docs/old/` は過去の製品構想を保存する資料です。現在の製品像を確認するときは [製品構想](docs/CONCEPT.md) を参照してください。
