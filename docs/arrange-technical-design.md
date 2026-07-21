# Arrange Timeline 技術設計

## 1. 目的

本書は、Arrange Timeline の時間表現、保存状態と Audio Runtime の同期契約、Transport の状態遷移を定義する。

対象は次の三つである。

- Project、Audio、MIDIで共有する時間モデル
- Rustが所有する制作状態からC++ Audio Runtimeへ渡すSnapshotとDiff
- Audio EngineのTimeline Sample位置を正本とするTransport

画面配置、色、個別の編集ジェスチャーは本書の対象外とする。

## 2. 所有権と依存方向

```text
React
表示、Pointer操作、一時的なDrag表示
        │
        │ ArrangementEdit
        ▼
Rust
制作状態、検証、履歴、保存、Asset解決、Revision
        │
        │ RuntimeTimelineSnapshot / RuntimeTimelineDiff
        ▼
C++ Audio Runtime
Audio Clock、Timeline位置、再生Schedule、Read-ahead、Resampling、DSP
        │
        ▼
Audio Device
```

責務は次のように分離する。

| 層    | 所有する状態                                                               | 所有しない状態                          |
| ----- | -------------------------------------------------------------------------- | --------------------------------------- |
| React | Selection、Scroll、Zoom、編集中のPointer座標                               | 永続Timeline、Audio Clock、ファイルパス |
| Rust  | Arrangement、AssetId、Undo/Redo、保存、Runtime Revision                    | Audio Buffer、再生用Source Cursor       |
| C++   | Prepared Timeline、Audio Clock、Timeline Sample位置、Source Cache、DSP状態 | AssetId、編集履歴、Project保存          |

Rustの保存が成功した編集は、Audio Runtimeが停止または故障していても取り消さない。Runtimeとの同期に失敗した場合は、保存状態を維持したまま`outOfSync`として報告し、復旧後に完全Snapshotを再送する。

## 3. 時間モデル

### 3.1 基本単位

Projectは固定PPQ、単一BPM、単一拍子を持つ。

| 項目               | 型    | 規則                                            |
| ------------------ | ----- | ----------------------------------------------- |
| PPQ                | `u32` | `960`固定                                       |
| Timeline Tick      | `u64` | `1.1.1`を`0`とする                              |
| BPM                | `f64` | 有限かつ`1.0`以上`999.0`以下                    |
| 拍子分子           | `u8`  | `1`以上                                         |
| 拍子分母           | `u8`  | `1, 2, 4, 8, 16, 32`のいずれか                  |
| Audio Frame        | `u64` | Source Asset固有Sample Rate上のFrame index      |
| Audio Clock Sample | `u64` | Device開始後に処理したFrame数。稼働中は単調増加 |
| Timeline Sample    | `u64` | Project先頭を0とする再生位置。SeekとLoopで変化  |

PPQを960とすることで、通常音価と仕様で必要な三連符を整数Tickで表現できる。

| Grid         | Tick |
| ------------ | ---: |
| 1 Bar、4/4   | 3840 |
| 1/4          |  960 |
| 1/8          |  480 |
| 1/16         |  240 |
| 1/32         |  120 |
| 1/8 Triplet  |  320 |
| 1/16 Triplet |  160 |

浮動小数点秒やミリ秒を、Clip位置、MIDI位置、Snap結果の正本として保存しない。

拍子分母を考慮した1 Beatと1 Barの長さは次のとおりとする。

```text
ticksPerBeat = PPQ × 4 / timeSignatureDenominator
ticksPerBar = ticksPerBeat × timeSignatureNumerator
```

### 3.2 時間型

概念上、Rust Domainは次の型を持つ。

```rust
struct TimelineTick(u64);

struct TickRange {
    start: TimelineTick,
    end: TimelineTick,
}

struct FrameRange {
    start: u64,
    end: u64,
}

struct FrameDuration {
    frames: u64,
    sample_rate: u32,
}

struct ProjectTimebase {
    ppq: u32,
    bpm: f64,
    time_signature_numerator: u8,
    time_signature_denominator: u8,
}
```

`TickRange`と`FrameRange`は半開区間`[start, end)`とし、`end > start`を必須とする。`0`を「末尾まで」などのSentinelとして使用しない。

### 3.3 Audio Clip

