# Riffra アーキテクチャ

## 1. 目的

Riffraは、音を最初に扱いながら、音色設計、録音、編集、分析、再利用を一つの制作文脈でつなぐWindows向けワークベンチです。

この文書で定めるのは、画面や個別機能の一覧ではなく、製品を構成する責務と境界です。要求の定義は [製品挙動要件](./behavior-requirements.md)、適合判定と課題の進捗は [挙動確認・課題管理表](./behavior-verification.md) で管理します。

## 2. 製品の中心モデル

Riffraでは、ユーザーが扱うものを「音声ファイル」と「画面上の状態」に分けません。演奏、音色、編集、判断の履歴を、再利用可能なAssetとして一つの系統に保持します。

| 概念                       | 役割                                                                                         |
| -------------------------- | -------------------------------------------------------------------------------------------- |
| **Scratch Session**        | 起動直後から存在する作業空間。プロジェクト作成を要求せず、思いつきを受け止める               |
| **Project**                | Session、Track、Clip、Rack、Plugin、MIDI、設定、履歴をまとめた制作単位                       |
| **Track / Clip**           | 時間軸上の編集単位。原本を変更せず、位置・範囲・音量・パンなどの参照を持つ                   |
| **Rack / Device**          | 入力から出力までの音の経路。Plugin、内部音源、Utility、Macro、Snapshotを含む                 |
| **Recording / Take Group** | 演奏の原本と、その時点のDevice、Rack、設定、演奏条件を結ぶ記録                               |
| **Asset**                  | Recording、Sample、Preset、Rack、Analysis、Stem、MIDI、Projectなど、後から探して使える成果物 |
| **Library**                | Assetの検索、タグ、評価、関連付け、履歴を横断的に扱う索引                                    |
| **Job**                    | Analysis、Render、Separation、Scanなど、音声経路と独立して実行される処理                     |
| **AI ChangeSet**           | AIの提案を、対象・現在値・提案値・理由・影響・危険性とともに可逆な変更として表現したもの     |

このモデルにより、「録音した音」「その時の音色」「編集で得た結果」「なぜその変更をしたか」を切り離さずに辿れます。

## 3. 責務の層

```text
┌─────────────────────────────────────────────────────────┐
│ Interaction                                               │
│ Global Bar / Library / Workspace / Inspector / Transport │
└──────────────────────────┬──────────────────────────────┘
                           │ commands + state
┌──────────────────────────▼──────────────────────────────┐
│ Session & Orchestration                                   │
│ Session / Project / Timeline / Undo / Recovery / Jobs     │
└─────────────┬──────────────────────┬────────────────────┘
              │ assets                │ realtime intent
┌─────────────▼─────────────┐  ┌─────▼────────────────────┐
│ Asset & Creative Memory    │  │ Audio Runtime             │
│ Inbox / Library / Metadata │  │ Device / Rack / Meter     │
│ Provenance / Package       │  │ Plugin / MIDI / Recording │
└─────────────┬─────────────┘  └─────┬────────────────────┘
              │ offline jobs          │ process boundary
┌─────────────▼──────────────────────▼────────────────────┐
│ Platform & Providers                                      │
│ Windows Audio / MIDI / VST3 / Filesystem / AI Provider   │
└───────────────────────────────────────────────────────────┘
```

### 3.1 Interaction層

Interaction層は、製品の全画面で同じ操作言語を提供します。

- **Global Bar**: Session、音声状態、安全操作、検索、設定を常に見せる
- **Library Panel**: Assetを探し、現在のWorkspaceへ投入する
- **Main Workspace**: Home、Play、Arrange、Sample、Analyze、Separateを目的ごとに切り替える
- **Inspector**: 選択対象の詳細と編集を表示する
- **Transport**: 再生、停止、録音、位置、ループを共有する
- **Command / Keyboard**: ポインター操作と同じ意味をキーボードから実行する

この層は音声処理の実装を持たず、状態とCommandを表示・発行します。長い処理やエラーの責任を画面ごとに抱えないことが重要です。

### 3.2 Session & Orchestration層

Session & Orchestration層は、ユーザーの意図を永続化可能な状態へ変換します。

- SessionとProjectのライフサイクル
- Timeline、Track、Clip、Rack、設定の整合性
- Undo/Redo、Snapshot、AI ChangeSetの履歴
- Autosave、Recovery generation、Missing dependencyの扱い
- Analysis、Render、Separation、Scan Jobの開始・進捗・停止・結果

ここで保持するのは「どの音をどこで使うか」という意図です。音声の原本やPluginの実行状態そのものをこの層に埋め込まず、参照と再現に必要な記述を持ちます。

### 3.3 Asset & Creative Memory層

Asset & Creative Memory層は、成果物を失わず、後から文脈ごと再利用できるようにします。

