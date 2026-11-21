# フロントエンドのディレクトリ構成と責務

`apps/desktop/src` の各ディレクトリが何を持つかと、新しいファイルをどこへ置くかを定める。7章はテストの置き場所、8章は現状からこの構成へ移るための作業の単位と手順である。

## 1. 目的

- ファイルの置き場所を、内容から1つに決められるようにする。同じ概念が複数のディレクトリに分かれない。
- 依存の向きを一方向に保つ。下の層が上の層を知らない。
- 「コンポーネント」「hook」「純粋関数」を、どのディレクトリでも同じ規則で置き分ける。

## 2. 層と依存の向き

### 2.1 トップレベル

```text
src/
  main.tsx
  app/        起動・画面の骨格・アプリ全体の状態表示・Workspaceの組み立て
  features/   機能ごとの画面と、その機能だけが使う hook・計算
  session/    セッションストア（画面をまたいで共有する状態と操作、Nativeイベントの接続）
  model/      React・DOM・Nativeに依存しない型とドメインの計算（generated/ を含む）
  native/     Tauri との境界（Native API、命令の窓口、ダイアログ、ブラウザ表示用の代替実装）
  shared/     ドメインを知らない共通部品（UI部品、DOM操作、汎用hook、通知、エラー文字列）
  styles/     全体のスタイルとデザイントークン
  test/       テスト（実装と同じ階層）と、テスト支援（support/）
```

### 2.2 依存の向き

```text
app ──▶ features ──▶ session ──▶ native ──▶ model
  │         │           │                    ▲
  └─────────┴───────────┴──▶ shared ─────────┘
```

| 層         | 依存してよい先                                     |
| ---------- | -------------------------------------------------- |
| `app`      | `features`、`session`、`native`、`shared`、`model` |
| `features` | `session`、`native`、`shared`、`model`             |
| `session`  | `native`、`shared`（`ui` を除く）、`model`         |
| `native`   | `model`                                            |
| `shared`   | `model`                                            |
| `model`    | なし                                               |

- `features` は `app` に依存しない。
- 機能同士の依存は次の2つだけを認める。どちらも、他の機能の入口となるコンポーネントを1つ使うだけである。
  - `devices → mixer`：`SidechainSendControls`（サイドチェインの送りはMixerのルーティングとデバイスの両方で編集する）
  - `mixer → render`：`MixdownDialog`（Mixerから書き出しを開く）
- `shared/ui` は `session` に依存しない。ストアの値を受け取る部品は、必要な形の型を自分で定義し、propsで受け取る。
- `features` から `native` への直接の依存は、`native/invoke`（`HostConnectionChangedError`、`logNativeError`）、`native/dialog`、型だけの import に限る。Native APIの呼び出しは `useSessionContext().api` から行う。

## 3. 各層の中身

### 3.1 `model/`

React・DOM・Native・ストアに依存しない型と関数。複数の機能、または `session` が使うものを置く。1つの機能だけが使う計算は、その機能の `model/` に置く（4章）。

| ファイル                 | 内容                                                                               |
| ------------------------ | ---------------------------------------------------------------------------------- |
| `generated/`             | Rust から ts-rs で生成した型。手で編集しない                                       |
| `asset-id.ts`            | AssetId の変換                                                                     |
| `audio-settings.ts`      | 要求した音声設定と実際に有効な設定の突き合わせ                                     |
| `audio-status.ts`        | AudioStatus の解釈（音声エンジンを使えるか、緊急ミュート中か）                     |
| `automation.ts`          | オートメーションの対象・所有者・値                                                 |
| `clip-grid.ts`           | スナップグリッド                                                                   |
| `clip-time.ts`           | クリップの長さと終端                                                               |
| `editor-selection.ts`    | 編集中の選択                                                                       |
| `instrument-search.ts`   | 音源・プラグインの検索                                                             |
| `midi.ts`                | ノート名、黒鍵の判定、ノートオン・オフのバイト列                                   |
| `mix.ts`                 | ゲイン・パンの範囲（`MixRange`）と表示形式（`formatGainDb`、`formatPan`）          |
| `placement.ts`           | ライブラリの項目をアレンジへ配置するときの種類（`BrowserPlacement`）と配置先の決定 |
| `reconcile-canonical.ts` | 正準スナップショットの参照の保持                                                   |
| `session-read-model.ts`  | 受け入れたセッションの読み取り専用の索引                                           |
| `time-format.ts`         | 音楽的な位置・長さ・時計表示の書式と解釈                                           |
| `timebase.ts`            | tick・秒・フレーム・小節の変換                                                     |
| `track-color.ts`         | トラックの表示色                                                                   |