Audio Clipは、音楽時間上の開始位置と実時間上の長さを分けて保持する。

```rust
struct AudioClip {
    id: AudioClipId,
    track_id: TrackId,
    asset_id: AssetId,
    start_tick: TimelineTick,
    source_range: FrameRange,
    source_sample_rate: u32,
    timeline_duration: FrameDuration,
    fade_in: FrameDuration,
    fade_out: FrameDuration,
    gain_db: f64,
    pan: f64,
    loop_enabled: bool,
    muted: bool,
}
```

- `start_tick`はBPM変更後も維持する。
- `source_range`と`timeline_duration`はBPM変更の影響を受けない。
- 通常再生ではSourceを1倍速で読む。BPM変更による自動Stretchは行わない。
- `timeline_duration`、`fade_in`、`fade_out`のSample Rateは`source_sample_rate`と一致させる。
- 非Loop Clipの`timeline_duration.frames`は`source_range`のFrame数と一致させる。
- Loop Clipは`source_range`を周期として、周期以上の`timeline_duration`まで繰り返す。
- Fade長は`timeline_duration`以下とする。
- Fadeは実時間であり、BPM変更の影響を受けない。
- AssetがMissingになっても、保存済みの位置、Source範囲、実時間長を保持する。

`source_sample_rate`は、Missing AssetでもFrame位置を解釈できるようClip作成時のAsset情報を保存する。再リンク時にSource形式が変化した場合は、自動換算せず明示的な再リンク検証を行う。

### 3.4 MIDI ClipとMIDI Event

MIDI ClipとMIDI Eventは音楽時間だけで保持する。

```rust
struct MidiClip {
    id: MidiClipId,
    track_id: TrackId,
    asset_id: Option<AssetId>,
    start_tick: TimelineTick,
    length_ticks: u64,
    events: Vec<MidiEvent>,
    loop_enabled: bool,
    muted: bool,
}

struct MidiEvent {
    id: MidiEventId,
    tick_offset: u64,
    sequence: u32,
    message: MidiMessage,
}
```

- `tick_offset < length_ticks`を必須とする。
- 同一TickのEventは`sequence`で安定順序を持つ。
- Note On、Note Off、Control Change、Pitch Bend、Channel Pressureを保持する。
- Note Lengthは、対応するNote On/OffのTick差として扱う。
- BPM変更時はTickを維持し、再生Sample位置だけを再計算する。

### 3.5 TickとTimeline Sampleの変換

単一BPMにおける基本変換は次のとおりとする。

```text
seconds = tick × 60 / (BPM × PPQ)
timelineSample = round(seconds × deviceSampleRate)
```

逆変換は表示、Seek、Engine Statusに使用する。

```text
tick = round(timelineSample × BPM × PPQ / (60 × deviceSampleRate))
```

境界計算は共通関数へ集約し、UI、Rust、C++で異なる丸め規則を持たない。RustとC++は同じGolden Vectorで変換結果を検証する。

### 3.6 BPM変更

停止中のBPM変更は、全ClipとEventのTickを維持してScheduleを再構築する。

再生中のBPM変更は次の順序で適用する。

1. Audio Engineが現在のPlayhead Tickを確定する。
2. 新しいTimebaseを次のAudio Block境界で適用する。
3. Playhead Tickを維持し、以後のScheduleを新BPMで再計算する。
4. Active VoiceはSource再生速度を変更せず、不連続が必要な場合は短いDe-click Envelopeを適用する。

再生停止後に再開した場合は、Project先頭から新しい単一BPMの変換を使用する。

## 4. Arrangement Revisionと編集Transaction

Arrangementは単調増加する`revision: u64`を持つ。保存対象となる一回のユーザー操作を一つのTransactionとして扱う。

```rust
struct ArrangementEditRequest {
    base_revision: u64,
    edit: ArrangementEdit,
}

struct ArrangementEditResult {
    revision: u64,
    changes: Vec<ArrangementChange>,
    runtime_sync: RuntimeSyncState,
}
```

確定編集は次の順序で処理する。

1. 現在Revisionに対して編集を検証し、新しいArrangementを構築する。
2. Undo用のInverse Editを記録する。
3. Revisionを増加させ、SessionをAtomic Saveする。
4. 保存成功後にCanonical Sessionを交換する。
5. Runtimeが同期中なら対応するDiffを送る。
6. 保存結果とRuntime同期状態をReactへ返す。

