# Riffra テスト戦略

## 1. 目的

この文書は、Riffraの変更を十分な確度で検証しながら、ビルド時間、実機操作、調査時間を必要以上に増やさないための方針を定めます。

テストの目的は、テスト件数や網羅率そのものを増やすことではありません。ユーザーに影響する不具合を、原因に近く、速く、再現可能な方法で検出することです。高価な検証は、それでしか確認できない挙動に限定します。

## 2. テスト構成の全体像

検証はコードのレイヤーではなく「依存の境界」を単位とし、下層を厚く、上層ほど対象を絞るピラミッドとします。

```text
            Native実機受入  (Windows / Audio / MIDI / VST3)
                     少数・バッチ単位

                 End-to-End Smoke  (WebdriverIO, 主要フロー)

                    Integration
         Tauriコマンド契約 / Rust↔sidecar プロトコル / Filesystem

                    Component  (React + FakeNativeApi)

                       Unit
         Domain / 変換 / 安全条件 / Rust / TS / C++(juce::UnitTest)
                     多数・常時実行
```

| 層                       | 境界                                        | 主な検証対象                                                                                        | フレームワーク / 入口                                  |
| ------------------------ | ------------------------------------------- | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| 単体 Unit                | Rust純ロジック・C++内部                     | 状態遷移、純計算、安全条件、sidecar応答のパース・マッピング、DC blocker、feedback検出、rack状態遷移 | Rust `cargo test` / TS `vitest` / C++ `juce::UnitTest` |
| コンポーネント Component | React ↔ Rust 縫合                           | 表示整合、操作→コマンド発行、状態反映、Empty/Error/Recovery                                         | React + `FakeNativeApi`（vitest）                      |
| 結合 Integration         | Rust ↔ sidecar / Tauriコマンド / Filesystem | プロトコル契約、コマンドDTO、保存・Job・Manifest整合                                                | 実sidecar spawn / `tauri::test` / 一時FS               |
| E2E Smoke                | 全体                                        | 主要フロー5本                                                                                       | WebdriverIO + `@wdio/tauri-service`                    |
| Native実機               | Windows / Device                            | 実音、ASIO/MIDI/VST3、Window配置                                                                    | 実機操作                                               |

本プロジェクトの3境界は次のとおりです（詳細は§4）。

```text
React (UI)
   │  Tauri IPC  (JSON over invoke)
   ▼
Rust (Tauri command / アプリケーション層)
   │  Sidecar起動 + JSON Lines  (stdin/stdout, 子プロセス境界・FFI/unsafeなし)
   ▼
riffra-audio.exe (JUCE / C++)
   └─ 純ロジックは juce::UnitTest で単体検証
```

## 3. 基本原則

### 3.1 最も低い層で確認する

同じ不具合を複数の方法で検出できる場合は、実行が速く、失敗原因を特定しやすい方法を選びます。層の割り当ては§2の表に従います。上位のテストで下位の詳細を重複して確認しません。ただし、データ消失、意図しない音声出力、破壊的変更など重大なリスクには、異なる層で防御を重ねます。

### 3.2 変更単位ではなく、挙動単位で検証する

ファイルや関数ではなく、ユーザーから見た挙動を検証単位とします。内部実装を置き換えても期待結果が変わらないテストを優先します。

### 3.3 修正には再現テストを添える

自動化可能な不具合を修正するときは、原則として修正前に失敗し、修正後に成功するテストを追加します。同じ種類の不具合を再び手作業で探さないためです。

実機依存で自動化できない場合は、確認対象、操作、期待結果、観測した証拠を挙動確認・課題管理表へ記録します。

### 3.4 実機確認を小さな修正ごとに行わない

Native buildと画面操作は高価なため、関連する挙動をまとめた検証バッチの終端で行います。途中では低い層のテストを使い、バッチ内の自動テストが通るまでは実機確認へ進みません。

### 3.5 テストの維持費も評価する

実装の内部構造に強く依存し、軽微な変更で頻繁に壊れるテストは減らします。重要度が低く、失敗しても容易に発見できる表示細部は、自動化しない判断も許容します。

## 4. 各層の詳細

### 4.1 単体テスト (Unit)

外部プロセス、実デバイス、実ファイルシステムへ依存しない純ロジックを対象とします。