### 3.2 `native/`

| パス                     | 内容                                                                                              |
| ------------------------ | ------------------------------------------------------------------------------------------------- |
| `native-api.ts`          | `NativeApi` の型（各 `api/*` の型の合成）                                                         |
| `invoke.ts`              | 命令の呼び出し、Host接続の世代管理、Nativeエラーの型                                              |
| `dialog.ts`              | ファイル・フォルダの選択ダイアログ                                                                |
| `api/`                   | 領域ごとの Native API（`audio.ts`、`clips.ts`…）、`index.ts`、`native-implementation.ts`          |
| `preview/`               | Tauri の外（ブラウザ）で動かすときの代替実装                                                      |
| `preview/preview-api.ts` | `createBrowserPreviewApi`                                                                         |
| `preview/fixtures.ts`    | 代替実装とテストが使う初期セッション（`canonicalState`、`defaultSession`、`defaultProjectState`） |

このコードベースで「Browser」はライブラリのパネル（`BrowserPanel`）を指す。Webブラウザ上での実行は「preview」と呼び、ファイル名にも `browser` を使わない。

### 3.3 `session/`

画面をまたいで共有する状態と、その状態を変える操作。共有する状態の範囲は `docs/architecture.md` のとおり（接続・Project・Runtime・音声・Transport・ジョブ・録音・見つからない素材・カタログ・Workspaceの選択と下部エリア）。

| パス                  | 内容                                                                    |
| --------------------- | ----------------------------------------------------------------------- |
| `store.ts`            | ストアの生成と型                                                        |
| `session-context.tsx` | `SessionProvider`、`useSessionContext`、`useSessionStore`、`useSession` |
| `commit.ts`           | 編集結果の受け入れ（`SessionCommit`）                                   |
| `selectors.ts`        | ストアから値を取り出す関数                                              |
| `runtime.ts`          | Nativeイベントとタイマーをストアへ接続する（`bindSessionRuntime`）      |
| `audio-meters.ts`     | 高頻度で更新するメーター値のストア（React の描画とは独立して配信する）  |
| `slices/`             | 各スライス。`audio-defaults.ts` は audio スライスの初期値               |
| `hooks/`              | ストアや Native API を使う、機能をまたいだ hook                         |

`session/hooks/` に置く hook：

| hook                          | 内容                                             |
| ----------------------------- | ------------------------------------------------ |
| `useSessionRuntime`           | アプリの寿命のあいだ `bindSessionRuntime` を保つ |
| `useSessionCommand`           | 開始した画面の寿命に結びついたコミット           |
| `useSessionRead`              | 補助的な Native 読み取りの共有                   |
| `usePreviewedValue`           | プレビューとコミットを伴う値の編集               |
| `useChannelMixValue`          | トラック・バスのゲインとパン                     |
| `useMasterGainValue`          | マスターゲイン                                   |
| `useInstrumentMidiInspection` | 音源の MIDI 設定の検査結果                       |

### 3.4 `shared/`

ドメインの概念を持たない共通部品。`session` と個別の機能に依存しない。