保存に失敗した場合はCanonical SessionとRevisionを変更しない。Runtime反映だけが失敗した場合はCanonical Sessionを維持し、`outOfSync`へ遷移する。

- Rustは`base_revision`が現在値と一致する場合だけ編集を適用する。
- 成功時のRevisionは現在値から一つだけ増加させる。
- Drag中の多数のPointer Moveを保存履歴へ積まない。
- Pointer Upで確定したMove、Trim、Fade等を一つのUndo単位とする。
- Undo/Redoも新しいRevisionを作り、過去のRevision番号へ戻さない。
- Revision競合時は現在状態を返し、React側で黙って上書きしない。

連続操作の試聴には、保存しないRuntime Previewを使用できる。

```rust
struct RuntimePreviewRequest {
    interaction_id: String,
    base_revision: u64,
    changes: Vec<RuntimeTimelineChange>,
}
```

PreviewはGain、Pan、Mute、Solo、Clip位置、Trim、Loop境界など、安全に差し替え可能な項目だけを許可する。確定または取消し時にPreviewを破棄し、Canonical Revisionの状態へ収束させる。

## 5. Runtime Timeline契約

### 5.1 完全Snapshot

完全Snapshotは次の場合に送る。

- Audio Runtime起動時
- Project読込み時
- Device復旧またはSidecar再起動後
- Runtime Revision不一致時
- Diffより完全再構築が単純な大規模変更時

概念上のPayloadは次のとおりとする。

```json
{
  "type": "loadTimelineSnapshot",
  "requestId": 41,
  "protocolVersion": 1,
  "snapshot": {
    "revision": 120,
    "timebase": {
      "ppq": 960,
      "bpm": 120.0,
      "timeSignatureNumerator": 4,
      "timeSignatureDenominator": 4
    },
    "loopRange": {
      "enabled": true,
      "startTick": 3840,
      "endTick": 7680
    },
    "tracks": [],
    "audioClips": [],
    "midiClips": [],
    "automation": []
  }
}
```

Runtime DTOではAssetIdを送らず、Rustが解決したSource情報を送る。

```json
{
  "clipId": "clip-id",
  "trackId": "track-id",
  "path": "C:\\resolved\\source.wav",
  "sourceSampleRate": 48000,
  "sourceChannels": 2,
  "sourceFrames": 960000,
  "startTick": 3840,
  "sourceStartFrame": 0,
  "sourceEndFrame": 480000,
  "durationFrames": 480000,
  "durationSampleRate": 48000,
  "fadeInFrames": 0,
  "fadeOutFrames": 0,
  "gainDb": 0.0,
  "pan": 0.0,
  "loopEnabled": false,
  "muted": false
}
```

Missing AssetはProjectから削除しない。RustはRuntime Snapshotから再生Sourceを除外し、`unavailableClipIds`としてRuntime Sync結果へ含める。他のClipは引き続き再生できる。

### 5.2 Diff

通常の確定編集はRevision付きDiffとして送る。

```json
{
  "type": "applyTimelineDiff",
  "requestId": 42,
  "protocolVersion": 1,
  "baseRevision": 120,
  "targetRevision": 121,
  "changes": [
    {
      "type": "upsertAudioClip",
      "clip": {}
    }
  ]
}
```

正式なChange種別は次の集合とする。

- `replaceTimebase`
- `replaceLoopRange`
- `upsertTrack` / `removeTrack`
- `upsertAudioClip` / `removeAudioClip`
- `upsertMidiClip` / `removeMidiClip`
- `replaceTrackAutomation`

Diffは全件検証後に一括適用する。一件でも不正な場合は全体を拒否し、直前のPrepared TimelineとRevisionを維持する。

Rustが`outOfSync`または`unavailable`を認識している間は、後続編集をDiffで同期しない。Runtimeが利用可能になった時点で最新Revisionの完全Snapshotを送り、ACK後にDiff同期へ戻る。

### 5.3 ACKと同期状態

SnapshotとDiffの成功応答は、Audio Block境界で交換可能なPrepared Timelineが完成したことを表す。

```json
{
  "type": "timelineAck",
  "requestId": 42,
  "revision": 121,
  "appliedAtAudioClockSample": 492880128,
  "unavailableClipIds": []
}
```

