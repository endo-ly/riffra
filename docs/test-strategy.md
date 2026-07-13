# Riffra テスト戦略

## 1. 目的

この文書は、Riffraの変更を十分な確度で検証しながら、ビルド時間、実機操作、調査時間を必要以上に増やさないための方針を定めます。

テストの目的は、テスト件数や網羅率そのものを増やすことではありません。ユーザーに影響する不具合を、原因に近く、速く、再現可能な方法で検出することです。高価な検証は、それでしか確認できない挙動に限定します。

## 2. 基本原則

### 2.1 最も低い層で確認する

同じ不具合を複数の方法で検出できる場合は、実行が速く、失敗原因を特定しやすい方法を選びます。

- 計算、変換、状態遷移はUnit Testで確認する
- UIとアプリケーション状態の接続はComponent Testで確認する
- Process、Filesystem、Protocolの接続はIntegration Testで確認する
- Windows Device、VST3実体、音、表示配置はNative実機で確認する

上位のテストで下位の詳細を重複して確認しません。ただし、データ消失、意図しない音声出力、破壊的変更など重大なリスクには、異なる層で防御を重ねます。

### 2.2 変更単位ではなく、挙動単位で検証する

ファイルや関数ではなく、ユーザーから見た挙動を検証単位とします。内部実装を置き換えても期待結果が変わらないテストを優先します。

### 2.3 修正には再現テストを添える

自動化可能な不具合を修正するときは、原則として修正前に失敗し、修正後に成功するテストを追加します。同じ種類の不具合を再び手作業で探さないためです。

実機依存で自動化できない場合は、確認対象、操作、期待結果、観測した証拠を挙動確認・課題管理表へ記録します。

### 2.4 実機確認を小さな修正ごとに行わない

Native buildと画面操作は高価なため、関連する挙動をまとめた検証バッチの終端で行います。途中では低い層のテストを使い、バッチ内の自動テストが通るまでは実機確認へ進みません。

### 2.5 テストの維持費も評価する

実装の内部構造に強く依存し、軽微な変更で頻繁に壊れるテストは減らします。重要度が低く、失敗しても容易に発見できる表示細部は、自動化しない判断も許容します。

## 3. テスト構成

Riffraでは、厳密な件数比率を目標にしません。下層を厚くし、上層ほど対象を絞るテストピラミッドを採用します。

```text
                    Native実機受入
             Windows / Audio / MIDI / VST3
                   少数・バッチ単位

                 End-to-End Smoke
              主要フローとProcess境界

                  Integration Test
          Tauri Command / Sidecar / Filesystem

                  Component Test
             React表示 / 操作 / 状態接続

                     Unit Test
       Domain / Reducer / 変換 / 安全条件 / DSP
                 多数・常時実行
```

### 3.1 Unit Test

外部Process、実Device、実Filesystemへ依存しない処理を対象とします。

主な対象は次のとおりです。

- Scratch Session、Project、Track、Clip、Rack、Snapshotの状態遷移
- Undo/Redo、非破壊編集、範囲計算、時間変換
- Manifestの読込、正規化、検証、Migration
- Recording、Render、Analysis、Separationの純粋な計算部分
- Gain、Pan、Fade、Limiter、異常値処理などの安全条件
- エラー分類、Fallback選択、表示用View Model
- AI ChangeSetの権限、差分、適用、取消

Unit Testでは実装行をなぞるのではなく、入力、結果、不変条件を確認します。Private関数を直接テストするためだけの公開化は避けます。

### 3.2 Component Test

ReactコンポーネントをNative Runtimeから切り離し、ユーザー操作と表示結果を確認します。Native APIは成功、失敗、遅延、部分成功を返すFakeへ差し替えます。

主な対象は次のとおりです。

- Workspace、Library、Inspector、Transport間の表示整合性
- Button、入力、選択操作からCommandが発行されること
- Command結果がSessionと画面へ正しく反映されること
- Empty、Loading、Error、Disabled、Recovery状態
- 要求値と実効値が異なる場合の表示
- 操作失敗を成功扱いしないこと
- Keyboard操作と主要なAccessibility Name

色、余白、細かな座標は原則として対象外です。要素が到達不能になる、重なる、消えるなど操作成立に影響する配置は実機受入で確認します。

### 3.3 Integration Test

複数ComponentまたはProcess境界の契約を対象とします。実Deviceを必要としない構成を優先します。

主な対象は次のとおりです。

- Tauri CommandとSession保存の接続
- Autosave、世代管理、Recovery、Project入出力
- Recording ManifestとRaw/Processed/MIDIファイルの整合性
- Library IndexとFilesystemの同期
- Audio sidecar、Plugin scannerとのJSON Lines Protocol
- Child Processの起動、終了、Timeout、異常終了
- Plugin隔離、Quarantine、Missing Pluginの復元

Filesystemを使う場合はテスト専用の一時ディレクトリを使い、ユーザーのAppData、VST3フォルダ、制作ファイルへ書き込みません。

### 3.4 End-to-End Smoke Test

主要なユーザーフローが層をまたいで接続されていることを、少数の代表シナリオで確認します。すべての分岐を網羅する場所にはしません。

代表シナリオは次の観点から選びます。

- 起動してScratch Sessionを利用できる
- 音声設定またはFake Audio Runtimeを接続できる
- Pluginまたは内蔵音源をRackへ追加し、状態を保存できる
- 録音または既存AudioをArrangeへ配置し、Renderできる
- 終了と再起動を経ても作業を復元できる
- 失敗時にデータを保持し、復旧方法を表示できる

実Deviceを使わず成立するシナリオは自動化し、Hardwareや第三者Pluginが必要なシナリオだけNative実機へ残します。

### 3.5 Native実機受入

