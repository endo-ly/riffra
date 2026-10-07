# Ample Sound Lite の演奏表現

Ample Bass P Lite 4（ABPL）と Ample Guitar M Lite 4（AGML）を、Riffra から MIDI で演奏するための参照資料。対象は**演奏表現に関わる機能**で、アンプ、EQ、コンプレッサー、ディレイ、リバーブなどのミックス機能は扱わない。

両音源は、通常の演奏ノートに加えて、キースイッチ、Velocity、ノート同士の重なり、弦・ポジション指定などを使って演奏を組み立てる。Riffra では主要な奏法を MIDI Note と MIDI Event で表現できる。

## 1. Lite 版の範囲

|            | ABPL                                                               | AGML                                                            |
| ---------- | ------------------------------------------------------------------ | --------------------------------------------------------------- |
| 音源       | Fender Precision Bass 系                                           | Martin D-41 系アコースティックギター                            |
| ライブラリ | Fingerstyle                                                        | Fingerstyle                                                     |
| 基本音域   | E1–F4（MIDI 40–77）                                                | E1–C5（MIDI 40–84）。ドロップチューニングで最低 D1（MIDI 38）   |
| 収録奏法   | Sustain / Accent / Palm Mute / Legato Slide / Hammer-On & Pull-Off | Sustain / Pop / Palm Mute / Legato Slide / Hammer-On & Pull-Off |
| Riffer 4   | 編集機能は利用可能。プリセットライブラリ制限、最大64小節           | 同左                                                            |
| Strummer   | —                                                                  | 利用可能                                                        |

Lite 版は音程ごとのサンプリングで、異なる弦上の同じ音程は同じ基礎サンプルを共有する。弦指定やポジション指定は運指、レガート、ノイズなどの演奏ロジックには関わるが、フル版のように各弦・各フレットが別サンプルになっているわけではない。

ABPL のメインパネルマニュアルには奏法ごとのトリガー範囲として B0–F4 なども、Lite 製品比較には D1–F4 も記載されるが、既定状態の ABPL は E1（MIDI 40）未満を発音しない。通常のベースラインは E1–F4 を基準にする。

## 2. MIDI で演奏を作る仕組み

Ample Lite の演奏表現は、主に4つの情報で決まる。

| 情報           | 役割                               | Riffra での表現                     |
| -------------- | ---------------------------------- | ----------------------------------- |
| 演奏ノート     | 音高、発音位置、音価               | Note の pitch / position / duration |
| キースイッチ   | 奏法や演奏状態の選択               | 低音域の MIDI Note                  |
| Velocity       | 強弱と奏法内の変化                 | Note の velocity                    |
| ノートの重なり | Slide、Hammer-On / Pull-Off の接続 | 前後 Note の時間範囲                |

### 音名と発音する高さ

Ample の音名は `C3 = MIDI 60` の表記である。Riffra は `C4 = MIDI 60` の表記を使うため、Ample の音名は Riffra では1オクターブ上の音名になる（Ample の C0 は Riffra の C1 = MIDI 24）。本書の音名はすべて Ample の表記で、MIDI番号を併記する箇所はその値を正とする。

発音する高さは音源で異なる。

| 音源 | 発音する高さ                                                                                   |
| ---- | ---------------------------------------------------------------------------------------------- |
| AGML | MIDI の音高どおりに鳴る。MIDI 40 は E（約82 Hz）で、6弦開放                                    |
| ABPL | MIDI の音高より1オクターブ低く鳴る。MIDI 40 は E（約41 Hz）で、4弦開放。ベースの記譜と同じ関係 |

ABPL で特定の高さを鳴らすときは、Riffra の音名で狙う高さより1オクターブ上を指定する。たとえば約73 Hz の D を鳴らすには Riffra の D3（MIDI 50）を送る。

### キースイッチ

主要キースイッチは両音源で共通している。

| キースイッチ（Ample） | Riffra の音名 | MIDI番号 | 内容                 |
| --------------------- | ------------- | -------: | -------------------- |
| C0                    | C1            |       24 | Sustain 系           |
| D0                    | D1            |       26 | Palm Mute 系         |
| E0                    | E1            |       28 | Legato Slide         |
| F0                    | F1            |       29 | Hammer-On / Pull-Off |