同期状態は次の三つとする。

| 状態          | 意味                                 |
| ------------- | ------------------------------------ |
| `synced`      | 保存RevisionとRuntime Revisionが一致 |
| `outOfSync`   | 保存は成功したがRuntime反映に失敗    |
| `unavailable` | Audio Runtimeが停止または未起動      |

SourceのOpen、Decode、Read-ahead準備はAudio Thread外で行う。準備できない一部SourceはUnavailableとして扱い、Timeline全体の交換を妨げない。

## 6. Transport

### 6.1 公開状態

Transportの公開状態は次の五つとする。

```text
Stopped
Starting
Playing
Stopping
Faulted
```

録音状態はTransportと直交する別の状態機械として扱う。Seekは永続状態ではなく、現在状態に対してAudio Block境界で適用するOperationとする。

Transport Commandは次の集合とする。

| Command             | 引数                             | 挙動                             |
| ------------------- | -------------------------------- | -------------------------------- |
| `playTimeline`      | `expectedRevision`               | 保持中のTimeline位置から再生する |
| `stopTimeline`      | なし                             | 現在位置で停止する               |
| `seekTimeline`      | `expectedRevision`, `targetTick` | 状態を維持したまま位置を変更する |
| `goToTimelineStart` | `expectedRevision`               | Tick 0へ移動する                 |

`expectedRevision`がPrepared Timelineと一致しない場合は、再生またはSeekを開始せずRevision不一致を返す。`stopTimeline`はRuntime同期状態にかかわらず常に受け付ける。

### 6.2 状態遷移

| 現在               | 操作・事象               | 次           | 挙動                                  |
| ------------------ | ------------------------ | ------------ | ------------------------------------- |
| Stopped            | Play                     | Starting     | 現在位置のSourceを準備する            |
| Starting           | 最初のBlockを開始        | Playing      | Timeline位置を進める                  |
| Starting           | Stop                     | Stopping     | 発音前でも開始を取消す                |
| Playing            | Stop                     | Stopping     | 短いFade後、現在位置を保持する        |
| Stopping           | Fade完了                 | Stopped      | Clockを停止する                       |
| Stopped            | Seek                     | Stopped      | 位置を更新して保持する                |
| Starting / Playing | Seek                     | 同じ公開状態 | VoiceとRead-aheadを新位置へ切り替える |
| Playing            | Timeline End             | Stopped      | End位置を保持する                     |
| Playing            | Loop End                 | Playing      | 同一Block内でLoop Startへ戻る         |
| 任意               | Audio Device障害         | Faulted      | 出力を止め、位置と保存状態を保持する  |
| Faulted            | Device復旧とSnapshot同期 | Stopped      | 復旧前の位置を保持する                |

`Play`、`Stop`の重複要求は冪等に扱う。無効なSnapshotまたはDiffはTransportをFaultedにせず、コマンド失敗として直前のTimelineを維持する。

### 6.3 Seek

Seekは停止中・再生中の両方で使用できる。

1. 要求Tickを有効範囲へClampする。
2. 次のAudio Block境界を適用点とする。
3. Active MIDIへAll Notes Offを送る。
4. Audio Source CursorとRead-aheadを新位置へ切り替える。
5. De-click Envelopeを適用する。
6. 再生中なら新位置から継続し、停止中なら位置だけを保持する。

SeekによってAudio DeviceまたはAudio Engine Processを再起動しない。

### 6.4 Loop境界

Loop範囲はTickの半開区間とし、`end > start`を必須とする。

Audio BlockがLoop Endを跨ぐ場合、Blockを境界で分割して前半をLoop Endまで、後半をLoop Startから処理する。AudioとMIDIは同じDevice Sample境界で折り返す。

Loop折り返し時は、範囲外へ伸びるMIDI Noteを停止し、Loop Startに存在する状態を再構築する。Audio ClipはLoop Start位置に対応するSource Offsetから再開する。

### 6.5 Transport Status

C++はTransport状態を独立したEventとして通知する。

```json
{
  "type": "transportStatus",
  "sequence": 8001,
  "clockGeneration": 4,
  "state": "playing",
  "revision": 121,
  "audioClockSample": 492880128,
  "timelineSample": 918528,
  "sampleRate": 48000,
  "timelineTick": 36741,
  "loopIteration": 2,
  "discontinuity": 7
}
```

