import type { TransportStatus } from './contracts';

/** Returns whether a status belongs after the last accepted status in a Host generation. */
export function isNewerTransportStatus(
  status: TransportStatus,
  lastAcceptedSequence: number | null,
): boolean {
  return lastAcceptedSequence === null || status.sequence > lastAcceptedSequence;
}
