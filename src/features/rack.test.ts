import { describe, expect, it } from "vitest";
import { makePluginEntry, makePluginStatus, makeRackPlugin } from "../test-fixtures";
import { rackWithPluginBypassed, rackWithPluginLoaded, rackWithPluginParameter, rackWithoutPlugin } from "./rack";

function pluginRack(bypassed = false) {
  return [
    { id: "input:1", name: "In", kind: "input" as const, bypassed: false, gainDb: 0, parameterValues: [], stateData: null },
    makeRackPlugin({ parameterValues: [0.5], stateData: "blob", bypassed }),
    { id: "output:1", name: "Out", kind: "output" as const, bypassed: false, gainDb: 0, parameterValues: [], stateData: null },
  ];
}

describe("rackWithPluginLoaded", () => {
  it("replaces any existing plugin device but preserves other devices", () => {
    const runtime = makePluginStatus({
      bypassed: true,
      parameters: [{ index: 0, name: "Gain", value: 0.25, defaultValue: 0, automatable: true }],
      stateData: "runtime-blob",
    });
    const next = rackWithPluginLoaded(pluginRack(), makePluginEntry(), runtime, { parameterValues: [0.5], bypassed: true, stateData: null });
    const plugin = next.find((d) => d.kind === "plugin");
    expect(next.filter((d) => d.kind === "plugin")).toHaveLength(1);
    expect(next.filter((d) => d.kind !== "plugin")).toHaveLength(2);
    expect(plugin?.parameterValues).toEqual([0.25]);
    expect(plugin?.stateData).toBe("runtime-blob");
    expect(plugin?.bypassed).toBe(true);
  });

  it("falls back to caller values when the runtime reports nothing", () => {
    const next = rackWithPluginLoaded([], makePluginEntry(), null, { parameterValues: [0.1, 0.2], bypassed: false, stateData: "fallback" });
    const plugin = next.find((d) => d.kind === "plugin");
    expect(plugin?.parameterValues).toEqual([0.1, 0.2]);
    expect(plugin?.stateData).toBe("fallback");
  });
});

describe("rackWithoutPlugin", () => {
  it("removes only the plugin device", () => {
    const next = rackWithoutPlugin(pluginRack());
    expect(next.find((d) => d.kind === "plugin")).toBeUndefined();
    expect(next).toHaveLength(2);
  });
});

describe("rackWithPluginBypassed", () => {
  it("toggles bypass on the plugin device only", () => {
    const next = rackWithPluginBypassed(pluginRack(false), true);
    expect(next.find((d) => d.kind === "plugin")?.bypassed).toBe(true);
    expect(next.find((d) => d.kind === "input")?.bypassed).toBe(false);
  });
});

describe("rackWithPluginParameter", () => {
  it("updates values and state on the plugin device only", () => {
    const next = rackWithPluginParameter(pluginRack(), [0, 1, 0], "new-blob");
    const plugin = next.find((d) => d.kind === "plugin");
    expect(plugin?.parameterValues).toEqual([0, 1, 0]);
    expect(plugin?.stateData).toBe("new-blob");
    expect(next.find((d) => d.kind === "input")?.parameterValues).toEqual([]);
  });
});
