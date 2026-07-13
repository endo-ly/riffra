# Riffra 作業計画

## 1. 目的

この文書は、[製品挙動要件](./behavior-requirements.md) を、依存関係と製品リスクに沿って実装・検証する順序を定めます。個別要求の現在の判定や発見済み不具合は [挙動確認・課題管理表](./behavior-verification.md) で管理し、この文書には一時的な進捗、件数、担当者名を書きません。

作業は次の結果を最短距離で得ることを目的とします。

- 既存機能の不具合を、未実装機能より先に取り除く
- データ、安全性、Process境界、保存を、上位機能より先に安定させる
- 自動テストで判定できる内容を、Native実機で繰り返し確認しない
- 複数のエージェントが、Computer Useなしでも独立した作業単位を担当できるようにする
- Testabilityを妨げる構造は、挙動を変えない範囲で先にリファクタリングする

## 2. 計画の読み方

実施順序は「マイルストーン → 要求ID → 検証バッチ」です。マイルストーン内では、要求IDを一件ずつ完成させるのではなく、同じ状態、Protocol、保存形式を共有する3〜8件を一つの検証バッチとして扱います。

各要求は、着手時に次のいずれかへ分類します。

| 分類             | 扱い                                           |
| ---------------- | ---------------------------------------------- |
| 既存挙動の不具合 | 同じバッチの新規実装より先に修正する           |
| 未実装           | 依存する下位要求が成立した後に実装する         |
| Testability阻害  | 必要最小限のリファクタリングを先に行う         |
| 実機依存         | 自動検証を完了させ、Native確認待ちキューへ送る |
| 外部条件待ち     | 管理表を保留にし、他の要求へ進む               |

要求がすでに成立している場合は実装をやり直さず、回帰テストと証跡を確認して次へ進みます。後続要求の不具合がデータ消失、危険な音声出力、全体の保存停止を起こす場合は、順序を前倒しできます。

## 3. 一つの検証バッチの進め方

### 3.1 調査

1. 対象の挙動要件と期待結果を固定する
2. React、Tauri、Sidecar、Filesystem、保存モデルの実効経路を追う
3. 画面要素の存在ではなく、操作から保存結果までの断点を列挙する
4. 既存不具合、未実装、実機依存を分ける

### 3.2 自動検証と修正

1. 再現可能な不具合には、修正前に失敗するテストを置く
2. Testabilityが低い場合は、純粋なDomain処理、状態遷移、Native Adapter、表示Componentを抽出する
3. 既存挙動の不具合をすべて修正する
4. 同じ依存層にある未実装要求を実装する
5. 対象テストを実行し、バッチ終端で非GUI検証をまとめて実行する

### 3.3 統合と受入

1. 保存形式、Protocol、公開型、隣接機能への影響を確認する
2. `npm run verify`を実行する
3. C++、Audio、MIDI、VST3へ変更がある場合だけ`npm run verify:native`を実行する
4. 実機依存項目をNative確認待ちキューへまとめる
5. マイルストーン終端でNative実機確認を一度行う
6. 管理表へ判定、証跡、残作業を反映する

自動テストが失敗している状態、保存データが不明な状態、Native確認項目が定義されていない状態ではComputer Useへ進みません。

## 4. リファクタリング方針

リファクタリングは独立した美化作業ではなく、要求の検証速度と不具合の局所化を改善するために行います。次の状態がある場合は、対象バッチの冒頭で実施できます。

- 一つのReact ComponentがNative呼出し、状態遷移、表示、保存を同時に持つ
- Audio、Plugin、MIDI、Filesystemの実体を使わないと状態遷移をテストできない
- 同じ検証、正規化、Fallback処理が複数箇所にある
- Commandの成功、失敗、Timeout、部分成功をFakeへ差し替えられない
- 一つの変更で無関係なテストや画面が大量に壊れる

優先する分割単位は次のとおりです。

- Domainの純粋関数と不変条件
- Feature単位の状態遷移とView Model
- Native APIのInterfaceとAdapter
- 小さなReact Componentと操作イベント
- Filesystem、Clock、ID、Process起動の注入境界

全面的な書き直し、要求と無関係な命名整理、テストのためだけの内部実装公開は行いません。リファクタリング前後で同じ回帰テストが成功することを完了条件とします。

## 5. マイルストーンと要求実施順

### M0. 検証基盤と構造整理

特定の要求を完成させる段階ではなく、後続作業の重複コストを下げる段階です。既存のUnit、Component、Rust、Native self-testを維持し、必要になったFeatureから順にTest Doubleと状態遷移を分離します。

主な作業対象:

