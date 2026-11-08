# Riffra MCPサーバー

`riffra-mcp` は、起動中の Riffra Host を Model Context Protocol (MCP) のstdio経由で操作するサーバーである。正準状態、履歴、音声、プラグイン、Jobの処理はHostが所有し、MCPサーバーはツール定義、入力の解釈、Control要求の転送、結果の整形だけを担う。DesktopとCLIが使うControl Commandをそのまま呼び出すため、MCPから編集した内容はGUIへ即座に反映される。

## 利用の前提

MCPサーバーはHostを自動起動しない。先にDesktopを起動するか、`riffra --data-root <path> serve` でLive Hostを起動しておく。MCPサーバーとHostは同じOSユーザーのローカル環境で動かす。WindowsのHostへ接続する場合はWindows用の`riffra-mcp.exe`を使う。

サーバーは起動時にHostを1つ選び、そのinstance IDへ接続を固定する。Hostが終了した場合は`hostUnavailable`を返し、別のHostへ自動で切り替わらない。接続先を変えるときはMCPサーバーを再起動する。

| 状況                        | 動作                                         |
| --------------------------- | -------------------------------------------- |
| Hostが1件                   | そのHostへ接続する                           |
| Hostが0件                   | `no Riffra Host is running` で起動に失敗する |
| Hostが複数件                | 候補のinstance IDを表示して起動に失敗する    |
| `--host <instance-id>` 指定 | 指定したHostへ接続する                       |

## 起動

開発時はワークスペースのルートで実行する。

```powershell
cargo run -p riffra-mcp -- --host <instance-id>
```

`--host` を省略できるのは、接続できるHostが1件だけのときである。候補は `riffra host list` で確認する。

インストール済みのRiffraでは、`riffra.exe` と同じディレクトリ(既定 `%LOCALAPPDATA%\Riffra`)に `riffra-mcp.exe` が配置される。MCPクライアントには実行ファイルの絶対パスを指定する。

```json
{
  "mcpServers": {
    "riffra": {
      "command": "C:\\Users\\<user>\\AppData\\Local\\Riffra\\riffra-mcp.exe",
      "args": ["--host", "<instance-id>"]
    }
  }
}
```

`--host` はHostが複数ある場合だけ必要である。MCP通信には標準出力を使い、ログと接続エラーは標準エラーへ出す。起動に失敗した場合はクライアントのログまたは直接実行したときの標準エラーを確認する。

## ツールの呼び方

MCPツールは操作対象ごとにグループ化され、複数のControl Commandを持つツールは `action` で命令を選ぶ。`params` はControl CommandのcamelCase JSONと一致する。ツールの入力スキーマは各actionの `params` 型から生成され、未知のaction、未知のフィールド、必須値の欠落はHostへ送る前に拒否される。

```json
{
  "action": "insert",
  "params": {
    "clipId": "midi-clip:01...",
    "notes": [{ "pitch": "C4", "position": "1:1", "duration": "1/8" }]
  },
  "expectedProjectId": "project:01...",
  "expectedSequence": 42
}
```

`expectedProjectId` と `expectedSequence` は要求の事前条件であり、`params` には含めない。Projectに属する命令で `expectedProjectId` を省略すると、Hostの現在のactive Projectがその要求だけに適用される。複数の操作で同じProjectを編集する場合は、応答のsequenceを次の `expectedSequence` へ渡し、必要ならProject IDも引き回す。競合が返った場合は自動再送しない。

複数の変更を一度に確定する場合は `session_edit` の `apply` を使う。`operations` にはControl Command名とその `params` を並べ、TrackとClipは `trackName`/`clipName`、Deviceは `bind`/`@name` で参照できる。`operations[].command` に指定できるのは `session.apply` がバッチ実行できるCommandだけで、入力Schemaの `enum` に列挙される。全operationが成功したときだけ1回の変更として適用される。

通常の状況把握には `session_read` の `inspect` を使う。`session.get` は正準Session全体が必要な場合だけ使う。音符やクリップの位置は、通常は音楽座標(`1:1`、`1/8`、`C4`)を使い、tickやMIDI値は必要な場合だけ扱う。音名の臨時記号はASCIIの`#`/`b`のほか、`♯`/`♭`/`𝄪`/`𝄫`も使える。

## 応答

成功時は `structuredContent` にControl Command名、sequence、結果種別、結果値が入る。生成されたID、Render Job ID、変更内容は結果値に残る。

```json
{
  "command": "music.note.insert",
  "sequence": 43,
  "result": {
    "type": "mutation",
    "value": { "createdEntityIds": { "notes": ["note:01..."] } }
  }
}
```

`session.get` や `host.bootstrap` のように正準状態全体を返す命令は結果を縮小しない。通常のMutationは全Sessionを含めず、生成IDとsequenceだけを返す。

失敗時は `isError: true` とし、`structuredContent.error` にHostの `code`、`message`、`details` をそのまま残す。主なコードは次のとおり。

| code                 | 意味                                      |
| -------------------- | ----------------------------------------- |
| `invalidRequest`     | 入力または要求が不正                      |
| `conflict`           | Projectまたはsequenceが要求時から変化した |
| `commandFailed`      | コマンドの実行に失敗した                  |
| `runtimeUnavailable` | Safe ModeなどでRuntime機能が使えない      |
| `hostUnavailable`    | Hostへ接続できない、またはHostが終了した  |

存在しないツール名など、要求をツールへ振り分けられない失敗はJSON-RPCのプロトコルエラーになる。ツールが見つかった後の入力不正や実行失敗はツール結果のエラーになる。

## Job

Render、ステム、分析の測定、非同期スキャンはJob IDを返す。`job` ツールの `get` で状態を読み、`cancel` で中止する。`wait` は `get` を100ms間隔で参照し、`completed`、`failed`、`cancelled` のいずれかで終了する。`timeoutMs` を指定すると待機だけを打ち切る。待機のタイムアウトやMCPクライアントからのキャンセルはJob本体を中止しないため、中止が必要なら `cancel` を呼ぶ。

`wait` が成功するのはJobが `completed` になったときだけである。`failed`、`cancelled`、Jobが見つからない場合、タイムアウト、Host切断はエラーとして返す。

## 配布

Windowsのデスクトップ配布物では、`riffra-mcp.exe` をNSISインストーラーへ同梱する。ビルド時に `binaries/riffra-mcp-<target-triple>.exe` をステージングし、Tauriの `externalBin` として配置する。MCPサーバーはDesktopプロセスから自動起動されない。利用者がMCPクライアントへ実行ファイルを登録する。

MCPサーバーはHTTPを公開せず、Resources、Prompts、OAuth、独自のProject状態、独自のUndo履歴を持たない。Hostが提供するControl Commandの範囲を超える操作は行わない。
