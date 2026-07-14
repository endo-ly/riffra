# Riffra 製品挙動要件

この文書は、`CONCEPT.md` を、ユーザーが確認できる挙動単位へ分解した要件定義です。画面のモックや実装状況ではなく、ユーザーが行う操作と、製品が守るべき結果を記述します。

要求IDは、要件定義・設計・実装・確認の会話で同じ対象を指すための名前です。進捗、証跡、発見した課題、修正後の再確認は [挙動確認・課題管理表](./behavior-verification.md) に記録します。

## 要求の読み方

各項目は、次の順で読めるように構造化しています。

- **条件**: どの状態から始めるか
- **操作**: ユーザーまたはシステムが何をするか
- **期待結果**: ユーザーが確認できる事実
- **確認対象**: 画面、音声、保存データ、状態、履歴のどこに結果が現れるか
- **出典**: 元仕様書の章、または製品の完成基準

「期待結果」は、実装方法ではなく外から観測できる振る舞いで書きます。たとえば「SQLiteに保存する」ではなく「再起動後に同じAssetと関連を辿れる」と書きます。

## 1. 全体・セッション・安全

- **G-001 [P0] Scratch Sessionを即時に開く** — 条件: 初回起動またはプロジェクト未指定。操作: アプリを起動する。期待結果: プロジェクト作成ダイアログを挟まず、編集可能なScratch Sessionが表示される。証跡: Native UIの初期表示、session JSON。出典: 4.1、5.1、6.1、27.1。
- **G-002 [P0] 単一ウィンドウで状態を共有する** — 条件: Home/Play/Arrange/Sample/Analyze/Separateのいずれか。操作: Workspaceを切り替える。期待結果: Session、選択、Transport、Mute状態が失われず共有される。証跡: 各Workspaceを往復した画面とsession JSON。出典: 5.1–5.7、27.1。
- **G-003 [P0] 起動時は安全にMuteされる** — 条件: Cold start、Device変更、復旧直後。操作: 起動またはDeviceを切り替える。期待結果: 出力はMuteされ、Fade-inと保守的な初期Gainを経てから明示操作で音が出る。証跡: Audio status、Output meter、実聴。出典: 3.1、3.10、7.3。
- **G-004 [P0] Emergency Muteを全Workspaceから実行できる** — 条件: 音声がReadyまたは再生中。操作: Global Muteまたは指定ショートカットを押す。期待結果: 即座に無音になり、録音済みデータとSessionは変更されない。証跡: 音声、status、Session差分。出典: 7.3、20.2、27.1。
- **G-005 [P0] 失敗範囲を限定する** — 条件: Plugin、Device、Job、Libraryの一つが失敗。操作: 失敗を発生させる。期待結果: 影響範囲、データ安全性、復旧操作が表示され、他のWorkspaceは使用できる。証跡: エラー表示、保存状態、再操作。出典: 3.10、6.7、21。
- **G-006 [P1] Empty/Loading/Errorを区別する** — 条件: Assetなし、Job中、エラー発生。期待結果: 状態が文言と形状で区別され、原因・影響・次の操作が表示される。証跡: 画面キャプチャ。出典: 20.5。
- **G-007 [P1] Focus Modeは表示だけを整理する** — 条件: 編集中。操作: Focus Modeを切り替える。期待結果: 不要なPanelが隠れるが、音声・Session・Undo状態は変わらない。証跡: 切替前後のSession差分。出典: 5.7、19、20。

## 2. 中核ユーザーフロー