- App全体からFeature固有処理を段階的に抽出する
- Audio Runtime、Plugin Host、MIDI、Filesystem、JobのFakeを用意する
- Protocolの要求ID付き応答、Timeout、異常終了を自動再現可能にする
- 短いWAV、MIDI、Manifest、Session fixtureを共通化する

M0だけを長期間続けません。次のマイルストーンで必要になった分だけ実施します。

### M1. 安全性・障害・データ保全

最初に、以後の検証でデータ消失や危険な音声出力を起こさない基盤を完成させます。

対象要求:

- `DONE-002`
- `G-003`〜`G-006`
- `AUD-004`、`AUD-006`
- `REC-004`
- `REL-001`〜`REL-004`
- `PRJ-002`、`PRJ-004`
- `SEC-001`〜`SEC-003`
- `FLOW-007`

推奨バッチ:

1. Mute、Limiter、異常Sample、Device切断
2. Recording partial、Disk障害、Manifest、Recovery
3. Plugin/Sidecar/Background Jobの異常終了と孤立防止
4. Autosave世代、破損Session、Missing File/Plugin
5. Local First、Credential、診断Log

### M2. Session・保存・再起動の一貫性

すべての上位機能が共通して使う永続状態を固めます。

対象要求:

- `G-001`、`G-002`
- `PLG-005`
- `LIB-003`
- `PRJ-001`、`PRJ-003`、`PRJ-005`、`PRJ-006`
- `WIN-002`

推奨バッチ:

1. Scratch Session、Workspace共有、Undo/Redo
2. Plugin State、Snapshot、再起動復元
3. Project全体保存、Format Version、Migration
4. Inbox保全と設定保存

### M3. Audio・MIDIの基本動作

制作機能が依存するリアルタイム入出力を完成させます。

対象要求:

- `FLOW-001`
- `AUD-001`〜`AUD-003`、`AUD-005`
- `MIDI-001`〜`MIDI-003`
- `Q-002`

推奨バッチ:

1. Driver、Sample Rate、Buffer、Meter、Latency表示
2. Global Mute、Fade-in、全Workspace操作
3. MIDI Port、Event、Sidecar、Panic
4. ScanやJobとAudio Callbackの競合

### M4. Plugin Host・Rack・音色比較

実VST3を含む音色作成と再利用を完成させます。

対象要求:

- `FLOW-002`
- `PLG-001`〜`PLG-004`
- `RACK-001`〜`RACK-006`

推奨バッチ:

1. 隔離Scan、Catalog、検索、Quarantine
2. Load、Bypass、Remove、Parameter View
3. Rack編集、Signal Flow、Macro
4. Snapshot、A/B、Tone Exploration
5. Freeze、Render fallback

### M5. Recording・Arrange・Export

録音した素材を壊さず編集し、成果物へ進める中核フローを完成させます。M1で扱った`REC-004`も、実際の編集フローとの統合を再確認します。

対象要求:

- `FLOW-003`、`FLOW-004`
- `REC-001`〜`REC-004`
- `ARR-001`〜`ARR-005`
- `EXP-001`、`EXP-002`

推奨バッチ:

1. Raw/Processed/MIDI録音、Count-in、Punch、Loop
2. Take Group、Rating、Selected State、Provenance
3. Audio/MIDI配置、Track/Mixer、非破壊編集
4. Tempo、Marker、Automation
5. Master/Track/Stem Render、DAW handoff、原本hash確認

### M6. Sample・内蔵音源

外部Pluginがなくても素材を演奏できる経路を完成させます。

対象要求:

- `FLOW-005`
- `SMP-001`〜`SMP-006`

推奨バッチ:

1. 非破壊Import、範囲、Loop、Preview
2. Pad、Kit、Velocity、Choke、Round Robin
3. Keyboard mapping、Layer、Release Sample
4. Internal Synth、Utility Device、Preset

### M7. Analyze・Reference・Separate・Library

素材を理解し、派生物と関連情報を再利用する機能を完成させます。

対象要求:

- `ANL-001`〜`ANL-003`
- `SEP-001`、`SEP-002`
- `LIB-001`、`LIB-002`、`LIB-004`

推奨バッチ:

1. WAV解析値、表示用データ、異常WAV
2. Reference比較、同期Preview、Loop
3. Separation Job、進捗、取消、Partial Result
4. Metadata、横断検索、Related Asset

### M8. AI Assistant・外部送信制御

AI機能は、Session、Undo、権限、Local Firstが成立した後に実装します。外部送信を伴う機能は、送信制御を機能本体より先に作ります。

対象要求:

