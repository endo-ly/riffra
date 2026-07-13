# Windows向け統合音楽制作ワークベンチ 製品仕様書

> **文書種別**: 完成製品仕様書  
> **対象**: Codexを含む実装エージェント、設計者、将来の自分  
> **対象OS**: Windows 11 x64  
> **基本思想**: What / Whyを中心に、製品判断に必要な最低限のHowを定義する  
> **仕様バージョン**: 2.0

---

## 0. 本書の役割

本書は、Windows上で動作する個人向け統合音楽制作アプリケーションの**完成時の製品像、価値、振る舞い、品質基準**を定義する。

本書は単なる機能一覧ではない。実装中に複数の選択肢が生じた際、何を優先し、何を捨て、どの状態を「正しい製品」とみなすかを判断するための基準である。

本書で定義するのは、特定ライブラリ、フレームワーク、内部アルゴリズムではなく、主として以下である。

- 何を作るか
- なぜ作るか
- 誰の、どの行為を改善するか
- ユーザーから見てどう振る舞うか
- どの品質を満たすべきか
- 要求が衝突したとき、何を優先するか
- 何を作らないか

実装上の詳細は、本書の製品意図を守るために必要な範囲だけ定義する。

---

# 1. 製品定義

## 1.1 一文で表す製品

本アプリケーションは、**音をすぐに試し、作り、録り、比較し、再利用可能な制作資産として残すための、Windows向け統合音楽制作ワークベンチ**である。

## 1.2 製品の中心価値

本アプリケーションが最適化する一連の行為は、次の循環である。

```text
Hear
音を出す
  ↓
Shape
音を作る
  ↓
Capture
演奏・発想・状態を記録する
  ↓
Compare
候補や変化を公平に比べる
  ↓
Reuse
良い結果を再利用可能な資産として残す
  ↺
```

既存DAWの多くは「楽曲を完成させるプロジェクト」を中心に設計されている。本アプリケーションは、それより前段にある以下の時間を中心に設計する。

- ギターを数分だけ弾く
- VSTの音を試す
- 良いエフェクトチェーンを作る
- リフや音色を思いつく
- 複数候補を比べる
- サンプルを切り出して遊ぶ
- 音の問題を分析する
- 後で使える形に整理する

必要な場合は、そのまま録音・編集・簡易アレンジまで進められる。ただし、製品の価値を「小型DAWであること」へ縮退させてはならない。

## 1.3 製品を構成する五つの能力

本アプリケーションは、以下の五つを一つの体験として統合する。

1. **Immediate Play**  
   起動後すぐに、ギター、マイク、MIDI、VSTから音を出せる。

2. **Tone & Rack Design**  
   VST、内部音源、内部エフェクトを組み合わせ、音色を設計・比較できる。

3. **Capture & Arrange**  
   思いついた演奏や音を即座に録音し、必要十分なタイムライン編集ができる。

4. **Creative Memory**  
   ラック、プリセット、録音、テイク、サンプル、MIDI、分析結果を、由来と意図を含めて保存・検索・再利用できる。

5. **Assisted Understanding**  
   AI、音声解析、比較、ステム分離によって、制作判断を奪わず、理解と試行回数を増やす。

## 1.4 本製品が解決する問題

既存環境には、次の摩擦がある。

- 音を試すだけでも、DAW起動、プロジェクト作成、トラック設定が必要になる
- VST、プリセット、録音、サンプル、メモが別々の場所へ散らばる
- 良い音を作っても、何を意図して、何と組み合わせたかが残らない
- 過去に作った音色を再発見しにくい
- プラグインごとに操作体系や用語が異なり、理解の負担が大きい
- 候補比較が音量差や記憶に引っ張られ、判断しにくい
- ステム分離、スペクトラム解析、AI相談が別ツールに分断されている
- 高機能DAWは便利だが、短い演奏、練習、音作りには重い
- クラッシュやプラグイン不調によって、制作の流れが切れる
- 新しいプラグインを増やすことが、制作能力の向上と混同されやすい

本製品は、音楽制作を「ファイルとプロジェクトの管理」ではなく、**試行と学習が蓄積される継続的な制作環境**として扱う。

## 1.5 想定ユーザー

主な対象は、以下のような単一の個人ユーザーである。

- Windows環境でギター、マイク、オーディオインターフェース、MIDI機器を使用する
- アンプシミュレーター、シンセ、エフェクトなどのVSTを頻繁に使用する
- 大規模DAWを毎回起動することに心理的・操作的な負担を感じる
- 音色、リフ、サンプル、録音、チェーンを長期的に蓄積したい
- 完全自動生成より、自分で選び、比較し、編集する制作を好む
- AIを代作者ではなく、説明者、分析者、提案者、操作補助として使いたい
- 実験性、拡張性、データ所有権を重視する
- クラウドアカウントやサブスクリプションなしでも中核機能を使いたい

複数ユーザー共同利用、商用スタジオの大規模セッション管理、クラウド中心の制作は主要対象としない。

---

# 2. 製品判断の優先順位

要求が競合する場合、以下の順で優先する。

1. **聴覚上の安全**
2. **リアルタイム音声の継続性**
3. **録音・編集内容の保全**
4. **ユーザーの意図と可逆性**
5. **音が出るまでの速さ**
6. **操作の一貫性と理解しやすさ**
7. **制作資産の再利用性**
8. **見た目と触り心地**
9. **機能数**
10. **実装上の都合**

例えば、美しいアニメーションが音切れを増やすなら、アニメーションを簡略化する。複雑な機能が操作体系を壊すなら、その機能を削るか、段階的に開示する。実装しやすさを理由にユーザーのデータを破壊的に扱ってはならない。

---

# 3. 製品原則

## 3.1 Sound First

アプリケーションの最初の仕事は、画面を見せることではなく、**安全かつ確実に音を扱える状態を作ること**である。

起動、画面切り替え、検索、AI処理、波形描画、バックグラウンドジョブは、リアルタイム音声より優先されてはならない。

## 3.2 No Project Before Sound

音を出す前に、プロジェクト名、保存先、テンポ、トラック構成を要求しない。

アプリケーションは常に自動保存される一時作業領域を持ち、ユーザーは「保存を決める前」から演奏、音作り、録音を始められる。

## 3.3 Non-Destructive by Default

元音声、元MIDI、元プリセットを直接破壊しない。

トリミング、ゲイン、フェード、ピッチ、タイムストレッチ、エフェクト、ステム分離、AI変更は、原本と変更履歴を保持したまま扱う。

明示的な破壊操作を提供する場合は、結果と影響を事前に示す。

## 3.4 Every Good Result Becomes an Asset

良い音、良い演奏、良い設定、良い分析結果が、現在の画面やプロジェクトに閉じ込められてはならない。

ユーザーは、成果を名前、タグ、メモ、評価、試聴音、由来とともに保存し、後から検索・比較・再利用できる。

## 3.5 Preserve Intent, Not Only Values

保存するのは数値だけではない。

「明るいクリーン」「夜に小音量で練習」「ボーカルの隙間に入るギター」「候補Bより低域を抑えた」など、ユーザーの意図、比較対象、使用場面を記録できること。

## 3.6 Progressive Disclosure

初見では、今必要な情報だけを見せる。

複雑なルーティング、詳細パラメータ、解析値、履歴、メタデータは、必要なときに展開できる。

高機能であることを、常時高密度であることと混同しない。

## 3.7 One Interaction Language

同じ種類の対象は、同じ方法で選択、保存、複製、削除、比較、タグ付けできる。

重要操作を右クリックメニューだけに隠さない。画面ごとに別の用語や別の操作規則を増やさない。

## 3.8 AI Is a Reversible Collaborator

AIは、説明、提案、分析、操作の下書きを行う。AIがユーザーの代わりに不可視の変更を行ってはならない。

AIによる変更は、一つの変更セットとして、内容、理由、影響、送信データ、適用結果を確認でき、まとめて取り消せる。

