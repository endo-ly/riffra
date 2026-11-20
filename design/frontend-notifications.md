# フロントエンドの通知とエラー表示

アプリがユーザーに知らせるもの（状態、出来事、失敗、待ち）を、どこに・どの見た目で・どのコードから出すかを定める。新しい通知やエラー、待ちの表示を足すときは、この文書の判断手順と配置に従う。

## 1. 表示の種類

### 1.1 5つの表示先

知らせる内容は「続くものか、一度きりか」「作業を止めるか」「どこに属するか」で、次の5つのどれか1つだけに出す。同じ内容を2箇所に出さない。

| 表示先            | 出すもの                                                                             | 消えるとき                       |
| ----------------- | ------------------------------------------------------------------------------------ | -------------------------------- |
| **待機表示**      | 作業を続けられない状態（Host 未接続、プロジェクトを開いている、楽器の読み込み）      | 状態が終わったとき               |
| **上部バー**      | 音声エンジンの状態（ミュート、再生の準備中を含む）                                   | 状態が変わったとき               |
| **通知バー**      | 作業と並行して続く、アプリ全体の状態（対応や判断が要るもの、バックグラウンドの処理） | 状態が解消したとき・対応したとき |
| **トースト**      | 操作やバックグラウンド処理の結果として、一度だけ起きたこと                           | 時間経過・閉じる操作             |
| **InlineMessage** | 開いている画面の入力・設定の内容に結びつくこと                                       | 入力を直したとき・再実行したとき |

次のものはこの仕組みの対象外であり、それぞれの機能が自分の見た目で表す。

- 特定のトラック・クリップ・デバイスに属する状態（欠落したプラグインの表示、録音中など）
- 起動できないときの画面（BootScreen）と、確認ダイアログ（ConfirmDialog）

### 1.2 判断手順

1. **状態か、出来事か。** 条件が続くあいだ表示し続けるべきなら状態、起きた瞬間に知らせれば済むなら出来事。
2. 状態なら:
   - その状態のあいだ、編集しても無駄になるか壊れる → **待機表示**
   - 音声エンジンの状態 → **上部バー**
   - 特定のトラック・クリップ・デバイスに属する → 対象外（その対象の上に出す）
   - それ以外でアプリ全体に関わる → **通知バー**
3. 出来事なら:
   - 開いている画面の入力欄や設定の値に結びつく（入力の検証、設定の適用失敗） → **InlineMessage**
   - それ以外（コマンドの成功・失敗、ドロップの拒否など） → **トースト**

### 1.3 原則

- **作業を止めるときは、理由と進み具合を必ず見せる。** 理由の見えない操作の遮断（透明な膜、半透明にするだけ）は作らない。止めるのは待機表示だけが行う。
- **状態をトーストで出さない。** トーストは自動で消える前提であり、消えない（`persistent`）トーストは作らない。
- **出来事を通知バーに出さない。** 通知バーは「今そうなっている」ことだけを表す。
- **対応の操作は、その状態の表示の中に置く。** 再試行・再リンク・取り消しなどは通知バーやInlineMessageの中にあり、トーストは操作ボタンを持たない。
- **ユーザーが結果を待っている操作の失敗は、必ず画面に出す。** console だけに出してよいのは、ユーザーが結果を待っていない処理（画面を閉じるときの試聴停止など）の失敗だけである。

## 2. 見た目

### 2.1 共通の決まり

- **トーン**は `info`・`success`・`warning`・`danger` の4つ。色は `tokens.css` の `--color-accent`（info）、`--color-status-success`、`--color-status-warning`、`--color-status-danger` を使う。
- **トーンの色を付けるのはアイコンと、ごく薄い背景だけ**。見出しは `--color-text-primary`、本文は `--color-text-secondary` で、警告色で文字を塗らない。
- **アイコン**は14px、塗りの円（info・success・danger）と三角（warning）に記号を抜いた形。4つのトーンで形が違うので、色が区別できなくても読める。
- **見出し**は先頭だけ大文字の英語（`Score sync failed`、`Safe mode`）で、本文と同じ書体・太さ600。全部大文字・等幅・字間を広げた見出しは使わない。
- **失敗の原因**（例外のメッセージ）は本文と分け、等幅・小さめ・`--color-text-muted` で出す。
- **進捗**は高さ3pxの細いバー（地 `--color-border-default`、進んだ分 `--color-accent`）で表し、数値（`6 / 30`）を添える。待機表示と通知バーで同じ見た目を使う。
- 背景の薄さは、通知バー7%、InlineMessage 8%（`color-mix()` で地の色に混ぜる）。

