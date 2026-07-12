# 仕様トレーサビリティ

この文書は、一次仕様書の章、要求ID、製品の責務、実装領域、残課題をつなぐための進捗台帳です。要求の意味は [満たすべき挙動一覧](./behavior-requirements.md)、責務の境界は [アーキテクチャ](./architecture.md) を参照します。

## 状態の意味

| 状態 | 意味 |
| --- | --- |
| 未着手 | 要求に対応する設計・実装がまだない |
| 実装中 | 設計または実装が進行中で、要求を満たすとは判定していない |
| 実装済み | コードと自動化された確認があるが、製品全体の動作確認が残っている |
| 製品確認済み | ユーザーの一連の操作、成果物、異常時の戻り方まで確認できている |
| 保留 | 仕様上の依存、外部環境、対象外範囲などにより判断を保留している |

「実装済み」は「製品確認済み」より弱い状態です。画面にボタンがあることやビルドが通ることだけでは、要求の完了を意味しません。

## 要求領域と実装責務

| 製品領域 | 要求ID | 主な責務 | 状態 | 代表的な残課題 |
| --- | --- | --- | --- | --- |
| A. Instant Play | `G-001`–`G-007`, `FLOW-001`, `AUD-*`, `MIDI-001` | Session起動、Device、Safety、Transport、MIDI | 実装済み | 実演奏までの一貫性、Device切替、Latencyの扱い |
| B. Tone Design | `FLOW-002`, `PLG-*`, `RACK-*` | VST3 Host、Rack、Parameter、Macro、Snapshot | 実装済み | 複雑なRack経路、PDC、Plugin差異の吸収 |
| C. Capture | `FLOW-003`, `REC-*` | Raw/Processed録音、Take、Count-in、Inbox | 実装中 | 録音途中の異常、manifest形式の互換、長時間安定性 |
| D. Arrange | `FLOW-004`, `ARR-*`, `EXP-*` | Timeline、Clip、Track、MIDI、Render、Export | 実装済み | Tempo/Automation、DAW handoff、同期精度 |
| E. Sample | `FLOW-005`, `SMP-*`, `MIDI-002`–`003` | Source mapping、Pad、Keyboard、内部音源、Preview | 実装済み | フルKeyboard Instrument、内部Synth、Utilityの広がり |
| F. Analyze | `ANL-*` | Waveform、音量、Spectrum、Phase、Reference | 実装済み | 指標の拡充、Reference編集、表示密度 |
| G. Separate | `SEP-*` | Background Job、Derived Asset、Source Link | 実装済み | モデル式Stem、キャンセル、再実行、比較 |
| H. AI | `FLOW-006`, `AI-*` | Context、Explain、Suggest、ChangeSet、Provider | 実装済み | External Provider、送信範囲、より豊かな提案 |
| I. Creative Memory | `LIB-*`, `PRJ-*` | Library、Metadata、Provenance、Package、Recovery | 実装済み | 大量Asset、Migration、Missing Fileの編集体験 |
| J. Recovery | `FLOW-007`, `REL-*`, `SEC-*` | 失敗の隔離、復旧、権限、ログ、所有権 | 実装中 | sidecar lifecycle、エラーの表現、長時間動作 |

## 製品横断の対応

| 横断要求 | 対応する責務 | 関連ID |
| --- | --- | --- |
| 原本を守る | Asset、Recording、Timeline、Render | `FLOW-003`、`FLOW-004`、`ARR-001`、`EXP-001` |
| 意図を残す | Project、Provenance、Snapshot、ChangeSet | `PRJ-001`、`LIB-004`、`AI-003`、`RACK-004` |
| 失敗範囲を限定する | Audio Runtime、Plugin Worker、Job、Recovery | `G-005`、`PLG-001`、`REL-001`–`REL-004` |
| ローカル中心で持ち運ぶ | Storage、Library、Package、Privacy | `SEC-001`–`SEC-003`、`PRJ-005` |
| 同じ操作言語を保つ | Interaction、Transport、Selection、Undo | `G-002`、`G-004`、`PRJ-003`、`DONE-001` |

## 一次仕様書との対応

| 一次仕様書 | 主な要求ID | 設計上の焦点 |
| --- | --- | --- |
| 3–4章 製品原則・メンタルモデル | `G-*`, `FLOW-*` | Scratch Session、非破壊、Asset、可逆性、Local First |
| 5–6章 画面構造・中核フロー | `G-*`, `FLOW-*` | Global Bar、Workspace、Inspector、Transport、Focus Mode |
| 7–9章 Audio・Plugin・Rack | `AUD-*`, `MIDI-*`, `PLG-*`, `RACK-*` | Safety、Realtime境界、VST3、Signal Flow、Snapshot |
| 10–12章 Recording・Arrange・Sample | `REC-*`, `ARR-*`, `SMP-*`, `EXP-*` | 原音保全、時間軸、内部音源、Export |
| 13–15章 Analyze・Separate・AI | `ANL-*`, `SEP-*`, `AI-*` | Offline Job、Reference、ChangeSet、権限 |
| 16–18章 Library・Project・Import/Export | `LIB-*`, `PRJ-*`, `EXP-*` | Provenance、検索、Recovery、Package、DAW handoff |
| 19–20章 Visual・Interaction | `G-*`, `WIN-*` | 密度、状態表現、入力、DPI、キーボード |
| 21–25章 Reliability・性能・Security・Windows | `REL-*`, `SEC-*`, `Q-*`, `WIN-*` | Fail Softly、Callback、所有権、設定、長時間動作 |
| 26–28章 対象外・完成基準・実装境界 | `DONE-*` | 製品の境界と、完了を名乗る条件 |

## 更新の単位

要求を追加・変更するときは、次の順で同じIDを更新します。

1. 一次仕様書の出典と期待するユーザー結果を確定する
2. `behavior-requirements.md` に条件・操作・結果を追加する
3. `architecture.md` の責務または境界に変更があれば更新する
4. 実装、回帰確認、既知課題を要求IDへ紐付ける
5. この台帳の状態を、実装の事実に合わせて更新する

実装詳細だけが変わる場合は要求IDを増やしません。ユーザーが観測する結果、責務の境界、データの所有権が変わる場合だけ、要求・アーキテクチャ・台帳を同時に見直します。