## 3.9 Local First, Portable Always

中核機能はアカウント登録、常時接続、クラウド保存なしで利用できる。

ユーザーの録音、プロジェクト、プリセット、メタデータは、アプリケーションの終了やサービス停止後もユーザーが保持・移行できる形式で管理する。

## 3.10 Fail Softly

一つのプラグイン、一つの音声ファイル、一つのAIサービス、一つのバックグラウンド処理の失敗によって、アプリケーション全体を使用不能にしない。

問題箇所を隔離し、残りの作業を続けられることを優先する。

---

# 4. 製品のメンタルモデル

## 4.1 常に存在するScratch Session

アプリケーションは、起動時に必ず一つの**Scratch Session**を開く。

Scratch Sessionは、名前や保存先を決める前の作業領域である。

- 前回の状態を自動復元する
- 演奏、ラック編集、録音、簡易配置ができる
- 録音や重要な状態は自動的に保全される
- 任意の時点でProjectへ昇格できる
- Projectへ昇格しなくても、録音やラックはLibraryへ保存できる
- 「保存しなかったから全て消えた」という状態を作らない

これにより、「すぐ音を出したい」と「作業を確実に残したい」を両立する。

## 4.2 Project

Projectは、楽曲、音色研究、練習、録音セッションなど、ユーザーが名前を与えて継続的に扱う作業単位である。

ProjectはScratch Sessionの上位概念ではなく、**保存・共有・再開の意図が明確になったSession**である。

## 4.3 Library

Libraryは、プロジェクトをまたいで再利用する制作資産の保管・検索領域である。

Libraryには以下が含まれる。

- Plugin
- Preset
- Rack
- Rack Fragment
- Recording
- Take Group
- Audio Clip
- MIDI Clip
- Sample
- Sampler Kit
- Instrument
- Reference Track
- Analysis
- Separation Result
- Template
- AI Suggestion
- User Note

## 4.4 Asset Provenance

すべての派生資産は、可能な範囲で由来を保持する。

例:

```text
Recording
├─ recorded from: Input 1
├─ at: 2026-07-11 20:41
├─ sample rate: 48 kHz
├─ used rack: Glass Clean v3
├─ rack snapshot: B
├─ plugin versions: ...
├─ source: raw DI
└─ derived assets:
   ├─ normalized clip
   ├─ trimmed loop
   └─ exported WAV
```

由来が残ることで、後から音を再現し、比較し、別の形で作り直せる。

## 4.5 主要概念

### Session

現在開いている作業状態。Scratch SessionまたはProjectとして存在する。

### Project

名前、履歴、素材、タイムラインを持つ継続的なSession。

### Track

時間軸上の音声、MIDI、グループ、マスターのまとまり。

### Clip

Track上に配置されるAudioまたはMIDIの参照単位。

### Rack

入力から出力までの音声・MIDI処理構成を表す再利用可能な単位。

### Device

Plugin、内部エフェクト、内部音源、入出力、分岐、ミキサーなど、Rackを構成する要素。

### Plugin

インストール済み外部プラグインの定義。

### Plugin Instance

特定のRackまたはTrack上で実体化したPlugin。

### Preset

単一Deviceの保存状態。

### Snapshot

Rack、Track、Projectなどの比較可能な一時状態。

### Macro

複数パラメータを、意味のある一つの操作へまとめたもの。

### Recording

録音音声と録音条件、使用Rack、入力、タイミングを含む記録。

### Take Group

同じ目的で録音した複数Takeのまとまり。

### Asset

再利用、検索、関連付けが可能な制作物。

### Job

ステム分離、解析、波形生成、書き出しなど、時間を要する処理。

### AI ChangeSet

AIが提案または適用する、説明付きで可逆な変更のまとまり。

---

# 5. アプリケーション全体構造

## 5.1 一つのメインウィンドウ

通常の制作は、一つのメインウィンドウで完結する。

```text
┌─────────────────────────────────────────────────────────────────────┐
│ Global Bar                                                         │
├───────────────┬───────────────────────────────────┬─────────────────┤
│ Library       │ Main Workspace                    │ Inspector       │
│               │                                   │                 │
│               │                                   │                 │
├───────────────┴───────────────────────────────────┴─────────────────┤
│ Transport / Meter / Audio Status / Jobs                            │
└─────────────────────────────────────────────────────────────────────┘
```

プラグイン独自GUI、詳細解析、ミキサーなどは必要に応じて別ウィンドウへ分離できるが、主要操作を複数ウィンドウへ分散させない。

## 5.2 Global Bar

常時、以下を確認・操作できる。

- Scratch SessionまたはProject名
- 保存・同期状態
- Undo / Redo
- ワークスペース切り替え
- グローバル検索
- コマンドパレット
- AI Assistant
- Audio Engine状態
- CPU負荷
- 入出力レイテンシ
- Background Job状態
- 設定
- 緊急ミュート

## 5.3 Library Panel

以下の表示を切り替える。

- Plugins
- Racks
- Presets
- Samples
- Recordings
- MIDI
- Projects
- Separation Results
- References
- Favorites
- Recent
- Inbox
- Saved Searches

Library Panelは単なるファイルブラウザーではなく、制作資産を探し、試聴し、現在の作業へ適用する入口である。

## 5.4 Main Workspace

主なワークスペースは以下とする。

### Home

作業開始と再開のための画面。

- 前回のScratch Session
- 最近のProject
- 前回使用したRack
- 最近のRecording
- Favorites
- Quick Record
- Quick Play
- Audio Device状態
- 未完了Job
- Crash Recovery
- 取り込み待ちのInbox

### Play

リアルタイム演奏と音作りの中心画面。

- Input
- Rack
- Macro
- Snapshot
- Meter
- Quick Record
- MIDI Monitor
- Tuner
- Plugin GUI
- Performance-friendly view

### Arrange

Audio / MIDIを時間軸上で録音・編集・構成する画面。

### Sample

音声の切り出し、ループ、パッド割り当て、鍵盤配置、内部シンセを扱う画面。

### Analyze

波形、ラウドネス、スペクトラム、ピッチ、位相、比較、Referenceを扱う画面。

### Separate

Stem分離の設定、処理、同期試聴、結果管理を行う画面。

ワークスペースを切り替えても、再生、録音待機、Rack状態、選択中の素材、Undo履歴は失われない。

## 5.5 Inspector

現在選択中の対象に応じて以下を表示する。

- 基本情報
- Parameter
- Routing
- Metadata
- Tag
- Note
- File Information
- Provenance
- AI Suggestions
- Analysis
- History
- Related Assets
- Missing Dependencies

Inspectorは対象の「詳細」と「由来」を一か所で確認する領域とする。

## 5.6 Transport

以下を常時操作できる。

- Play
- Stop
- Record
- Record Arm
- Loop
- Metronome
- Count-in
- Pre-roll
- Punch In / Out
- Playhead
- BPM
- Time Signature
- Master Volume
- Input / Output Meter
- Mute
- Emergency Stop

## 5.7 Focus Mode

Play、Arrange、Sample、Analyzeは、左右パネルを隠して作業領域を最大化できる。

Focus Modeでも、緊急停止、録音状態、クリッピング、保存状態を見失わない。

---

# 6. 中核ユーザーフロー

## 6.1 起動してギターを弾く

1. アプリケーションを起動する
2. 前回のAudio DeviceとRackが復元される
3. 入力レベルと出力状態を確認できる
4. 追加操作なし、または一回の入力有効化で演奏できる
5. 気に入った演奏を即座に録音できる

**成立条件**:

- Project作成を要求しない
- 突然大音量が出ない
- Device不在時は安全な代替候補を示す
- 前回異常終了時は危険なPluginを自動再有効化しない

## 6.2 音色を作り、候補を比べる