次のように、Windows実機でしか判断できない事項を対象とします。

- WASAPI、ASIO、実Audio Interfaceの列挙と切替
- 実際の入力、出力、Mute、Fade、Latency、Dropout
- MIDI Deviceの接続、切断、Panic
- 実VST3のScan、Load、Editor、State復元、Crash隔離
- WebViewを含むWindow配置、Scroll、Focus、Dialog
- Process終了、Sidecar孤立、File Lock、再起動
- 実際に聞こえるノイズ、クリック、Feedback、音量変化
- OS権限、長いPath、Unicode、外部Deviceの抜き差し

computer-useはこの層で使用します。自動テストで確認済みの内部計算を、画面上でもう一度細かく検証する用途には使いません。

## 4. 検証方法の選び方

新しい挙動または不具合を扱うときは、次の順で最も低い検証層を選びます。

1. 外部環境なしで入力と結果を表現できるか
2. Native応答をFakeにすればUI結果を表現できるか
3. 一時Filesystemまたは子Processで契約を表現できるか
4. 実Windows API、Device、Pluginがなければ結果が決まらないか
5. 人間の聴覚または視覚による判断が必要か

1ならUnit、2ならComponent、3ならIntegration、4または5ならNative実機を選びます。

## 5. 開発時の検証サイクル

### 5.1 通常の検証バッチ

関連する挙動要件を小さなまとまりにし、次の順で進めます。

1. 対象挙動と失敗条件を決める
2. コード、保存データ、Protocolをまとめて調査する
3. 自動化できる再現テストを追加する
4. バッチ内の問題を修正する
5. Unit、Component、Integration Testを実行する
6. Nativeに影響する場合だけSidecarとTauriをビルドする
7. バッチ全体をNative実機で一度確認する
8. 挙動確認・課題管理表へ証拠と残課題を記録する

バッチは、一度の失敗で原因範囲を追える大きさにします。製品全体を一つのバッチにせず、小さな修正一件ごとにも分割しません。

### 5.2 変更に応じた実行範囲

| 変更 | 常に実行 | 必要な場合に追加 |
| --- | --- | --- |
| Domain、Reducer、変換 | 対象Unit Test | Rust/TypeScript全体 |
| React表示、操作 | 対象Unit・Component Test | E2E Smoke |
| Tauri Command、保存 | Rust Unit・Integration Test | Native再起動確認 |
| C++ Audio、MIDI、Plugin | Sidecar Test、Protocol Test | Native Audio/VST3/MIDI確認 |
| Build、Installer、Lifecycle | BuildとProcess Test | Cold start、終了、再起動 |
| 文書のみ | Link、Format、差分確認 | 原則として実機不要 |

変更していない領域の高価なテストを毎回実行する必要はありません。統合前とRelease候補では全自動テストを実行します。

## 6. 不具合修正の完了条件

不具合修正は、コードを書き終えた時点では完了としません。次を満たした時点で修正済みとします。

- 原因と影響範囲が説明できる
- 自動化可能なら再現テストがある
- 修正後に対象テストが成功する
- 関連する既存テストが成功する
- 保存データと外部Processを壊していない
- 実機依存ならNative実機で期待結果を観測している
- 挙動確認・課題管理表に証拠が記録されている

Build成功、型検査成功、画面に要素が存在することだけでは、ユーザー挙動の成立を意味しません。

## 7. 自動化しない判断

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

## 8. Test Doubleとテスト用入口

外部依存は、製品コードと同じ契約を実装するTest Doubleへ差し替えられる構造にします。

- Audio Runtime: Ready、Muted、Faulted、設定不採用、切断
- Plugin Host: Load成功、検証失敗、Crash、Missing、State復元
- MIDI: Port列挙、Note、切断、Panic
- Filesystem: 保存成功、容量不足、Lock、破損Manifest
- Job: 完了、進行、取消、Timeout、部分成果
- AI Provider: 無効、応答、拒否、外部送信確認

Test Doubleは本番にない成功経路を作るためではなく、実際に起こり得る応答を決定的に再現するために使います。

## 9. テストデータ

テストデータは小さく、決定的で、再生成可能にします。

- 短いPCM WAV、MIDI、ManifestをFixtureとして管理する
- 時刻、乱数、IDは注入または固定可能にする
- 第三者VST3本体やユーザー制作物をFixtureへ含めない
- 大容量Audioと長時間試験は通常テストから分離する
- 生成物は一時ディレクトリへ置き、成功・失敗の両方で片付ける

音声比較では、必要に応じてSample数、Peak、RMS、Hash、許容誤差を使います。浮動小数点処理へ完全一致を要求しません。

## 10. 実行時間と安定性

テストは、用途に応じて次の性質を保ちます。

- 編集中に使うテストは短時間で終わる
- 通常の自動テストはHardware、Network、ユーザー設定に依存しない
- Timeoutと再試行で不安定さを隠さない
- Flaky Testは放置せず、原因修正または通常実行から隔離する
- 失敗時に対象、期待値、実値、保存先を確認できる出力を残す

性能、長時間録音、多数Plugin、Device抜き差しなどは、通常テストと分けた耐久・互換性試験として実行します。

## 11. 管理と見直し

挙動要件ごとの成立状況と証拠は [挙動確認・課題管理表](./behavior-verification.md) で管理します。この文書には個別不具合、現在の件数、一時的な進捗を書きません。

テストを追加するときは、どの不具合またはリスクを検出するのかを明確にします。役割が重複するテスト、長時間かかる割に検出力が低いテスト、実装変更だけで壊れるテストは定期的に整理します。

この戦略は、製品構造、主要なリスク、開発速度が変わったときに見直します。単にテスト件数を増やす目的では変更しません。
