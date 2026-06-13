import type { BuiltInProviderConfig, HostProfileConfig } from "../generated/contracts";
import type { AppI18n } from "../i18n";
import { providerModel } from "./store";

export type EnvironmentSummaryItem = {
  label: string;
  value: string;
};

export function environmentSummaryItems(
  profile: HostProfileConfig["profiles"][number],
  i18n: AppI18n,
): EnvironmentSummaryItem[] {
  return [
    { label: i18n.t("settings.summaryProvider"), value: providerLabel(profile.provider.type) },
    { label: i18n.t("settings.summaryModel"), value: providerModel(profile.provider) },
    { label: i18n.t("settings.summaryReasoning"), value: reasoningSummaryLabel(profile, i18n) },
    { label: i18n.t("settings.summaryContext"), value: pluginSummaryLabel(profile.plugins?.length ?? 0, i18n) },
  ];
}

function providerLabel(providerType: HostProfileConfig["profiles"][number]["provider"]["type"]): string {
  switch (providerType) {
    case "chatgpt_responses":
      return "ChatGPT";
    case "responses":
      return "Responses";
    case "chat_completions":
      return "Chat Completions";
    case "anthropic_messages":
      return "Anthropic";
  }
}

function reasoningSummaryLabel(profile: HostProfileConfig["profiles"][number], i18n: AppI18n): string {
  return providerReasoningSummaryLabel(profile.provider, i18n);
}

function providerReasoningSummaryLabel(provider: BuiltInProviderConfig, i18n: AppI18n): string {
  switch (provider.type) {
    case "responses":
    case "chatgpt_responses":
      return responsesReasoningSummaryLabel(provider.reasoning, i18n);
    case "chat_completions":
      return chatCompletionsReasoningSummaryLabel(provider.reasoning, i18n);
    case "anthropic_messages":
      return anthropicReasoningSummaryLabel(provider.reasoning, i18n);
  }
}

function responsesReasoningSummaryLabel(
  reasoning: Extract<BuiltInProviderConfig, { type: "responses" | "chatgpt_responses" }>["reasoning"],
  i18n: AppI18n,
): string {
  if (reasoning?.enabled === false) {
    return i18n.t("settings.summaryReasoningOff");
  }
  const effort = reasoning?.effort ? String(reasoning.effort) : i18n.t("settings.summaryReasoningDefault");
  return reasoning?.summary ? `${effort} / ${reasoning.summary}` : effort;
}

function chatCompletionsReasoningSummaryLabel(
  reasoning: Extract<BuiltInProviderConfig, { type: "chat_completions" }>["reasoning"],
  i18n: AppI18n,
): string {
  if (reasoning?.enabled === false) {
    return i18n.t("settings.summaryReasoningOff");
  }
  return reasoning?.effort ? String(reasoning.effort) : i18n.t("settings.summaryReasoningDefault");
}

function anthropicReasoningSummaryLabel(
  reasoning: Extract<BuiltInProviderConfig, { type: "anthropic_messages" }>["reasoning"],
  i18n: AppI18n,
): string {
  if (reasoning?.thinking === "disabled") {
    return i18n.t("settings.summaryReasoningOff");
  }
  const effort = reasoning?.effort ? String(reasoning.effort) : i18n.t("settings.summaryReasoningDefault");
  return reasoning?.thinking ? `${effort} / ${reasoning.thinking}` : effort;
}

function pluginSummaryLabel(count: number, i18n: AppI18n): string {
  if (count === 0) {
    return i18n.t("settings.summaryPluginsNone");
  }
  if (count === 1) {
    return i18n.t("settings.summaryPluginsOne");
  }
  return i18n.t("settings.summaryPluginsMany", { count });
}