1. Pluginまたは保存済みRackを追加する
2. Signal Flowを見ながら順序を変更する
3. 重要ParameterをMacroへまとめる
4. Snapshotを複数保存する
5. 音量を揃えてA/B比較する
6. 良い候補を名前、Tag、Note、Preview付きで保存する

**成立条件**:

- 比較によって現在の設定を失わない
- Snapshot切替で危険な音量変化が起きない
- Plugin固有GUIが使えなくてもParameterへアクセスできる
- 保存したRackから試聴音と意図を確認できる

## 6.3 思いつきをすぐ録る

1. HomeまたはPlayからRecordを押す
2. 必要ならCount-in後に録音が始まる
3. Raw InputとProcessed Outputを同時に保存する
4. 録音後、名前を付けなくてもInboxへ保全される
5. 後からProject、Loop、Sample、Assetへ昇格できる

**成立条件**:

- 保存先ダイアログを録音前に出さない
- Device切断や停止後も取得済みデータを残す
- 録音条件と使用Rackを自動記録する

## 6.4 簡易アレンジへ進む

1. 録音、Audio、MIDIをTimelineへ配置する
2. 分割、複製、Trim、Loop、Fadeを行う
3. TrackへRackを適用する
4. 音量、Pan、Automationを調整する
5. 選択範囲、Track別、MasterをExportする

**成立条件**:

- 元ファイルを変更しない
- Recording Latencyが補正される
- Plugin LatencyがTimeline上で補償される
- Missing PluginがあってもProjectを開ける

## 6.5 Sampleから楽器を作る

1. AudioをSampleへ読み込む
2. 必要範囲を切り出す
3. PadまたはKeyboardへ割り当てる
4. Pitch、Envelope、Loopを調整する
5. KitまたはInstrumentとして保存する
6. MIDIから演奏・録音する

**成立条件**:

- 元Audioを保持する
- Sample編集結果を他のProjectでも再利用できる
- MIDI MappingとRoot Keyが明示される

## 6.6 AIへ相談し、変更を選ぶ

1. 現在のRack、Track、RecordingについてAIへ相談する
2. AIが問題の説明と複数の変更案を提示する
3. 変更対象、現在値、変更後、理由を確認する
4. 全体または一部を適用する
5. 適用前後を比較する
6. AI ChangeSetを一括Undoする

**成立条件**:

- AIが勝手にProject全体を変更しない
- 外部送信内容を確認できる
- 音声送信には明示操作が必要
- AI不在でも手動操作を妨げない

## 6.7 障害から復旧する

1. PluginがCrashまたはHangする
2. 音声が安全にMuteまたはBypassされる
3. アプリケーション本体は操作可能なまま残る
4. 問題Pluginが特定される
5. 自動保存状態を復元する
6. Pluginを無効化またはPlaceholder化してProjectを開く

**成立条件**:

- 一つのPlugin障害で録音済みデータを失わない
- 起動ループに入らない
- 原因と選択肢をユーザーへ示す

---

# 7. Audio Engineと入出力

## 7.1 対応環境

Windows上で以下を扱う。

- ASIO
- WASAPI
- Audio Input / Output
- Mono / Stereo
- 複数Input Channel
- MIDI Input / Output
- Device Hot Plug
- Sample Rate変更
- Buffer Size変更
- Sleep / Resume

主なリアルタイム演奏用途ではASIOを優先する。WASAPIは、汎用再生、簡易入力、Audio Interfaceを使わない環境を支える。

## 7.2 Device設定

ユーザーは以下を確認・変更できる。

- Driver Mode
- Input Device
- Output Device
- Input Channel
- Output Channel
- Sample Rate
- Buffer Size
- Mono / Stereo
- Software Monitoring
- Input Gain表示
- Output Gain
- Exclusive / Shared状態
- Estimated Input Latency
- Estimated Output Latency
- Estimated Round-trip Latency

設定変更によって音声が一時停止する場合、停止理由と復帰状態を明示する。

## 7.3 Audio Safety

以下を必須とする。

- 起動時のFade-in
- Device切替時のMute
- Master Limiter
- 異常Peak検知
- Feedback疑いの検知
- Emergency Mute
- Audio Engine Stop
- DCまたは異常値への保護
- Plugin読込直後の安全なGain
- Snapshot切替時のClick / Pop抑制
- Headphone利用を想定した保守的な初期Volume

安全機能は、通常利用で音を不自然に変えない範囲で動作する。

## 7.4 Monitoring

ギター、マイク、ライン入力、MIDI Instrumentを低LatencyでMonitoringできる。

常時表示する情報:

- Input Meter
- Output Meter
- Peak
- Clip
- CPU Load
- Audio Dropout
- Buffer Underrun / Overrun
- Current Driver
- Current Sample Rate
- Round-trip Latency

## 7.5 Latency Compensation

以下を補償する。

- Plugin Processing Latency
- Recording Input Latency
- Software Monitoring Path
- Render / Export時のDelay
- Parallel Path間のDelay差

ユーザーは補償量と、補償できない状態を確認できる。

## 7.6 MIDI

以下を扱う。

- Note On / Off
- Velocity
- Sustain
- Pitch Bend
- Modulation
- Channel Pressure / Aftertouch
- MIDI Channel
- MIDI Clockの受信または送信
- MIDI Learn
- DeviceごとのMapping
- Hot Plug
- Panic / All Notes Off

MIDI Deviceが切断されても、鳴り続けるNoteを残さない。

---

# 8. Plugin Host

## 8.1 対応形式

主要対象はWindows版VST3とする。

CLAPは、Plugin側の対応とアプリケーション品質を確保できる範囲で、VST3と同等の体験を提供する。

VST2は製品の中心対象としない。対応する場合も、VST3 / CLAPの品質を損なわない。

## 8.2 Scan

標準Folderおよびユーザー指定FolderをScanする。

保存する情報:

- Name
- Vendor
- Version
- Format
- Instrument / Effect
- Category
- Audio I/O
- MIDI I/O
- Parameter Count
- File Path
- Last Scan
- Load Success / Failure
- Scan Duration
- Crash / Hang History
- Stability State
- User Rating
- Tag
- Favorite

ScanはメインUIとAudio Engineを停止させない。

新規、更新、移動されたPluginだけを再Scanできる。Scan中にCrashしたPluginを特定し、次回以降隔離できる。

## 8.3 Browser

以下で検索・絞り込みできる。

- Name
- Vendor
- Format
- Instrument / Effect
- Amp
- Cabinet
- EQ
- Compressor
- Delay
- Reverb
- Modulation
- Distortion
- Utility
- Sampler
- Synth
- Favorite
- Recent
- Tag
- Stability
- Installed / Missing

PluginをRack、Track、SlotへDrag & Dropできる。

## 8.4 Plugin UI

二つの表示を持つ。

1. Plugin Native GUI
2. Application Common Parameter View

Native GUIが開けない、DPI表示が崩れる、別Windowが不安定な場合でも、Common Parameter Viewから操作できる。

Common ViewはParameterの型に応じて以下を表示する。

- Knob
- Slider
- Toggle
- Selector
- Numeric Input
- Text / Value
- Meter

重要ParameterをPinし、Macro候補として保存できる。

## 8.5 Plugin State

以下を保存・復元する。

- Complete Plugin State
- Parameter Values
- Internal Preset
- Bypass
- Dry / Wet
- Input / Output Gain
- Macro Mapping
- Automation
- User Note
- Plugin Version
- State Compatibility情報

復元できない場合、Project全体を開けなくせず、そのPluginをDisabled Placeholderとして開く。

Placeholderは以下を保持する。

- 元Plugin名
- Version
- State Data
- Routing位置
- Automation
- 代替Pluginの割当
- 再Scan / 再読込操作

## 8.6 Isolation and Stability

不安定なPluginのCrash、Hang、過剰CPU、異常出力が、アプリケーション全体へ波及しにくい構造を必須とする。

ユーザーから見て以下が成立すること。