### 2.2 待機表示

- 作業領域（上部バーと通知バーより下のすべて）を幕で覆い、中央に1枚のカードを出す。**上部バーと通知バーは覆わない。** MUTE・音声設定・プロジェクトと接続の切り替えは、待っているあいだこそ使える必要があるからである。
- 幕は `--color-bg-app` を半分ほど透かしたもので、下の画面が何かは分かる濃さにする。
- カード: 幅360px、`--color-bg-surface-raised` の面、`--color-border-default` の枠、`--shadow-popover`。中身は上から次の順。
  - 回転するインジケーター（Host 未接続のときは warning のアイコン）と見出し
  - 本文1文（何を待っているか・どうすれば終わるか）
  - 進捗が分かるときは進捗バーと数値
  - 補足の1行（いま処理しているもの、経過時間）
  - 1つのものに長く止まっているときは、warning の InlineMessage（`SampleTank 4 is taking longer than usual.`）
- 入力は状態が始まった瞬間に止め、カードと幕は0.5秒続いたときだけ出す。一瞬で終わる待ちで画面がちらつかないようにするためである。
- 見出し（待っている理由）は1つだけ出す。優先は Host 未接続 → プロジェクトを開いている → 楽器の読み込み。
- 楽器の読み込みの進み具合（進捗・補足・止まっているか）は、見出しとは別に、読み込みが進んでいるあいだはいつも出す。プロジェクトを開く処理は、そのプロジェクトの楽器の読み込みが終わってから返るので、`Opening project` の待ちの大半は楽器の読み込みである。

| 状態                     | 見出し                | 本文・進捗                                                                 |
| ------------------------ | --------------------- | -------------------------------------------------------------------------- |
| Host 未接続              | `Host disconnected`   | 切断の理由と、`Reconnect from the Host menu to continue.`                  |
| プロジェクトを開いている | `Opening project`     | 楽器の読み込みと同じ進捗・補足（読み込みが始まってから）                   |
| 楽器の読み込み           | `Loading instruments` | 進捗（読み込んだ数 / 全体）、補足に `トラック名 · プラグイン名 · 経過秒数` |

### 2.3 上部バー

- **音声の状態**（`audioStatus` ボタン）が、音声エンジンの状態の唯一の表示になる。
  - ミュート中は赤い点と `Muted`。補足の行は、Runtime がハウリングを検知したときはその原因の文（`AudioStatus.message`）、メーターだけがハウリングを疑っているときは `Feedback detected`。
  - 楽器の読み込みを伴わない再生の準備（編集直後の組み直し、再試行）のあいだは、補足の行が `Preparing audio`（再試行なら `Retrying audio preparation`）になる。
  - それ以外は今までどおりドライバーと遅延を出す。
- **MUTEボタン**は点灯で状態を表す（今のまま）。`RESET FEEDBACK` は `Reset feedback` に表記を改める。
- 上部バーには、これ以外の文の表示を足さない。幅の狭い画面でも項目が重ならないようにするためである。

### 2.4 通知バー

- 上部バーのすぐ下に**固定で差し込む帯**で、タイムラインの上に重ねない。通知があるあいだだけ現れ、その分だけ下の作業領域が縮む。
- 1件ごとに1行: アイコン、見出し、本文、右端に操作（文字だけのボタン）。本文は折り返してよいが、文言は最小幅（1000px）で1行に収まる長さを目安にする。
- バックグラウンドの処理は、本文の右に進捗バーと数値を出し、取り消せるなら操作に `Cancel` を置く。
- 詳細（Scoreの診断など）がある通知は、右端の `N issues ▾` で開閉し、本文の下に等幅で並べる。
- 閉じるボタン（×）は持たない。「後で」が意味を持つ通知だけが、その通知の操作として `Later` などを持つ。
- 複数あるときの並びは重要度の順（danger → warning → info）で、NoticeBar に書いた順で決まる。

### 2.5 トースト

