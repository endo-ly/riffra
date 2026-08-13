# ヘッドレス Linux 対応

本書は、現在利用できるヘッドレス操作の範囲と、Linux上でレンダーやリアルタイム音声を扱うための将来方針を分けて説明する。

## 現在の実装

`riffra-core` は制作状態、編集規則、履歴を管理するOS非依存のApplication層である。現在、これをデスクトップアプリとは別のHostから利用できる実装として、`riffra-cli` がある。

```text
AI agent
   │ spawn
   ▼
riffra-cli --interactive --session ./project.json
   │ JSON Lines (stdin / stdout)
   ▼
Dispatcher → riffra-core::AppCore → SessionFileStorage
```

CLIはセッションファイルを読み込み、`AppCore` のApplication APIを呼び出し、成功した変更を同じファイルへ保存する。デスクトップのレンダーワーカーやリアルタイム音声サイドカーは、このCLIからは起動されない。

### CLIの動作モード

ワンショットモードは、コマンドライン引数で1つの操作を実行して終了する。対話モードはプロセスを維持し、標準入力から複数の操作を受け付ける。`undo` と `redo` の履歴はプロセス内に保持されるため、対話モードでのみ利用できる。

```bash
cargo run -p riffra-cli -- --session ./project.json add-track --name drums --kind audio
cargo run -p riffra-cli -- --session ./project.json list-tracks
cargo run -p riffra-cli -- --interactive --session ./project.json
```

現在のコマンドは次のとおりである。

| 分類               | コマンド                                   |
| ------------------ | ------------------------------------------ |
| セッション         | `get-session`                              |
| トラック           | `list-tracks`、`add-track`、`remove-track` |
| 設定               | `update-session-settings`                  |
| 履歴（対話モード） | `undo`、`redo`                             |

### CLIのJSON Lines境界

対話モードでは、1行のJSON要求に対して1行のJSON応答を返す。要求は`requestId`とflattenされたコマンドを持つ。

```json
{"requestId":"1","type":"addTrack","name":"Bass","kind":"instrument"}
{"requestId":"2","type":"listTracks"}
{"requestId":"3","type":"updateSessionSettings","loopEnabled":true}
```

成功応答は`ok`と`result`を持ち、失敗応答は`ok: false`と`error`を持つ。

```json
{"requestId":"1","ok":true,"result":{}}
{"requestId":"4","ok":false,"error":{"code":"commandFailed","message":"..."}}
```

現在のCLIプロトコルには、`protocolVersion`、`params`、非同期イベント、ジョブ進捗通知、共通Diagnosticsフレームはない。ワンショットモードはコマンドライン引数を受け取り、1つのJSON結果を標準出力へ出して終了する。

## デスクトップに存在する別の境界

デスクトップの`render_timeline`命令は、`riffra-render-worker`を1要求ごとに起動してオフラインレンダーを実行する。また、リアルタイム再生・録音・MIDI・デバイス制御は音声サイドカーが担当する。これらはデスクトップHostの境界であり、現在のCLIの機能には含まれない。詳細は [docs/ipc.md](ipc.md) を参照する。

この分離により、CLIはオーディオデバイスやGUIのない環境でも、セッションの読み出しと最小限の制作状態編集を実行できる。CLIが外部プラグインをスキャンしたり、音声デバイスを開いたりすることはない。

## 将来のLinux対応

Linux上で制作状態の編集を超えた処理を提供する場合も、まずCoreのApplication APIとHostの境界を分ける。

### オフラインレンダー

CLIからレンダーを呼び出す必要が生じた場合は、現在のCLIコマンドへデスクトップ専用の処理を直接追加せず、CoreのRender Portとレンダーワーカーを利用するHost側の経路を追加する。音声デバイスを初期化しないレンダー経路として成立させ、プラグインを含む未対応トラックは成功扱いにせず、明示的なエラーとして返す。

### Linuxのリアルタイム音声

再生、録音、MIDI、デバイス管理をLinuxで提供する場合は、リアルタイム音声HostへALSAまたはJACKの実装を追加する。CLIのセッション編集と音声デバイスの所有を同じプロセスへ混在させない。

### プロトコルの拡張

レンダーや長時間ジョブをCLIから操作する段階で、必要な要求のバージョン管理、パラメータの名前空間、非同期イベント、診断情報を追加する。その時点で実装した契約だけを [docs/ipc.md](ipc.md) とCLIのプロトコル定義へ反映する。

## Linuxでの検証方針

現時点でLinux Hostとして検証対象にできるのは、`riffra-core` と `riffra-cli` のセッション操作である。オフラインレンダーとリアルタイム音声をLinuxで検証する段階では、音声デバイスやX Serverを必要としないレンダー経路と、ALSA/JACKを使うリアルタイム経路を別々にビルド・テストする。

```bash
# CLI
cargo test -p riffra-cli
cargo run -p riffra-cli -- --session ./project.json get-session

# 将来のレンダーHost検証時に追加する経路
cmake -B build-linux -S native/audio-engine -DCMAKE_BUILD_TYPE=Release
```
