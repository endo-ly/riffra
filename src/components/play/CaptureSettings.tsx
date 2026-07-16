import type { CreativeSession } from '@/lib/domain';

export function CaptureSettings({
  session,
  setSession,
}: {
  session: CreativeSession;
  setSession: (value: CreativeSession) => void;
}) {
  return (
    <section className="section-card capture-settings">
      <header>
        <div>
          <span className="eyebrow">CAPTURE</span>
          <h2>Quick Record timing</h2>
        </div>
        <small>Stored with this Scratch Session</small>
      </header>
      <label>
        <span>Visual count-in</span>
        <select
          value={session.settings.countInBeats}
          onChange={(event) =>
            setSession({
              ...session,
              settings: { ...session.settings, countInBeats: Number(event.target.value) },
            })
          }
        >
          {[0, 1, 2, 3, 4, 8].map((beats) => (
            <option value={beats} key={beats}>
              {beats === 0 ? 'Off' : `${beats} beats`}
            </option>
          ))}
        </select>
      </label>
      <p className="inspector-copy">
        When enabled, recording starts after the countdown. Existing audio is never captured during
        the count-in.
      </p>
    </section>
  );
}