- Inboxを入口にしたRecording、Import、AI、Separation結果の保全
- SQLite等による横断索引と検索
- Name、Tag、Favorite、Rating、Note、Created/Updatedのメタデータ
- Recording→Rack→Snapshot、Sample→Kit、Analysis→Audioなどの関連
- ProjectのPortable Packageと参照ファイルの収集
- 原本、生成物、参照関係を示すProvenance

Assetは破壊的に上書きせず、編集結果を新しい参照・生成物として表現します。削除、Archive、Promoteなどの操作も履歴から意味を辿れるようにします。

### 3.4 Audio Runtime層

Audio Runtime層は、時間制約の厳しい音声経路を担当します。

- WindowsのAudio Device、Sample Rate、Buffer、Latency
- 入出力、Meter、Mute、Limiter、Feedback保護
- Rack内のPlugin、Utility、内部音源、Parallel経路
- MIDI Port、Note、Controller、Panic
- Quick Record、Raw/Processed、Preview Voice

リアルタイム処理と、保存・UI・重い計算は分離します。Audio callbackは固定時間で処理できるデータだけを扱い、File I/O、JSON、SQLite、Network、Plugin Scanなどを直接行いません。

外部Pluginは、Riffra本体の状態と別の実行境界として扱います。Pluginの失敗がSession、録音原本、他のRackへ波及しないよう、Bypass、Disable、Placeholder、Fallbackを段階的に提供します。

### 3.5 Platform & Providers層

Platform & Providers層は、Windows固有の機能や外部サービスとの接点です。

- Windows Audio API、ASIO、MIDI、Filesystem、DPI、複数モニター
- VST3の発見、検証、ロード、状態保存
- Local AIとExternal AI Provider
- 認証情報、送信対象、保持期間、匿名化などのPrivacy制御

この層の都合を画面やProjectモデルに漏らさず、失敗・権限・利用できない機能を明示的な状態として上位へ返します。

## 4. 二つのデータフロー

### 4.1 音のフロー

```text
Audio Device / MIDI
        │
        ▼
Input Safety ──► Rack ──► Meter ──► Output Safety ──► Audio Device
        │             │
        │             ├─ Plugin Worker / Internal Instrument
        │             └─ Preview Voice / Monitor
        ▼
Recording Writer ──► Raw / Processed / MIDI Sidecar ──► Inbox Asset
```

音のフローでは、Muteと安全制限を最初から経路に含めます。録音は処理音だけでなく、後から意図を再現できる条件と原音を保存します。

### 4.2 意図のフロー

```text
User Command
    │
    ▼
Session State ──► Project / Undo / Autosave
    │                         │
    ├─► Timeline / Rack       ├─► Recovery generations
    ├─► Asset reference       └─► Portable package
    └─► Job request
              │
              ▼
       Analysis / Render / Separation
              │
              ▼
       Derived Asset + Provenance + Library index
```

音のフローと意図のフローは同期しますが、同じ実行系にはしません。音声処理が一時的にFaultになっても、既に保存したSession、Asset、履歴は引き続き扱える構造にします。

## 5. 保存と所有権

Riffraの保存は、次の順序を基本とします。

1. **原本を保持する**: Recording、Import、Plugin Stateの元データを上書きしない。
2. **参照と生成物を分ける**: ClipやSampleは原本への範囲・変換情報を持ち、Render結果は別Assetにする。
3. **意味を保存する**: 値だけでなく、Rack、Snapshot、Device、AI ChangeSet、Provenanceを残す。
4. **途中状態を表現する**: Partial、Pending、Missing、Failed、Completedを区別する。
5. **持ち出せるようにする**: Projectと参照Audio、MIDI、Fallback、Versionを収集し、Plugin BinaryやLicenseは所有権なく複製しない。

保存先は、アプリケーションデータ、Project Package、外部Audioを役割ごとに分けます。保存場所が変わっても、Asset IDとProvenanceで同じ制作履歴を辿れることを優先します。

## 6. 設計原則

- **Sound First**: 音が出るまでの経路を最優先し、見た目の完成で代替しない
- **No Project Before Sound**: 起動直後からScratch Sessionで演奏・録音を始められる
- **Non-Destructive by Default**: 原本を守り、変更を参照または新しい生成物として表現する
- **Every Good Result Becomes an Asset**: 良い音、設定、分析、提案を再利用可能にする
- **Preserve Intent**: 数値だけでなく、状態へ至った文脈と理由を残す
- **Progressive Disclosure**: 初心者には安全な既定値、上級者には詳細な経路と制御を提供する
- **One Interaction Language**: Workspaceが変わっても、選択、Transport、Undo、安全操作の意味を変えない
- **AI Is Reversible**: Explain、Suggest、Applyを分け、変更はPreview、Reject、Undoできるようにする
- **Local First, Portable Always**: 中核制作はオフラインで動き、成果物は持ち運べる
- **Fail Softly**: 失敗範囲を限定し、ユーザーが次の操作を選べる状態を残す
