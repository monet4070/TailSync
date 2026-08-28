import { useMemo, useState } from "react";
import { Check, Code2, Copy, Search, Type, WrapText } from "lucide-react";
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import php from "highlight.js/lib/languages/php";
import powershell from "highlight.js/lib/languages/powershell";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import { previewFileExtension } from "../../utils/historyPreview";
import { useModifierWheelZoom, usePreviewTextFontSize, zoomFromWheel } from "../previewPreferences";
import { MarkdownPreview } from "./MarkdownPreview";

hljs.registerLanguage("bash", bash);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("csharp", csharp);
hljs.registerLanguage("css", css);
hljs.registerLanguage("go", go);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("java", java);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("json", json);
hljs.registerLanguage("kotlin", kotlin);
hljs.registerLanguage("php", php);
hljs.registerLanguage("powershell", powershell);
hljs.registerLanguage("python", python);
hljs.registerLanguage("ruby", ruby);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("swift", swift);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("yaml", yaml);

const LANGUAGE_BY_EXTENSION: Record<string, string> = {
  c: "cpp",
  cc: "cpp",
  cpp: "cpp",
  cs: "csharp",
  css: "css",
  go: "go",
  h: "cpp",
  hpp: "cpp",
  html: "xml",
  java: "java",
  js: "javascript",
  jsx: "javascript",
  json: "json",
  kt: "kotlin",
  kts: "kotlin",
  mjs: "javascript",
  php: "php",
  ps1: "powershell",
  py: "python",
  rb: "ruby",
  rs: "rust",
  sh: "bash",
  sql: "sql",
  swift: "swift",
  toml: "ini",
  ts: "typescript",
  tsx: "typescript",
  xml: "xml",
  yaml: "yaml",
  yml: "yaml",
};

function likelyCode(source: string): boolean {
  const sample = source.slice(0, 8_000);
  if (sample.length === 0) return false;
  const signals = [
    /(^|\n)\s*(import|export|class|interface|function|fn|def|struct|enum)\b/m,
    /(^|\n)\s*[.#]?[A-Za-z_-][\w-]*\s*\{[^}]*:/m,
    /(^|\n)\s*(SELECT|INSERT|UPDATE|CREATE)\b/im,
    /(^|\n)\s*<\/?[A-Za-z][^>]*>/m,
    /[;{}]\s*(\n|$)/m,
  ];
  return signals.filter((pattern) => pattern.test(sample)).length >= 2;
}

function countMatches(source: string, query: string): number {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return 0;
  let count = 0;
  let offset = 0;
  const haystack = source.toLocaleLowerCase();
  while ((offset = haystack.indexOf(needle, offset)) >= 0) {
    count += 1;
    offset += Math.max(needle.length, 1);
  }
  return count;
}

export function TextPreview({
  data,
  name,
  forceCode = false,
  t,
}: {
  data: Uint8Array;
  name: string;
  forceCode?: boolean;
  t: (key: string) => string;
}) {
  const source = useMemo(() => new TextDecoder("utf-8").decode(data), [data]);
  const extensionLanguage = LANGUAGE_BY_EXTENSION[previewFileExtension(name)];
  const [mode, setMode] = useState<"text" | "code" | "markdown">(
    forceCode || Boolean(extensionLanguage) || likelyCode(source) ? "code" : "text",
  );
  const codeMode = mode === "code";
  const [wrap, setWrap] = useState(true);
  const [fontSize, setFontSize] = usePreviewTextFontSize();
  const [query, setQuery] = useState("");
  const [copied, setCopied] = useState(false);
  const sourceScrollRef = useModifierWheelZoom<HTMLDivElement>((deltaY) => {
    setFontSize((value) => zoomFromWheel(value, deltaY, 12, 32));
  });
  const matches = useMemo(() => countMatches(source, query), [query, source]);
  const highlighted = useMemo(() => {
    if (!codeMode) return "";
    try {
      const value = extensionLanguage
        ? hljs.highlight(source, { language: extensionLanguage, ignoreIllegals: true }).value
        : hljs.highlightAuto(source).value;
      return DOMPurify.sanitize(value, { ALLOWED_TAGS: ["span"], ALLOWED_ATTR: ["class"] });
    } catch {
      return DOMPurify.sanitize(hljs.highlightAuto(source).value, {
        ALLOWED_TAGS: ["span"],
        ALLOWED_ATTR: ["class"],
      });
    }
  }, [codeMode, extensionLanguage, source]);

  const copyAll = async () => {
    try {
      await navigator.clipboard.writeText(source);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  };

  return (
    <section className="preview-source-viewer" data-testid="preview-text">
      <div className="preview-content-toolbar">
        <label className="preview-search-field">
          <Search size={14} aria-hidden="true" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("history.preview.search")}
          />
          {query && <span>{matches}</span>}
        </label>
        <div className="preview-segmented" aria-label={t("history.preview.mode")}>
          <button
            type="button"
            className={mode === "text" ? "is-active" : ""}
            onClick={() => setMode("text")}
            title={t("history.preview.textMode")}
          >
            <Type size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            className={codeMode ? "is-active" : ""}
            onClick={() => setMode("code")}
            title={t("history.preview.codeMode")}
          >
            <Code2 size={14} aria-hidden="true" />
          </button>
          <button
            type="button"
            className={mode === "markdown" ? "is-active" : ""}
            onClick={() => setMode("markdown")}
            title={t("history.preview.markdownMode")}
          >
            MD
          </button>
        </div>
        <button
          type="button"
          className={wrap ? "preview-icon-button is-active" : "preview-icon-button"}
          onClick={() => setWrap((value) => !value)}
          title={t("history.preview.wrap")}
        >
          <WrapText size={15} aria-hidden="true" />
        </button>
        <div className="preview-stepper">
          <button type="button" onClick={() => setFontSize((value) => value - 1)}>−</button>
          <span>{fontSize}</span>
          <button type="button" onClick={() => setFontSize((value) => value + 1)}>+</button>
        </div>
        {codeMode && extensionLanguage && (
          <span className="preview-language-label">{extensionLanguage}</span>
        )}
        <button
          type="button"
          className="preview-icon-button"
          onClick={() => void copyAll()}
          title={t("history.copyAll")}
        >
          {copied ? <Check size={15} aria-hidden="true" /> : <Copy size={15} aria-hidden="true" />}
        </button>
      </div>
      <div
        ref={sourceScrollRef}
        className="preview-source-scroll"
      >
        {mode === "markdown" ? (
          <MarkdownPreview data={data} />
        ) : codeMode ? (
          <div className={wrap ? "preview-code-layout is-wrapped" : "preview-code-layout"} style={{ fontSize }}>
            <ol className="preview-code-lines" aria-hidden="true">
              {source.split("\n").map((_, index) => <li key={index} />)}
            </ol>
            <pre className="preview-code"><code className="hljs" dangerouslySetInnerHTML={{ __html: highlighted }} /></pre>
          </div>
        ) : (
          <pre className={wrap ? "preview-plain-text is-wrapped" : "preview-plain-text"} style={{ fontSize }}>
            {source}
          </pre>
        )}
      </div>
      <footer className="preview-text-stats">
        <span>{source.split(/\r?\n/).length} {t("history.preview.lines")}</span>
        <span>{source.length} {t("history.preview.characters")}</span>
      </footer>
    </section>
  );
}
