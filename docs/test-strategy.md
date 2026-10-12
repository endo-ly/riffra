# Riffra テスト戦略

## 1. 検証の構成

下層を厚く、上層ほど代表的な制作フローへ絞る。

```text
              Windows実機での受入
        実音・実機器・実VST3・画面配置

             全体を通す簡易確認
            Arrange / 保存復元

                  結合テスト
      Tauri命令・子プロセス通信・ファイル保存

                 部品テスト
           React操作・表示・状態反映

                 単体テスト
       制作規則・変換・安全条件・純粋計算
```

| 層           | 主な境界                     | 確認する内容                           | 実行頻度           |
| ------------ | ---------------------------- | -------------------------------------- | ------------------ |
| 単体         | 関数、状態遷移、音声処理部品 | 計算、変換、不変条件、異常値、安全条件 | 常時               |
| 部品         | ReactとNative API契約        | 操作、命令発行、表示、失敗、到達性     | 常時               |
| 結合         | Rust、C++、Tauri、ファイル   | 通信、保存、録音情報、索引、子プロセス | 変更領域ごと       |
| 全体簡易確認 | アプリ全体                   | 主要フローと保存復元                   | 作業単位の終端     |
| Windows実機  | OS、機器、VST3、実音         | 自動化で代替できない挙動               | マイルストーン終端 |

## 2. 基本原則

### 2.1 原因に近い層で確認する

同じ問題を複数の層で検出できる場合は、速く、原因を特定しやすい層を選ぶ。画面を通さなくても確認できる計算や状態遷移を、実機操作で繰り返さない。

ただし、データ消失、危険な音声、破壊的変更には、単体と結合など異なる境界で防御を重ねる。

### 2.2 利用者の挙動を検証する

ファイル名や内部関数の呼出し順ではなく、操作と結果を確認する。内部実装を置き換えても、期待結果が同じならテストを維持できる形を優先する。

### 2.3 維持費に見合うテストを残す

軽微な内部変更で頻繁に壊れ、重要な不具合を検出しないテストは減らす。色、余白、装飾だけの違いは原則として自動化しない。主要操作が隠れる、重なる、到達できない問題は検出対象とする。

同じ不具合を複数の層・複数のメッセージから検出できる場合は代表的な1つにまとめる。実装と同じ計算をテスト側で再現するもの、モックの戻り値や呼び出しだけを確認するもの、型システムとライブラリが保証する動作の再確認は残さない。

状態遷移は個別の状態ごとではなく、代表的な操作の流れで確認する。

## 3. Project 管理の境界

Project管理は、DataRootのファイル境界、Coreの正準状態、Hostの制御境界、Desktopの表示状態を分けて確認する。1つのテストへ全層を詰め込まず、失敗した層で原因が分かる粒度を保つ。

| 層                       | 確認する内容                                                                                                                                                                 |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `riffra-host` 単体・結合 | 新規DataRootの初期Project、UUIDによるディレクトリ分離、`workspace.json` の再起動復元、Projectごとの保存・世代回復、壊れたProjectの一覧表示と復旧候補                         |
| `riffra-core` 単体       | Project activationがCanonical Sessionを交換し、Undo/Redoを消去し、SessionStorageへ再保存しないこと。正準状態・sequence・履歴は1つのCommitシナリオで確認する                  |
| `riffra-runtime` 結合    | `project.list/create/open/rename/import/export` の結果、Active ProjectとCanonical Sessionの一致、録音中の拒否、切替完了時のProjectActivated、Importによる既存Project非上書き |
| Desktop部品              | Project一覧、新規作成、切替、改名、Import / Exportの命令発行、`.riffra` picker、Export保存先通知、`Opening...` 中の操作無効化、切替失敗時の旧Project維持、Host切替との独立性 |
| CLI                      | StandaloneとAttachedで同じProject command契約を使い、一覧・作成・切替・改名、Import / Exportを実行できること                                                                 |

Project切替テストでは、旧ProjectのUndo履歴や表示選択が新Projectへ持ち越されないことも確認する。保存処理のテストでは、Project Aの変更がProject Bの `session.json` と `generations/` に影響しないことを確認する。

