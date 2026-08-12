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
- 外部 VST3 プラグインに依存せず、音源エンジンまたは MIDI / オーディオクリップで完結する
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
  riffra-render-worker/        # riffra-render process adapter
native/
  audio-engine/                # CMake / C++
```

`riffra-core`はAsset、Rack、`CreativeSession`の正準モデルと`AppCore<A>`を持つ。オフラインレンダー要求は`AudioRuntime` portで定義し、`riffra-render-worker`が独立workerのprocess adapterとして実装する。

デスクトップホストとCLIホストは同じ`RenderWorker`を利用する。リアルタイム音声の`AudioSupervisor`、オーディオ設定、バックグラウンドジョブはデスクトップホスト側に置く。

### ネイティブエンジンの変更

ネイティブ処理は、音声デバイスを開かないオフライン経路と、デバイスを所有するリアルタイム経路を分ける。

- `riffra-render`: Timelineと音声ファイルを扱い、AudioDeviceManagerを初期化しない
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
{"protocolVersion":2,"requestId":"1","type":"addTrack","params":{"name":"audio-1","kind":"audio"}}
{"protocolVersion":2,"requestId":"2","type":"addAudioClip","params":{"trackId":"track-1","startTick":0}}
{"protocolVersion":2,"requestId":"3","type":"render","params":{"outPath":"./output.wav"}}
{"protocolVersion":2,"requestId":"4","type":"getStatus","params":{}}
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

`PluginRack` はTrack単位の処理グラフとプラグインスキャンで利用される。

- `TimelineEngine`
- `PluginChain`
- `AudioMain.cpp`

オフラインレンダーでは、VST3 Deviceを含むTrackを明示的な未対応依存として報告する。`PluginRack`をno-op化して無音レンダーを成功扱いにはしない。

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

まず実装するコマンド例です。プロトコルの詳細は「プロトコル設計」を参照してください。

| カテゴリ     | 書き込み                                                                                  | 読み出し                                 |
| ------------ | ----------------------------------------------------------------------------------------- | ---------------------------------------- |
| プロジェクト | `createProject`, `loadProject`, `saveProject`                                             | `getSession`                             |
| トラック     | `addTrack`, `removeTrack`, `updateTrack`                                                  | `listTracks`, `getTrack`, `getRackChain` |
| クリップ     | `addAudioClip`, `addMidiClip`, `removeClip`, `moveClip`, `trimClip`                       | `listClips`, `getClip`                   |
| 音符         | `updateNote`（ピッチ、ベロシティ、CC、タイミング）                                        | `getNotes`                               |
| 再生         | `play`, `stop`, `seek`                                                                    | `getTransportStatus`                     |
| 録音         | `startArrangeRecording`, `stopArrangeRecording`                                             | —                                        |
| 音源         | `loadInstrument`, `setInstrumentParameter`                                                | `listInstruments`, `getInstrument`       |
| 書き出し     | `render`, `cancelJob`                                                                     | `getJobs`, `getJob`                      |
| 状態         | —                                                                                         | `getStatus`                              |
| 編集の安全   | `undo`, `redo`, `beginEdit`, `commitEdit`, `rollbackEdit`                                    | —                                        |

---

## プロトコル設計

対話モードのJSON Linesは、リクエスト・レスポンス・イベントの3種類のフレームで構成する。1リクエストにつき1行のJSONで、`protocolVersion`でプロトコルを判別する。

```json
{"protocolVersion":2,"requestId":"1","type":"addTrack","params":{"name":"drums","kind":"audio"}}
{"status":"ok","requestId":"1","result":{"trackId":"track-1"}}
{"type":"event","requestId":"5","eventType":"jobProgress","payload":{"jobId":"j-1","progress":0.5}}
```

| フレーム | 方向             | 内容                                                             |
| -------- | ---------------- | ---------------------------------------------------------------- |
| request  | エージェント→CLI | コマンド呼び出し。`requestId`と`params`を持つ                    |
| response | CLI→エージェント | `status`、`result`、または`diagnostics`。`requestId`で対応を取る |
| event    | CLI→エージェント | `eventType`を持つ非同期通知。`requestId`に紐付く場合がある       |

エラーは共通のDiagnostics形式（`code`、`severity`、`message`、`detail`）で返す。外部エンジンの失敗もこの形式へ正規化する。

### 読み出し

書けるものはすべて読めることを原則とする。書き込みコマンドと対になる読み出しコマンドを揃える。音符はフィルター付きで参照できる。

```json
{
  "protocolVersion": 2,
  "requestId": "7",
  "type": "getNotes",
  "params": { "trackId": "track-1", "rangeTicks": [0, 1920], "velocityRange": [1, 40] }
}
```

時刻はtickとbar.beatの両方を受け付ける。ユーザーの指示（「2小節目のベロシティを上げる」など）をそのまま渡せるようにする。

### 非同期ジョブ

`render`や`startArrangeRecording`など時間のかかる操作は、ジョブとして即座に`jobId`を返す。進行状況は`jobProgress`イベントで通知し、完了時は`jobDone`、失敗時は`jobFailed`を送る。`cancelJob`で中止できる。レスポンスを待つ間もほかのコマンドを受け付けられる。

### 編集の安全

| 機能             | コマンド                                        | 内容                                             |
| ---------------- | ----------------------------------------------- | ------------------------------------------------ |
| 原子性           | 全コマンド                                      | 1コマンドは1つの原子操作。失敗時に状態が壊れない |
| 取り消し         | `undo`, `redo`                                  | セッションの操作履歴を戻す、やり直す             |
| トランザクション | `beginEdit`, `commitEdit`, `rollbackEdit`       | 複数コマンドの編集をまとめて適用または破棄する   |
| 冪等性           | リクエスト側                                    | 処理済み`requestId`の再送は無視する。再送に安全  |

### 外部エンジンとの境界

riffra-cliはセッション状態と編集の責務を持ち、音源やレンダリングの実体は独立した外部プロセスへ委譲できる。境界ではMIDIやイベント列など標準的なデータだけを渡し、riffra-coreは外部エンジンの内部構造を知らない。外部エンジンの診断はDiagnostics形式へ正規化する。

---

## ネイティブエンジンの変更点

| 項目               | Windows       | Linux       |
| ------------------ | ------------- | ----------- |
| 音声バックエンド   | ASIO / WASAPI | ALSA / JACK |
| VST3 ホスティング  | 有効          | 無効        |
| GUI イベントループ | 必要          | 不要        |
| エントリポイント   | `wmain`       | `main`      |

CMake targetは機能別に分ける。

- render coreは`juce_audio_formats`、`juce_audio_processors`、DSP、Timeline処理をリンクする
- `riffra-render`はrender coreとJSON Linesの入出力だけをリンクする
- `riffra-audio`だけが`juce_audio_devices`をリンクする
- `JUCE_ASIO=1`とUI付き`JUCE_PLUGINHOST_VST3=1`はWindowsデスクトップtargetに限定する
- Linuxリアルタイムtargetでは`JUCE_ALSA=1`、`JUCE_JACK=1`を使用する
- `PluginEditorHost.cpp` / `.h`はUI付きデスクトップtargetだけへ含める

---

## ロードマップ

### オフラインレンダー基盤

- Linux targetでGUIとVST3ホストをrender coreから分離する
- 音声デバイスとX ServerのないLinuxコンテナでレンダーを検証する
- VST3 Deviceを含むTrackを構造化された未対応依存として報告する

### CLI 実装

- `riffra-cli` crate を作る
- ワンショットモードを実装する
- 対話モード（`--interactive`）を実装する
- JSON Lines コマンドディスパッチを実装する
- 最小コマンドセットを動かす
- 読み出しコマンドとフィルター付き音符参照を実装する
- イベントフレームとジョブ状態を実装する
- `undo` / `redo` とトランザクションを実装する
- 処理済み`requestId`の冪等処理を実装する

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

Linux版`riffra-render`はJUCE GUI moduleと`juce_audio_devices`をリンクせず、X Serverとオーディオデバイスのない環境で実行する。X11へのリンクが検出された場合は実行環境へX11を追加せず、render targetへGUI moduleが混入した依存違反として扱う。

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
