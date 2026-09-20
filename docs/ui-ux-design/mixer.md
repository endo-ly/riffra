# Riffra Mixer 画面仕様

Mixer は Arrange の Lower Area に表示する Track ミックスと Master 出力の監視面である。Timeline の構成編集、Properties の属性編集、Play Surface の演奏入力とは責務を分け、同じ Arrangement の音声経路を確認しながら調整できるようにする。

## 1. 画面構造

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ MIXER                                      Track balance and stereo output    │
├──────────────────────────────────────────────────────────────┬───────────────┤
│ Track 1   Track 2   Track 3   Track 4   …                    │ MASTER        │
│ ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐                       │ ┌───────────┐ │
│ │ Pan  │  │ Pan  │  │ Pan  │  │ Pan  │                       │ │ L/R Meter│ │
│ │Meter │  │Meter │  │Meter │  │Meter │  horizontal scroll   │ └───────────┘ │
│ │ Gain │  │ Gain │  │ Gain │  │ Gain │                       │ Master Gain  │
│ │ M S R│  │ M S R│  │ M S R│  │ M S R│                       │ Safety       │
│ └──────┘  └──────┘  └──────┘  └──────┘                       │ Diagnostics  │
└──────────────────────────────────────────────────────────────┴───────────────┘
```

Track Channel Strip は横方向に並び、Track 列だけがスクロールする。Master Channel Strip は右側に固定し、Track 数が増えても左右の出力を常に参照できる。表示面は Lower Area の他の編集面と排他的に切り替わるが、Arrange Selection、Active MIDI Clip、Focused Instrument Track はそれぞれの状態を維持する。

## 2. Track Channel Strip

各 Track は次の順序で表示する。

1. Track color、名前、Track kind
2. FX summary（Device 数と欠落状態）
3. Pan と左右 Meter
4. Gain fader と dB readout
5. Mute、Solo、Record Arm

Track 名を選択すると Arrange Selection をその Track にする。Mixer からの選択は Focused Instrument Track を変更しないため、MIDI Editor や Play Surface の演奏先を保ったままバランスを確認できる。FX summary は Device の追加・削除を行わず、編集面を開く入口だけを提供する。

Automation Lane に Volume または Pan の点が一つ以上ある場合は、該当する操作の横に `AUTO` を表示する。Lane が空の場合は表示しない。

M、S、R はそれぞれ Track の Canonical state を更新する。更新中は対象のスイッチを保留状態として表示し、応答の Canonical state を受け取ってから確定する。

## 3. 一時プレビューと確定

Gain と Pan の連続操作では、入力値を短い間隔でまとめて `preview_track_mix` へ送る。Native Runtime はアクティブな Track Runtime の atomics へ値を反映し、次の Audio block の先頭で読み込む。プレビューは音を確認するための一時状態であり、Canonical state、履歴、保存、Runtime 投影の結果を変更しない。

操作の終了時は保留中のプレビューを先に送信し、最後の値について `updateTrack` を一度だけ実行する。確定応答の Canonical state を Desktop が適用し、失敗または Host generation の変更が起きた場合は Canonical の Gain / Pan を Runtime へ戻す。Project または Host が切り替わったとき、古い世代のプレビュー応答は画面やRuntimeへ適用しない。

Track の Canonical Gain は -90 dB から +24 dB、Pan は -1 から +1 の範囲で扱う。UI はこの範囲を提示し、Native と Core の入力検証も同じ意味を持つ。

## 4. Meter と Safety 診断

Track Meter は、Track の Effect Chain、出力補償、Fader、Pan、Automation、Mute を通過して Master mix へ加わる直前の左右信号を測定する。Peak は線形値をそのまま保持し、1.0 を超える値もクランプしない。RMS は同じ Audio block の二乗平均平方根として表示する。

Master Meter は、Master gain、Safety limiter、最終ハードクリップを通過した左右出力を測定する。Master Channel Strip は次の診断値を同じ表示面に置く。

| 診断     | 意味                                         |
| -------- | -------------------------------------------- |
| Pre      | Safety limiter 前の最大出力レベル            |
| Limiter  | Safety limiter が適用した最大 Gain reduction |
| Clip     | 最終 hard clamp が発生したサンプル数         |
| Feedback | Feedback protection が検知中かどうか         |

Meter frame は Native からおよそ 50 ms ごとに送信する。Audio callback は atomics への peak hold のみを行い、ロック、ヒープ確保、IPC、UI状態の変更を行わない。Rust は `audioMeters` の Track / Master データを既存の Host event 経路へ転送し、Desktop のメーター購読だけが高頻度更新で再描画される。

データが未接続、世代切替中、または対象 Track が現行 Runtime に存在しない場合は、前回値を補間せず `—` として表示する。

## 5. Canonical state との境界

Track の Gain、Pan、Mute、Solo、Record Arm は `CreativeSession.arrangement.tracks` が正本である。Master Gain は `CreativeSession.settings.masterDb` が正本である。Mixer はこれらの既存フィールドを編集するだけで、Mixer専用のTrack、Master、Bus、Effect Chainモデルを追加しない。

Audio Runtime が保持する Track mix preview と Meter accumulator は実行中だけ存在する。Graph の再投影、サイドカー再起動、Device 切替では最新 Canonical state から再構築し、進行中のプレビューや前世代のMeterを復元しない。

通信の詳細は [IPC契約](../ipc.md)、正準状態とRuntime投影の責務は [アーキテクチャ](../architecture.md)、Lower Area との配置は [Arrange画面仕様](arrange-screen.md) と [共通画面構造](application-layout.md) を参照する。
