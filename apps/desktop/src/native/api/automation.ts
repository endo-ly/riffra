import type {
  ArrangementMutationResult,
  AutomationInterpolation,
  AutomationPoint,
  AutomationTarget,
} from '@/model/domain';
import { dispatchControl } from '../invoke';

export interface AutomationApi {
  setAutomation(
    target: AutomationTarget,
    interpolation: AutomationInterpolation,
    points: AutomationPoint[],
  ): Promise<ArrangementMutationResult>;
  previewAutomationValue(
    target: AutomationTarget,
    value: number | null,
    previewToken: number,
  ): Promise<void>;
}

async function setAutomation(
  target: AutomationTarget,
  interpolation: AutomationInterpolation,
  points: AutomationPoint[],
): Promise<ArrangementMutationResult> {
  return dispatchControl({ command: 'automation.set', params: { target, interpolation, points } });
}

async function previewAutomationValue(
  target: AutomationTarget,
  value: number | null,
  previewToken: number,
): Promise<void> {
  await dispatchControl({
    command: 'automation.preview',
    params: { target, value, previewToken },
  });
}

export const automationApi: AutomationApi = {
  setAutomation,
  previewAutomationValue,
};
