# ヘッドレス Linux 対応

本書は、GUIを使わずにLinux上で制作状態を編集・操作するCLI Hostと、Native audio engineの境界および検証方法を説明する。

## CLI Host

Linuxの`riffra`には、短時間のStandalone操作と、`riffra serve`で起動するLive Hostがある。Live Hostは共有`riffra-runtime::DawHost`を使い、Canonical state、Undo / Redo、Runtime projection、Transportを一つのDataRoot所有期間にまとめる。

```text
AI agent
   │ spawn
   ▼
riffra --data-root ./riffra-data serve --safe-mode
   │ foreground Host
   ▼
DawHost → DataRootLease → AppCore<AudioSupervisor>
   │ local control
   ▼
riffra --data-root ./riffra-data --attach session get
```

DataRootは次の構造を持つ。Desktop、Standalone CLI、Live Hostは同じDataRootを同時に所有できない。Attached CLIはDataRootを開かず、接続先Hostの所有する状態を利用する。

```text
<data_root>/
├─ scratch/
│  ├─ current.json
│  └─ generations/
├─ library/
│  └─ riffra.db
├─ assets/
├─ recordings/
├─ exports/
└─ control/
   └─ host.json
```

起動例:

```bash
cargo run -p riffra-cli -- --data-root ./riffra-data session get
cargo run -p riffra-cli -- --data-root ./riffra-data track add --name drums --kind audio
cargo run -p riffra-cli -- --data-root ./riffra-data --interactive
cargo run -p riffra-cli -- --data-root ./riffra-data serve --safe-mode
cargo run -p riffra-cli -- --data-root ./riffra-data --attach session get

# Native audio engineを使う通常モード
./native/audio-engine/build.sh Debug
cargo run -p riffra-cli -- --data-root ./riffra-data serve
```

`serve`は起動後に`control/host.json`を公開し、終了シグナルを受けるまでフォアグラウンドで動作する。LinuxのControl transportはowner-only Unix Domain Socketである。`--attach`はこのdescriptorを読み、handshake後に要求を転送する。

Native build scriptはDesktop用のtriple付きバイナリを`apps/desktop/src-tauri/binaries/`へ、Headless用の`riffra-audio`、`riffra-plugin-scan`、`riffra-render`をCLI実行ファイルと同じ`target/debug/`または`target/release/`へインストールする。通常モードのHostは後者を自動解決する。実際の音声入力・出力にはALSAデバイスが必要で、デバイスが利用できない環境ではHostは起動してもRuntimeをReadyにできない。

ワンショットは1つの操作を実行してJSONを出力する。対話モードは標準入力から要求を複数受け取り、要求ごとにJSON Linesで応答する。Standaloneの`undo`と`redo`はプロセス内の履歴を使うため、対話モードで利用する。

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
  "requestId": "42",
  "command": "track.add",
  "expectedSequence": 18,
  "params": { "name": "Bass", "kind": "instrument" }
}
```

応答には、その結果が対応する正準シーケンスを含める。

```json
{
  "requestId": "42",
  "ok": true,
  "sequence": 12,
  "result": { "type": "session", "value": {} }
}
```

不正なJSON、要求形式、`params`、未知のコマンドは`invalidRequest`、Core・Hostの失敗は`commandFailed`、`expectedSequence`の不一致は`conflict`として返す。応答の`requestId`は要求の値を保持する。StandaloneでRuntime操作を受けた場合は`runtimeUnavailable`として返す。

## Runtimeとの境界

Standaloneのワンショット／対話モードはAudio Runtimeを起動せず、保存済みの制作状態を編集する。`serve`は通常モードでは明示的に解決された`riffra-audio`を起動し、Safe Modeでは音声・MIDI・外部プラグインをオフラインにしてHostだけを起動する。

| 範囲               | Live Hostでの扱い                                                           |
| ------------------ | --------------------------------------------------------------------------- |
| Canonical state    | Session、History、Track操作、Undo / Redo                                    |
| Runtime projection | 投影状態、Transport、Audio status / probe                                   |
| Audio Runtime      | 通常モードで`riffra-audio --serve`を起動。Safe Modeでは`runtimeUnavailable` |
| Plugin / Missing   | カタログ、スキャン、音源・エフェクト、欠落依存の操作をHostで実行            |
| Recording          | Native capture、take確定、canonical Session反映をHostで実行                 |
| Render / Jobs      | HostがRenderWorkerとJobRegistryを所有し、`job get/cancel`で状態を返す       |
| Library / Analysis | 索引、検索、metadata更新、関連素材、音声解析をHostで実行                    |
| Preview            | HostのAudio RuntimeへAsset previewを依頼（Safe Modeでは拒否）               |

LinuxのNative audio engineはCLIとは別プロセスのC++ / JUCEサイドカーであり、ALSAを使用する。Live HostのDataRoot所有とNative audio engineのデバイス所有はHostのライフサイクル内で分離される。

## Linuxでの検証

セッション編集とNative audio engineは別々に検証する。

```bash
# Core / Host / CLI
cargo test -p riffra-core
cargo test -p riffra-host
cargo test -p riffra-cli
cargo run -p riffra-cli -- --data-root ./riffra-data session get
cargo test -p riffra-control -p riffra-runtime
cargo run -p riffra-cli -- --data-root ./riffra-data serve --safe-mode
# 別の端末から
cargo run -p riffra-cli -- --data-root ./riffra-data --attach session get

# Native audio engine
./native/audio-engine/build.sh Debug
```

Native audio engineのデバイスオープンは物理デバイスを必要とするため、CIではCMake configure、build、CTestをデバイス非依存の範囲で実行する。実機の入出力はALSAデバイスを備えた環境で検証する。