Ample は「キースイッチを押してから演奏ノートを鳴らす」という順序で奏法を解釈する。同一 tick に置く場合はイベント順序に依存するため、Riffra では短い時間だけ先行させると安定する。1/64〜1/16拍程度あれば十分で、音楽上のタイミングとして大きくずらす必要はない。

C0 と D0 は基本奏法の状態を切り替える。E0 と F0 は前後2音を接続するレガート操作で、演奏後は直前の奏法へ戻る。

## 3. 収録奏法

### Sustain / Accent / Pop / Palm Mute

| 音源 | 奏法             | 指定                                  |
| ---- | ---------------- | ------------------------------------- |
| ABPL | Sustain          | C0、演奏ノート Velocity 1–125         |
| ABPL | Accent           | C0、演奏ノート Velocity 126–127       |
| ABPL | Palm Mute        | D0。高い Velocity 側で Palm Mute      |
| ABPL | Dead Note 系発音 | D0。低い Velocity 側で発音            |
| AGML | Sustain          | C0、演奏ノート Velocity 1–126         |
| AGML | Pop              | C0、演奏ノート Velocity 127           |
| AGML | Palm Mute        | D0。Velocity が低いほどミュートが深い |

ABPL の Dead Note は、Lite 製品比較では独立した収録奏法として数えられていない。一方、Lite のメインパネルマニュアルでは D0 選択中の低 Velocity が Dead Note を発音すると説明されている。したがって「独立したキースイッチを持つ奏法」ではなく、Palm Mute 状態の Velocity 側にある発音変化として扱う。

両音源とも C0 と D0 を同時に押すと、Velocity によって Sustain と Palm Mute を振り分ける混合状態になる。この状態では高 Velocity が Sustain、低 Velocity が Palm Mute になる。

### Legato Slide

E0 を選択し、開始音が鳴っている間に移動先の音を開始すると Slide が発生する。

```text
開始音       ───────────────
移動先               ───────────────
                     ↑ overlap
```

2フレットを超える Slide では、**移動先ノートの Velocity が Slide 速度を決める**。Velocity が高いほど速く移動する。また、E0 自体を高 Velocity で送ると Slide 後のフレットポジションも移動し、低 Velocity では現在のポジションを維持する。

### Hammer-On / Pull-Off

F0 を選択し、Legato Slide と同様に前後のノートを重ねる。上行では Hammer-On、下行では Pull-Off として使える。F0 自体の Velocity もポジション更新に関係し、高 Velocity では移動後の位置へ更新される。

### Poly Legato

Legato Slide と Hammer-On / Pull-Off は複数弦にも対応する。異なる弦で複数の開始音を同時に鳴らし、その後に移動先を置くと、各開始音から同じ音程幅のレガートが発生する。

## 4. 運指と演奏状態

### Play Mode

| Mode        | ABPL | AGML | 動作                                               |
| ----------- | ---- | ---- | -------------------------------------------------- |
| Instrument  | ○    | ○    | 実楽器の運指制約を反映。同じ弦の同時発音などを制限 |
| Keyboard    | ○    | ○    | 実楽器の弦制約を外して演奏                         |
| Solo        | ○    | ○    | 単音演奏                                           |
| Power Chord | —    | ○    | 入力音と5度を同時発音                              |
| Octave      | —    | ○    | 入力音とオクターブを同時発音                       |

Keyboard と Solo では Auto Legato が無効になる。

### Auto Legato

両音源に Automatic Slide、Automatic Hammer-On & Pull-Off、Off の3状態がある。D#6 を高 Velocity で送ると Automatic Slide、低 Velocity で送ると Automatic Hammer-On & Pull-Off へ切り替わる。Off はプラグイン側の状態として設定する。

明示的に E0 / F0 とノートの重なりを作る方法と Auto Legato は、同じレガート表現を別の方法で生成する機能である。

### 弦指定

弦を直接指定すると、同じ音高でも運指位置やレガートの解釈を制御できる。

| 音源 | キースイッチ | 対応      |
| ---- | ------------ | --------- |
| ABPL | E6–G6        | 4弦 → 1弦 |
| AGML | G0–C1        | 6弦 → 1弦 |

範囲内の半音ごとに隣の弦へ対応する。弦指定キースイッチを高 Velocity で送る String Force はフレットポジションにも影響し、低 Velocity では弦だけを指定する。

### ポジション指定

ポジションは2段階で指定する。