MixerのCoreテストでは、Bus・Sendの参照と値域、チャンネルをまたぐDevice IDの一意性、参照中のBus削除拒否、Trackの複製・削除に伴うSendの所有関係を確認する。保存・再読込とUndo/Redoはエフェクト状態を含む編集フローで確認し、保存失敗時は正準状態と履歴が変わらないことを保証する。Runtimeの一括適用テストでは、同じBatchで作成したBus・Returnを名前で参照し、全体を1回のコミットとUndoで扱えることを確認する。

Project packageのテストでは、Importが現在のProjectを上書きせず新しいProjectとして追加すること、ExportがSave dialogで指定された絶対パスを成功通知へ含めることを確認する。Renderのテストでは、成果物が `DataRoot/renders/` に作られ、`DataRoot/exports/` を作成しないことを確認する。

## 4. 実行グラフ契約

Rust が生成する契約フィクスチャと C++ の厳密デコーダを同じファイルで検証し、両言語間のフィールド名・型・範囲を固定する。フィクスチャを手で編集せず、Rust の鮮度テストと C++ のデコードテストから契約のずれを検出する。

| 層                               | 確認する内容                                                                                                                                                   |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `riffra-runtime`                 | 資源解決と純粋な投影を分け、実行グラフと投影診断を構築する。Presentation-only の変更では投影比較が一致する                                                     |
| Coordinator / Machine            | 同一 Project ID・実行グラフ・診断・音声環境を持つ正準要求は準備を省略し、同一投影は新しい正準キーを採用する。接続・非同期・最終状態を1つのシナリオで確認する   |
| `riffra-audio` / `riffra-render` | 必須・未知キー、型、enum、範囲をデコードし、契約違反を拒否する。代表的なオブジェクト階層で欠落・未知キーを確認する。Master Gain はグラフのコミットで適用される |
| Desktop                          | 現役投影の診断を `RuntimeProjectionStatus` から表示し、Transport 状態に依存しない                                                                              |

契約変更では `contracts/execution-graph/` の Rust 生成フィクスチャを更新し、C++ の正常デコードと欠落・未知・範囲外入力の拒否を確認する。

## 5. サイドカー通信契約

Rust と `riffra-audio` / `riffra-render` の間の全メッセージを、送る側が生成し受け取る側が検証するフィクスチャで固定する。フィクスチャは `RIFFRA_UPDATE_CONTRACT_FIXTURES=1` のときだけ書き換わり、それ以外は生成結果との一致を検査する。

| 方向                     | 生成する側                                                 | 検証する側                                                                                                                      |
| ------------------------ | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| 命令（Rust → C++）       | Rust の全 `SidecarCommand` → `contracts/sidecar/commands/` | C++ が受け付ける全命令にフィクスチャがあり、すべて厳格にデコードできる                                                          |
| メッセージ（C++ → Rust） | C++ の全エンコーダ → `contracts/sidecar/messages/`         | Rust の全応答・イベント・レンダー結果・デバイスプローブ・プラグインスキャンにフィクスチャがあり、それぞれの厳密な型として読める |

- 全 Sidecar Command に Rust Fixture があり、C++ がすべて正常にデコードできることを確認する。不正入力は代表的な Command とオブジェクト階層を使い、必須フィールド欠落、未知フィールド、不正型、不正 MIDI Bytes の拒否を確認する
- 応答の単一性（未応答の破棄で `noResponse`、2 回目の応答は出力しない）は C++ の `CommandResponder` で、期待と異なる応答型・解釈できない行による即時失敗は Rust の `CommandBus` とプロトコル処理で確認する
- 実プロセスとの往復は `apps/cli` の `headless_host_process` テストで確認する。Linux では Safe Mode、Windows では実サイドカーを使う通常モードを実行し、同一内容を2環境で繰り返さない

契約変更では両方向のフィクスチャを再生成し、差分をレビューの対象にする。

## 6. 音声エンジンの並行性

音声スレッドと制御スレッドの境界は、決定的な単体テストと ThreadSanitizer で確認する。