| パス                | 内容                                                                                                                                            |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `ui/`               | 共通 UI 部品（`Dialog`、`Modal`、`Toolbar`、`ValueSlider`、`ResizeHandle`、`Icon`、`Meter` など）と `controls.module.css`、`Surface.module.css` |
| `ui/message/`       | 通知・エラー・待ちの表示部品（`design/frontend-notifications.md`）                                                                              |
| `hooks/`            | `usePointerGesture`、`useAutomationValueDisplay`                                                                                                |
| `browser-drag.ts`   | ライブラリの項目をドラッグで運ぶときの DataTransfer の書き込みと読み取り。読み取りは `BrowserPlacement` または拒否の理由を返す                  |
| `errors.ts`         | エラーの表示文字列                                                                                                                              |
| `input.ts`          | 文字入力を持つ要素かどうかの判定                                                                                                                |
| `notify.ts`         | トーストの発行と購読                                                                                                                            |
| `path.ts`           | パスからファイル名を取り出す（`fileName`）                                                                                                      |
| `rect-selection.ts` | 矩形選択に含まれる要素                                                                                                                          |

`shared/ui/primitives.tsx` は置かない。`Icon` は `ui/Icon.tsx`、`Meter` は `ui/Meter.tsx`（`Meter.module.css` と同じ場所）に分ける。

### 3.5 `styles/`

| ファイル                    | 内容                                           |
| --------------------------- | ---------------------------------------------- |
| `tokens.css`                | Root のデザイントークン（正本）                |
| `styles.css`                | 全体のスタイル                                 |
| `BrandTokens.module.css`    | ブランドの配色（AppShell が使う）              |
| `PianoKeyTokens.module.css` | 鍵盤の配色（live-input と midi-editor が使う） |
| `TimelineTokens.module.css` | 時間軸の配色（timeline と midi-editor が使う） |

機能をまたいで使う機能固有のトークンは `styles/` に置き、1つの機能だけが使うトークンはその機能の CSS に置く。

### 3.6 `app/`

アプリの骨格を組み立て、アプリ全体に関わる状態を表示する。機能の中身（何を編集するか）は持たない。

```text
app/
  App.tsx                 SessionProvider とストアの生成
  AppShell.tsx            画面の骨格。コマンドパレットの項目
  AppShell.module.css
  hooks/
    useAppShortcuts.ts    どの画面でも効くショートカット（ミュート、コマンドパレット、Undo/Redo）
    useAppUpdater.ts      アプリの更新確認
  layout/                 骨格を構成する領域
    BootScreen.tsx
    CommandPalette.tsx
    GlobalControlBar.tsx
    LeftColumn.tsx
    ProjectBoundRegion.tsx
  status/                 アプリ全体の状態の表示（待機表示と通知バー）
    NoticeBar.tsx
    WorkspaceWait.tsx
    BackgroundJobNotice.tsx
    PlaybackNotice.tsx
    PreviewModeNotice.tsx
    SafeModeNotice.tsx
    UpdateNotice.tsx
    hooks/useWorkspaceWait.ts
    model/runtime-status.ts   Runtime の状態から、待機・上部バー・通知バーに出す内容を作る
  workspace/              編集画面の組み立て
    EditorWorkspace.tsx
    WorkspaceLowerArea.tsx
    hooks/useWorkspaceShortcuts.ts   Workspace でだけ効くショートカット（再生・停止、PCキーボード演奏の切り替え）
```

- 機能が持つ通知（`RecoveryNotice`、`MissingDependenciesNotice`、`ScoreSyncNotice`）は機能に置き、`NoticeBar` が並べる。並び順は `NoticeBar` が決める。
- `EditorWorkspace` は演奏入力（`useLivePlay`）の寿命を持つ。演奏入力はパネルの表示から独立した Workspace の寿命を持つためである。演奏入力の表示内容（音域の表記、使えない理由）は live-input が作る（5.3）。

### 3.7 `features/`

| 機能          | 持つもの                                                                             |
| ------------- | ------------------------------------------------------------------------------------ |
| `audio`       | 音声設定ダイアログ、音声モニター                                                     |
| `devices`     | デバイスチェーン、パラメーター、プラグインの選択、音源の MIDI 設定                   |
| `inspector`   | Properties パネルと、トラック・クリップ・テイクの Inspector                          |
| `library`     | Browser パネル（ライブラリの一覧・検索・編集・受信箱・試聴）                         |
| `live-input`  | 演奏入力（PCキーボード、ドラムパッド、Play Surface）                                 |
| `midi-editor` | MIDI エディター                                                                      |
| `mixer`       | ミキサー、ルーティング、送り                                                         |
| `project`     | プロジェクトと Host の選択、プロジェクトの削除、復旧、見つからない素材、Score の同期 |
| `render`      | 書き出しと書き出し履歴                                                               |
| `timeline`    | アレンジの描画と編集、ライブラリの項目の配置                                         |
| `transport`   | 再生・録音・カウントインの操作                                                       |

