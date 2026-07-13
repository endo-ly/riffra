import { describe, expect, it } from "vitest";
import {
  MAX_PERSISTED_PLUGIN_PARAMETERS,
  pluginParameterValuesForSession,
  shouldRestoreIndividualParameters,
} from "./plugin-session";

describe("plugin session persistence", () => {
  it("bounds auxiliary parameter values for plugins with large parameter lists", () => {
    const parameters = Array.from({ length: 900 }, (_, index) => ({ value: index / 900 }));
    const values = pluginParameterValuesForSession(parameters);
    expect(values).toHaveLength(MAX_PERSISTED_PLUGIN_PARAMETERS);
    expect(values[0]).toBe(0);
  });

  it("uses individual parameter replay only when no complete state blob exists", () => {
    expect(shouldRestoreIndividualParameters(null)).toBe(true);
    expect(shouldRestoreIndividualParameters("base64-state")).toBe(false);
  });
});