- **FLOW-001 [P0] 起動して演奏する** — 条件: 前回SessionとDevice設定がある。操作: 起動して必要なら一度だけ入力を有効化する。期待結果: 前回のDevice/Rackが復元され、安全な音量で演奏可能になる。証跡: Device、Rack、Input/Output meter、実聴。出典: 6.1。
- **FLOW-002 [P0] 音色候補を比較する** — 条件: Rackと2個以上のSnapshotがある。操作: Snapshotを切り替える。期待結果: 現在状態を失わず、音量差を抑えたA/B比較ができる。証跡: Snapshot state、実聴、Gain/meter。出典: 6.2、9.6。
- **FLOW-003 [P0] 思いつきを録る** — 条件: DeviceがReady。操作: Home/Play/ArrangeからRecordを開始・停止する。期待結果: Raw/ProcessedがInboxへ保全され、名前なしでも後から利用できる。証跡: WAV、manifest、Inbox表示。出典: 6.3、10.1。
- **FLOW-004 [P0] 録音をArrangeへ進める** — 条件: 完了したRecordingがある。操作: Place、編集、Renderを行う。期待結果: 原本を変更せず、Timelineから新しい音声を書き出せる。証跡: 原本hash、Session、出力WAV。出典: 6.4、11、18.2。
- **FLOW-005 [P1] AudioからSampleを作る** — 条件: 読み取り可能なAudioがある。操作: Slice、Pad割当、Preview、MIDI Triggerを行う。期待結果: 原本を保持したまま、指定範囲が再生される。証跡: Sample mapping、音声、Session。出典: 6.5、12。
- **FLOW-006 [P1] AI案を確認して適用する** — 条件: Analysis、Reference、変更対象がある。操作: Contextを選び、ChangeSetをPreview/Reject/Applyする。期待結果: Target、Current、Proposed、Reason、Effect、Risk、適用結果が確認でき、適用は明示操作だけで行われる。証跡: ChangeSet履歴、Session差分、Undo。出典: 6.6、15。
- **FLOW-007 [P0] 障害から復旧する** — 条件: Plugin/Device/Processの異常。操作: 再試行、Bypass、Safe Mode、Recoveryを行う。期待結果: 録音済みデータを失わず、問題箇所と選択肢が表示される。証跡: recovery世代、ログ、UI。出典: 6.7、21、27.1-J。

## 3. Audio Engine・Device・MIDI

- **AUD-001 [P0] Driverを列挙する** — 条件: Windows上でAudio APIが利用可能。期待結果: ASIO、WASAPIなどを検出し、入力/出力数を表示する。出典: 7.1、7.2。
- **AUD-002 [P0] Sample Rate/Bufferを適用する** — 条件: Driverを選択。操作: Sample RateとBufferを変更する。期待結果: 一時Mute後に設定値がNative DeviceとSessionへ反映される。出典: 7.2、24、25。
- **AUD-003 [P0] Input/OutputをMeter表示する** — 条件: DeviceがReady。期待結果: Peak、Clip、Invalid sample、Latencyが更新される。出典: 7.4。
- **AUD-004 [P0] Device切断を安全に処理する** — 条件: 再生または録音中にDeviceを切断。期待結果: EngineがFault/Mutedになり、取得済みデータは読み取り可能なまま残る。出典: 7.1、7.3、10.5、21.2。
- **AUD-005 [P1] Plugin/ParallelのLatencyを補償する** — 条件: Latencyの異なる処理Pathがある。期待結果: Timeline、Render、Parallel Pathの同期位置が維持され、補償量と未補償状態が表示される。出典: 7.5、11.1。
- **AUD-006 [P0] 異常値とFeedbackを保護する** — 条件: NaN、Inf、過大Peak、Feedback疑い。期待結果: 出力がLimiter/Muteで保護され、原因が表示される。出典: 7.3。
- **MIDI-001 [P0] MIDI Portを検出・開閉する** — 条件: Windows MIDI Deviceが接続/切断される。期待結果: Port一覧が更新され、Open/Close結果が表示される。出典: 7.6、24。
- **MIDI-002 [P1] MIDIイベントを処理する** — 条件: Note、Velocity、Sustain、Pitch Bend等を受信。期待結果: 対応する音源/Pluginへ伝達され、録音時に再現可能なSidecarが保存される。出典: 7.6、10.2、11.4。
- **MIDI-003 [P0] Panicで全Noteを止める** — 条件: MIDI音が鳴っている、またはDeviceが切断。操作: Panic。期待結果: Pad、Synth、Pluginの鳴り続けるNoteが停止する。出典: 7.6、12。

## 4. Plugin Host・Rack・Tone Design