機能名と UI 上の呼び名が違うものは、機能名を Native API とストアに揃え、コンポーネント名を UI 上の呼び名に揃える（`features/library/BrowserPanel.tsx`）。

## 4. ディレクトリの中の置き分け

`app/` の各領域、`features/*`、`session/`、`shared/` のすべてで、次の規則を使う。

### 4.1 種類ごとの置き場所

| 種類                                                                      | 置き場所                        | ファイル名              |
| ------------------------------------------------------------------------- | ------------------------------- | ----------------------- |
| コンポーネント                                                            | ディレクトリ直下                | `PascalCase.tsx`        |
| コンポーネントの CSS                                                      | コンポーネントと同じ場所        | `PascalCase.module.css` |
| hook                                                                      | `hooks/`                        | `useXxx.ts`             |
| hook だけが使う、React に依存しない副作用のある補助（DOM の直接操作など） | それを使う hook と同じ `hooks/` | `kebab-case.ts`         |
| React・DOM・Native に依存しない計算と型                                   | `model/`                        | `kebab-case.ts`         |
| テスト                                                                    | `src/test/` の、対象と同じ階層  | `対象名.test.ts(x)`     |

- `hooks/` と `model/` は、その種類のファイルが1つでもあれば作る。1ファイルでもフォルダに入れ、置き場所の規則に例外を作らない。
- コンポーネントのファイルに hook を定義しない。そのコンポーネントだけが使う hook でも `hooks/` に置く。
- hook のファイルに、他のファイルから使う純粋関数を置かない。
- 機能の中で使い回す計算は、その機能の `model/` に置く。2つ以上の機能、または `session` が使うようになったら `src/model/` へ移す。

### 4.2 コンポーネントのファイルに残してよい関数

次の両方に当てはまる関数は、コンポーネントのファイルの中に非公開で置いてよい。

- そのファイルの中だけで使う。
- ドメインのデータを解釈しない。文字列の組み立て（`plural`）や、そのコンポーネント固有の寸法の計算（左カラムの幅の制約）がこれにあたる。

ドメインのデータを解釈する関数（テイクの並び順、既定で使う音源、設定どうしの比較、カウントインの次の値）は、1か所で使うだけでも `model/` に置く。

### 4.3 コンポーネントの公開

- 機能の入口となるコンポーネントは、ストアから自分で値を読む。props で受け取るのは、ストアにない値（Workspace の寿命を持つ演奏入力など）とコールバックだけである。
- ストアの状態を props で受け取るだけの「View」版のコンポーネントは作らない。テストは `TestSessionProvider` でストアを与えて入口のコンポーネントを描く（`docs/test-strategy.md`）。
- テストからしか使わない export は作らない。
- コンポーネント名に、置かれる場所の名前（`Workspace`）を付けない。

## 5. 1か所にまとめるもの

### 5.1 ライブラリの項目の配置

ライブラリの項目をアレンジへ置く経路は2つある：Browser の「適用」（再生位置へ）と、Timeline へのドロップ（ドロップした位置へ）。どちらもアレンジの編集なので、実行は timeline が持つ。

| 責務                                          | 置き場所                                         |
| --------------------------------------------- | ------------------------------------------------ |
| 配置の種類と配置先の決定                      | `model/placement.ts`                             |
| ドラッグで運ぶ内容の書き込み・読み取り        | `shared/browser-drag.ts`                         |
| 配置の実行（Native API の呼び出しとコミット） | `features/timeline/hooks/usePlaceBrowserItem.ts` |

`usePlaceBrowserItem` は、位置を指定する配置と、再生位置への配置の両方を返す。`useTimelineDrop` は前者を、`AppShell` は後者を `BrowserPanel` の `onApply` に渡す。

