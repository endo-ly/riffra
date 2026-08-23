# ヘッドレス Linux 対応

本書は、GUIを使わずにLinux上で制作状態を編集するCLIと、Native audio engineの境界および検証方法を説明する。

## CLI Host

`riffra` は `riffra-core` のApplication操作を `riffra-host` の永続化へ接続するStandalone Hostである。

```text
AI agent
   │ spawn
   ▼
riffra --data-root ./riffra-data --interactive
   │ Protocol v1 JSON Lines
   ▼
DataRootLease → SessionStore → AppCore<Application>
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

ワンショットは1つの操作を実行してJSONを出力する。対話モードは標準入力からProtocol v1の要求を複数受け取り、要求ごとにJSON Linesで応答する。`undo` と `redo` の履歴はプロセス内に保持されるため、対話モードでのみ利用できる。

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

要求は次の形式である。

```json
{
  "protocolVersion": 1,
  "requestId": "42",
  "command": "track.add",
  "params": { "name": "Bass", "kind": "instrument" }
}
```

成功応答にはcanonical sequenceを含める。

```json
{
  "protocolVersion": 1,
  "requestId": "42",
  "ok": true,
  "sequence": 12,
  "result": { "type": "session", "value": {} }
}
```

不正なJSON・Protocol version・要求形式は `invalidRequest`、未知のコマンドやCore・Hostの失敗は `commandFailed` として返す。応答の `requestId` は要求の値を保持する。

## Runtimeとの境界

CLIはAudio Runtimeを起動しない。再生、録音、Live MIDI、デバイス制御、Preview、Render、Plugin scan、Plugin editor、VSTの追加・置換・パラメータ変更はDesktop Adapterまたは専用Workerの責務である。CLIは音声デバイスやGUIのない環境でも、保存済みの制作状態を編集できる。

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