| 音源 | 開始キー | ポジション選択 |
| ---- | -------- | -------------- |
| ABPL | A#0      | 続けて E1–A#2  |
| AGML | C#1      | 続けて E1–G#2  |

各ポジションは通常5フレット分の範囲を持つ。弦指定と組み合わせることで、エージェント側から演奏位置をかなり細かく制御できる。

### Vibrato

ABPL は Mod Wheel を往復させて手動 Vibrato を作る。値が 0.75 以上になると Vibrato Noise も発生する。Auto Mod を有効にすると、時間、深さ、Pitch、カーブをプラグイン側で設定できる。

AGML は Auto Vibrato が基準で、深さと速度を Settings から調整する。深さが 0.75 以上では Vibrato Noise が加わる。

Riffra の正準 MIDI は Control Change を保持できるため、ABPL の Mod Wheel は CC1 として表現できる。Note だけで作る奏法とは制御経路が異なる。

### 演奏ノイズと発音状態

ABPL は Finger Release Noise を自動生成できる。AGML は指が弦へ触れる FA と、離れる FR の Fingering Noise を持つ。

AGML にはさらに、複音をストロークしたときの Stroke Noise、左右で異なるサンプルを使う Doubled Guitars、アコースティック共鳴の Resonance、開放弦を優先する Open String First がある。Open String First は G#6 を高 Velocity で送ると有効、低 Velocity で無効になり、E1 / A1 / D2 / G2 / B2 / E3 を開放弦として扱う。

Sound Mode は ABPL が Stereo / Mono DI、AGML が MS1 / MS2 / AB / Mono を持つ。これらは音符ごとの奏法ではなく、プラグイン状態として音色全体を決める。

Hold Pedal Toggle も両音源に備わる。Lite のメインパネルマニュアルでは専用 MIDI ノートの対応までは記載されていないため、Riffra から使う場合は実際に公開される MIDI / Parameter 対応を確認して扱う。

## 5. フレーズ操作と効果音

### ABPL

ABPL は現在または直前の音を基準に、音程パターンやリピートを呼び出せる。

| MIDI Note | 動作                               |
| --------- | ---------------------------------- |
| B4        | 下行4度の Octave Pattern           |
| C5        | 同音の Octave Pattern              |
| D5        | 上行5度の Octave Pattern           |
| E5        | 上行オクターブの Octave Pattern    |
| C#5 / D#5 | 発音中の音を繰り返す Note Repeater |

独立した演奏ノイズは次のとおり。

| MIDI Note | FX Sound                                     |
| --------- | -------------------------------------------- |
| F5 / F#5  | Scratch 1 / Scratch 2                        |
| G5        | Single String Slap                           |
| G#5 / A5  | Left-Hand Slap Noise / Right-Hand Slap Noise |
| A#5 / B5  | Fx Slide Turn 4 / Fx Slide Turn 3            |
| C6 / C#6  | Fx Slide Down 4 / Fx Slide Down 3            |

これらは音程付きの Slap や Slide 奏法ではなく、演奏中へ挿入する独立した効果音である。

### AGML

AGML は D6 で発音中の音を繰り返せる。複数音にも対応する。

| MIDI Note     | FX Sound                                  |
| ------------- | ----------------------------------------- |
| F5            | Scratch                                   |
| F#5           | Slap                                      |
| G5 / G#5      | Muting / Strum Mute                       |
| A5 / A#5      | Downstroke Noise 1 / Upstroke Noise 1     |
| B5 / C6       | Downstroke Noise 2 / Upstroke Noise 2     |
| F6 / F#6 / G6 | Hit Top (Open) / Hit Top (Mute) / Hit Rim |

こちらも通常の音程付き奏法とは別の演奏ノイズとして使う。

## 6. Riffer / Strummer / Tab Reader

Riffer 4 は両 Lite 版で利用できる。Lite の制限はプリセットライブラリと最大64小節で、編集機能自体は、ピアノロール / タブ表示、弦の可視化、運指、奏法、レガート、Velocity、MIDI CC の編集に対応する。MIDI をホストへ書き出した場合も、Riffer 内の演奏情報を反映した再生結果を維持する。

そのため、Riffra から直接 MIDI を作る方法と、Riffer で作った演奏を MIDI として取り込む方法の両方が使える。特殊奏法の MIDI 構造を確認したい場合にも、Riffer で短い例を作って書き出すと、キースイッチ、Velocity、重なり、弦指定を比較できる。

