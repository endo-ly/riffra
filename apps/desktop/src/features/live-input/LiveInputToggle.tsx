import { midiNoteName } from '@/model/midi';
import { ToolbarToggle } from '@/shared/ui/Toolbar';
import type { LivePlay } from '@/features/live-input/hooks/useLivePlay';

const PAD_RANGE_LABEL = 'Pads';

/**
 * Toolbar toggle for the computer keyboard, showing what it plays and why it
 * stays off when the focused Track cannot sound.
 */
export function LiveInputToggle({ play }: { play: LivePlay }) {
  const on = play.computerKeyboard && play.blockedReason === null;
  const rangeLabel =
    play.settings.mode === 'pads' ? PAD_RANGE_LABEL : midiNoteName(play.settings.octave * 12 + 12);
  return (
    <ToolbarToggle
      active={on}
      icon="computerKeyboard"
      ariaLabel="Computer keyboard input"
      title={
        play.blockedReason === null
          ? 'Play the focused Instrument Track from the computer keyboard (Ctrl+Shift+K)'
          : `${play.blockedReason} Computer keyboard input stays off.`
      }
      onClick={play.toggleComputerKeyboard}
    >
      {on ? rangeLabel : undefined}
    </ToolbarToggle>
  );
}
