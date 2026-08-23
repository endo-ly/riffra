# ヘッドレス環境でのCLI利用

ヘッドレス環境では、画面や音声デバイスを起動せずに、保存済みの制作状態をCLIから編集できます。本書は、LinuxでStandalone CLIを使う方法と、WindowsでDesktopへ接続する方法の違いを説明します。

プロトコルの詳しい形式は [IPC契約](ipc.md)、責務の分担は [アーキテクチャ](architecture.md) を参照してください。

## 1. 実行形態

CLIは、Data Rootを自分で開くStandaloneと、起動中のDesktopへ接続するAttachedに分かれます。

| OS      | 実行形態   | 接続先                                | 音声ランタイム    |
| ------- | ---------- | ------------------------------------- | ----------------- |
| Linux   | Standalone | 自身の `riffra-host` と `riffra-core` | 起動しない        |
| Windows | Standalone | 自身の `riffra-host` と `riffra-core` | 起動しない        |
| Windows | Attached   | DesktopのControl Server               | Desktopが所有する |

LinuxのCLIは音声デバイスやGUIを必要としません。保存、セッション編集、Assetの取込み、MIDIの配置、プロジェクト入出力など、CoreとHostだけで完結する操作を利用できます。

```text
Standalone
AI agent → riffra CLI → DataRootLease → SessionStore → AppCore

Attached（Windows）
AI agent → riffra CLI → Named Pipe → Desktop Adapter → AppCore / Runtime
```

同じData RootをDesktopとStandalone CLIが同時に開くことはできません。Attached CLIはData Rootを開かず、Desktopが保持する排他の内側で操作します。

## 2. Data Root

CLIのData Rootには、セッション、世代、素材、録音、書き出し、ライブラリ索引が保存されます。

```text
<data_root>/
├─ scratch/
│  ├─ current.json
│  └─ generations/
├─ library/
│  └─ riffra.db
├─ assets/
├─ recordings/
└─ exports/
```

Standalone CLIは起動中 `DataRootLease` を保持します。終了時にファイルを削除して所有権を示すのではなく、OSのファイルロックで同時利用を防ぎます。

## 3. 起動と操作

一つの操作だけを実行するワンショットと、標準入力から複数の要求を受け取る対話モードがあります。

```bash
cargo run -p riffra-cli -- --data-root ./riffra-data session get
cargo run -p riffra-cli -- --data-root ./riffra-data track add --name drums --kind audio
cargo run -p riffra-cli -- --data-root ./riffra-data --interactive
```

対話モードでは、1行の要求に対して1行のJSON応答を返します。`requestId` は応答へ引き継がれ、`expectedSequence` を指定した要求は、期待した正準シーケンスが一致するときだけ実行されます。

```json
{
  "protocolVersion": 2,
  "requestId": "42",
  "command": "track.add",
  "expectedSequence": 18,
  "params": { "name": "Bass", "kind": "instrument" }
}
```

成功応答には、操作後の正準シーケンスが含まれます。

```json
{
  "protocolVersion": 2,
  "requestId": "42",
  "ok": true,
  "sequence": 19,
  "result": { "type": "session", "value": {} }
}
```

不正な要求は `invalidRequest`、CoreやHostの失敗は `commandFailed`、正準シーケンスの不一致は `conflict` です。Desktopへ接続できない場合は `hostUnavailable`、Standaloneから音声ランタイムを必要とする操作を依頼した場合は `runtimeUnavailable` になります。機械判定にはエラーコードを使い、メッセージ本文を解析しません。

## 4. Standaloneで扱える範囲

Standaloneでは、次の操作を画面なしで実行できます。

| 分野         | 操作                                                             |
| ------------ | ---------------------------------------------------------------- |
| セッション   | 設定の取得・更新、履歴の取得、Undo、Redo                         |
| トラック     | 追加、更新、削除、複製、並べ替え、Audio/MIDI入力の設定           |
| クリップ     | Audio/MIDIクリップの作成、配置、移動、範囲変更、分割、複製、削除 |
| MIDI         | ノートの追加、更新、削除、複製、クオンタイズ、変換               |
| 時間軸       | テンポ、拍子、マーカー、ループ、パンチ、オートメーション         |
| 素材         | Audio/MIDI Assetの取込み、Assetの配置、参照の検証                |
| プロジェクト | パッケージの書き出しと取込み                                     |
| ラック       | Instrumentの解除、Effectの削除・並べ替え、デバイスのバイパス     |

CLIは引数とJSONの形式を検査します。トラック、クリップ、ノート、素材参照の意味と正規化は `riffra-core` が、WAV/MIDIの解析とプロジェクトパッケージは `riffra-host` が担います。

StandaloneのUndoとRedoは、そのプロセスが開いている履歴を使います。連続した操作を一つの対話セッションで扱うことで、履歴の順序も保てます。

## 5. Desktopへ依頼する操作

Attached CLIでは、Desktopが所有するRuntime、Transport、Live MIDI、プラグイン情報、デバイス設定、欠落依存の処理、レンダリングを制御できます。CLIは要求を転送するだけで、音声サイドカーやレンダーワーカーを直接起動しません。

`render start` はDesktopのバックグラウンドジョブを開始し、返されたジョブIDで状態の取得や取消を依頼します。録音画面、プラグインエディター、Preview、ライブラリの管理情報編集など、GUI固有の操作は公開制御境界の対象外です。

Attachedの接続にはWindows Named Pipeを使います。descriptorを読み込んだだけでは接続済みとみなさず、プロトコルのhandshakeまで完了してから操作を送ります。`--attach` が失敗しても、Standaloneへ自動で切り替えません。

LinuxではDesktopへ接続せず、Standaloneとして動作します。LinuxのNative audio engineはCLIとは別のC++ / JUCEサイドカーであり、CLIのData Root所有とは別の責務です。

## 6. 検証

セッション編集とNative audio engineは分けて検証します。

```bash
# Core / Host / CLI
cargo test -p riffra-core
cargo test -p riffra-host
cargo test -p riffra-cli
cargo run -p riffra-cli -- --data-root ./riffra-data session get

# Native audio engine
./native/audio-engine/build.sh Debug
```

Native audio engineの実機入出力はALSAデバイスを備えた環境で確認します。CIでは、デバイスを開かないCMakeの構成、ビルド、CTestを実行します。