- 右下に積む。幅300px、`--color-bg-surface-raised` の面、`--color-border-default` の枠、`--shadow-toast`。左にアイコン、見出し、必要なら本文と原因、右上に閉じるボタン。
- トーンは `info`・`success`・`danger` の3つ（warning のトーストは作らない）。
- 自動で消えるまでの時間は info・success が4秒、danger が8秒。ポインターが重なっているあいだは止まる。
- 同時に見せるのは3件まで。超えたら古いものから消す。見出し・本文・原因がすべて同じトーストが既にあれば、新しく積まずにその表示時間を延ばす。
- 現れるときだけ下から 8px 動かす（`--duration-slow`）。

### 2.6 InlineMessage

- 結びつく入力欄や設定のすぐ下、またはダイアログの操作ボタンの上に置く。
- アイコンと1〜2文の本文だけ。角丸 `--radius-sm`、トーンの薄い背景、枠線なし。
- トーンは `info`・`warning`・`danger` の3つ。入力欄の検証エラーなら、その入力欄の枠も `--color-status-danger` にする。

## 3. 文言

- 言語は英語に揃える（画面の他の部分と同じ）。
- **見出し**は「何が起きたか・どうなっているか」を5語程度で書く。`Export failed`、`Project exported`、`Score sync failed`、`Loading instruments`。`Error:` などの前置きは付けない。
- **本文**は必要なときだけ1文。「今どうなっているか」か「次に何ができるか」を書く。`The current session is unchanged.`
- **原因**には例外の文字列をそのまま入れ、見出しや本文に連結しない（`` `Export failed: ${errorText(error)}` `` のような書き方はしない）。
- 操作ボタンは動詞で書く（`Retry`、`Review…`、`Update & restart`）。ダイアログを開くものは末尾に `…` を付ける。

## 4. コードの設計

### 4.1 配置

```
src/
├─ shared/
│  ├─ notify.ts                 トーストの発行（notify）と、表示中のトーストの保持
│  ├─ errors.ts                 errorText（今のまま）
│  └─ ui/
│     └─ message/               通知と待ちの見た目の部品（状態もストアも持たない）
│        ├─ tone.ts             Tone 型
│        ├─ tone.module.css     トーンごとの色（コンポーネント内の --tone）
│        ├─ ToneIcon.tsx
│        ├─ Progress.tsx / .module.css     進捗バーと数値
│        ├─ InlineMessage.tsx / .module.css
│        ├─ Notice.tsx / .module.css
│        ├─ WaitCard.tsx / .module.css     待機表示のカード
│        └─ ToastStack.tsx / .module.css
├─ app/
│  ├─ layout/
│  │  ├─ NoticeBar.tsx / .module.css     通知バーの枠と、通知の並び順
│  │  └─ WorkspaceWait.tsx / .module.css 作業領域を覆う幕と待機表示のカード
│  ├─ runtime/
│  │  ├─ runtime-projection.ts           再生用オーディオの状態を、読み込み・準備中・問題に読み替える純粋関数
│  │  └─ useWorkspaceWait.ts             作業を止める状態を1つ選ぶ
│  └─ notices/                           アプリ自体の状態の通知
│     ├─ SafeModeNotice.tsx
│     ├─ BrowserPreviewNotice.tsx
│     ├─ UpdateNotice.tsx
│     ├─ PlaybackNotice.tsx
│     └─ BackgroundJobNotice.tsx
└─ features/
   └─ project/                           プロジェクトの状態の通知
      ├─ ScoreSyncNotice.tsx
      ├─ RecoveryNotice.tsx
      └─ MissingDependenciesNotice.tsx
```

**置き場所の規則**

- 見た目の部品は `shared/ui/message/` にだけ置く。機能のディレクトリに、通知・エラー・待ち用の CSS クラス（`.error`、`.warning`、`.notice`、操作を止める膜など）を作らない。
- 表示先ごとの「枠と並び」は `app/layout/` に1つずつ置く（`NoticeBar`、`WorkspaceWait`、`ToastStack` は `AppShell` に1つ）。
- 通知バーに出す通知は、`XxxNotice.tsx` という1つのコンポーネントにし、**その状態を持っている機能のディレクトリ**に置く。アプリ自体（起動の状態、アップデート、再生用オーディオ、バックグラウンドの処理）の状態は `app/notices/` に置く。
- 並び順を決めるのは `NoticeBar` と `useWorkspaceWait` だけ。通知を足すときは NoticeBar に1行、待ちを足すときは `useWorkspaceWait` に1つ条件を足す。

### 4.2 各モジュール

