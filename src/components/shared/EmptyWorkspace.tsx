import type { Workspace } from '@/lib/domain';
import { Icon } from './ui';

export function EmptyWorkspace({ workspace }: { workspace: Workspace }) {
  const copy: Record<Workspace, { title: string; body: string; action: string }> = {
    home: { title: '', body: '', action: '' },
    play: { title: '', body: '', action: '' },
    arrange: {
      title: 'Timelineへ素材を置く',
      body: 'Recording、Audio、MIDIを非破壊で配置し、同期位置を保ったまま編集します。',
      action: 'Inboxを開く',
    },
    sample: {
      title: '音から楽器を作る',
      body: 'Audioを切り出し、PadまたはKeyboardへ割り当て、再利用可能なInstrumentとして保存します。',
      action: 'Audioを選択',
    },
    analyze: {
      title: '測定して、理解する',
      body: 'Waveform、Loudness、Spectrum、PhaseをReferenceと音量を揃えて比較します。',
      action: '素材をAnalyze',
    },
    separate: {
      title: 'StemをBackgroundで分離',
      body: 'Originalと結果を同期試聴し、Artifactの可能性を確認してからTimelineやLibraryへ送ります。',
      action: 'Jobを作成',
    },
  };
  const item = copy[workspace];
  return (
    <div className="empty-workspace">
      <span className={`empty-orbit orbit-${workspace}`}>
        <i />
        <b />
      </span>
      <span className="eyebrow">{workspace.toUpperCase()} WORKSPACE</span>
      <h1>{item.title}</h1>
      <p>{item.body}</p>
      <button className="primary">
        <Icon name="plus" />
        {item.action}
      </button>
      <small>
        このワークスペースの処理エンジンは後続ゲートで接続されます。Scratch Sessionは維持されます。
      </small>
    </div>
  );
}