- 問題Pluginを特定できる
- Kill / Disable / Retryできる
- Projectを安全に開ける
- Pluginなしでも他のTrackを再生できる
- Scan Crashによる起動不能を防ぐ
- Pluginの状態を可能な範囲で保全する

---

# 9. Rackと音色設計

## 9.1 Rack

Rackは、InputからOutputまでの処理を表す再利用可能なSignal Flowである。

```text
Input
→ Noise Gate
→ Overdrive
→ Amp
→ Cabinet
→ EQ
→ Delay
→ Reverb
→ Output
```

Rackは以下を含む。

- Input / Output
- External Plugin
- Internal Device
- Serial Path
- Parallel Path
- Send / Return
- Mixer
- Macro
- Snapshot
- Automation
- Note
- Tag
- Preview Recording
- Compatibility情報

## 9.2 Rack Editor

提供する操作:

- Add
- Remove
- Duplicate
- Reorder
- Bypass
- Solo
- Mute
- Dry / Wet
- Input / Output Gain
- Group
- Select Range
- Save Selection as Rack Fragment
- Insert Rack Fragment
- Replace Device
- Compare Rack
- Copy / Paste
- Render through Rack

Drag中は、挿入位置、接続結果、Channel構成、Feedbackの危険を明確に示す。

## 9.3 Signal Flow表示

Signal Flowは本製品の中心的な視覚表現である。

以下が一目で分かること。

- 音がどこから入り、どこへ出るか
- 現在音が流れているPath
- Bypass / Mute / Solo
- Serial / Parallel
- Channel数
- 遅延の大きいDevice
- 異常または無音の箇所
- Recording Tap Point

視覚表現はNode Editorの自由度を目的化せず、通常の音作りが一直線の操作で完結することを優先する。

## 9.4 Parallel Processing

Signalを複数Pathへ分岐し、再度Mixできる。

```text
Input
├─ Clean
└─ Distortion
   ↓
Blend
   ↓
Output
```

Pathごとに以下を設定する。

- Gain
- Pan
- Mute
- Solo
- Phase Invert
- Delay Compensation
- Dry / Wet

複雑なRoutingは折り畳み可能とし、通常状態の視覚的負荷を増やさない。

## 9.5 Macro

複数Parameterを意味のある一つの操作にまとめる。

例:

- Brightness
- Gain
- Width
- Space
- Attack
- Aggression
- Warmth
- Distance

各Mappingに以下を設定できる。

- Min
- Max
- Invert
- Curve
- Step
- Multiple Targets
- Unit / Display Name

MacroはRackの操作面として、Native GUIを開かなくても主要な音色変化を行える品質を持つ。

## 9.6 Snapshot

Rackの現在状態を即座に保存し、切り替え、比較できる。

保存内容:

- Name
- Created At
- Description
- Tag
- Changed Parameters
- Preview Recording
- Rating
- Intended Use
- Parent Snapshot
- Difference Summary

A/Bだけでなく、複数候補を順番またはBlindで比較できる。

比較時は、可能な範囲でLoudnessを揃え、音量差による錯覚を減らす。

## 9.7 Tone Exploration

以下を提供する。

- Selected Parameters Randomize
- Range-limited Randomize
- Parameter Lock
- Subtle Variation
- Snapshot Morph
- Candidate Batch Generation
- Auto Preview Recording
- Keep / Reject
- Safety Limits

Gain、Feedback、Resonance、Outputなど危険Parameterは、安全範囲、確認、Limiterの対象とする。

## 9.8 Freeze and Render

RackまたはTrackをAudioへRenderし、Pluginがなくても再生可能な状態を作れる。

- Freeze: 後で元へ戻せる
- Render in Place: 派生Audioとして作る
- Flatten: 明示確認後に単純化する
- Keep Source: 元RackとMIDI / DIを保持する

これはCPU節約だけでなく、将来Pluginが失われた場合の保全手段でもある。

---

# 10. Recording

## 10.1 Quick Record

Home、Play、Arrangeから、Project作成なしで録音を開始できる。

録音後は必ずInboxへ保全し、その後に以下を選べる。

- Rename
- Tag
- Add Note
- Favorite
- Add to Project
- Create Loop
- Open in Sample
- Analyze
- Save with Rack
- Delete

Deleteは即時完全削除ではなく、一定期間復元可能とする。

## 10.2 Recording Sources

以下を個別または同時に録音できる。

- Raw Input
- Rack Output
- Specific Device Output
- Track Output
- Master Output
- Multiple Tap Points
- MIDI Input

ギター録音では、Raw DIとProcessed Outputを同時保存できる。

## 10.3 Recording Controls

以下を提供する。

- Record Arm
- Input Monitoring
- Count-in
- Pre-roll
- Punch In / Out
- Loop Recording
- Retrospective Capture
- Auto Naming
- Take Number
- Marker

Retrospective Captureは、録音ボタンを押す前の直近の演奏を可能な範囲で救済する。ただしプライバシーと保存容量を明示し、無制限・不可視に録音し続けない。

## 10.4 Take Management

同じ目的で録音した複数TakeをTake Groupとして管理する。

保存情報:

- Recorded At
- Input
- Rack
- Snapshot
- BPM
- Time Signature
- Latency Compensation
- Note
- Rating
- Selected State
- Quality Warning

Take間を即座に切り替え、必要に応じて区間単位の簡易Compを作れる。

## 10.5 Recording Integrity

録音中は、UI停止、Plugin障害、Device切断、Disk速度低下が起きても、取得済みデータの保全を最優先する。

録音が不完全な場合は以下を示す。

- Dropout位置
- Missing Range
- Device Disconnect
- Disk Warning
- Clip
- Recovery Status

---

# 11. Arrange

## 11.1 Positioning

Arrangeは、本格DAWの全機能を再現するのではなく、アイデアを一つの流れへまとめ、外部DAWへ渡すか、そのまま簡易作品として書き出せる範囲を担う。

中心操作:

- Place
- Move
- Duplicate
- Split
- Trim
- Loop
- Fade
- Crossfade
- Gain
- Pan
- Mute
- Solo
- Rack
- Automation
- Marker
- Export

## 11.2 Track Types

- Audio Track
- MIDI Track
- Group Track
- Return Track
- Master Track

各Trackは以下を持つ。

- Name
- Color
- Volume
- Pan
- Mute
- Solo
- Arm
- Input
- Output
- Rack
- Automation
- Note
- Freeze State

## 11.3 Audio Clip

設定できる内容:

- Timeline Start / End
- Source In / Out
- Clip Gain
- Fade In / Out
- Crossfade
- Loop
- Reverse
- Pitch
- Playback Rate
- Time Stretch
- Mute
- Warp / Stretch Mode
- Source Link

元ファイルを変更しない。

## 11.4 MIDI Clip

編集できる内容:

- Note
- Start
- Length
- Velocity
- Channel
- Quantize
- Transpose
- Duplicate
- Humanize
- Sustain
- Pitch Bend
- Modulation
- Expression
- Aftertouch

MIDI Editorは基本的なPiano Rollとして機能する。

楽譜編集、複雑なArticulation管理、オーケストラ向けExpression Mapは中心目的としない。

## 11.5 Tempo and Structure

以下を扱う。

- BPM
- Time Signature
- Tempo Change
- Marker
- Section
- Loop Region
- Count-in
- Grid / Snap

Tempo Changeを含む場合でも、AudioとMIDIの同期状態を明示する。

## 11.6 Automation

対象:

- Track Volume
- Pan
- Rack Macro
- Plugin Parameter
- Dry / Wet
- Bypass
- Send Level

点、直線、曲線で編集できる。

Recordingした操作をAutomationとして取り込める。過剰なPointを整理し、後から編集可能であること。

## 11.7 Mixer

Arrangeから必要な範囲のMixerを開ける。

- Volume
- Pan
- Meter
- Mute
- Solo
- Arm
- Send
- Return
- Group
- Master