### 5.2 機能をまたいで使う、値の編集用 hook

`useMasterGainValue` は `session/hooks/` に置き、`audio` と `mixer` がそこから使う（3.3）。`mixer → audio` の依存はなくす。

### 5.3 演奏入力の表示

`TimelineView` は演奏入力の型（`TimelineLiveInputControls`）を受け取らない。代わりに、ツールバーの主のグループの最後に置く要素を `toolbarAccessory: ReactNode` で受け取る。`TimelineToolbar` はその要素を、組み込みの切り替えの直後に描き、ルーラー表示とズームの前に置く。live-input は、その要素として `LiveInputToggle`（`LivePlay` を受け取り、音域の表記と使えない理由を作って切り替えボタンを描く）を提供する。`EditorWorkspace` は `<TimelineView toolbarAccessory={<LiveInputToggle play={livePlay} />} />` と組み立てる。

### 5.4 ショートカット

| どこで効くか      | 置き場所                                       |
| ----------------- | ---------------------------------------------- |
| どの画面でも      | `app/hooks/useAppShortcuts.ts`                 |
| Workspace でだけ  | `app/workspace/hooks/useWorkspaceShortcuts.ts` |
| 1つの機能の中だけ | その機能の `hooks/`（`useTimelineShortcuts`）  |

演奏入力のキー（PCキーボード、ドラムパッド）はショートカットではなく演奏の入力なので、live-input の hook が持つ。

### 5.5 パネルの寸法の変更

`LeftColumn`（幅と、Browser / Properties の境界）と `WorkspaceLowerArea`（高さ）は、どちらもポインターのドラッグと矢印キー（8px、Shift で 24px）で寸法を変える。この操作を `ResizeHandle` に持たせる。

```ts
interface ResizeHandleProps {
  orientation: 'horizontal' | 'vertical';
  ariaLabel: string;
  ariaValueMin: number;
  ariaValueMax?: number;
  ariaValueNow: number;
  /** 操作を始めた時点の値。 */
  value: number;
  /** 右または下へ動かしたときに値が増えるなら 1、減るなら -1。 */
  direction: 1 | -1;
  /** 開始時の値と移動量から、次の値を決める。制約（最小・最大・折りたたみ）は呼び出し側が持つ。 */
  onResize(startValue: number, delta: number): void;
  onDraggingChange?(dragging: boolean): void;
  style?: CSSProperties;
}
```

`WorkspaceLowerArea` は `document.querySelector('[data-arrange-workspace]')` で親の高さを読まず、親の要素を ref で受け取る。

### 5.6 重複している関数

| 関数                         | 今ある場所                                                                                                                                | まとめる先                                                                                                                             |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| パスからファイル名を取り出す | `MixdownDialog`（`fileName`）、`RenderHistoryPanel`（`shortPath`）、`ProjectHostSelector`（`basename`）、`runtime-status`（プラグイン名） | `shared/path.ts` の `fileName`。末尾の区切り文字と空のパスを扱う `basename` の挙動に揃える                                             |
| 線形の値から dBFS への変換   | `MixerMeter`（`dbfs`、`formatDb`）、`MixerMasterChannelStrip`（`formatMeterDb`）                                                          | `features/mixer/model/meter-level.ts`。下限の扱い（`MixerMeter` は -60 dB 以下を −∞ と表記）は表示ごとに引数で残し、表示結果を変えない |
| トラックの表示色             | `resolveTrackColor(track, tracks.indexOf(track))` を書いている5か所                                                                       | `model/track-color.ts` の `resolveTrackColorById`                                                                                      |

## 6. 置き場所を変えるファイル

ここにないファイルは今の場所のままである。CSS Module は、対応するコンポーネントと一緒に移す。

### 6.1 下の層