#### `shared/ui/message/tone.ts`

```ts
export type Tone = 'info' | 'success' | 'warning' | 'danger';
```

各部品は受け付けるトーンを `Extract<Tone, ...>` で絞る（トースト: info・success・danger、InlineMessage: info・warning・danger、Notice: info・warning・danger）。

#### `shared/ui/message/tone.module.css`

トーンのクラスごとに、コンポーネント内だけで使う `--tone` に Root トークンの色を入れる。ToneIcon・Notice・InlineMessage は、トーンのクラスを要素に付けて `var(--tone)` を参照する。`tokens.css` には何も足さない。

#### `ToneIcon`

`<ToneIcon tone={tone} />`。4つの形（塗りの円または三角に、地の色で記号を抜いたもの）を自分で持ち、トーンの色を付けて出す。`Icon` は単一パスの塗りなので、記号を抜いた形は持たせない。

#### `Progress`

`<Progress ratio={0.2} label="6 / 30" />`。進み具合の割合（0〜1）と、量を表す文字（`6 / 30`、`42%`）を受け取り、進捗バーと文字を出す。件数で届く楽器の読み込みと、割合で届くバックグラウンドの処理の両方を同じ見た目で出すためである。Notice と WaitCard が使う。

#### `InlineMessage`

```tsx
<InlineMessage tone="danger">
  Playable note range must have integer bounds in ascending order.
</InlineMessage>
```

- props は `tone` と `children` だけ。
- `danger` は `role="alert"`、それ以外は `role="status"`。
- 表示するかどうかは呼び出し側が決める（`{error && <InlineMessage …>}`）。エラーの文字列は今までどおり各画面の `useState` に持つ。

#### `Notice`

```tsx
<Notice
  tone="danger"
  title="Score sync failed"
  message={`${path} could not be applied. The Project keeps its last successful sync.`}
  actions={[
    { label: 'Retry', onClick: retry, disabled },
    { label: 'Link another Score…', onClick: relink, disabled },
  ]}
  details={{ summary: `${n} issues`, content: <DiagnosticList … /> }}
/>
```

- 通知バーの1行の見た目だけを持つ。操作は `{ label, onClick, disabled? }` の配列で受け取り、ボタンの見た目は Notice が決める。
- バックグラウンドの処理は `progress={{ ratio, label }}` を渡し、本文の右に `Progress` を出す。
- 詳細の開閉状態は Notice の中に持つ。
- `danger` は `role="alert"`、それ以外は `role="status"`。

#### `WaitCard`

```tsx
<WaitCard
  title="Loading instruments"
  message="Audio becomes available when every instrument is ready."
  progress={{ ratio: 0.2, label: '6 / 30' }}
  detail="ep · SampleTank 4 · 42s"
  stalled="SampleTank 4 is taking longer than usual."
/>
```

- 待機表示のカード1枚の見た目だけを持つ。`tone` を持たず、見出しの左は回転するインジケーター。Host 未接続のように「待っても終わらない」状態だけ `waiting={false}` で warning のアイコンに替える。
- `role="status"` と `aria-busy` を持つ。補足の行（経過秒数）は毎秒変わるので、読み上げの対象から外す（`aria-hidden`）。

#### `XxxNotice`（各通知）

- ストアなどから状態を読み、出す条件を満たさなければ `null` を返す。満たせば `<Notice>` を1つ返す。
- 自分の操作に必要なダイアログ（復旧する世代の確認など）は、このコンポーネントの中で開く。
- `<Notice>` 以外の見た目を自分で描かない。

#### `NoticeBar`

- `AppShell` のグリッドで、上部バーと作業領域のあいだの行に置く。
- 中身は各 `XxxNotice` を重要度の順に並べるだけ。全部が `null` なら帯は表示しない（`:empty`）。

#### `useWorkspaceWait` と `WorkspaceWait`

- `useWorkspaceWait()` は、Host の接続・プロジェクトの切り替え・再生用オーディオの読み込みから、待っている理由を優先順に1つ選んで見出しと本文にし、楽器の読み込みが進んでいればその進捗・補足・止まっているかを、理由にかかわらず添えて `WaitCard` に渡す内容を返す。無ければ `null`。
- `WorkspaceWait` は `AppShell` のグリッドで作業領域と同じ領域に重ね、幕とカードを描く。0.5秒の表示の遅れはここが持つ。
- 入力を止めるのは作業領域の要素（`LeftColumn` の根）に付ける `inert` だけで、`useWorkspaceWait()` が `null` でないあいだ付ける。window に登録した編集のショートカット（取り消し・やり直し、タイムラインのショートカット）も、同じ値を見て受け付けない。

