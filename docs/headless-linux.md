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
Cargo.toml                     # Rust workspace
apps/
  desktop/
    src/                        # React / TypeScript
    src-tauri/                  # Tauriデスクトップホスト
crates/
  riffra-core/                 # Tauri / OS非依存
native/
  audio-engine/                # CMake / C++
```

`riffra-core`はAsset、Rack、`CreativeSession`の正準モデルと`AppCore<A>`を持つ。`AppCore`が保持するAudio Runtimeは`AudioRuntime` portを実装したホスト注入型であり、Tauriの`AppHandle`、sidecar process、イベント配信を参照しない。オフラインレンダーは共有の`OfflineRenderRequest`を通して実行し、制作処理は具体的な`AudioSupervisor`型へ依存しない。

デスクトップホストでは`AudioSupervisor`が`AudioRuntime`を実装する。CLIホストは同じportに、オフラインレンダーworkerまたはLinuxリアルタイムworkerとの接続を注入する。オーディオ設定、バックグラウンドジョブ、プロセス監視は、それを必要とするホスト側に置く。

### ネイティブエンジンの変更

ネイティブ処理は、音声デバイスを開かないオフライン経路と、デバイスを所有するリアルタイム経路を分ける。

- `riffra-render`: Timelineと音声ファイルだけを扱い、AudioDeviceManager、GUI、VST3に依存しない
- `riffra-audio`: ライブ再生、録音、MIDI、デバイス管理を扱う

Linux対応は`riffra-render`を先に成立させ、その後で`riffra-audio`へALSA / JACKを追加する。PipeWire環境ではJACK互換層を利用する。

### AI エージェントとの接続

AI エージェントは `riffra-cli` をサブプロセスとして起動し、標準入出力で対話する。MCP や HTTP API は今のところ導入しない。

```text
AI Agent
   │ spawn
   ▼
riffra-cli --interactive
   │ stdin / stdout JSON Lines
   ├───────────────► riffra-render
   └───────────────► riffra-audio（ライブ機能を使う場合）
```

既存の sidecar と同じく JSON Lines プロトコルを使う。

---

## CLI モード

`riffra-cli` は 2 つの動作モードを用意する。

### ワンショットモード

1 コマンド実行してすぐ終了する。シェルスクリプトやバッチ処理に向く。

```bash
riffra-cli add-track --project ./project/project.json --name drums
riffra-cli render --project ./project/project.json --out ./output.wav
```

### 対話モード

プロセスを常駐させて、標準入力から JSON Lines 形式のコマンドを受け付ける。状態をメモリ上に保つので、連続操作が速い。

```bash
riffra-cli --interactive --project ./project/project.json
```

入力例：

```json
{"protocolVersion":1,"requestId":"1","type":"addTrack","params":{"name":"audio-1","kind":"audio"}}
{"protocolVersion":1,"requestId":"2","type":"addAudioClip","params":{"trackId":"track-1","startTick":0}}
{"protocolVersion":1,"requestId":"3","type":"render","params":{"outPath":"./output.wav"}}
{"protocolVersion":1,"requestId":"4","type":"getStatus","params":{}}
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

オフラインMVPでは、VST3 Deviceを含むTrackを明示的な未対応依存として報告する。`PluginRack`をno-op化して無音レンダーを成功扱いにはしない。

Instrument Trackの音声化を追加する場合は、Track processor graphに内蔵音源processorを実装する。内蔵音源が存在しない状態では、MIDIデータの保存・編集・書出しまでを対応範囲とする。

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

| 項目               | Windows       | Linux       |
| ------------------ | ------------- | ----------- |
| 音声バックエンド   | ASIO / WASAPI | ALSA / JACK |
| VST3 ホスティング  | 有効          | 無効        |
| GUI イベントループ | 必要          | 不要        |
| エントリポイント   | `wmain`       | `main`      |

CMake targetは機能別に分ける。

- render coreは`juce_audio_formats`、DSP、Timeline処理だけをリンクする
- `riffra-render`はrender coreとJSON Linesの入出力だけをリンクする
- `riffra-audio`だけが`juce_audio_devices`をリンクする
- `JUCE_ASIO=1`とUI付き`JUCE_PLUGINHOST_VST3=1`はWindowsデスクトップtargetに限定する
- Linuxリアルタイムtargetでは`JUCE_ALSA=1`、`JUCE_JACK=1`を使用する
- `PluginEditorHost.cpp` / `.h`はUI付きデスクトップtargetだけへ含める

---

## ロードマップ

### オフラインレンダー基盤

- Timelineとオフラインレンダー処理をAudioDeviceManager、GUI、VST3から分離する
- `riffra-render` workerを作る
- 音声デバイスとX ServerのないLinuxコンテナでレンダーを検証する
- VST3 Deviceを含むTrackを構造化された未対応依存として報告する

### CLI 実装

- `riffra-cli` crate を作る
- ワンショットモードを実装する
- 対話モード（`--interactive`）を実装する
- JSON Lines コマンドディスパッチを実装する
- 最小コマンドセットを動かす

### Linux リアルタイム音声

- CMakeLists.txt の Linux 分岐を追加する
- AudioDeviceManagerをリアルタイムworkerだけへリンクする
- ALSA / JACK環境でデバイス列挙、再生、録音を検証する
- PipeWireのJACK互換環境でsmoke testをする

### AI エージェント連携検証

- 対話モードをサブプロセスとして起動するサンプルスクリプトを作る
- プロジェクト作成 → トラック追加 → レンダリングの流れを自動化する
- 状態取得・エラー処理を安定させる

---

## ビルド・実行環境

### 開発環境

Debian / Ubuntuを想定する。`riffra-core`とCLIに追加のネイティブパッケージは不要である。オフラインレンダーworkerのビルド環境は次を基準とする。

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential cmake ninja-build pkg-config
```

リアルタイムworkerをビルドする環境だけにALSA / JACKの開発パッケージを追加する。

```bash
sudo apt-get install -y libasound2-dev libjack-jackd2-dev
```

Linux上でネイティブビルドする場合、既定のRust targetを使用する。別OSから`x86_64-unknown-linux-gnu`へクロスコンパイルする場合は、Rust targetだけでなく対応するリンカーとsysrootも必要になる。

### ビルド例

```bash
# Rust 側
cargo build -p riffra-desktop --release

# ネイティブ側
cd native/audio-engine
cmake -B build-linux -S . -DCMAKE_BUILD_TYPE=Release
cmake --build build-linux --target riffra-render
```

### ヘッドレス実行

`riffra-render`はJUCE GUI moduleと`juce_audio_devices`をリンクせず、X Serverとオーディオデバイスのない環境で実行する。X11へのリンクが検出された場合は実行環境へX11を追加せず、render targetへGUI moduleが混入した依存違反として扱う。

---

## 制約と将来追加できる機能

### 今回の範囲の制約

- VST3 プラグインは使えない
- プラグインエディタは表示できない
- 既存の React UI はヘッドレスモードでは使えない
- ライブ音声入出力はALSA / JACK対応後に使える

### 後から追加できる機能

- ヘッドレス VST3 / CLAP ホスティング
- HTTP API 層
- MCP server
- OSC 入出力
- クラウドストレージ連携