AGML の Strummer はコードの Select / Detect、任意コード、Strum SEQ、リズムライブラリ、MIDI Drag & Drop を持つ。コードストロークをまとめて作る用途では Strummer、単音や細かな運指を直接組む用途では Riffra / Riffer が使いやすい。

Tab Reader 4 は Guitar Pro 3–8 のファイルを読み込み、運指、奏法、演奏情報を保ったまま Riffer へ渡せる。

## 7. Riffra での表現

### キースイッチ

通常の演奏ノートと同じ `music note insert` でキースイッチを入れられる。音名は Riffra の表記で指定する。たとえば ABPL の4弦開放（MIDI 40）を Palm Mute で鳴らす場合は、D0（Riffra の D1）を少し前へ置く。

```json
[
  {
    "pitch": "D1",
    "position": "5:1+15/16",
    "duration": "1/128",
    "velocity": 100,
    "channel": 1
  },
  {
    "pitch": "E2",
    "position": "5:2",
    "duration": "1/8",
    "velocity": 88,
    "channel": 1
  }
]
```

`music note list --raw` では、C0 / D0 / E0 / F0 が MIDI 24 / 26 / 28 / 29 として保存されていることを確認できる。

### Legato

Legato Slide と Hammer-On / Pull-Off は、キースイッチと2音の重なりを組み合わせる。

```text
E0 または F0    ──
開始音             ─────────────
移動先                    ─────────────
                          ↑ overlap
```

重なり量に固定値はない。短いフレーズでは短く、長い音価では少し広めに取り、通常は1/64〜1/16拍程度から出音を調整すると扱いやすい。

### Velocity

Velocity は音量だけでなく奏法の一部になる。ABPL の126–127は Accent、AGML の127は Pop、AGML の Palm Mute は深さ、長い Slide では移動先 Velocity が速度を変える。Velocity のばらつきを作るときは、現在の奏法で何を変える値なのかを先に見る。

### MIDI CC

ABPL の手動 Vibratoなど、CCを使う表現は MIDI Clip の `events` に Control Change として保持する。Mod Wheel は CC1 で、`data1 = 1`、`data2` が 0–127 の値になる。Note と違い tick ベースの MIDI Event なので、Riffra の低レベル MIDI 操作として扱う。

### Plugin State

Play Mode、Sound Mode、Auto Legato の Off、AGML の Doubled Guitars / Resonance / Stroke Noise など、曲中で頻繁にノートイベントとして切り替えない設定は Plugin State として保存できる。これにより、同じ MIDI でもプラグイン内部状態の違いで結果が変わる問題を避けやすい。

### 切り分け

| 聞こえ方                              | 主に確認する情報                                                  |
| ------------------------------------- | ----------------------------------------------------------------- |
| ノートが鳴らない                      | 音域（ABPL は MIDI 40 未満を発音しない）                          |
| 1オクターブずれて聞こえる             | 音名の表記と発音する高さ（ABPL は MIDI より1オクターブ低い）      |
| 奏法が切り替わらない                  | キースイッチの音高（Riffra の音名で C1 / D1 / E1 / F1）と開始順序 |
| Accent / Pop が意図せず出る           | Velocity                                                          |
| Slide / Hammer-Pull が通常の2音になる | E0 / F0 とノートの重なり                                          |
| Slide の速さが不自然                  | 移動先ノートの Velocity                                           |
| 運指やレガートの弦が不自然            | String Assignment / Position Assignment / Play Mode               |
| 同じ MIDI なのに結果が変わる          | Auto Legato、Play Mode、Sound Mode など Plugin State              |

## 参照資料

- Ample Bass P Lite 4: https://www.amplesound.net/en/pro-pd.asp?id=19
- Ample Bass P Lite Main Panel Manual: https://www.amplesound.net/en/Main_Panel_Manual-ABPL.pdf
- Ample Guitar M Lite 4: https://www.amplesound.net/en/pro-pd.asp?id=7
- Ample Guitar M Lite Main Panel Manual: https://www.amplesound.net/en/Main_Panel_Manual-AGML.pdf
- Guitar Riffer 4 Manual: https://www.amplesound.net/en/Guitar_Riffer.pdf
- Guitar Strummer Manual: https://www.amplesound.net/en/Guitar_Strummer.pdf