#### `shared/notify.ts`

```ts
export const notify = {
  info(title: string, options?: { message?: string }): void,
  success(title: string, options?: { message?: string }): void,
  error(title: string, options?: { message?: string; cause?: unknown }): void,
};
export function useToasts(): readonly Toast[];
export function dismissToast(id: number): void;
```

- `cause` は `errorText` で文字列にして原因の欄に入れる。呼び出し側で `errorText` を呼ばない。
- 保持するのは表示中のトーストの配列だけ。3件の上限と、同じ内容の重複の扱い（表示時間の延長）はここで行う。
- 自動で消すタイマーは持たない。表示時間は表示側（ToastStack の各項目）が持つ。重複で表示時間を延ばすときは、そのトーストの `revision` を進めて表示側に知らせる。
- コンポーネント、フック、ストアのスライス、どこからでも呼べる（React の外からも呼べるように、モジュール単位の値として持つ）。

#### `ToastStack`

- `useToasts()` を描く。各項目が自分の表示時間を管理し、ポインターが重なっているあいだは止める。
- アンマウントで表示中のトーストを空にする。

#### `app/runtime/runtime-projection.ts`

再生用オーディオの状態から、次の3つを作る純粋関数を置く。

- 楽器の読み込み（`loadProgress` があるとき）: 進捗、いま読み込んでいるトラックとプラグイン、経過時間、止まっているか → 待機表示
- 楽器の読み込みを伴わない準備・再試行 → 上部バーの音声の状態の補足
- 問題（準備の失敗、欠落した素材・デバイスを飛ばしての再生） → `PlaybackNotice`

#### ハウリングによる自動ミュート

- 音声のスライスの `setStatus` で、`feedbackSuspected` が false から true に変わったときに1回だけ `notify.error('Feedback detected', { message: 'Output was muted to protect your hearing and speakers.' })` を出す。その後の状態は上部バーだけが表す。
- ミュートの補足の文は、上部バーで `status.feedbackSuspected`・`status.message`・`useAudioFeedbackSuspected()` から決める。

### 4.3 禁止事項

- `toast` / `showToast` / `clearToast` のような、表示先を消したり差し替えたりする API を作らない。状態はトーストに出さないので不要である。
- 状態の変化を `useEffect` で監視してトーストに変換しない。トーストは、失敗や成功が分かったその場所（スライスの関数、フックのハンドラー）で `notify` を呼ぶ。
- 機能のコードで、通知・エラー・待ち用の見た目（CSS クラス、位置を固定した帯、独自のアイコン、操作を止める膜）を作らない。
- `inert`・`pointer-events: none`・半透明で、作業領域の操作を個別に止めない。止めるのは `useWorkspaceWait` だけである。
- 上部バーに、音声の状態以外の文の表示を足さない。
- エラーの文字列を作るためだけの state（`message` と `error` の組）をフックに持たせない。InlineMessage に出すものだけを state に持つ。

## 5. 今の表示の振り分け

### 5.1 中央バナー（ShellNotices）

| 今の表示          | 振り分け先                                                                                                                                      |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| OUTPUT MUTED      | 削除。上部バーの音声の状態とMUTEボタンが表す                                                                                                    |
| SCORE SYNC FAILED | `ScoreSyncNotice`（danger）                                                                                                                     |
| SAFE MODE         | `SafeModeNotice`（warning）。本文を短くする                                                                                                     |
| RECOVERY CHOICE   | `RecoveryNotice`（info）。操作は `Choose generation…` と `Keep recovered session`。世代の一覧と確認はダイアログで出す                           |
| UPDATE AVAILABLE  | `UpdateNotice`（info）。操作は `Update & restart` と `Later`（次に手動で確認するか、次に起動するまで隠す。隠した状態は `useAppUpdater` が持つ） |
| JOB · 状態        | `BackgroundJobNotice`（info、進捗、`Cancel`）                                                                                                   |
| BROWSER PREVIEW   | `BrowserPreviewNotice`（info）。本文を1文にする                                                                                                 |

### 5.2 消えないトースト・状態を出していたトースト