- **PLG-001 [P0] VST3 Scanを隔離実行する** — 条件: VST3 Folderに正常/異常Pluginが混在。期待結果: 異常PluginがUIやAudioを停止させず、対象Pluginと理由が特定される。出典: 8.2、8.6。
- **PLG-002 [P1] Plugin Browserを検索する** — 条件: Catalogがある。操作: Name、Vendor、Category、Favorite、Stabilityで絞り込む。期待結果: 結果が正確に更新される。出典: 8.3。
- **PLG-003 [P0] PluginをLoad/Bypass/Removeする** — 条件: Validated VST3がある。期待結果: Rack表示、Native process、Bypass、Removeが一致する。出典: 8.4、8.5。
- **PLG-004 [P1] Common Parameter Viewを操作する** — 条件: Parameterを持つPluginがLoad済み。操作: Slider/Toggle等を変更する。期待結果: Native側の値、表示値、Session保存値が一致する。出典: 8.4。
- **PLG-005 [P0] Plugin Stateを完全保存・復元する** — 条件: Parameter、内部Preset、Bypass、Stateを変更。操作: 保存、再起動、Snapshot切替。期待結果: 可能な範囲で完全に復元し、復元不能時はDisabled Placeholderになる。出典: 8.5、17.1。
- **RACK-001 [P1] Serial/Parallel Signal Flowを表示する** — 条件: Rackに複数Deviceがある。期待結果: InputからOutputの経路、Bypass、Mute、Channel数、異常箇所が読める。出典: 9.1–9.4。
- **RACK-002 [P1] Rackを編集・再利用する** — 操作: Add、Remove、Duplicate、Reorder、Fragment保存/挿入。期待結果: 接続結果とSession stateが一致する。出典: 9.2。
- **RACK-003 [P1] Macroを複数ParameterへMapする** — 操作: Min/Max/Invert/Curveを設定。期待結果: Macro操作が対象Parameterへ安全範囲で反映される。出典: 9.5。
- **RACK-004 [P1] Snapshotを複数保存・比較する** — 操作: Save、Rename、Tag、A/B、Blind比較。期待結果: 現在状態を壊さず、DifferenceとPreviewを確認できる。出典: 9.6。
- **RACK-005 [P2] Tone Explorationを安全に行う** — 操作: Lock、Randomize、Variation、Candidate Batch。期待結果: 危険Parameterが制限され、Keep/Reject/Undoが可能。出典: 9.7。
- **RACK-006 [P1] Freeze/Render fallbackを作る** — 操作: Freeze、Render in Place、Flatten。期待結果: 原Rack/MIDI/DIを保持し、Pluginなしでも当時の音を再生できる。出典: 9.8、18.4。

## 5. Recording・Arrange・Export

- **REC-001 [P0] Raw/Processedを同時録音する** — 期待結果: 両方のWAV、同一条件のmanifest、録音中のFlush、停止後のFinalizationが成立する。出典: 10.1、10.2。
- **REC-002 [P1] Count-in/Pre-roll/Punch/Loopを扱う** — 期待結果: 録音開始位置と実データ範囲が設定どおりで、不要部分が明示される。出典: 10.3。
- **REC-003 [P1] Take Groupを管理する** — 期待結果: Recorded At、Input、Rack、Snapshot、Latency、Rating、Selected Stateを保持し、Takeを切り替えられる。出典: 10.4。
- **REC-004 [P0] 録音異常時に取得済みデータを保全する** — 条件: UI停止、Device切断、Disk低速、Plugin異常。期待結果: Dropout位置、Missing Range、Recovery Statusを表示し、Partial WAVを失わない。出典: 10.5、21.6。
- **ARR-001 [P0] Audio/MIDIを非破壊配置する** — 操作: Place、Move、Duplicate、Split、Trim、Loop、Fade、Gain、Pan、Mute。期待結果: 原本Fileのhashが変わらない。出典: 11.1、11.3。
- **ARR-002 [P1] Track/Mixerを共有する** — 期待結果: Audio Track、MIDI Track、Group、Return、MasterのVolume/Pan/Mute/Solo/Arm/RackがTimelineと一致する。出典: 11.2、11.7。
- **ARR-003 [P1] MIDI Clipを編集する** — 操作: Note、Start、Length、Velocity、Channel、Quantize、Transpose、Humanize。期待結果: Piano Roll、再生、標準MIDI Exportが一致する。出典: 11.4、18.3。
- **ARR-004 [P1] Tempo/Marker/Loop Regionを扱う** — 期待結果: BPM、Time Signature、Tempo Change、Marker、GridがAudio/MIDI同期に反映される。出典: 11.5。
- **ARR-005 [P2] Automationを編集する** — 期待結果: Track Volume、Pan、Macro、Plugin Parameter等を点/線/曲線で編集し、記録操作を整理できる。出典: 11.6。
- **EXP-001 [P0] Master/Track/Stemを書き出す** — 条件: Timelineに有効Clipがある。期待結果: Range、Normalize、Format、Sample Rate、Bit Depth、Mono/Stereo、Metadataが設定どおりで、原本は変わらない。出典: 18.2、27.1-D。
- **EXP-002 [P1] DAW handoffを行う** — 期待結果: Track WAV、MIDI、BPM、Time Signature、Marker、Used Plugin/Rack情報が同期位置を保って出力される。出典: 18.3。

## 6. Sample・内部音源