- `FLOW-006`
- `AI-001`〜`AI-005`
- `SEC-001`〜`SEC-003`の再確認

推奨バッチ:

1. Explain、Suggest、ChangeSetの純粋な状態遷移
2. Preview、Reject、Apply、Undo
3. Context選択、Permission、Provider表示
4. 外部送信確認、Trim/Downmix/Anonymize、監査情報

### M9. Windows品質・性能・完成判定

主要フロー完成後に、横断品質とRelease条件を確認します。

対象要求:

- `G-007`
- `WIN-001`
- `Q-001`、`Q-003`
- `DONE-001`

推奨バッチ:

1. Focus Mode、DPI、複数モニター、最小Window
2. Cold/Warm startと構成別Startup測定
3. 長時間再生、録音、Plugin、Job、Memory/Handle監視
4. 全中核フロー、Known Gap、Release証跡の最終監査

## 6. 複数エージェントでの分担

### 6.1 調整担当

調整担当は要求バッチの切り出し、依存関係、ファイル所有、統合、管理表、Native確認待ちキューを管理します。複数エージェントが同じFeature、型、Protocol、保存モデルを同時に変更しないようにします。

調整担当だけが行う作業:

- バッチ範囲と完了条件の確定
- 共有型、Protocol、Migrationの統合判断
- `behavior-verification.md`の最終判定更新
- Release Native buildとComputer Use
- 実音、実Device、実VST3、OS挙動の適合判定

### 6.2 実装担当エージェント

Computer Useを使えないエージェントは、次の作業を独立して担当できます。

- 要求と実装の差分調査
- Unit、Component、Integration Testの追加
- Domain、React、Rust、C++の修正または新規実装
- Test Double、Fixture、Self-testの整備
- Build、型検査、非GUIテスト
- Native確認シナリオと期待証跡の作成

実装担当は、Native UIを見ていない状態で`適合`と判定しません。実機でしか決まらない項目は、コードと自動回帰を完了させたうえで`Native確認待ち`として調整担当へ返します。

### 6.3 並行作業の条件

並行作業は、書き込み対象と契約が分離できる場合だけ行います。

並行化しやすい組合せ:

- Rust Domain/StorageとReact Component
- C++ self-testとTypeScript View Model
- Fixture作成と既存コード調査
- 異なるFeatureディレクトリのUnit Test

直列化する組合せ:

- 同じProtocolの送信側と受信側
- Session型とMigration
- App全体の状態管理と複数Featureの同時抽出
- 同じC++ Audio Callbackまたは録音Lifecycle
- 同じ管理表行の更新

共有作業ツリーを使う場合は、調整担当がファイル所有を先に割り当てます。担当外の整形、名称変更、依存更新を混ぜません。

## 7. エージェントからの引き渡し

各実装担当は、作業完了時に次を返します。

- 対象要求ID
- 確認した既存不具合と未実装
- 原因と影響範囲
- 変更した契約、保存形式、主要ファイル
- 追加したテストと実行結果
- 実行していない高価な検証
- Nativeで行う操作、期待結果、確認するFile/status/音
- 残課題と、後続要求への依存

「Build成功」「テスト成功」だけでは引き渡し完了としません。どのユーザー挙動を、どの層まで確認したかを明示します。

## 8. Computer Useの集約

Computer Useは調査のたびに起動せず、マイルストーン内の自動検証が通った後にまとめて使います。Native確認待ちキューは、同じ起動、Device、Plugin、録音素材を共有できる順に並べます。

一回のNative確認では、可能な範囲で次をまとめます。

1. Cold startまたは再起動状態
2. Device、Mute、Meter、Process
3. 対象Featureの主要成功フロー
4. 代表的な失敗フロー
5. 保存File、manifest、Session
6. 再起動後の復元

Computer Useへ残すのは、Window到達性、実Device、実VST3、Process、実音、OS連携など、自動テストで代替できない判定だけです。内部計算、JSON変換、状態遷移、エラー分類は事前に自動化します。

## 9. マイルストーン完了条件

マイルストーンは、対象コードを書き終えた時点では完了しません。次を満たした時点で次へ進みます。

- 対象要求の既存不具合が修正されている
- 未実装として残す要求と理由が明確である
- 修正した不具合に再現テストがある、または自動化不能理由がある
- 対象のUnit、Component、Integration、Self-testが成功する
- 保存、Migration、Process終了、安全性への回帰がない
- Native依存項目がまとめて確認されている
- 管理表の判定と証跡が更新されている

次のマイルストーンへ進んだ後に重大な回帰が見つかった場合は、新規実装を止め、原因を含む最も近いマイルストーンへ戻して修正します。