| 今の表示                                         | 振り分け先                                                                                                         |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| 再生用オーディオの読み込み（デバイスごとの進捗） | 待機表示（`Loading instruments`）                                                                                  |
| 読み込みを伴わない準備中・再試行中               | 上部バーの音声の状態の補足（`Preparing audio`）                                                                    |
| 再生用オーディオの準備の失敗（Retry 付き）       | `PlaybackNotice`（danger、`Retry`）                                                                                |
| 欠落した素材・デバイスを飛ばしての再生           | `PlaybackNotice`（warning）                                                                                        |
| ドライバーの設定の調整（`app.audio-preference`） | 設定の適用が成功するとダイアログは閉じるので、一度きりの結果としてトースト（info `Audio settings adjusted`）にする |
| 欠落の一覧（左上に浮かぶ MissingDependencies）   | `MissingDependenciesNotice`（warning、`Review…`）。一覧はダイアログで開く                                          |

### 5.3 理由を出さずに操作を止めていた表示

| 今の表示                                                                         | 振り分け先                      |
| -------------------------------------------------------------------------------- | ------------------------------- |
| プロジェクト切り替え中の透明な膜（`ProjectBoundRegion` の `inert` と `::after`） | 待機表示（`Opening project`）   |
| Host 未接続で作業領域を半透明にする（`App.module.css` の `.hostDisconnected`）   | 待機表示（`Host disconnected`） |

`ProjectBoundRegion` は、プロジェクトが替わったときに中身を作り直す役割（`key`）と、Host 未接続で入力欄を止める `fieldset disabled` だけを残す。

### 5.4 一度きりのトースト

すべて `notify` に置き換える。トーンと文言は次のとおりに揃える。

| 場所                                                           | トーン           | 見出し / 本文・原因                                                                                                                          |
| -------------------------------------------------------------- | ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| 書き出し・取り込み・削除の成功                                 | success          | `Project exported` / パス、`Project imported` / 名前、`Project deleted` / 名前                                                               |
| 書き出し・取り込み・削除の失敗                                 | danger           | `Export failed` / `The current session is unchanged.` + 原因（取り込み・削除も同じ形）                                                       |
| Score の同期・保存の結果                                       | success          | `Score synchronized`、`Score saved`、`Score link updated`。変化が無いときは info `Score is already synchronized`                             |
| Score の再構築（`rebuildScore`）の失敗                         | danger           | `Score rebuild failed` + 原因。ユーザーが実行したコマンドの結果であり、Score ファイルの監視による同期の状態（`ScoreSyncNotice`）とは別のもの |
| 世代の復旧・復旧したセッションの継続                           | success          | `Generation restored`、`Recovered session kept`                                                                                              |
| Quantize の結果                                                | success / info   | `Quantized N notes` / 既に揃っていれば info `Notes are already on the grid`                                                                  |
| Quantize の失敗                                                | danger           | `Quantize failed` + 原因                                                                                                                     |
| クリップのコピー                                               | success          | `N clips copied`                                                                                                                             |
| 分割・結合・トリムができない位置                               | info             | `Click inside the selected clip to split it` など（失敗ではなく案内なので info）                                                             |
| ドロップ・移動先のトラックが合わない                           | info             | `MIDI clips go on instrument tracks` など（同上）                                                                                            |
| トラック操作・トランスポート操作の失敗                         | danger           | 何の操作かを見出しにし、原因を付ける                                                                                                         |
| セッションへの編集の確定の失敗（`createSessionCommit` の既定） | danger           | `Edit could not be applied` + 原因                                                                                                           |
| VST3 のスキャン完了                                            | success          | `Found N plug-ins`。注意があれば本文に `N issues`（日本語の文言は英語に改める）                                                              |
| VST3 のスキャン・その後の準備の失敗                            | danger           | `Plug-in scan failed` + 原因                                                                                                                 |
| プラグインを開けない・試聴の失敗                               | danger           | `Plug-in could not open` + 原因、`Preview failed` + 原因                                                                                     |
| 録音の失敗・録音ファイルの退避                                 | danger           | `Recording failed` + 原因、`Recording could not be finalized` / `The recorded files were kept in the Inbox.` + 理由                          |
| 音声エンジンの再起動                                           | info             | `Audio engine restarted`                                                                                                                     |
| 緊急ミュート・ハウリング保護の解除の失敗                       | danger           | `Mute failed` + 原因、`Feedback reset failed` + 原因                                                                                         |
| アップデートの確認                                             | info / danger    | `Riffra is up to date` / `Update check failed`（新しい版がある場合は通知バーに出るのでトーストは出さない）                                   |
| アップデートのインストールの失敗                               | danger           | `Update failed` + `Try again later.`                                                                                                         |
| 受信箱（Inbox）の操作の結果                                    | success / danger | `Recording renamed` など。失敗は見出し + 原因。`useInbox` の `message` / `error` の state は削除する                                         |
| 書き出し履歴の書き出し・削除                                   | success / danger | `Exported N files` / 書き出し先、`Render deleted`。失敗は見出し + 原因。履歴の読み込みの失敗だけはパネル内の InlineMessage に残す            |
| 楽器ライブラリの読み込みの失敗                                 | danger           | `Instruments could not load` + 原因。`BrowserPanel` の effect は削除する                                                                     |
| ミキサーのルーティングの失敗                                   | danger           | `Routing failed` + 原因                                                                                                                      |