- **SMP-001 [P0] AudioをSampleへ非破壊Importする** — 期待結果: One-shot、Loop、Pad、Keyboard用のSource Linkが保持される。出典: 12.1。
- **SMP-002 [P1] Sample範囲・Loop・Envelopeを編集する** — 期待結果: Start/End、Loop、Crossfade、Pitch、Rate、Reverse、ADSR、Filter、Root Keyが保存される。出典: 12.2。
- **SMP-003 [P1] Drum Pad/Kitを保存する** — 期待結果: MIDI Note、Velocity、Layer、Choke、Round Robin、OutputがKitとして再利用できる。出典: 12.3。
- **SMP-004 [P1] Keyboard Instrumentを作る** — 期待結果: Root Key、Key Range、Velocity Range、Layer、Release Sampleを再生できる。出典: 12.4。
- **SMP-005 [P1] Internal Synthを使う** — 期待結果: Oscillator、Detune、ADSR、Filter、LFO、Unison、Portamento、Effect、Presetが外部Pluginなしで動く。出典: 12.5。
- **SMP-006 [P1] Utility Deviceを使う** — 期待結果: Gain、Pan、EQ、Compressor、Limiter、Gate、Delay、Reverb等が安全な範囲で動作する。出典: 12.6。

## 7. Analyze・Reference・Separate

- **ANL-001 [P0] WAVを解析する** — 期待結果: Waveform、Peak、True Peak、RMS/LUFS、Spectrum、Spectrogram、Phase、Correlation、Clipping、Dynamic Range、Durationが表示される。出典: 13.1。
- **ANL-002 [P1] Referenceを比較する** — 期待結果: Loudness、Peak、Duration、Spectrum、Phase等の差分をRead-onlyで確認できる。出典: 13.2。
- **ANL-003 [P1] Referenceを同期Previewする** — 期待結果: Current/Referenceを同じ開始条件で再生し、Loopと停止が機能する。出典: 13.2、13.3。
- **SEP-001 [P0] Separation Jobを実行する** — 条件: 対応Audioがある。期待結果: UIをブロックせず、進捗、失敗範囲、キャンセル結果を表示する。出典: 14.2、14.4。
- **SEP-002 [P0] Separation Resultを非破壊保存する** — 期待結果: Source、Model、設定、生成File、Manifest、Preview、Timeline追加が関連付く。出典: 14.3、16.7。

## 8. AI Assistant

- **AI-001 [P0] Explainは変更しない** — 期待結果: 状態説明だけを行い、Session、File、Projectを変更しない。出典: 15.2。
- **AI-002 [P0] SuggestはChangeSetだけを作る** — 期待結果: 自動適用せず、Target、Current、Proposed、Reason、Effect、Confidence、Riskを表示する。出典: 15.2、15.4。
- **AI-003 [P0] Applyは確認済みChangeSetだけを適用する** — 期待結果: Apply Selected/All、Reject、Preview、Undoが機能し、不可逆操作を自動実行しない。出典: 15.2、15.4、17.4。
- **AI-004 [P0] Contextを制御する** — 期待結果: Selected Rack、Parameter、Analysis、Clip、Project、Note、Snapshot、Preview Audio、Error Logの送信対象を選択できる。出典: 15.5。
- **AI-005 [P0] Providerと外部送信を明示する** — 期待結果: Local/External、対象Data、Destination、Purpose、Retention、Trim/Downmix/Anonymizeを送信前に確認でき、音声送信は明示操作のみで行う。出典: 15.6、15.7、23。

## 9. Library・Project・履歴

- **LIB-001 [P1] Asset種別を横断検索する** — 期待結果: Plugin、Preset、Rack、Audio、Recording、MIDI、Stem、Project、Analysis、AI Suggestionを同じ検索で扱える。出典: 16.1、16.5。
- **LIB-002 [P1] Metadataを後から編集する** — 期待結果: Name、Tag、Favorite、Rating、Note、Created/Updated、Usage、Provenance、Missing Dependencyを保持する。出典: 16.2、16.4。
- **LIB-003 [P0] Inboxを保全領域として使う** — 期待結果: 未整理Recording/Import/AI/Separationが消えず、Preview、Rename、Tag、Promote、Archive、Delete、Duplicate Detectionが可能。出典: 16.3。
- **LIB-004 [P1] Related Assetを辿る** — 期待結果: Recording→Rack、Rack→Snapshot、Sample→Kit、Analysis→Audio、AI→Version等の関連を確認できる。出典: 16.7。
- **PRJ-001 [P0] Project内容を完全に保存する** — 期待結果: Timeline、Track、Rack、Plugin State、MIDI、Audio参照、AI History、View/I/O/Export設定を復元できる。出典: 17.1。
- **PRJ-002 [P0] Auto Saveを世代管理する** — 期待結果: Current、未確定変更、Recovery用世代を分け、破損時に最新の有効世代へ戻れる。出典: 17.2、27.1。
- **PRJ-003 [P1] Version/Undo/Redoを操作する** — 期待結果: Parameter、Rack、Clip、Track、Metadata、AI ChangeSet、Import/Deleteを取り消せる。出典: 17.3、17.4。
- **PRJ-004 [P0] Missing File/Pluginでも開く** — 期待結果: Missing List、Relink、Replace、Ignore、Disabled Placeholderを使い、残りのProjectを開ける。出典: 17.5、8.5。
- **PRJ-005 [P1] Portable Packageを作る** — 期待結果: Referenced Audio、MIDI、Project State、Rack、Fallback、Used Plugin List、Version、Noteを安全に収集し、Plugin Binary/Licenseを無断同梱しない。出典: 17.6。
- **PRJ-006 [P1] Format Migrationを安全に行う** — 期待結果: Format Versionを確認し、変換前Backupを作成し、旧Formatの扱いを明示する。出典: 17.7。

