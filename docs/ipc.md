# Riffra IPC契約

Riffraは、画面、Rustバックエンド、音声サイドカー、レンダーワーカー、外部CLIを別の実行境界として扱います。本書は、それぞれの境界をどの経路で結び、どの状態を正本として扱うかを定めます。

メッセージの全フィールドや型の正本はコードにあります。システム構造は [アーキテクチャ](architecture.md)、エンティティの意味は [データモデル](data-model.md) を参照してください。

## 1. 境界の一覧

```text
┌──────────────────────────── WebView ────────────────────────────┐
│ React ── NativeApi ── Tauri commands / events                   │
└───────────────┬──────────────────────────────┬──────────────────┘
                │                                │
        ┌───────▼────────────────────────────────▼───────┐
        │ Rust backend / Desktop Adapter                  │
        └───────┬───────────────┬───────────────┬─────────┘
                │               │               │
          JSON Lines       JSON 1行        Named Pipe
        ┌───────▼──────┐ ┌──▼───────────┐ ┌──▼──────────────┐
        │ riffra-audio│ │ render worker│ │ Desktop Control │
        │ 常駐音声     │ │ 単発書出し    │ │ Attached CLI    │
        └──────────────┘ └──────────────┘ └─────────────────┘
```

| 境界 | 接続                   | 目的                                            |
| ---- | ---------------------- | ----------------------------------------------- |
| A    | WebView → Rust         | 制作操作、照会、音声やジョブの制御              |
| B    | Rust → WebView         | 正準状態、音声状態、Transport、ランタイムの通知 |
| C    | Rust ↔ `riffra-audio`  | 音声グラフ、演奏、録音、MIDI、デバイス制御      |
| D    | Rust → render worker   | オフラインレンダリング                          |
| E    | Rust → probe / scanner | デバイス、チャンネル、VST3の列挙                |
| F    | Attached CLI ↔ Desktop | Desktopの正準状態、履歴、Runtimeの外部制御      |

常時稼働する低遅延の処理はC、完了まで待つバッチはD、列挙はEを使います。通常のデスクトップ操作はA、外部CLIから起動中のDesktopを操作するときだけFを使います。Standalone CLIの標準入出力はFとは別の境界です。

## 2. WebViewとRust

### 命令

Reactは `src/native/native-api.ts` のcapabilityを通してTauri命令を呼びます。コンポーネントがTauriの命令名や引数名を直接持たないことで、画面と通信方式を分離します。

命令は次の責務で分かれます。

| 分類                 | 例                                                         | 結果                                   |
| -------------------- | ---------------------------------------------------------- | -------------------------------------- |
| 制作状態             | トラック、クリップ、ノート、設定、履歴、素材、プロジェクト | Coreが確定したセッションまたは履歴状態 |
| ランタイム           | 再生、停止、シーク、MIDI送信、ミュート、投影の再試行       | Runtimeの状態                          |
| デバイスとプラグイン | デバイスの照会、VST3の走査、プラグイン状態                 | 状態またはバックグラウンドジョブ       |
| 録音                 | 録音の開始・停止、テイクの確認と採用                       | 録音状態、テイク、セッション           |
| レンダリング         | 書き出しの開始                                             | ジョブIDと進行状態                     |

制作状態を変更する命令はCoreのコミット順序に従います。UIは応答に含まれるセッションと正準シーケンスを表示へ反映し、独自の全体マージや競合解決を行いません。

重いファイル処理や解析は、Tauriの非同期処理を塞がない実行経路へ委ねます。読み取り専用の照会、正準セッションの変更、音声ランタイムの制御は、それぞれの所有する排他範囲を分けます。

### イベント

RustからWebViewへ送るイベントは、画面が現在の状態を表示するための通知です。制作状態を変更する入力経路には使いません。

| イベント                         | 内容                                                |
| -------------------------------- | --------------------------------------------------- |
| `runtime-startup-finished`       | ランタイム初期化の完了                              |
| `audio-status`                   | デバイス、ミュート、録音、Previewなど音声状態の要約 |
| `audio-meters`                   | 入出力ピークと異常サンプルの高頻度通知              |
| `transport-status`               | 再生位置と再生中かどうか                            |
| `runtime-projection-status`      | 音声グラフの準備、稼働、失敗                        |
| `runtime-restarted`              | サイドカー再起動と再投影の世代                      |
| `canonical-state-changed`        | GUI以外のDesktop操作を含む正準セッションの変更      |
| `track-plugin-state-changed`     | プラグインのロードやバイパスの変化                  |
| `track-plugin-parameter-changed` | プラグインパラメーターの変化                        |

`canonical-state-changed` は、Attached CLIなど別の入口から正準状態が変わったことをGUIへ知らせます。イベントを受けたUIは、シーケンスを確認して古い通知を表示へ適用しません。

## 3. 音声サイドカー

`riffra-audio --serve` は標準入力からJSON Linesを読み、各要求に対してJSON Linesで応答します。要求と応答には相関IDを含め、Rustは応答を待つ間も状態イベントを受け取ります。

音声サイドカーが扱う操作は、状態照会、タイムライン投影、Transport、デバイスと安全制御、トラックデバイス、録音、Preview、テイク比較、MIDIに分かれます。Tauriプロセスはそれらを直接実行せず、Runtimeの所有者へ依頼します。

成功時は音声状態またはメーターを返します。失敗時は処理の範囲と、保存済みデータが保たれているかを含めます。応答が時間内に届かなかった場合は、現在の正準セッションから再接続と再投影を判断します。

