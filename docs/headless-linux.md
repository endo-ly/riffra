# ヘッドレス Linux 対応

本書は、GUIを使わずにLinux上で制作状態を編集するCLIと、Native audio engineの境界および検証方法を説明する。

## CLI Host

Linuxの`riffra`は、`riffra-core`のApplication操作を`riffra-host`の永続化へ接続するStandalone Hostである。Windowsでは、同じCLIバイナリから起動中のDesktopへ接続するAttached modeも利用できる。LinuxではDesktop Attachを提供しない。

```text
AI agent
   │ spawn
   ▼
riffra --data-root ./riffra-data --interactive
   │ Protocol v2 JSON Lines
   ▼
DataRootLease → SessionStore → AppCore<Application>
```

実行モードは、Data Rootを誰が所有するかで分かれる。

```text
Windows
├─ Standalone CLI → DataRootLease → SessionStore → AppCore<()>
└─ --attach       → Named Pipe → Desktop AppCore / Runtime

Linux
└─ Standalone CLI → DataRootLease → SessionStore → AppCore<()>
```

DataRootは次の構造を持つ。DesktopとCLIは同じDataRootを同時に所有できない。

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

起動例:

```bash
cargo run -p riffra-cli -- --data-root ./riffra-data session get
cargo run -p riffra-cli -- --data-root ./riffra-data track add --name drums --kind audio
cargo run -p riffra-cli -- --data-root ./riffra-data --interactive
```

ワンショットは1つの操作を実行してJSONを出力する。対話モードは標準入力からProtocol v2の要求を複数受け取り、要求ごとにJSON Linesで応答する。Standaloneの`undo`と`redo`はプロセス内の履歴を使うため、対話モードで利用する。

## 編集できる制作状態

CLIは次の状態を編集できる。

- Session設定、履歴、Undo / Redo
- Trackの追加・更新・削除・複製・並べ替え
- Audio / MIDI Input Routing
- Audio ClipとMIDI Clipの配置・更新・移動・Trim・Split・複製
- MIDI Noteの追加・一括挿入・更新・削除・Quantize・Transform・複製
- Marker、Automation、Timebase、Loop、Punch
- canonical MIDI AssetのimportとAudio / MIDI Assetの配置
- Projectのexport / import
- Instrumentの解除、Effectの削除・並べ替え、Device bypass

引数の形式はCLIが検査する。Track、Clip、Note、Automationなどの制作上のValidationと正規化は `riffra-core::Application` が実行する。MIDI AssetのSMF検証、Audio ClipのWAV metadata解決、Project package、Asset参照の整合性は `riffra-host` が担当する。

## JSON Lines境界

Standaloneの要求は次の形式である。`expectedSequence`は任意で、指定した場合は正準シーケンスが一致するときだけ操作する。

```json
{
  "protocolVersion": 2,
  "requestId": "42",
  "command": "track.add",
  "expectedSequence": 18,
  "params": { "name": "Bass", "kind": "instrument" }
}
```

成功応答には操作後の正準シーケンスを含める。

```json
{
  "protocolVersion": 2,
  "requestId": "42",
  "ok": true,
  "sequence": 12,
  "result": { "type": "session", "value": {} }
}
```

不正なJSON、Protocol version、要求形式、`params`、unknown commandは`invalidRequest`、Core・Hostの失敗は`commandFailed`、`expectedSequence`の不一致は`conflict`として返す。応答の`requestId`は要求の値を保持する。StandaloneでDesktop専用のRuntime commandを受けた場合は`runtimeUnavailable`として返す。

## Runtimeとの境界

LinuxのStandalone CLIはAudio Runtimeを起動しない。再生、録音、Live MIDI、デバイス制御、Preview、Render、Plugin scan、Plugin editor、VSTの追加・置換・パラメータ変更はDesktop Adapterまたは専用Workerの責務である。CLIは音声デバイスやGUIのない環境でも、保存済みの制作状態を編集できる。WindowsのAttached CLIでは、公開されたRuntime操作をDesktopのNamed Pipe経由で依頼できる。

Linux Desktop Attachは提供せず、LinuxではStandaloneを使う。Desktop接続を追加する場合のローカル通信にはUnix Domain Socketを使う。

LinuxのNative audio engineはCLIとは別プロセスのC++ / JUCEサイドカーであり、ALSAを使用する。CLIのDataRoot所有とNative audio engineのデバイス所有は混在させない。

## Linuxでの検証

セッション編集とNative audio engineは別々に検証する。

```bash
# Core / Host / CLI
cargo test -p riffra-core
cargo test -p riffra-host
cargo test -p riffra-cli
cargo run -p riffra-cli -- --data-root ./riffra-data session get

# Native audio engine
./native/audio-engine/build.sh Debug
```

Native audio engineのデバイスオープンは物理デバイスを必要とするため、CIではCMake configure、build、CTestをデバイス非依存の範囲で実行する。実機の入出力はALSAデバイスを備えた環境で検証する。