| 今の場所                                                                                                   | 移す先                                      |
| ---------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| `shared/session/store.ts`、`session-context.tsx`、`commit.ts`、`selectors.ts`、`runtime.ts`                | `session/`                                  |
| `shared/session/slices/*`                                                                                  | `session/slices/*`                          |
| `shared/session/use*.ts`                                                                                   | `session/hooks/use*.ts`                     |
| `app/runtime/useSessionRuntime.ts`                                                                         | `session/hooks/useSessionRuntime.ts`        |
| `features/audio/hooks/useMasterGainValue.ts`                                                               | `session/hooks/useMasterGainValue.ts`       |
| `shared/audio/audio-meters.ts`                                                                             | `session/audio-meters.ts`                   |
| `shared/audio/audio-defaults.ts`                                                                           | `session/slices/audio-defaults.ts`          |
| `shared/audio/audio-safety.ts`                                                                             | `model/audio-status.ts`                     |
| `shared/audio/reconcile-settings.ts`                                                                       | `model/audio-settings.ts`                   |
| `shared/audio/mix-format.ts`、`model/mix-range.ts`                                                         | `model/mix.ts`（1ファイルにまとめる）       |
| `shared/midi.ts`                                                                                           | `model/midi.ts`                             |
| `shared/browser-placement.ts`                                                                              | `model/placement.ts`                        |
| `shared/usePointerGesture.ts`                                                                              | `shared/hooks/usePointerGesture.ts`         |
| `shared/useAutomationValueDisplay.ts`                                                                      | `shared/hooks/useAutomationValueDisplay.ts` |
| `shared/ui/primitives.tsx`                                                                                 | `shared/ui/Icon.tsx`、`shared/ui/Meter.tsx` |
| `shared/ui/PianoKeyTokens.module.css`、`shared/ui/TimelineTokens.module.css`、`app/BrandTokens.module.css` | `styles/`                                   |
| `native/api/browser-preview.ts`                                                                            | `native/preview/preview-api.ts`             |
| `native/browser-defaults.ts`                                                                               | `native/preview/fixtures.ts`                |

`shared/ui/ValueSlider.tsx` は `PreviewedValue` を import しない。`control` の型（`value`、`begin`、`change`、`commit`、`cancel`）を `ValueSlider` 側で定義する。`usePreviewedValue` の戻り値はその型を満たす。

### 6.2 app と features

| 今の場所                                                                          | 移す先                                                                     |
| --------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `app/useAppShortcuts.ts`                                                          | `app/hooks/useAppShortcuts.ts`                                             |
| `app/runtime/useAppUpdater.ts`                                                    | `app/hooks/useAppUpdater.ts`                                               |
| `app/layout/ProjectHostSelector.tsx`                                              | `features/project/ProjectHostSelector.tsx`                                 |
| `app/layout/NoticeBar.tsx`、`app/layout/WorkspaceWait.tsx`                        | `app/status/`                                                              |
| `app/notices/*.tsx`                                                               | `app/status/*.tsx`（`BrowserPreviewNotice` は `PreviewModeNotice` に改名） |
| `app/runtime/useWorkspaceWait.ts`                                                 | `app/status/hooks/useWorkspaceWait.ts`                                     |
| `app/runtime/runtime-projection.ts`                                               | `app/status/model/runtime-status.ts`                                       |
| `app/workspace/useBrowserPlacement.ts`、`features/timeline/place-browser-item.ts` | `features/timeline/hooks/usePlaceBrowserItem.ts`（1つにまとめる）          |
| `EditorWorkspace.tsx` の中の keydown 処理                                         | `app/workspace/hooks/useWorkspaceShortcuts.ts`                             |

`app/runtime/` と `app/notices/` はなくなる。

### 6.3 機能の中