Mixerは別世界の画面にせず、Trackと同じ概念・状態を共有する。

---

# 12. Sampleと内部音源

## 12.1 Sample Import

任意のAudioを以下として使用できる。

- One-shot
- Loop
- Drum Pad
- Keyboard Instrument
- Texture
- Effect Sound

## 12.2 Sample Edit

- Start / End
- Loop Start / End
- Crossfade
- Pitch
- Playback Rate
- Reverse
- Normalize
- Fade
- ADSR
- Filter
- Pan
- Volume
- Root Key
- Slice
- Transient Detection

編集は元Audioを保持した非破壊操作とする。

## 12.3 Drum Pad

各Padに以下を設定できる。

- Sample
- Pitch
- Volume
- Pan
- ADSR
- Output
- MIDI Note
- Choke Group
- Velocity Response
- Round Robin
- Layer

Pad構成をKitとして保存する。

## 12.4 Keyboard Instrument

単一または複数SampleをKeyboard Rangeへ配置できる。

- Root Key
- Key Range
- Velocity Range
- Round Robin
- Loop
- Pitch Tracking
- Release Sample
- Layer
- Group

簡易的な自作Instrumentを構築し、Projectをまたいで使用できる。

## 12.5 Internal Synth

外部Pluginがなくても、実際の制作に使用できる基本Synthを持つ。

最低限:

- Sine
- Triangle
- Saw
- Square
- Noise
- Multiple Oscillators
- Detune
- ADSR
- Low-pass / High-pass / Band-pass
- LFO
- Modulation Routing
- Unison
- Portamento
- Saturation
- Chorus
- Delay
- Reverb
- Macro
- Preset

機能数を増やすより、基本操作の音質、安定性、視認性、Preset再利用性を優先する。

## 12.6 Utility Devices

外部Pluginがなくても基本的な処理ができるよう、以下の内部Deviceを提供する。

- Gain
- Pan
- Utility / Mono
- EQ
- Filter
- Compressor
- Limiter
- Gate
- Saturation
- Delay
- Reverb
- Chorus
- Tuner
- Spectrum Meter
- Loudness Meter

内部Deviceは、緊急回避用の低品質な代替ではなく、日常的に使用できる一貫した品質を持つ。

---

# 13. AnalyzeとReference

## 13.1 Basic Analysis

以下を表示する。

- Waveform
- Peak
- True Peak
- RMS
- LUFS
- Spectrum
- Spectrogram
- Phase
- Correlation
- Stereo Width
- BPM Estimate
- Key Estimate
- Pitch
- Transient
- Silence
- Clipping
- Dynamic Range

解析値には、推定であること、信頼度、解析範囲を表示する。

## 13.2 Compare

二つ以上のAudio、Rack、Snapshot、Trackを比較できる。

比較対象:

- Loudness
- Frequency Balance
- Dynamics
- Stereo Width
- Peak
- Waveform
- Spectrum
- Phase
- Transient

比較再生では、Loudness Matchを提供する。

ユーザーはLoudness Matchの有無を切り替え、補正量を確認できる。

## 13.3 Reference Library

Reference TrackをProjectから独立して管理する。

各Referenceに以下を保存できる。

- Name
- Artist / Source Note
- Tag
- Intended Comparison
- Analysis
- Loop Region
- Cue Point
- Loudness
- Personal Note

Referenceは著作物の再配布を目的とせず、ユーザーのローカル比較用途として扱う。

## 13.4 Explanation

解析値を列挙するだけでなく、ユーザーが制作判断へ結び付けられる説明を提供する。

例:

- どの帯域が突出しているか
- 比較音源との差が何か
- Mono化で消える成分があるか
- Clipがどこで起きたか
- Dynamicsが過度に狭いか
- 推定結果の確実性が低い理由

説明は断定しすぎず、測定事実と解釈を区別する。

---

# 14. Stem Separation

## 14.1 Positioning

Stem Separationは、完成音源を分析、練習、再編集、Arrangementへ利用する補助機能である。

分離結果を、常に高品質で独立した原音として扱わない。Artifact、Bleed、Phase変化、High Frequency Lossなどが起こり得ることをUI上で明示する。

## 14.2 Separation

Audioを選択し、利用可能なModelに応じて分離する。

例:

- Vocal
- Drums
- Bass
- Other
- Guitar
- Piano

Modelごとに以下を示す。

- Stem構成
- Quality傾向
- Processing Time
- CPU / GPU
- Memory Requirement
- License / Usage Note
- Local / External Processing

## 14.3 Result

- Originalとの同期再生
- StemごとのMute / Solo
- Difference確認
- Waveform比較
- Gain
- Phase / Alignment確認
- Add to Timeline
- Export
- Save Processing Conditions
- Quality Note
- Retry
- Compare Models

## 14.4 Background Job

分離はJobとして扱う。

表示項目:

- Target
- Model
- Progress
- Elapsed Time
- Estimated Remaining
- Device
- Resource Usage
- Pause
- Cancel
- Failure Reason
- Retry
- Output Location

Audio再生・録音中は、Jobがリアルタイム処理を妨げないよう自動的に負荷を抑える。

アプリケーション終了後も再開可能な処理は状態を保全する。

---

# 15. AI Assistant

## 15.1 Role

AIは以下を支援する。

- Plugin Explanation
- Parameter Explanation
- Rack Explanation
- Tone Adjustment Proposal
- Audio Analysis Explanation
- MIDI Proposal
- Arrangement Suggestion
- Naming
- Tagging
- Note Organization
- Preset Comparison
- Troubleshooting
- Repetitive Operation
- Search Assistance

AIは、ユーザーの制作判断を置き換えず、理解、比較、探索、整理を支える。

## 15.2 Permission Levels

AI操作は明確に三段階へ分ける。

### Explain

状態を読み、説明する。変更しない。

### Suggest

変更案をChangeSetとして作る。適用しない。

### Apply

ユーザーが確認したChangeSetだけを適用する。

初期設定はSuggestまでとする。不可逆操作、外部送信、File削除はAIへ自動許可しない。

## 15.3 Chat Examples

- 「もう少し明るくして」
- 「歪みを増やしつつ、ピッキングの輪郭は残して」
- 「空間系を控えめにして、近い音にして」
- 「この音色を三方向に派生させて」
- 「重要なParameterだけMacroへ出して」
- 「録音が埋もれる原因を分析して」
- 「この8小節に合うBass MIDIを提案して」
- 「現在のRackを初心者にも分かるように説明して」
- 「似たPresetを探して」
- 「このPluginが重い理由を調べて」

曖昧な自然言語指示では、AIは必要な仮定を表示し、危険な変更を避ける。

## 15.4 ChangeSet Preview

AI変更案には以下を表示する。

- Target
- Current Value / State
- Proposed Value / State
- Reason
- Expected Audible Effect
- Confidence
- Risk
- Apply All
- Apply Selected
- Reject
- Preview
- Save as Alternative Snapshot

適用前後を即座に比較できる。

## 15.5 Context Control

AIへ渡す情報をユーザーが確認・制御できる。

- Selected Rack
- Parameter List
- Analysis Result
- Selected Clip
- Project Structure
- User Note
- Snapshot
- Preview Audio
- Error Log

不必要にProject全体を送らない。

## 15.6 Provider

外部Service、Local Modelを切り替えられる。

Provider不調、Rate Limit、Offline時でも中核制作機能を妨げない。

API Keyは安全に保存し、Log、Project、Export Packageへ平文で含めない。

## 15.7 Audio Sending

外部ServiceへAudioを送信する場合、送信前に以下を示す。

- 対象Audio
- Length
- Approximate Size
- Destination
- Purpose
- Retention / Privacy情報
- Trim / Downmix / Anonymizeの有無

音声送信は明示操作を必須とする。

---

# 16. LibraryとCreative Memory

## 16.1 Unified Library

