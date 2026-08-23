# Riffra Native Audio Engine

Native audio engineは、リアルタイム音声の時間管理を担うC++ / JUCEサイドカーです。Tauriプロセスはサイドカーを起動・監督しますが、音声コールバックや第三者プラグインのコードを実行しません。

IPC全体の契約は [IPC契約](../../docs/ipc.md) にまとめています。本書では、サイドカーの役割と開発時の扱いだけを説明します。

## 1. 実行モード

```text
riffra-audio --probe
  デバイスの種類を列挙する。音声ストリームは開かない。

riffra-audio --serve
  音声デバイスを安全な状態で開き、標準入力からJSON Linesを受け取る。
```

WindowsではASIOとWASAPI、LinuxではALSAを使います。デバイスの列挙、通常の音声処理、プラグインの走査は別の起動モードとして扱い、常駐する音声セッションへ不要な影響を与えません。

## 2. 音声の安全

サイドカーは、音声を出す前に安全側の状態を作ります。起動時のミュート、出力ゲインの制限、解除時のフェード、非有限値の拒否、出力の直流成分の遮断、音響フィードバックの検知を一つの安全経路として扱います。

フィードバックを検知した場合は自動的にミュートし、状態通知へ原因を含めます。プラグインのロードやグラフ構築に失敗しても、デバイスの安全状態と保存済みデータの扱いを分けて報告します。Rustが安全条件を確認した後に、起動時のミュートを解除します。

## 3. プロトコル

`--serve` は1行のJSONを1要求として読み、JSON Linesで応答します。主な要求は、状態照会、タイムライン投影、Transport、デバイスと安全制御、トラックデバイス、録音、Preview、テイク比較、MIDIです。

```json
{"type":"status"}
{"type":"setEmergencyMute","muted":true}
{"type":"prepareTimelineSnapshot","snapshot":{}}
{"type":"commitTimelineSnapshot"}
{"type":"sendTrackMidi","trackId":"track:1","bytes":[144,60,100]}
{"type":"shutdown"}
```

応答には要求との相関に使うIDを含めます。失敗時は、音声デバイス、プラグイン、録音、プロトコルなどの範囲と、保存済みデータが保たれているかを伝えます。状態イベントは要求への応答を待っている間も流れます。

音声サイドカーはArrangementのスナップショットから一時的なグラフを作ります。グラフを保存済みセッションへ直接書き戻さず、Coreが確定した順序に従って準備・有効化・破棄を行います。

## 4. ビルド

CMakeのラッパースクリプトを使います。

```powershell
# Windows
.\build.ps1 -Configuration Debug
```

```bash
# macOS / Linux
./build.sh Debug
```

スクリプトは、CMakeの構成、`riffra-audio` と `riffra-plugin-scan` のビルド、CTest、デスクトップ用バイナリのインストールを順に実行します。生成物は `apps/desktop/src-tauri/binaries/` に配置されます。

別のジェネレーターやアーキテクチャを使う場合は、ラッパースクリプトの引数で指定します。クロスコンパイル時のターゲット指定はCMakeの `RIFFRA_TARGET_TRIPLE` を使います。

デスクトップアプリはサイドカーが配置されてから起動します。音声デバイスを使わないビルドとCTestはCIで実行でき、実機の入出力は対応するOSとALSA、ASIO、WASAPIの機器を備えた環境で確認します。