- `audioClockSample`はAudio Deviceが処理したFrame数であり、Device稼働中は単調増加する。
- `clockGeneration`はDevice開始または再開始のたびに増加し、`audioClockSample`の基準変更を識別する。
- `timelineSample`はProject先頭を0とする再生位置であり、SeekとLoopによって不連続に変化する。
- Playhead位置の正本は`timelineSample`とする。
- `timelineTick`は共通変換規則による表示・検算値とする。
- `sequence`はEventの逆転や欠落を検出するため単調増加させる。
- Seek、Loop、Snapshot交換など位置が不連続になるたび`discontinuity`を増加させる。
- Rustは内容を再解釈せず、`transport-status`としてReactへ転送する。

定期Statusは20 Hzを基準とし、状態遷移、Seek、Loop折り返し、Revision交換、Faultは次の定期通知を待たず送る。Audio CallbackはLock-freeなStatus Slotだけを更新し、JSON生成と標準出力への書込みは非Realtime Threadが行う。

Reactは最新のEngine StatusをAnchorとして`requestAnimationFrame`上で`timelineSample`の表示位置だけを補間できる。補間値をTransport状態や編集結果として保存せず、新しいEngine Status受信時に必ず補正する。停止、Fault、Discontinuity検出時は補間を直ちに止める。

## 7. Realtime制約

Audio Callbackでは次の処理を禁止する。

- ファイルOpen、Read、Seek
- JSON処理
- Mutex、CriticalSection、条件変数の待機
- 動的メモリー確保と解放
- RustまたはReactへの同期通知
- Source解析とSample Rate判定

Command ThreadとSource Workerが、検証済みのPrepared TimelineとRead-ahead Bufferを構築する。Audio CallbackはAtomicに公開されたPrepared TimelineをAudio Block開始時に取得する。

```text
Rust Diff
   ↓
Command Threadで検証・構築
   ↓
Source WorkerでOpen・Read-ahead・Resampling準備
   ↓
Atomic Prepared Timeline
   ↓ Block境界
Audio Callback
```

Read-aheadが間に合わない場合はCallbackを待たせず、該当範囲を無音化してDropout Counterへ記録する。失敗を正常再生として扱わない。

## 8. 検証条件

### 8.1 時間変換

- 44.1、48、96 kHzで同じTickが期待Sampleへ変換される
- 1/8、1/16 Tripletが整数Tickになる
- 長時間位置で変換誤差が累積しない
- BPM変更後もAudioのSource Frame数が変化しない
- MIDI NoteのTick長が変化しない

### 8.2 SnapshotとDiff

- 完全Snapshot後にRustとC++のRevisionが一致する
- 不正Diffは全件拒否され、直前Revisionが残る
- Sidecar再起動後に完全Snapshotだけで復元できる
- Missing Assetがあっても他Clipを準備できる
- Runtime同期失敗後も保存済み編集を再読込できる

### 8.3 Transport

- 任意Tickから再生を開始できる
- 再生中Seek後もTransportが継続する
- Loop境界でAudioとMIDIが同一Sampleに戻る
- Playhead表示がEngineの`timelineSample`へ継続的に補正される
- Snapshot交換でDevice再起動やPlayhead Resetが起きない
- Device障害時にTransportが停止し、編集状態と位置を保持する

### 8.4 Realtime安全性

- Audio Callback内のAllocationとBlocking Lockがゼロである
- Source不足時にCallback Deadlineを超えずDropoutを報告する
- 長時間再生で`audioClockSample`が単調増加する
- 連続Seek、Loop変更、Clip移動で非有限Sampleを生成しない

## 9. 置換方針

- ミリ秒を正本とする既存Arrangement型は新しいTick/Frame型へ置き換える。
- コメントアウトされたArrange、MIDI、RenderのTauri Commandは再有効化せず削除する。
- 既存のSplit、Duplicate、Asset参照検証は新しい時間型で再実装する。
- TimelineをWAVへRenderしてPreviewするTransportは廃止する。
- Rustの既存Offline Rendererは共通Timeline DSPの検証材料として扱い、PlaybackとOffline Renderが同じ処理Coreを使用できた時点で置き換える。
- Session形式は新しい形式番号を使用し、未知または旧形式を黙って読み替えない。