## 10. Reliability・Security・Windows品質

- **REL-001 [P0] Plugin障害を隔離・復旧する** — 期待結果: 問題Pluginの特定、Kill/Disable/Retry、Placeholder化、Project継続利用が可能。出典: 8.6、21.1。
- **REL-002 [P0] Audio sidecarを孤立させない** — 条件: UI異常終了、再起動、Sidecar異常。期待結果: Sidecarが安全に終了または親プロセス喪失を検出し、孤立Processと録音Lockを残さない。出典: 21.1、21.3。
- **REL-003 [P0] Background Jobをキャンセル・再開する** — 期待結果: Analysis、Render、Separation、ScanがUIを止めず、キャンセル後もPartial Resultを誤って完成扱いしない。出典: 14.4、21.5。
- **REL-004 [P0] Disk障害を扱う** — 期待結果: 空き容量不足、書込失敗、破損、Partial Fileを明示し、取得済みデータを保全する。出典: 10.5、21.6。
- **SEC-001 [P0] Local Firstを守る** — 期待結果: Audio/Project/AI Contextは明示許可なしに外部送信されず、Offlineでも中核制作機能が動く。出典: 3.9、15.7、23.1。
- **SEC-003 [P1] Logを安全に扱う** — 期待結果: Logに必要な診断情報だけを保存し、Audio/個人情報を無制限に含めない。出典: 23.4、23.5。
- **WIN-001 [P1] DPI/複数モニターで使える** — 期待結果: DPI変更、Window移動、再接続、縦長/横長表示で主要操作が隠れない。出典: 19.9、24。
- **WIN-002 [P1] Settingsを明示的に保存する** — 期待結果: Audio、MIDI、Theme、Library、AI Provider、Privacy、Shortcut、保存先が再起動後も一致する。出典: 25。

## 11. 性能・製品完成判定

- **Q-001 [P0] Startupを測定する** — 期待結果: Cold/Warm、Deviceあり/なし、Plugin Scan中のUI表示時間を、Windows Buildと構成付きで記録する。出典: 22.1。
- **Q-002 [P0] UIをAudioより優先しない** — 期待結果: Scan、AI、Analysis、Library、Render中もAudio Callback、Mute、Recordingが破綻しない。出典: 2、22.2、22.3。
- **Q-003 [P0] 長時間安定性を確認する** — 期待結果: 長時間再生、録音、Plugin、Library操作でMemory/Handle/CPU/Dropoutが許容範囲内に収まる。出典: 22.5。
- **DONE-001 [P0] Product-level DoDを満たす** — 条件: A〜Jの全項目。期待結果: 各項目が実機受入、回帰テスト、証跡リンクを持ち、未完了のKnown Gapが明示されている。出典: 27.1。
- **DONE-002 [P0] 失敗状態を残さない** — 期待結果: 録音データ消失、無音のまま成功表示、壊れたmanifestの既定値化、孤立sidecar、外部送信の不可視化がない。出典: 27.2。

## 12. 実装へ渡すときの観測点

要求を設計・実装へ渡すときは、少なくとも次の観測点を一つ以上定義します。

- **音**: 出力、無音、音量、同期、鳴り終わり
- **画面**: 状態、選択、操作可能性、エラーの意味
- **データ**: 原本、生成物、manifest、Project、Library、Provenance
- **履歴**: Undo/Redo、Snapshot、AI ChangeSet、Recovery generation
- **境界**: Plugin、Device、MIDI、外部Providerが失敗したときの影響範囲

一つの要求が複数の層にまたがる場合でも、要求IDは一つに保ち、各層の責務と観測点を設計書・実装・テストから辿れるようにします。
