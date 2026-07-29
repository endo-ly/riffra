# ヘッドレス Linux 対応

Riffra をヘッドレス Linux 上で動かし、AI エージェントが CLI から操作できるようにする方針。

関連ドキュメント：

- `architecture.md`
- `ipc.md`
- `data-model.md`

---

## 背景

Riffra は現在 Windows 向けの Tauri + React + C++/JUCE sidecar 構成。AI エージェントに音源生成や編集を任せる用途では GUI や WebView は不要なので、Linux サーバーやコンテナ上で動くヘッドレス版が欲しくなる。

目指す形：

- Linux 上で動作する
- 外部 VST3 プラグインに依存せず、内蔵音源エンジンまたは MIDI / オーディオクリップで完結する
- AI エージェントがプログラムから操作できる
- 状態を保持しながら連続的に操作できる

---

## 全体構成

### Rust 側の分離

Rust のアプリケーションロジックを `riffra-core` に切り出して、デスクトップ版と CLI 版で共有する。デスクトップ版では Tauri と React を使う。ヘッドレス CLI ではこれらを使わない。

```text
src-tauri/
  crates/
    riffra-core/      # Tauri 非依存ドメイン層
    riffra-tauri/     # 既存デスクトップ
    riffra-cli/       # ヘッドレス CLI
```

`riffra-core` が持つ状態：

- `CreativeSession`
- `AudioSupervisor`（sidecar 抽象層）
- オーディオ設定
- バックグラウンドジョブレジストリ

Tauri command は `riffra-core` の操作を薄くラップする。

### ネイティブエンジンの変更

`native/audio-engine` を Linux 向けにビルドできるようにする。

- ASIO / WASAPI 非依存にする
- ALSA / JACK / PipeWire 対応を追加する
- VST3 ホスティングを無効にする
- `MessageManager` による GUI イベントループを不要にする

### AI エージェントとの接続

AI エージェントは `riffra-cli` をサブプロセスとして起動し、標準入出力で対話する。MCP や HTTP API は今のところ導入しない。

```text
AI Agent
   │ spawn
   ▼
riffra-cli --interactive
   │ stdin / stdout JSON Lines
   ▼
riffra-audio (Linux ヘッドレス版)
```

既存の sidecar と同じく JSON Lines プロトコルを使う。

---

## CLI モード

`riffra-cli` は 2 つの動作モードを用意する。

### ワンショットモード

1 コマンド実行してすぐ終了する。シェルスクリプトやバッチ処理に向く。

```bash
riffra-cli add-track --project ./project.riffra --name drums
riffra-cli render --project ./project.riffra --out ./output.wav
```

### 対話モード

プロセスを常駐させて、標準入力から JSON Lines 形式のコマンドを受け付ける。状態をメモリ上に保つので、連続操作が速い。

```bash
riffra-cli --interactive --project ./project.riffra
```

入力例：

```json
{"command":"addTrack","name":"synth-1","kind":"instrument"}
{"command":"loadInstrument","trackId":"track-1","engine":"basic-synth"}
{"command":"addMidiClip","trackId":"track-1","startTick":0,"durationTicks":3840,"notes":[]}
{"command":"render","outPath":"./output.wav"}
{"command":"getStatus"}
```

---

## Linux 版の VST3 対応

Linux 版の `riffra-audio` は VST3 プラグインホスティングを含まない。

理由：

- AI エージェントが操作する音源は内蔵エンジンで賄う
- VST3 エディタ GUI（`PluginEditorHost.cpp`）と `MessageManager` の GUI イベントループが、ヘッドレス化の最大の障害になっている
- VST3 を外せば JUCE GUI モジュールへの依存も減る
- 今必要なのは「タイムライン編集 + 録音 + MIDI + レンダリング」で、これは VST3 なしでも実現できる

影響箇所：

`PluginRack` は以下に直接組み込まれている。

- `SafetyAudioCallback`
- `TimelineEngine`
- `PluginChain`
- `Main.cpp`

Linux ビルドでは `RIFFRA_ENABLE_VST3` 定義を無効にして `PluginRack` を no-op スタブにする。Instrument Track は内蔵音源エンジンで処理するか、MIDI データのみ出力する。

必要になれば後から追加できる：

- ヘッドレス VST3 ホスティング（GUI なし）
- Linux ネイティブ VST3 対応
- CLAP / LV2 対応

---

## AI エージェント連携

AI エージェントは対話モードの `riffra-cli` をサブプロセスとして起動する。MCP や HTTP API は今のところ導入しない。

