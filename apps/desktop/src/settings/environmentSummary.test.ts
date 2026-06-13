import { describe, expect, it } from "vitest";
import type { BuiltInProviderConfig, HostProfileConfig } from "../generated/contracts";
import { createI18n } from "../i18n";
import { environmentSummaryItems } from "./environmentSummary";

describe("environmentSummaryItems", () => {
  it("summarizes provider-specific reasoning semantics", () => {
    expect(
      reasoningSummary({ type: "chatgpt_responses", model: "gpt-5.4", reasoning: { enabled: false } }),
    ).toBe("Off");
    expect(
      reasoningSummary({ type: "chat_completions", model: "gpt-5.4", reasoning: { enabled: false } }),
    ).toBe("Off");
    expect(
      reasoningSummary({
        type: "anthropic_messages",
        model: "claude-test",
        reasoning: { thinking: "disabled" },
      }),
    ).toBe("Off");
    expect(
      reasoningSummary({
        type: "anthropic_messages",
        model: "claude-test",
        reasoning: { effort: "medium", thinking: "adaptive" },
      }),
    ).toBe("medium / adaptive");
  });
});

function reasoningSummary(provider: BuiltInProviderConfig): string {
  const summary = environmentSummaryItems(profileWith(provider), createI18n("en"));
  return summary.find((item) => item.label === "Reasoning")?.value ?? "";
}

function profileWith(provider: BuiltInProviderConfig): HostProfileConfig["profiles"][number] {
  return {
    profileId: "test",
    displayName: "Test",
    provider,
    compaction: { type: "auto" },
    eventStore: { type: "memory" },
    plugins: [],
    manifestPatches: [],
    metadata: {},
  };
}