| 今の場所                                                                                                                                                                      | 移す先                                                                                    |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `features/audio/audio-settings.ts`                                                                                                                                            | `features/audio/model/settings-draft.ts`（`AudioSettingsDialog` の `sameDraft` もここへ） |
| `features/devices/device-target.ts`                                                                                                                                           | `features/devices/model/device-target.ts`                                                 |
| `features/devices/useDeviceDetails.ts`                                                                                                                                        | `features/devices/hooks/useDeviceDetails.ts`                                              |
| `features/live-input/drum-map.ts`、`musical-typing.ts`                                                                                                                        | `features/live-input/model/`                                                              |
| `features/live-input/use*.ts`                                                                                                                                                 | `features/live-input/hooks/`                                                              |
| `features/inspector/AudioClipInspector.tsx` の `useClipMixValue`                                                                                                              | `features/inspector/hooks/useClipMixValue.ts`                                             |
| `features/inspector/TakeInspector.tsx` のテイクの並び順と既定の音源（`passOrdinal`、`compareRecordingSessions`、`compareTakes`、`sourceForVariant`、`defaultVariantForTake`） | `features/inspector/model/takes.ts`                                                       |
| `features/mixer/MixerMeter.tsx`、`MixerMasterChannelStrip.tsx` のレベル変換                                                                                                   | `features/mixer/model/meter-level.ts`                                                     |
| `features/render/MixdownDialog.tsx` の `sameMixdownSettings`                                                                                                                  | `features/render/model/mixdown-settings.ts`                                               |
| `features/transport/TransportControls.tsx` のカウントイン（`describeCountIn`、`countInBadge`、`nextCountInBeats`）                                                            | `features/transport/model/count-in.ts`                                                    |
| `features/timeline/hooks/useTimelineDrop.ts` の `isBrowserItemDrag`、`readBrowserDrag`                                                                                        | `shared/browser-drag.ts`                                                                  |
| `features/library/hooks/useBrowserTreeNavigation.ts` の `rowId`、`visibleRows`                                                                                                | `features/library/model/browser-tree.ts`                                                  |
| `features/midi-editor/hooks/useMidiEditorViewport.ts` の `LANE_LABEL_WIDTH`                                                                                                   | `features/midi-editor/model/pitch-grid.ts`                                                |

上の表にない、コンポーネントや hook のファイルの中の関数は、4.2 の基準で判断する。

### 6.4 入口のコンポーネント

| 今                                                       | あるべき姿                                                         |
| -------------------------------------------------------- | ------------------------------------------------------------------ |
| `DevicesPanel` / `DevicesPanelView`                      | `DevicesPanel` の1つ                                               |
| `PropertiesPanel` / `PropertiesPanelView`                | `PropertiesPanel` の1つ                                            |
| `MixerPanel` / `MixerPanelView`                          | `MixerPanel` の1つ                                                 |
| `BrowserPanel` / `BrowserPanelView`、`BrowserPanelProps` | `BrowserPanel` の1つ（`onApply` だけを受け取る）                   |
| `AudioSettingsDialog` / `AudioSettingsDialogView`        | `AudioSettingsDialog` の1つ                                        |
| `ProjectHostSelector` / `ProjectHostSelectorView`        | `ProjectHostSelector` の1つ                                        |
| `MissingDependenciesDialog` / `MissingDependenciesView`  | `MissingDependenciesDialog` の1つ                                  |
| `WorkspacePlaySurface` / `PlaySurface`                   | `PlaySurface` の1つ（ストアから自分で読み、`play` だけを受け取る） |
| `WorkspaceMidiEditor` / `MidiEditorPanel`                | `MidiEditorPanel` の1つ（ストアから自分で読む）                    |

View 版を使っていたテストは、`TestSessionProvider`（`renderWithSession`）でストアと FakeNativeApi を与え、入口のコンポーネントを描くように書き直す。View の props で作っていた状態は、ストアへ渡す初期値（`TestSessionSeed`）か、FakeNativeApi の応答で作る。

## 7. テストの置き場所

- テストは実装と分けて `src/test/` に置く。`src/test/` の下は実装と同じ階層にし、対象のファイルに対応するテストは `test/<実装と同じパス>/対象名.test.ts(x)` に置く（`features/timeline/model/timeline-layout.ts` のテストは `test/features/timeline/model/timeline-layout.test.ts`）。
- 1つのファイルに対応しないテストは、検証する振る舞いを持つディレクトリに対応する階層に、振る舞いの名前で置く（`test/app/undo-redo.test.tsx`）。
- テスト支援（FakeNativeApi、Provider、描画ヘルパー）は `test/support/` に置く。
- 実装のファイルを移すときは、そのテストも同じコミットで対応する階層へ移す。実装にない階層を `src/test/` に残さない。

実装の階層と合っていないテストを、次のように直す。