### 5.5 画面の中のエラー

次の表示を `InlineMessage` に置き換え、各画面の CSS から `.error`・`.warning`・`.notice`・`.message` のうち通知用のものを削除する。

- `AudioSettingsDialog`、`DevicesPanel`、`DeviceParameterEditor`、`InstrumentMidiSettingsEditor`（`midiWarnings` を含む）、`MixerPanel`、`MixdownDialog`、`RenderHistoryPanel`、`RenderMeasurements`
- `AudioClipInspector`・`MidiClipInspector`・`MultiClipInspector`・`TakeInspector`・`TrackInspector` の操作結果
- `PlaySurface` の案内、`ProjectHostSelector` のエラー、`BootScreen` のエラー

### 5.6 console だけに出している失敗

`logNativeError` の呼び出しを1つずつ見て、ユーザーが結果を待っている操作（ファイルダイアログを開く、プロジェクトの書き出し先を選ぶなど）なら `notify.error` に変える。待っていない処理はそのまま残す。

## 6. 削除するもの

- `shared/toasts.ts`（`notify.ts` に置き換え）、`shared/ui/ToastStack.*`（`shared/ui/message/` に移す）
- `app/layout/ShellNotices.tsx` / `.module.css`
- `app/runtime/useRuntimeProjectionToast.ts`
- `features/project/MissingDependencies.module.css` の固定配置（一覧はダイアログの中身になる）
- `ProjectBoundRegion` の切り替え中の `inert` と `::after`、`App.module.css` の `.hostDisconnected`
- `tokens.css` の `--z-banner`（重ねて出す帯が無くなる）
- 各機能の CSS の、通知・エラー用のクラス（5.5）

## 7. docs への反映

`docs/ui-design.md` を次のように直す。

- 「1. 画面の全体像」に、通知バーと待機表示がどこに現れるかを書く。
- 「3.3 音声の状態」に、ミュートの理由と再生の準備中が上部バーの音声の状態に出ることを書く。
- 「8.2 状態と困ったとき」の表を、1.1 の5つの表示先に置き換える。待機表示が作業領域だけを覆い、上部バーは使えること、文言の規則（3章）もここに書く。

## 8. 実装の進め方（コミット分割）

1. `shared/ui/message/`（Tone、ToneIcon、Progress、InlineMessage、Notice、WaitCard、ToastStack）を追加する
2. 画面の中のエラーを `InlineMessage` に置き換え、各機能の通知用 CSS を削除する
3. `shared/notify.ts` を作り、一度きりのトーストをすべて `notify` に置き換えて `shared/toasts.ts` を削除する（`BrowserPanel` の effect と `useInbox` の state の削除を含む）
4. `runtime-projection.ts` を作り、上部バーの音声の状態にミュートの理由と準備中を出す。`useRuntimeProjectionToast` を削除する
5. `NoticeBar` と各 `XxxNotice` を作り、`ShellNotices` と浮かぶ MissingDependencies を削除する。`--z-banner` を削除する
6. `useWorkspaceWait` と `WorkspaceWait` を作り、プロジェクト切り替え・Host 未接続・楽器の読み込みの待ちを移す。`ProjectBoundRegion` の膜と `.hostDisconnected` を削除し、編集のショートカットを止める
7. `logNativeError` の呼び出しを見直す
8. `docs/ui-design.md` を直す