```json
{"type":"status"}
{"type":"setEmergencyMute","muted":true}
{"type":"prepareTimelineSnapshot","snapshot":{}}
{"type":"sendTrackMidi","trackId":"track:1","bytes":[144,60,100]}
{"type":"shutdown"}
```

音声サイドカーは安全状態で起動し、デバイスやプラグインの失敗をデータ保存の失敗と混同しません。詳細な安全動作とビルド方法は [Native audio engine](../native/audio-engine/README.md) を参照してください。

## 4. レンダーと列挙

### レンダーワーカー

レンダリングは、要求ごとに起動する専用ワーカーへ渡します。Desktop Adapterがスナップショット、時間範囲、出力先を決め、ワーカーはその計画を実行します。Rustは成功応答を受け取るまで出力を完成品として扱いません。異常終了や応答形式の不一致はジョブの失敗になり、不完全な書き出しを成功として公開しません。

### プローブとスキャン

デバイスやチャンネルの列挙は、通常の音声ストリームとは別のプロセス起動で行います。VST3の走査も専用スキャナーで実行し、結果はジョブとしてUIへ伝えます。プローブの失敗は、通常の音声セッションの状態を変更しません。

## 5. CLIとDesktop Control

CLIにはStandaloneとAttachedの二つの実行形態があります。

| 形態       | 状態の所有                                               | 接続               |
| ---------- | -------------------------------------------------------- | ------------------ |
| Standalone | CLI自身の `DataRootLease`、`SessionStore`、`AppCore<()>` | 標準入力・標準出力 |
| Attached   | DesktopのCore、履歴、Data Root、Runtime                  | Windows Named Pipe |

両形態のコマンドは同じ制御モデルへ変換されます。Standaloneはライブ音声やDesktop固有のRuntimeを持たず、AttachedはDesktopのAdapterへ依頼します。

### フレーミング

ワンショットは一つの操作を実行します。対話モードは標準入力の1行を1要求として読み、1行の応答をflushします。空行は無視します。

AttachedではCLIが受け取った要求をNamed Pipeのフレームへ変換します。Desktopは1接続内の要求を順番に処理します。フレームはUTF-8のJSONで、サイズは8 MiB以下でなければなりません。上限を超える入力や、JSONとして解釈できない入力は実行前に拒否します。

```bash
riffra --data-root ./data session get
riffra --data-root ./data --interactive
riffra --data-root ./data --attach session get
riffra --data-root ./data --attach --interactive
```

Attachedの接続は、descriptorの読込、Named Pipeへの接続、Protocol versionとDesktop instanceのhandshakeの順に成立します。descriptorが存在しても、handshakeに失敗した要求は実行しません。`--attach` の失敗をStandaloneへ自動的に切り替えることもありません。

## 6. Protocol v2

Standaloneの対話モードとAttachedの制御要求は、Protocol v2の要求と応答を使います。

```json
{
  "protocolVersion": 2,
  "requestId": "42",
  "command": "track.add",
  "expectedSequence": 18,
  "params": { "name": "Bass", "kind": "instrument" }
}
```

`requestId` は応答へそのまま返します。`expectedSequence` を指定した要求は、現在の正準シーケンスが一致するときだけ実行します。成功応答には操作後の `sequence` と結果を含めます。

```json
{
  "protocolVersion": 2,
  "requestId": "42",
  "ok": true,
  "sequence": 19,
  "result": { "type": "session", "value": {} }
}
```

### エラー

入力を受け付ける段階と、受け付けた操作を実行する段階を分けます。

| コード               | 返す状況                                                                 |
| -------------------- | ------------------------------------------------------------------------ |
| `invalidRequest`     | JSON、Protocol version、要求形式、必須パラメーター、型、コマンド名が不正 |
| `commandFailed`      | Core、Host、保存、素材の読込など、受理した操作の実行に失敗               |
| `conflict`           | `expectedSequence` と現在の正準シーケンスが一致しない                    |
| `hostUnavailable`    | Attached先のDesktopへ接続できない                                        |
| `runtimeUnavailable` | セーフモードやStandaloneなど、音声Runtimeを使えない状態                  |

```json
{
  "protocolVersion": 2,
  "requestId": "42",
  "ok": false,
  "sequence": 20,
  "error": {
    "code": "conflict",
    "message": "canonical state changed",
    "details": { "expectedSequence": 18, "currentSequence": 20 }
  }
}
```

エラーの機械判定には `code` を使います。`message` は人が読む説明であり、クライアントが文字列の内容を解釈しません。パラメーターの欠落や型の不正はCore操作の失敗ではなく、必ず `invalidRequest` として返します。

## 7. 権限と実行境界

WebViewには、Tauri命令を直接実行する権限を広く与えません。Reactは `NativeApi` のcapabilityだけを使い、低レベルの `invoke` とイベント購読は `src/native/` に閉じ込めます。

Tauriのshell権限はサイドカーの起動、標準入力への書込み、終了制御に限定します。`riffra-audio` の起動モードはserve、probe、probe-channelsへ制限し、音声デバイスとプラグインの所有者をDesktop Adapterから変更できない構成にします。

保存、正準状態、履歴、Runtimeの所有者を境界ごとに一つに決めることが、この契約の中心です。GUI、Standalone CLI、Attached CLIのどの入口から操作しても、Coreの検証とコミット順序を通過します。
