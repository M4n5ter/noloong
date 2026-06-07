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

  it("renders one language at a time", () => {
    const zh = createI18n("zh");
    const en = createI18n("en");

    expect(zh.t("settings.title")).toMatch(/[\u4e00-\u9fff]/);
    expect(zh.t("settings.title")).not.toBe(en.t("settings.title"));
    expect(en.t("settings.title")).not.toMatch(/[\u4e00-\u9fff]/);
  });

  it("formats runtime and run status copy through the catalog", () => {
    const zh = createI18n("zh");
    const en = createI18n("en");

    expect(zh.streamStatus("ready", null)).toBe("Display 流已连接");
    expect(en.streamStatus("ready", null)).toBe("Display stream ready");
    expect(zh.runStatus("failed", "boom")).toBe("失败 · boom");
    expect(en.runStatus("failed", "boom")).toBe("Failed · boom");
  });

  it("separates disconnected launch states", () => {
    const i18n = createI18n("en");

    expect(i18n.disconnected(null).title).toBe("Profile configuration is missing");
    expect(i18n.disconnected({ status: "unavailable" }).title).toBe(
      "Interaction runtime is unavailable",
    );
    expect(i18n.disconnected({ status: "failed", error: "rpc failed" })).toEqual({
      title: "Interaction initialization failed",
      detail: "rpc failed",
    });
  });
});