- **Rust**: Scratch Session / Project / Track / Clip / Rack / Snapshotの状態遷移、Undo/Redo、Manifestの読込・正規化・Migration、Render / Analysis / Separationの純計算、安全条件（gain / pan / fade / limiter / 異常値）、sidecar応答のパースとマッピング（`parse_native_line`、`native_status_to_audio_status`、`render_native_error`、`sidecar_restart_required`）
- **TypeScript**: domainモデル、recording / rack / plugin-session等の純関数
- **C++（juce::UnitTest）**: DCBlocker、FeedbackDetector、PluginRack状態遷移、RecordingSession、プロトコルコマンドのパース

実装行をなぞるのではなく、入力、結果、不変条件を確認します。Private関数を直接テストするためだけの公開化は避けます。

### 4.2 コンポーネントテスト (Component)

ReactをNative Runtimeから切り離し、ユーザー操作と表示結果を確認します。Native APIは `FakeNativeApi` へ差し替え、成功・失敗・遅延・部分成功を決定的に再現します。

- Workspace、Library、Inspector、Transport間の表示整合性
- 操作からコマンドが発行されること、結果がSessionと画面へ反映されること
- Empty、Loading、Error、Disabled、Recovery状態
- 要求値と実効値が異なる場合の表示
- 操作失敗を成功扱いしないこと
- Keyboard操作と主要なAccessibility Name

色、余白、細かな座標は原則として対象外とし、要素が到達不能になる・重なる・消えるなど操作成立に影響する配置は Native 実機受入で確認します。

### 4.3 結合テスト (Integration)

複数コンポーネントまたはプロセス境界の契約を対象とします。実デバイスを必要としない構成を優先します。

#### 4.3.1 Rust ↔ sidecar プロトコル結合

実際の `riffra-audio.exe` を起動し、stdinへコマンドを書き、stdoutのJSON Linesをアサートします。モックで代用しないこと（起動・パイプ・シリアライズ・`requestId`照合・再起動を通すことが目的だからです）。

- エンジン起動と応答、`shutdown`
- `status` 往復と `requestId` 照合
- エラーscope区分（audioDevice → fault / その他 → command failure）
- 再生・一時停止・シーク・再開・停止の状態遷移
- 不正な状態遷移、存在しないファイル、日本語を含むパス
- C++エラーからRustエラーへの変換
- 出力の NaN / Inf 非含有、peak / チャンネル数
- 破棄後にコールバックが残らないこと
- sidecar異常終了時の再起動（lost transport 検知）

#### 4.3.2 Tauriコマンド契約

`tauri::test::mock_builder()` / `get_ipc_response()` 等を使い、Rust側コマンド層をネイティブWebViewなしに検証します。

このテストがWindows環境で起動できない場合は、`tauri::test`全般の仕様とは断定せず、現在のテスト構成で発生した環境依存の事象として扱います。WebView2ランタイムと`WebView2Loader.dll`は別の配布物であり、実行プロセスとローダーのアーキテクチャ・バージョンを一致させます。`cargo test --features ipc-integration --no-run`はコンパイル確認に限られ、IPC実行確認の代わりにはなりません。

通常の回帰テストは`cargo test --lib`で常時実行し、IPC実行テストは一致するWindows環境を用意した専用ジョブで実行します。ローダーDLLはリポジトリへ追加せず、入手元と配置をスクリプトまたはCI設定で固定します。

実行時は、対象アーキテクチャのDLLを明示して`powershell -ExecutionPolicy Bypass -File scripts/test-ipc.ps1 -LoaderDll C:\path\to\WebView2Loader.dll`を使います。DLLが用意できない環境では`--no-run`までを確認し、実行済みとは扱いません。

- コマンドが登録されていること
- JSON引数がRust DTOへデシリアライズできること
- Managed Stateが渡されること
- 戻り値・エラーが正しくシリアライズされること
- コマンドからユースケースが呼ばれること

Tauriコマンドは「変換と委譲」に限定し、主要ロジックは4.1の単体テストで検証します。

#### 4.3.3 Filesystem / Job / 永続化

- Tauri CommandとSession保存の接続、Autosave / Recovery / Project入出力
- Recording ManifestとRaw / Processed / MIDIファイルの整合性
- Library IndexとFilesystemの同期
- Plugin scannerとのJSON Linesプロトコル、隔離 / Quarantine / Missing復元

Filesystemを使う場合はテスト専用の一時ディレクトリを使い、ユーザーのAppData、VST3フォルダ、制作ファイルへ書き込みません。

### 4.4 End-to-End Smoke (E2E)

主要なユーザーフローが層をまたいで接続されていることを、少数の代表シナリオで確認します。WebdriverIO + `@wdio/tauri-service` で実ビルドバイナリを操作し、実デバイスは使わずNull / offline構成とします。すべての分岐を網羅する場所にはしません。

