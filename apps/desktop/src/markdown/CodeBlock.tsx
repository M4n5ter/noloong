import { isValidElement, useEffect, useMemo, useState, type ReactNode } from "react";
import { createHighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import type { HighlighterCore, LanguageRegistration, ThemeRegistrationRaw } from "shiki/core";
import { HtmlPreview } from "./HtmlPreview";
import { MermaidBlock } from "./MermaidBlock";
import { PlainCodeBlock } from "./PlainCodeBlock";
import { isFullHtmlDocument } from "./streaming";

type CodePayload = {
  code: string;
  language: string;
};

const highlightedCache = new Map<string, Promise<string>>();
const loadedLanguages = new Set<string>();
let highlighterPromise: Promise<HighlighterCore> | null = null;

type LanguageTarget = {
  highlighterLang: string;
  loadKey: string;
  loader: () => Promise<{ default: LanguageRegistration[] }>;
};

const languageTargets = {
  bash: {
    highlighterLang: "bash",
    loadKey: "bash",
    loader: () => import("shiki/langs/bash.mjs"),
  },
  css: {
    highlighterLang: "css",
    loadKey: "css",
    loader: () => import("shiki/langs/css.mjs"),
  },
  html: {
    highlighterLang: "html",
    loadKey: "html",
    loader: () => import("shiki/langs/html.mjs"),
  },
  javascript: {
    highlighterLang: "javascript",
    loadKey: "javascript",
    loader: () => import("shiki/langs/javascript.mjs"),
  },
  js: {
    highlighterLang: "javascript",
    loadKey: "javascript",
    loader: () => import("shiki/langs/javascript.mjs"),
  },
  json: {
    highlighterLang: "json",
    loadKey: "json",
    loader: () => import("shiki/langs/json.mjs"),
  },
  jsonc: {
    highlighterLang: "jsonc",
    loadKey: "jsonc",
    loader: () => import("shiki/langs/jsonc.mjs"),
  },
  jsx: {
    highlighterLang: "jsx",
    loadKey: "jsx",
    loader: () => import("shiki/langs/jsx.mjs"),
  },
  markdown: {
    highlighterLang: "markdown",
    loadKey: "markdown",
    loader: () => import("shiki/langs/markdown.mjs"),
  },
  md: {
    highlighterLang: "markdown",
    loadKey: "markdown",
    loader: () => import("shiki/langs/markdown.mjs"),
  },
  python: {
    highlighterLang: "python",
    loadKey: "python",
    loader: () => import("shiki/langs/python.mjs"),
  },
  py: {
    highlighterLang: "python",
    loadKey: "python",
    loader: () => import("shiki/langs/python.mjs"),
  },
  rust: {
    highlighterLang: "rust",
    loadKey: "rust",
    loader: () => import("shiki/langs/rust.mjs"),
  },
  rs: {
    highlighterLang: "rust",
    loadKey: "rust",
    loader: () => import("shiki/langs/rust.mjs"),
  },
  sh: {
    highlighterLang: "shellscript",
    loadKey: "shellscript",
    loader: () => import("shiki/langs/shellscript.mjs"),
  },
  shell: {
    highlighterLang: "shellscript",
    loadKey: "shellscript",
    loader: () => import("shiki/langs/shellscript.mjs"),
  },
  toml: {
    highlighterLang: "toml",
    loadKey: "toml",
    loader: () => import("shiki/langs/toml.mjs"),
  },
  ts: {
    highlighterLang: "typescript",
    loadKey: "typescript",
    loader: () => import("shiki/langs/typescript.mjs"),
  },
  tsx: {
    highlighterLang: "tsx",
    loadKey: "tsx",
    loader: () => import("shiki/langs/tsx.mjs"),
  },
  typescript: {
    highlighterLang: "typescript",
    loadKey: "typescript",
    loader: () => import("shiki/langs/typescript.mjs"),
  },
  yaml: {
    highlighterLang: "yaml",
    loadKey: "yaml",
    loader: () => import("shiki/langs/yaml.mjs"),
  },
  yml: {
    highlighterLang: "yaml",
    loadKey: "yaml",
    loader: () => import("shiki/langs/yaml.mjs"),
  },
  zsh: {
    highlighterLang: "shellscript",
    loadKey: "shellscript",
    loader: () => import("shiki/langs/shellscript.mjs"),
  },
} satisfies Record<string, LanguageTarget>;

export function CodeBlock({
  children,
  streaming,
}: {
  children: ReactNode;
  streaming: boolean;
}) {
  const payload = useMemo(() => extractCodePayload(children), [children]);

  if (!payload) {
    return null;
  }

  if (payload.language === "mermaid") {
    return <MermaidBlock content={payload.code} streaming={streaming} />;
  }

  if (payload.language === "html" && isFullHtmlDocument(payload.code)) {
    return <HtmlPreview content={payload.code} streaming={streaming} />;
  }

  return <HighlightedCodeBlock payload={payload} />;
}

function HighlightedCodeBlock({ payload }: { payload: CodePayload }) {
  const [html, setHtml] = useState("");

  useEffect(() => {
    let cancelled = false;
    void highlightCode(payload)
      .then((nextHtml) => {
        if (!cancelled) {
          setHtml(nextHtml);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setHtml("");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [payload]);

  if (!html) {
    return <PlainCodeBlock code={payload.code} language={payload.language} />;
  }

  return (
    <div
      className="markdown-code-highlight"
      dangerouslySetInnerHTML={{ __html: html }}
      data-language={payload.language}
    />
  );
}

function extractCodePayload(children: ReactNode): CodePayload | null {
  const child = Array.isArray(children) ? children[0] : children;
  if (!isValidElement<{ className?: string; children?: ReactNode }>(child)) {
    return null;
  }

  const className = child.props.className ?? "";
  const language = className.replace(/^language-/, "") || "text";
  const code = reactNodeToText(child.props.children).replace(/\n$/, "");
  return { code, language };
}

function reactNodeToText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") {
    return String(node);
  }
  if (Array.isArray(node)) {
    return node.map(reactNodeToText).join("");
  }
  return "";
}

async function highlightCode({ code, language }: CodePayload): Promise<string> {
  const target = findLanguageTarget(language);
  if (!target) {
    return "";
  }

  const cacheKey = `${target.highlighterLang}:${code}`;
  const cached = highlightedCache.get(cacheKey);
  if (cached) {
    return cached;
  }

  const promise = getHighlighter()
    .then(async (highlighter) => {
      await ensureLanguage(highlighter, target);
      return highlighter.codeToHtml(code, {
        lang: target.highlighterLang,
        theme: "github-dark",
      });
    })
    .catch(() => "");

  highlightedCache.set(cacheKey, promise);
  if (highlightedCache.size > 100) {
    const firstKey = highlightedCache.keys().next().value;
    if (firstKey) {
      highlightedCache.delete(firstKey);
    }
  }
  return promise;
}

function findLanguageTarget(language: string): LanguageTarget | undefined {
  const normalized = language.trim().toLowerCase();
  return Object.hasOwn(languageTargets, normalized)
    ? languageTargets[normalized as keyof typeof languageTargets]
    : undefined;
}

async function getHighlighter(): Promise<HighlighterCore> {
  highlighterPromise ??= createHighlighterCore({
    themes: [importGithubDarkTheme()],
    langs: [],
    engine: createJavaScriptRegexEngine(),
  });
  return highlighterPromise;
}

async function importGithubDarkTheme(): Promise<ThemeRegistrationRaw> {
  return import("shiki/themes/github-dark.mjs").then((module) => module.default);
}

async function ensureLanguage(highlighter: HighlighterCore, target: LanguageTarget) {
  if (loadedLanguages.has(target.loadKey)) {
    return;
  }
  const module = await target.loader();
  await highlighter.loadLanguage(...module.default);
  loadedLanguages.add(target.loadKey);
}