- 命令の適用は、音声側の所有に切り替えた Engine へ命令を積み、ブロックの開始を 1 回呼んで `RealtimeFrame` を確認する。同じブロックに重なった Stop とカウントイン終了、Seek、キュー満杯、グラフの公開前後のステータス、退役したグラフだけの回収、回収スレッドの限定、共有デバイスの同一性を対象にする
- トラックワーカー0・1・7で、各トラックがブロックごとにちょうど1回処理されることを確認する。オーディオクリップ、内蔵音源、オートメーション、遅延補正を含む固定グラフをワーカー0と7でオフラインレンダーし、WAVのビット一致を確認する
- プレビューのキュー満杯は制御側へのエラー、録音 sink とプレビューの資源は退役後の回収を確認する。診断は1秒の窓ごとの計算と、トラック負荷のグラフ順を確認する
- ThreadSanitizer は実際に複数スレッドが動くケース、オブジェクトの公開・解放が競合するケース、リアルタイムコールバックと制御処理が競合するケースだけを対象にする。数値計算・MIDI内容・JSON形式・音声結果の一致だけを確認するテストは対象外とする
- 対象はスイートとケースを明示して指定する。ファイル単位の広い正規表現は使わない

```text
BoundedMpmcQueueTest.*
RealtimeCommandQueueTest.*
SeqLockFrameTest.*
TrackProcessingPoolTest.*
RecordingCaptureRuntimeTest.*
TimelineRealtimeTest.*
PreviewEngineTest.*
TimelineEngineTest.KeepsAudioCaptureOpenForTheWholeAudioCallback
TimelineEngineTest.StreamsOfflineProcessingForASingleLongRecordingSegment
TimelineEngineTest.ProcessesAnInstrumentRuntimeOncePerTransportChunk
TimelineEngineTest.ProcessesAnAudioEffectChainOncePerTransportChunk
TimelineEngineTest.KeepsCanonicalTrackStateWhenDeviceRuntimeIsReused
TimelineEngineTest.SendsEmergencyPanicToTheInstrumentRuntime
AudioRenderPipelineTest.EmergencyMutePanicsAuditionedInstrumentBeforeUnmuting
AudioRenderPipelineTest.PreviewControlUpdateKeepsAudioCallbackAvailable
AudioRenderPipelineTest.ConsumesPreviewCommandsUnderEverySafetyMute
AudioProtocolTest.ControlOutputDiscardsQueuedTelemetry
```

明示的なCTest選択では39ケースを実行し、列挙した並行性スイートの全ケースを含める。

- 抑制ファイル `native/audio-engine/tsan.supp` には JUCE 内部に起因する報告だけを理由付きで書く。Riffra のコードに対する報告は修正し、抑制しない

## 7. 音声・録音・レンダーの保証

| 領域           | 確認する内容                                                                                                                                                                                   |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 音源とミックス | 44.1 kHz モノと 48 kHz ステレオの混在、Bypass 時の Dry Signal、モノ→ステレオ、Feedback 保護と Emergency Mute の解除、Mute Reason ごとの独立解除                                                |
| Preview / MIDI | Voice の再利用と空き Voice の優先、上限到達時の最古 Voice 置換、Key 単位の停止、`process()` 前の `prepareToPlay`、MIDI 容量上限、Panic Controller                                              |
| 録音           | Raw / Processed の保全、Finalization 失敗時の Recoverable、中断 WAV の復元、ループ録音のセグメント境界、Pack ごとの Raw/Processed 整合、Prepared Block Size の尊重、長時間録音のストリーミング |
| Plugin         | 処理順序、指定 Device だけの Bypass、Parameter と Persisted State の適用、Latency / Tail の集計、実 VST3 のロードとアンロード                                                                  |
| レンダー       | 空範囲の拒否、Shared PDC の維持、Render 失敗時の一時ディレクトリ整理、正規化結果、MIDI Session を持たない Asset Source を生成しないこと                                                        |