1. アプリが起動し、JUCEエンジンが初期化される
2. テスト音源を開くとメタデータがUIに表示される
3. 再生操作でRustとJUCEの状態が `playing` になる
4. シーク・一時停止・停止が一連で動く
5. 不正ファイルを開くとRust / JUCEのエラーがUIに表示される

### 4.5 Native実機受入

Windows実機でしか判断できない事項を対象とします。

- WASAPI、ASIO、実Audio Interfaceの列挙と切替
- 実際の入力、出力、Mute、Fade、Latency、Dropout
- MIDI Deviceの接続、切断、Panic
- 実VST3のScan、Load、Editor、State復元、Crash隔離
- WebViewを含むWindow配置、Scroll、Focus、Dialog
- Process終了、Sidecar孤立、File Lock、再起動
- 実際に聞こえるノイズ、クリック、Feedback、音量変化
- OS権限、長いPath、Unicode、外部Deviceの抜き差し

## 5. 不具合修正の完了条件

不具合修正は、コードを書き終えた時点では完了としません。次を満たした時点で修正済みとします。

- 原因と影響範囲が説明できる
- 自動化可能なら再現テストがある
- 修正後に対象テストが成功する
- 関連する既存テストが成功する
- 保存データと外部Processを壊していない
- 実機依存ならNative実機で期待結果を観測している
- 挙動確認・課題管理表に証拠が記録されている

Build成功、型検査成功、画面に要素が存在することだけでは、ユーザー挙動の成立を意味しません。

## 6. 自動化しない判断

次の条件をすべて満たすものは、手動確認に留められます。

- 発生してもデータ、安全性、音量、互換性へ影響しない
- 変更頻度が低い
- 自動化の維持費が高い
- 目視ですぐ発見できる
- Release前の短い確認で十分に再現できる

一方、次は原則として自動化します。

- データ消失または破損につながる
- 音声出力の安全性に関わる
- 失敗を成功として表示する
- 保存と再起動で状態が変わる
- 過去に再発した
- 入力の組合せが多く、手作業で見落としやすい

## 7. Test Doubleとテスト用入口

外部依存は、製品コードと同じ契約を実装するTest Doubleへ差し替えられる構造にします。

| 依存          | Test Double                                                            |
| ------------- | ---------------------------------------------------------------------- |
| Audio Runtime | FakeNativeApi（Offline / Ready / Muted / Faulted / 設定不採用 / 切断） |
| Plugin Host   | Fake（Load成功 / 検証失敗 / Crash / Missing / State復元）              |
| MIDI          | Fake（Port列挙 / Note / 切断 / Panic）                                 |
| Filesystem    | 一時ディレクトリ（保存成功 / 容量不足 / Lock / 破損Manifest）          |
| Job           | Fake（完了 / 進行 / 取消 / Timeout / 部分成果）                        |
| AI Provider   | Fake（無効 / 応答 / 拒否 / 外部送信確認）                              |

Test Doubleは本番にない成功経路を作るためではなく、実際に起こり得る応答を決定的に再現するために使います。

Rust ↔ sidecar の結合テスト（4.3.1）だけは例外的にTest Doubleを使いません。プロトコル契約を確かめるため、実バイナリと通信します。Rust側の純粋なパース・マッピングは4.1の単体テストでFake不要に検証済みです。

## 8. テストデータ

テストデータは小さく、決定的で、再生成可能にします。

- 短いPCM WAV、MIDI、ManifestをFixtureとして管理する
- 時刻、乱数、IDは注入または固定可能にする
- 第三者VST3本体やユーザー制作物をFixtureへ含めない
- 大容量Audioと長時間試験は通常テストから分離する
- 生成物は一時ディレクトリへ置き、成功・失敗の両方で片付ける

音声比較では、必要に応じてSample数、Peak、RMS、Hash、許容誤差を使います。浮動小数点処理へ完全一致を要求しません。

## 9. 管理と見直し

挙動要件ごとの成立状況と証拠は [挙動確認・課題管理表](./behavior-verification.md) で管理します。この文書には個別不具合、現在の件数、一時的な進捗を書きません。

テストを追加するときは、どの不具合またはリスクを検出するのかを明確にします。役割が重複するテスト、長時間かかる割に検出力が低いテスト、実装変更だけで壊れるテストは定期的に整理します。

この戦略は、製品構造、主要なリスク、開発速度が変わったときに見直します。単にテスト件数を増やす目的では変更しません。