以下を統一的に管理する。

- Plugin
- Preset
- Rack
- Rack Fragment
- Audio
- Recording
- MIDI
- Sampler Kit
- Instrument
- Stem
- Reference
- Project
- Template
- Analysis
- AI Suggestion

Assetの種類が違っても、検索、Favorite、Rating、Tag、Note、Preview、Related Assetの操作は一貫させる。

## 16.2 Metadata

各Assetに以下を持てる。

- Name
- Type
- Tag
- Favorite
- Rating
- Note
- Created At
- Updated At
- Last Used
- Usage Count
- Related Project
- Related Rack
- Related Recording
- Preview
- Provenance
- Compatibility
- Missing Dependency
- Archived State

Metadata入力を強制しないが、後から整理しやすい候補や自動補完を提供する。

## 16.3 Inbox

名前や分類を決めていないRecording、Import、AI Result、Separation ResultをInboxへ集める。

Inboxは「未整理だから消える場所」ではなく、**捕捉したものを失わず、後で意味付けする場所**である。

以下を提供する。

- Preview
- Quick Tag
- Rename
- Promote to Asset
- Add to Project
- Archive
- Delete
- Duplicate Detection

## 16.4 Tag

Tagは自由入力を基本とし、候補、表記揺れ統合、使用頻度を支援する。

例:

- guitar
- synth
- bass
- clean
- distorted
- bright
- dark
- wide
- dry
- ambient
- aggressive
- J-rock
- practice
- unfinished
- verse
- chorus

Folderだけに依存せず、複数観点から同じAssetへ到達できる。

## 16.5 Search

Global Searchから全Assetを横断検索できる。

条件:

- Name
- Tag
- Note
- Type
- Plugin
- Created At
- Updated At
- Rating
- Usage Count
- Project
- Audio Characteristics
- Recent
- Stability
- Missing Dependency

検索結果をSaved Searchとして保持できる。

自然言語検索を提供する場合も、実際に適用された条件を確認できる。

## 16.6 Preview

Audio、Preset、Rack、Sample、Snapshotを、現在の作業を壊さずPreviewできる。

- 元の状態へ必ず戻る
- Preview Volumeを一定範囲へ整える
- Preview Sourceを選べる
- Current Inputを通してRackを試せる
- Preview中であることを明示する
- Previewをそのまま適用できる

## 16.7 Related Assets

Asset間の関連を確認できる。

例:

- このRecordingで使用したRack
- このRackから派生したSnapshot
- このSampleを使用するKit
- このProjectからExportされたAudio
- このAnalysisの対象Audio
- このAI Suggestionを適用したVersion

Creative Memoryは、単なる検索DBではなく、制作過程の関連を辿れることを重視する。

---

# 17. Project、保存、履歴

## 17.1 Project Contents

- Timeline
- Track
- Rack
- Plugin State
- MIDI
- Referenced Audio
- Recording
- Automation
- AI History
- Note
- View State
- I/O Setting
- Export Setting
- Marker
- Version
- Missing Dependency情報

## 17.2 Auto Save

編集内容は継続的にAuto Saveする。

- 最後の明示保存状態
- 現在の未確定変更
- Recovery用状態

を区別する。

Auto Saveは一つのFileを上書きし続けず、破損時に複数世代から復旧できる。

## 17.3 Version

任意時点で名前付きVersionを作成できる。

例:

- Before Vocal Edit
- Guitar Tone Candidate A
- Arrangement 2026-07-11
- Before AI Adjustment
- Stable Before Plugin Update

Version間で、主要な変更差分を確認できる。

## 17.4 Undo / Redo

以下を含む編集を取り消せる。

- Parameter
- Rack
- Clip
- Track
- Tag
- Note
- AI ChangeSet
- Sample
- Automation
- Routing
- Import
- Delete

連続したKnob操作は、一つの意味ある操作としてまとめる。

危険操作はUndoだけへ依存せず、事前確認またはTrash / Archiveを用意する。

## 17.5 Missing Files

参照Fileが見つからない場合:

- Auto Search
- Folder指定
- Individual Relink
- Search by Hash / Metadata
- Open with Missing
- Missing List
- Replace
- Ignore

Missing Fileがあっても、Projectの残りを開ける。

## 17.6 Collect and Package

Projectに必要なAudio、MIDI、Metadataを一つのPortable Packageへまとめられる。

Plugin BinaryやLicenseを無断で含めない。

Packageには以下を含められる。

- Referenced Audio
- MIDI
- Project State
- Rack / Preset
- Rendered Fallback
- Used Plugin List
- Version Information
- Note

## 17.7 Format Migration

Project、Rack、Preset、Library MetadataにはFormat Versionを持たせる。

アプリ更新後も旧Formatを読み込めるか、変換前Backupを作成して安全にMigrationする。

新Versionで保存したために、元Dataへ戻れなくなる場合は明示する。

---

# 18. ImportとExport

## 18.1 Import

- WAV
- FLAC
- MP3
- AIFF
- OGG
- MIDI
- Application Project
- Rack
- Preset
- Kit
- Template

Drag & Drop、File Picker、Folder Scanに対応する。

Unsupported Format、Corrupt File、Sample Rate差を分かりやすく示す。

## 18.2 Audio Export

設定項目:

- Format
- Sample Rate
- Bit Depth
- Mono / Stereo
- Normalize
- Dither
- Range
- Track
- Stem
- Master
- Raw Input
- Processed Output
- Tail Length
- Real-time / Offline Render
- Metadata

Export前にClip、Missing File、Disabled Plugin、Master Peak、予想File Sizeを確認できる。

## 18.3 DAW Handoff

Ableton等との専用連携を必須にせず、標準Fileで確実に渡せることを優先する。

一括出力:

- 各Trackの開始位置を揃えたWAV
- MIDI
- BPM
- Time Signature
- Tempo Change
- Marker
- Text Note
- Used Rack List
- Used Plugin List
- Raw DI
- Processed Track

他DAWへ移した後でも、意図と同期位置を失わない。

## 18.4 Rendered Fallback

外部Plugin依存のTrackまたはRackは、必要に応じてRendered Audioを併保存できる。

将来Pluginが使えなくなっても、少なくとも当時の音を再生・Exportできる状態を残す。

---

# 19. Visual Design

## 19.1 Design Character

目指す印象:

- Precise
- Quiet
- Contemporary
- Premium
- Technical
- Trustworthy
- Focused
- Comfortable for long sessions

避けるもの:

- 古い機材を無意味に模倣したSkeuomorphism
- 常時発光するGame UI
- 過剰なGlass / Blur
- 不要な立体感
- 情報密度の高さを専門性とみなす設計
- 装飾のためのAnimation
- 狭い領域へ大量の小文字を詰め込む設計

Plugin Native GUIのSkeuomorphismは尊重するが、アプリケーション本体は現代的で統一された道具感を持つ。

## 19.2 Visual Metaphor

本製品の中心的な視覚モチーフは、**Signal Flow、Layer、Memory**である。

- Signal Flow: 音の流れ
- Layer: Track、Parallel Path、Stem、Take
- Memory: Snapshot、History、Asset、Version

単なる黒い箱の集合ではなく、「今どこで何が起きているか」が視覚的に読めること。

## 19.3 Color

Dark Themeを標準とする。

- Base: 黒に近いNeutral Graphite
- Surface: Baseよりわずかに明るいGray
- Border: 低Contrast
- Primary Text: 純白ではなく淡いGray
- Secondary Text: 低輝度Gray
- Primary Accent: 冷たいBlue-Cyan系を標準
- Recording: Red
- Warning: Amber
- Safe / Connected: TealまたはGreen
- Disabled: Colorだけでなく形状・Opacityでも表現

Accent Colorは変更可能にしてよいが、標準Themeの一貫性を崩さない。

Colorだけで状態を伝えない。

## 19.4 Active Signal

