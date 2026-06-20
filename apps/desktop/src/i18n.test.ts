import { describe, expect, it } from "vitest";
import { createI18n, resolveUiLocale } from "./i18n";

describe("app i18n", () => {
  it("prefers the explicit launch locale over system detection", () => {
    expect(resolveUiLocale("zh", "en-US")).toBe("zh");
    expect(resolveUiLocale("en", "zh-CN")).toBe("en");
  });

  it("falls back to the detected system language", () => {
    expect(resolveUiLocale(null, "zh-Hans-CN")).toBe("zh");
    expect(resolveUiLocale(undefined, "fr-FR")).toBe("en");
  });

  it("separates disconnected launch states", () => {
    const i18n = createI18n("en");

    expect(i18n.disconnected(null).title).toBe("Profile configuration is missing");
    expect(i18n.disconnected({ status: "unavailable" }).title).toBe("Choose an environment");
    expect(i18n.disconnected({ status: "failed", error: "rpc failed" })).toEqual({
      title: "Interaction initialization failed",
      detail: "rpc failed",
    });
  });

});
