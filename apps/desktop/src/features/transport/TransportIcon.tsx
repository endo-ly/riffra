import type { ReactNode } from 'react';

type TransportIconName = 'loop' | 'play' | 'stop' | 'rewind' | 'record' | 'metronome';

const TRANSPORT_ICON_SHAPES: Record<TransportIconName, ReactNode> = {
  loop: (
    <>
      <path d="M7.5 7.5H18V4.75L21.25 8 18 11.25V9.5H7.5A3.5 3.5 0 0 0 4 13" />
      <path d="M16.5 16.5H6v2.75L2.75 16 6 12.75v1.75h10.5A3.5 3.5 0 0 0 20 11" />
    </>
  ),
  play: <path d="m9 5.5 10 6.5-10 6.5Z" />,
  stop: <rect x="7.5" y="7.5" width="9" height="9" rx="0.5" />,
  rewind: (
    <>
      <path d="M6.5 5.5v13" />
      <path d="m18.5 6.5-7 5.5 7 5.5V6.5Z" />
    </>
  ),
  record: <circle cx="12" cy="12" r="6.5" />,
  metronome: (
    <>
      <path d="m9.5 4.5-3 15h11l-3-15Z" />
      <path d="M12 8v4" />
      <path d="m14.5 5.5 3 3" />
    </>
  ),
};

export function TransportIcon({ name }: { name: TransportIconName }) {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {TRANSPORT_ICON_SHAPES[name]}
    </svg>
  );
}