| 今の場所                                            | 移す先                                                                                          |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `test/features/midi-editor/useNoteEditing.test.tsx` | `test/features/midi-editor/hooks/useNoteEditing.test.tsx`                                       |
| `test/app/layout/ProjectDeleteFlow.test.tsx`        | `test/features/project/ProjectDeleteFlow.test.tsx`（6.2 で `ProjectHostSelector` と一緒に移す） |
| `test/shared/session/resources.test.tsx`            | `test/session/hooks/session-reads.test.tsx`（6.1 で `session/` と一緒に移す）                   |
| `test/features/arrange/`（中身なし）                | 消す                                                                                            |

`test/app/workspace/LiveInput.test.tsx` は、Workspace の寿命を持つ演奏入力を検証するテストなので今の場所のままにする。

## 8. 作業の単位と順序

全体を1つの PR にし、次の単位ごとにコミットを分ける。単位は表の順に進め、各単位のコミットの時点で 8.2 の検証を通す。どの単位でも、画面の見た目と挙動を変えない。

| 順  | 単位                     | 内容                                                                 | 先に行う理由                                                   |
| --- | ------------------------ | -------------------------------------------------------------------- | -------------------------------------------------------------- |
| 1   | 下の層                   | 3.1〜3.5、5.2、5.6、6.1、7章の階層の修正（ProjectDeleteFlow を除く） | 上の層の import 先が確定する                                   |
| 2   | app と features の境界   | 3.6、5.1、5.3〜5.5、6.2                                              | 1 で確定した import 先を使う                                   |
| 3   | 機能の中の置き分けと入口 | 4章、6.3、6.4                                                        | 2 でファイルの置き場所が確定した後に、ファイルの中身を整理する |

### 8.1 各単位の進め方

1. ファイルを移すときは `git mv` を使い、履歴を残す。対応するテストも `src/test/` の中で同じように移す（7章）。
2. import はすべて `@/` から始める絶対パスに書き換える。同じディレクトリの中（`./`）だけ相対パスを許す。`../` は使わない（CSS の `composes` も同じ）。
3. 移したあと、古いパスを参照している箇所が残っていないことを確かめる（`rg "@/shared/session"` など、移す前のパスで検索して0件）。
4. 空になったディレクトリを消す。

### 8.2 検証

各単位のコミットで次を通す。

```powershell
npm run typecheck --workspace @riffra/desktop
npm run build --workspace @riffra/desktop
npm run test --workspace @riffra/desktop
npx eslint apps/desktop/src
npx prettier --check apps/desktop/src
npm run lint:css
```

加えて、依存の向き（2.2）を守っていることを検索で確かめる。

```powershell
rg "from '@/(app|features)/" apps/desktop/src/session apps/desktop/src/shared apps/desktop/src/native apps/desktop/src/model   # 0件
rg "from '@/app/" apps/desktop/src/features                                                                                 # 0件
rg "from '@/session/" apps/desktop/src/shared                                                                               # 0件
rg "from '@/features/" apps/desktop/src/features                                                                            # 2.2 の2件と、自分の機能の中だけ
```

### 8.3 docs の更新

単位3のあとに docs の更新を1つのコミットにし、次の記述を3章・4章・7章に合わせて書き直す。

- `docs/architecture.md`：フロントエンドの領域の表、Workspace の説明、依存の段落、セッションストアの場所
- `docs/test-strategy.md`：テストの階層として挙げている領域の名前（`session/` を加える）と、セッションストアの場所

## 9. 対象外

- コンポーネントの中で `commit(api.xxx())` を直接書く編集操作の書き方。今の書き方はどの機能でも同じなので、置き場所の整理では変えない。
- `shared/ui/controls.module.css` のクラスを、各機能の CSS が `composes` で直接使う作り。共通 UI をコンポーネントとして提供するか CSS のクラスとして提供するかは、別に決める。
- timeline の中をさらに領域（ルーラー、トラック、クリップ、オートメーション）で分けること。部品どうしの結びつきが強く、分けると領域をまたぐ import が増える。
- ストアのスライス名（`catalog`）と機能名（`library`）の違い。