録音は、ループ・Punch・複数 Track ごとのセグメント境界とファイル構造を `TimelineRecordingTest` と `RecordingSessionTest` / `ArrangeRecordingSessionTest` で確認する。1回の録音完了シナリオで音声ファイルと Manifest の両方を検証する。

Mixerは`riffra-test-effect`を使い、異なるEffectレイテンシのTrack・Group・Return・Masterが合流しても重複加算や時間ずれがないこと、Mute・加算的Solo・Pre/Post分岐、ミックス設定変更時のDevice再利用をNativeで確認する。Bus・MasterのParameter・Bypass・保存状態は同じDevice APIを通して検証し、Rustでは正準保存と再読込まで確認する。

Renderの結合テストは実Native workerでGroup・Return・Master FXを通した全体MixとStemを作り、フレーム数、線形FXでの加算一致、Manifestの測定値を確認する。Float WAVの0 dBFS超のSample PeakとFFmpegのラウドネス測定・二段階Masteringは完成ファイルで確認する。FFmpegの測定不能をゼロ値や通常Render失敗へ置き換えない。

DesktopはMixerのチャンネル作成、主出力・Send設定、フェーダーの一度だけの確定、Bus・MasterのDevices操作とMastering設定・Job結果表示を操作シナリオで検証する。数値・保存・DSPの検証は下層に置き、ブラウザーでは名前、フェーダー、固定Master、ダイアログへの到達と配置を確認する。

## 8. CI の構成

| ジョブ                 | 実行内容                                                                                                       |
| ---------------------- | -------------------------------------------------------------------------------------------------------------- |
| Node                   | `format:check` / `lint` / `knip` / `typecheck` / `test`。本番 Vite ビルドは `main` 更新時と手動実行のみ        |
| Rust host crates       | `cargo fmt --all --check`、clippy、workspace テスト（Tauri を除く）、Safe Mode の Headless Host プロセステスト |
| Rust Tauri             | 空の bundle 設定で clippy とテスト。パッケージングと Sidecar 実行は Native ジョブで確認する                    |
| Native audio (Linux)   | ネイティブビルド、変更された C++ の clang-format、ThreadSanitizer                                              |
| Native audio (Windows) | 実サイドカーによる通常モードの Headless Host プロセステスト、Sonalloy 実音比較、ネイティブテスト               |

- 配布対象は Windows であり、macOS 向けの CI は実行しない。macOS 用の C++ 実装とクロスプラットフォーム対応は保持する
- Sidecar は CMake install で生成・配置し、install が失敗すればビルドを失敗させる。実行ファイルの存在確認だけを行う独立 Step は設けない
- 実音・実 VST3 を使うテストと Slow Test は通常の PR 実行から外し、`main` 更新時と手動実行で実行する

### 8.1 Slow Test

| テスト                                                                | 内容                                | 実行                  |
| --------------------------------------------------------------------- | ----------------------------------- | --------------------- |
| `SonalloyInstrumentRuntimeTest.CompilesAndPlaysEveryReleasedPreset`   | 全プリセットのコンパイルと発音      | main 更新時・手動実行 |
| `TimelineEngineTest.StreamsALongLoopRecordingWithoutPreallocatingRam` | 130 Pass のループ録音ストリーミング | main 更新時・手動実行 |

各テストはCTestへ1件ずつ `slow` ラベルで登録する。ビルドスクリプトは環境変数が厳密に `RIFFRA_RUN_SLOW_TESTS=1` の場合だけこのラベルを実行し、長時間ループケースも同じ値以外はSkipする。

### 8.2 Headless Host プロセステスト

`apps/cli/tests/headless_host_process.rs` は `RIFFRA_HEADLESS_SAFE_MODE` でモードを切り替える。

| ジョブ                 | モード     | 確認する内容                                                               |
| ---------------------- | ---------- | -------------------------------------------------------------------------- |
| Rust host crates       | Safe Mode  | Host・CLI プロセスの起動、Endpoint 公開、Attached コマンド、終了後の片付け |
| Native audio (Windows) | 通常モード | 実サイドカーを使った Host・CLI プロセス往復                                |

Local Transport、ソケット権限、接続・再接続は Rust の通常テストで保証する。