音が流れているPathは、控えめなPulse、Meter、Highlightで分かる。

常時強く発光させず、選択、Recording、Clip、異常など意味のある状態だけContrastを上げる。

## 19.5 Typography

- 長時間読めるUI Sans
- 数値は桁が揃うTabular Numerals
- dB、Hz、ms、BPMなど単位を明示
- Log、Path、Technical Valueには読みやすいMonospaceを限定使用
- 小さすぎるTextに依存しない

## 19.6 Control Design

Knob、Slider、Buttonは以下を満たす。

- Current Value
- Unit
- Default Value
- Automation State
- Macro Assignment
- Fine Adjustment
- Direct Numeric Input
- Reset
- Keyboard Operation
- Mouse Wheel
- Touchpad-friendly Interaction

操作中だけValueを出すのではなく、重要値は平常時も確認できる。

## 19.7 Density

表示密度はCompact / Comfortableを切り替えられる。

CompactでもHit Areaを過度に小さくしない。

1920×1080で主要操作が成立し、2560×1440以上では情報をただ引き伸ばさず、有効に領域を使う。

## 19.8 Motion

Animationは状態変化の理解に使う。

- Panel Expand / Collapse
- Drag
- Snapshot Switch
- Processing Progress
- Signal Activity
- Error Transition

Audio処理負荷を増やさず、Reduced Motion設定を提供する。

## 19.9 DPI and Multi-monitor

Windows Scaling 100%〜200%で破綻しない。

主対象:

- 1920×1080
- 2560×1440
- 3840×2160

異なるScalingの複数Monitor間でWindowを移動しても、Plugin GUIとApplication UIが極端に崩れない。

---

# 20. Interaction Design

## 20.1 Drag & Drop

対応:

- Plugin → Rack / Track
- Audio → Timeline / Sample / Analyze / Separate
- MIDI → Timeline / Instrument
- Rack Device Reorder
- Clip Move
- Track Move
- Asset → Tag
- Preset → Device
- Snapshot → Compare Slot
- Recording → Project

Drag中に以下を示す。

- Insert Position
- Replace / Add
- Copy / Move
- Compatible / Incompatible
- Resulting Routing
- Destructive Risk

## 20.2 Keyboard

最低限:

- Space: Play / Stop
- R: Record
- Ctrl+Z: Undo
- Ctrl+Shift+Z: Redo
- Ctrl+S: Save / Promote
- Delete: Remove
- Ctrl+D: Duplicate
- Ctrl+C / Ctrl+V: Copy / Paste
- F or Ctrl+F: Search
- L: Loop
- M: Metronome
- Esc: Cancel
- Ctrl+K: Command Palette
- F2: Rename
- Shift: Fine Adjustment

Shortcutは変更可能とし、衝突を確認できる。

## 20.3 Command Palette

画面上で探しにくい操作を、名前から実行できる。

- Action Search
- Workspace Switch
- Asset Search
- Setting
- Audio Device
- Plugin
- Recent Project

Command Paletteは隠れた必須操作の唯一の入口にはしない。

## 20.4 Selection

どの対象が選択され、どこに操作が適用されるかを明確にする。

複数選択では、共通操作と、値が混在している状態を区別する。

## 20.5 Empty, Loading, Error

空画面には、次に行う意味のある操作を示す。

Loading中は、処理対象、進捗、操作可能範囲を示す。

Errorは以下を含む。

- What happened
- Affected scope
- Data safety
- Likely cause
- Available actions
- Log / Detail

単に「失敗しました」で終わらせない。

## 20.6 Onboarding

初回起動では、長いTutorialを強制しない。

最小限のSetup:

1. Output Device
2. Input Device
3. Safety Check
4. Plugin Folder
5. Test ToneまたはInput Check

その後、実際の画面上で段階的に説明する。

---

# 21. Reliability and Recovery

## 21.1 Plugin Error

表示:

- Plugin Name
- Path
- Version
- Failed Operation
- Crash / Hang / Invalid State
- Retry
- Disable
- Replace
- Re-scan
- Open Log
- Safe Mode

## 21.2 Audio Device Error

Device Disconnect時:

- Audioを安全停止
- 代替Device候補
- Reconnect
- Use Default
- Keep Project Open
- Recording Recovery

録音中に切断されても、取得済みAudioを失わない。

## 21.3 Crash Recovery

次回起動時:

- Recover Auto Save
- Open Last Stable Version
- Suspected Plugin
- Safe Mode
- Open without Problem Plugin
- Discard Recovery

復旧画面で、どの状態が新しく、どの状態が安定版か分かる。

## 21.4 Safe Mode

Safe Modeでは、問題となり得る外部要素を抑えて起動する。

ユーザーから見て最低限以下ができる。

- Projectを開く
- Audio / MIDIをExportする
- Problem Pluginを特定する
- PluginをDisable / Removeする
- Backupを作る
- Libraryへアクセスする

## 21.5 Background Jobs

Jobは以下を持つ。

- Queued
- Running
- Paused
- Completed
- Failed
- Cancelled
- Recoverable

Job FailureがProject編集やAudio再生を止めない。

## 21.6 Storage and Disk

以下を監視・通知する。

- Low Disk Space
- Write Failure
- Read-only Location
- Path Unavailable
- External Drive Disconnect
- Slow Disk
- File Lock

録音開始前に、明らかな容量不足を警告する。

---

# 22. Performance and Quality Targets

以下は完成品質の判断に用いる目標値である。実機、Plugin、Driverに依存する項目は、検証条件を明示して評価する。

## 22.1 Startup

- 通常起動でHomeが**3秒以内**に操作可能になることを目標とする
- Audio Engineは、前回Deviceが利用可能なら**5秒以内**に利用可能になることを目標とする
- 全Plugin Scanを起動前提にしない
- AI Model、Stem Model、Analysis ModelのLoadingをHome表示の前提にしない

## 22.2 UI Responsiveness

- 通常操作は入力から**100ms以内**に反応が見える
- Searchは一般的なLibrary規模で**200ms以内**に初期結果を返すことを目標とする
- Waveform、Meter、Animationは視認上滑らかである
- Heavy Job中もUI操作とAudioを維持する

## 22.3 Audio

- Low Latency設定でギター、MIDI演奏に実用的である
- Round-trip Latencyを表示する
- Plugin Latency Compensationが成立する
- 検証済み負荷範囲でDropoutを発生させない
- UI描画、Search、Auto Save、JobがAudio ThreadをBlockしない

## 22.4 Recording

- 録音停止またはCrash時に、取得済みデータを最大限復旧する
- Dropout位置を検知できる
- 数時間のRecordingでもFile破損やMemory肥大を起こさない
- Recording中のAuto Saveが音切れを引き起こさない

## 22.5 Long-running Stability

- 数時間連続でPlay、Record、Plugin切替、Workspace切替を行ってもMemory使用量が無制限に増えない
- Plugin GUIを繰り返し開閉してもHandleやMemoryをLeakしない
- Sleep / Resume後にAudioを再初期化できる
- Device Hot Plug後に再接続できる

## 22.6 Recovery

- Auto Saveにより、通常のCrashで失う編集量を極小化する
- Auto Save破損時に前世代へ戻れる
- 問題Pluginを無効化してProjectを開ける
- Missing File / Pluginがあっても残りを利用できる

## 22.7 Library Scale

少なくとも以下の規模で、日常操作が実用的であることを目標とする。

- 1,000 Plugins / Plugin Entries
- 50,000 Audio / MIDI Assets
- 10,000 Presets / Racks
- 1,000 Projects / Sessions

規模を増やした際に、起動ごとの全再Indexを必要としない。

---

# 23. Security, Privacy, Data Ownership

## 23.1 Local First

Project、Recording、Preset、Analysis、Library Metadataは原則Localへ保存する。

中核機能にCloud Loginを要求しない。

## 23.2 User Ownership

ユーザーは以下を行える。