標準入出力を使う理由：

- 状態保持と双方向通信が、サブプロセス連携だけで実現できる
- 既存の sidecar JSON Lines プロトコルと対称的
- ネットワーク・認証・ポート管理が不要
- 後から HTTP 層を追加しても `riffra-core` は変えなくて済む

まず実装するコマンド例：

| カテゴリ     | コマンド                                                            |
| ------------ | ------------------------------------------------------------------- |
| プロジェクト | `loadProject`, `saveProject`, `createProject`                       |
| トラック     | `addTrack`, `removeTrack`, `updateTrack`, `listTracks`              |
| クリップ     | `addAudioClip`, `addMidiClip`, `removeClip`, `moveClip`, `trimClip` |
| 再生         | `play`, `stop`, `seek`, `getTransportStatus`                        |
| 録音         | `startRecording`, `stopRecording`                                   |
| 音源         | `loadInstrument`, `setInstrumentParameter`                          |
| 書き出し     | `render`, `getJobStatus`                                            |
| 状態         | `getStatus`, `getSession`                                           |

---

## ネイティブエンジンの変更点

| 項目               | Windows       | Linux                  |
| ------------------ | ------------- | ---------------------- |
| 音声バックエンド   | ASIO / WASAPI | ALSA / JACK / PipeWire |
| VST3 ホスティング  | 有効          | 無効                   |
| GUI イベントループ | 必要          | 不要                   |
| エントリポイント   | `wmain`       | `main`                 |

CMakeLists.txt の主な変更点：

- `JUCE_ASIO=1` を Windows 限定にする
- Linux では `JUCE_ALSA=1`, `JUCE_JACK=1` を追加する
- `PluginEditorHost.cpp` / `.h` を Linux ビルドから除外する
- `RIFFRA_ENABLE_VST3` 定義を Windows 限定にする

---

## ロードマップ

### Rust コアの分離

- `src-tauri` を workspace 化する
- `riffra-core` crate を作る
- `AppState` を `AppCore` に移す
- Tauri command を `riffra-core` のラッパーにする
- sidecar 起動を `tauri_plugin_shell` から抽象化する
- 既存テストが通ることを確認する

### CLI 実装

- `riffra-cli` crate を作る
- ワンショットモードを実装する
- 対話モード（`--interactive`）を実装する
- JSON Lines コマンドディスパッチを実装する
- 最小コマンドセットを動かす

### Linux ネイティブビルド

- CMakeLists.txt の Linux 分岐を追加する
- VST3 / GUI 依存をコンパイル条件で切り離す
- Docker 内でビルド検証をする
- PipeWire / JACK 環境で smoke test をする

### AI エージェント連携検証

- 対話モードをサブプロセスとして起動するサンプルスクリプトを作る
- プロジェクト作成 → トラック追加 → レンダリングの流れを自動化する
- 状態取得・エラー処理を安定させる

---

## ビルド・実行環境

### 開発環境

Debian / Ubuntu を想定。

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential cmake ninja-build pkg-config \
  libasound2-dev libjack-jackd2-dev \
  libfreetype6-dev libfontconfig1-dev \
  libx11-dev libxcomposite-dev libxext-dev \
  libxrandr-dev libxrender-dev libxcursor-dev \
  libxinerama-dev libgl1-mesa-dev
```

Rust toolchain：

```bash
rustup target add x86_64-unknown-linux-gnu
```

### ビルド例

```bash
# Rust 側
cd src-tauri
cargo build -p riffra-cli --release

# ネイティブ側
cd native/audio-engine
cmake -B build-linux -S . -DCMAKE_BUILD_TYPE=Release
cmake --build build-linux --target riffra-audio
```

### ヘッドレス実行

X サーバーは不要。ただし JUCE の一部モジュールが X11 ライブラリにリンクを要求する場合があるので、ビルド時には上記の X11 開発パッケージが必要。実行時に実際のディスプレイは不要。

オーディオデバイスがない環境では、最初からオフライン・レンダリング専用モードを優先して検証する。

---

## 制約と将来追加できる機能

### 今回の範囲の制約

- VST3 プラグインは使えない
- プラグインエディタは表示できない
- 既存の React UI はヘッドレスモードでは使えない
- ライブ音声入出力は ALSA / JACK / PipeWire 対応後に使える

### 後から追加できる機能

- ヘッドレス VST3 / CLAP ホスティング
- HTTP API 層
- MCP server
- OSC 入出力
- クラウドストレージ連携
