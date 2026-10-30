export interface MixRange {
  min: number;
  max: number;
  step: number;
  /** Value restored by a double-click on the control. */
  defaultValue: number;
}

export const MIX_GAIN_DB: MixRange = { min: -90, max: 24, step: 0.1, defaultValue: 0 };
export const MASTER_GAIN_DB: MixRange = { min: -90, max: 0, step: 0.1, defaultValue: 0 };
export const MIX_PAN: MixRange = { min: -1, max: 1, step: 0.01, defaultValue: 0 };