- Data Location確認
- Backup
- Export
- Move
- Delete
- Restore
- App Uninstall後の保持選択

独自形式を使う場合も、Audio、MIDI、Metadata、Used Plugin Listなど主要情報を取り出せる。

## 23.3 Credentials

API Key、Token、CredentialはOSの安全なCredential領域へ保存する。

以下へ平文保存しない。

- Setting File
- Log
- Project
- Crash Report
- Export Package

## 23.4 Logs

Logへ以下を不要に出さない。

- API Key
- Auth Token
- Audio Data
- AI Prompt全文
- Private File内容
- 不要なFull Path

Debugのために必要なPathを出す場合、ユーザーが共有前に確認・匿名化できる。

## 23.5 Telemetry

Telemetryを提供する場合、初期状態で最小または無効とし、収集内容と目的を明示する。

Recording、Audio Content、Project Name、File Nameを無断送信しない。

---

# 24. Windows-specific Requirements

対象OSはWindows 11 x64とする。

必須:

- Windows DPI Scaling
- ASIO
- WASAPI
- Windows VST3
- MIDI Device
- Device Connect / Disconnect
- Japanese Path
- Long Pathへの可能な範囲の対応
- Sleep / Resume
- Multiple Monitor / Mixed DPI
- Windows DefenderやFile Lockによる失敗の安全な処理
- Installer
- Uninstaller
- User Data保持選択
- Crash Dump / Log
- OS Credential Store
- File AssociationまたはOpen With
- Start Menu登録

管理者権限を日常利用の前提にしない。

Plugin Folder、Project Folder、Library Locationをユーザーが変更できる。

---

# 25. Settings

設定は、目的別に整理し、実装概念をそのまま露出させない。

主な区分:

- Audio
- MIDI
- Plugins
- Library
- Recording
- Appearance
- Shortcuts
- AI
- Stem Separation
- Storage
- Privacy
- Updates
- Diagnostics

各設定は以下を明示する。

- Current Value
- Effect
- Restart Requirement
- Risk
- Default
- Reset

Audioに影響する設定は、適用前に予想される中断を示す。

---

# 26. 対象外

以下は本アプリケーションの中心目的としない。

- 楽譜作成
- オーケストラ総譜編集
- 映像編集
- ライブ配信
- DJ
- 複数ユーザー共同編集
- Cloud前提Project管理
- 音楽配信Service
- 楽曲販売
- 著作権管理
- 既存DAW独自Projectの完全互換
- 大規模Recording Studio向けConsole機能
- Dolby Atmos等の高度なImmersive Mixing
- Video同期Post Production
- 無確認の完全自動作曲
- 無確認の完全自動Mix / Master
- Plugin Marketplace
- SNS
- Subscription必須化

対象外機能を中途半端に追加し、起動速度、Audio安定性、操作一貫性を損なってはならない。

---

# 27. 完成状態の受け入れ基準

## 27.1 Product-level Definition of Done

以下が一つの自然な体験として成立していること。

### A. Instant Play

- 起動後、Project作成なしで音を出せる
- 前回のInput、Output、Rackを復元できる
- 安全なVolumeで開始する
- Device不在時に回復操作が分かる

### B. Tone Design

- Pluginを追加し、Signal Flowを構築できる
- Macro、Snapshot、Parallel Pathを使える
- Loudnessを揃えて比較できる
- RackをPreview、Tag、Note付きで保存できる

### C. Capture

- RawとProcessedを同時録音できる
- Count-in、Loop、Take管理ができる
- 録音がInboxへ自動保全される
- Device切断後も取得済みDataを救済できる

### D. Arrange

- Audio / MIDIを配置、編集、Automationできる
- Plugin Latencyを補償できる
- Track、Stem、Masterを書き出せる
- 外部DAWへ同期位置を維持して渡せる

### E. Sample

- Audioを切り出し、Pad / Keyboardへ割り当てられる
- Kit / Instrumentを保存・再利用できる
- MIDI演奏・録音できる

### F. Analyze

- Waveform、Spectrum、Loudness、Phaseを確認できる
- ReferenceとLoudness-matched Compareできる
- 測定事実と解釈を区別して確認できる

### G. Separate

- AudioをBackgroundでStem分離できる
- Originalと同期比較できる
- 結果をTimeline、Library、Exportへ利用できる
- Artifactの可能性をユーザーが理解できる

### H. AI

- AIが説明、提案、ChangeSet作成を行える
- Apply前に差分を確認できる
- 一部適用と一括Undoができる
- 外部送信Dataを制御できる

### I. Creative Memory

- Rack、Preset、Recording、Sample、MIDIを横断検索できる
- ProvenanceとRelated Assetを辿れる
- Previewで現在作業を壊さない
- Missing Dependencyを把握できる

### J. Recovery

- Plugin Crashから本体を守れる
- Auto Saveから復旧できる
- Safe ModeでProjectを開ける
- Missing Plugin / Fileがあっても残りを利用できる

## 27.2 製品として失敗とみなす状態

以下のいずれかが常態化する場合、機能が多くても完成とはみなさない。

- 起動して音を出すまでに毎回複数の設定が必要
- Plugin一つのCrashでアプリ全体が落ちる
- 録音が保存操作の漏れで消える
- UI操作でAudio Dropoutが起きる
- Snapshot比較によって現在設定を失う
- AIが何を変更したか分からない
- Libraryへ保存しても後から見つからない
- Projectが特定Plugin不在だけで開けない
- 見た目は美しいが、現在のSignal Flowが理解できない
- 高機能だが、日常の短い演奏や録音が面倒
- Dataが独自形式に閉じ込められ、Exportできない

---

# 28. 実装上の最低限の境界

本書はHowを中心としないが、製品品質を守るために、以下の責務を混同しない。

- UI / Interaction
- Session / Project Model
- Library / Asset Model
- Real-time Audio Engine
- MIDI Engine
- Plugin Host
- File I/O
- Background Jobs
- Analysis
- AI Integration
- Persistence
- Recovery / Diagnostics

最低限、以下を守る。

- Real-time Audio処理はUI、AI、Disk Scan、Jobから独立させる
- Plugin ScanまたはPlugin Crashで本体を巻き込まない
- Project保存形式へ特定Pluginの内部型を直接露出させない
- 特定AI Provider、Separation Model、Plugin Formatへ全体を密結合させない
- 保存形式にVersionを持たせる
- UI状態と音声状態のSource of Truthを曖昧にしない
- Undo、Auto Save、Recoveryで同じ変更概念を扱える
- Heavy JobはAudio処理より優先しない
- User Dataの削除は明示的かつ復元可能にする

技術選定は、これらの製品要件を満たすための手段として行う。

---

# 29. 最終的な製品像

本アプリケーションは、機能数を増やした小型DAWではない。

それは、ユーザーが音楽へ触れる瞬間から、発見したものを長期的な制作資産へ変えるまでを支える、個人用の音楽制作環境である。

目指す状態は以下である。

- 起動したらすぐ音が出る
- 音作りに集中できる
- 良い演奏や良い音が失われない
- 候補を公平に比較できる
- Pluginを使う行為そのものが整理される
- 制作の意図と由来が残る
- AIが判断を奪わず、理解と試行回数を増やす
- 高価なSoftwareを増やさなくても、自分の音を蓄積できる
- 見た目が美しく、触っていて気持ちがよい
- Windows上で安定して長時間使用できる
- Ableton等の本格DAWと競合せず、前段、補助、実験、資産化の場として共存できる
- PluginやServiceが変化しても、ユーザーの音とDataが残る
- 使うほど、ユーザー自身の制作方法を反映した道具へ育つ

製品の価値は、一曲を自動的に完成させることではない。

**音を出す、試す、理解する、記録する、比較する、整理する、再利用する。  
その循環を、最も安全に、速く、美しく、途切れなく行えること。**

それが本アプリケーションの完成形である。
