# Riffra Mixer 画面仕様

## 1. 制作Mixを確認する

MixerはTrackのバランス、共有するGroup・Return、最終Masterを同じ面で操作する。Arrange ToolbarのMixerからLower Areaに開き、Track群、Group群、Return群の順に横スクロールする。Master列は右側に固定する。

チャンネルの選択はDevicesの編集対象を変える。MixerでTrack・Bus・Masterを選択してもFocused Instrument Trackは変わらず、演奏先を維持してミックスを調整できる。

## 2. チャンネルとルーティング

Trackには名前、種類、FX件数、主出力先とSend件数、Pan、左右Meter、Gain、Mute・Solo・Record Armを表示する。Volume・PanのAutomationがある操作にはAUTOを表示する。

主出力先とSend件数のボタンからRoutingを開く。Main outputはMasterまたはGroupを選び、SendはReturn、Level、Pre/Post、Enabledを編集する。追加できるReturnだけをAdd Sendに表示し、同じTrackから同じReturnへのSendを重複させない。

Group・Returnには名前と種類、FX件数とMissing状態、Pan、左右Meter、Gain、Mute・Soloを表示する。ヘッダーで種類と名前を指定して追加し、選択したBusの名前と削除を操作する。参照中のBusを削除すると、参照を解除する必要がある旨と対象IDをエラーで表示する。

Mute・Soloによる可聴経路は[音声アーキテクチャ](../architecture.md)を参照する。GroupのMuteと、Trackから直接分岐するSendは別の操作である。

## 3. FXと連続操作

各チャンネルのFXボタンは、同じ[Devices編集面](arrange-screen.md#54-devices)を開く。TrackではInstrumentとEffects、Bus・MasterではEffectsを扱う。欠落・無効化したプラグインはFX欄に警告し、DevicesでRe-scan・Replace・Disableを操作する。

Gain、Pan、Send Levelの連続操作は、編集中の値をNativeへ一時プレビューする。PointerUp、キー操作の終了、Blurで最後の値を一度だけ正準状態へ確定する。EscapeやPointerCancelは正準値へ戻す。プレビュー値を保存やUndo/Redoへ含めず、Project・Hostを切り替えるときは前の操作の応答を破棄する。

正準値と値域は[データモデル](../data-model.md)、命令と世代管理は[IPC契約](../ipc.md)に従う。

## 4. Meterと安全状態

Track MeterはEffect、Fader、Pan、Automationを通った主出力を表示する。Group・Return MeterはそのチャンネルのEffect、Gain、Panを通ったMaster向け出力を表示する。左右のPeakとRMSを表示し、1.0を超えるPeakも保持する。

Master MeterはMaster FX・GainとRealtime安全処理を通った最終出力を表示する。Safety欄にはLimiter前のPeak、Limiter Gain reduction、Hard clip、Feedback protectionを表示する。Master FXは安全処理を置き換えない。

Meterは約50 msごとの専用購読で更新し、未接続や世代切替中の値は「—」とする。Meter更新で正準Session全体を再描画しない。

## 5. Mixdown設定と結果

MasterのMixdownから、正準のMixdown設定を編集して書き出す。Master FXとMaster Gainは制作Mixへ適用し、Loudness Masteringは完成音声のラウドネスを整える処理として区別する。

Loudness Masteringの有効・無効、目標Integrated LUFS、True Peak上限、目標Loudness Range、Tail、Fade outを編集する。Save settingsは設定だけを保存し、Save and renderは設定を保存してから書き出しJobを開始する。Entire arrangementまたはLoop rangeと、全Trackまたは単一Trackを選択できる。

Aligned Track stemsは選択Trackごとに同じ時間基準のWAVを作り、個別のLoudness Masteringを適用しない。共有する非線形FXでは、Stemの合計と全体Mixが一致しない場合があることを画面に示す。

書き出し中は設定変更を止め、進捗とCancel renderを表示する。完了後は保存先、Sample Peak、True Peak、Integrated Loudness、Loudness Range、警告をJob結果から表示する。測定不能は理由を表示し、Masteringを適用した結果には目標・入力測定・偏差・True Peak補正も表示する。

画面の選択、ドラフト値、ダイアログ状態はPresentation Stateに属する。Project保存へ含めるのはMixerとMixdownの正準設定だけである。
