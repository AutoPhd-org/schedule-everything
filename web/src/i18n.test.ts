import { afterEach, describe, expect, it } from "vitest";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import es from "./locales/es.json";
import ko from "./locales/ko.json";
import zh from "./locales/zh.json";
import { language, locale, msg, normalizeLanguage, restoreLanguage, setLanguage, t, tn } from "./i18n";

const SRC = dirname(fileURLToPath(import.meta.url));
const LITERAL = String.raw`"((?:[^"\\\n]|\\.)*)"`;
const CALLS = [
  new RegExp(String.raw`(?<![\w$.])t\(\s*${LITERAL}`, "g"),
  new RegExp(String.raw`(?<![\w$.])msg\(\s*${LITERAL}`, "g"),
  new RegExp(String.raw`(?<![\w$.])tn\(\s*${LITERAL}\s*,\s*${LITERAL}`, "g")
];

/** Every message key in the workspace's sources: the string literals passed to t(), tn(), and msg(). */
function sourceKeys(): Set<string> {
  const keys = new Set<string>();
  for (const file of readdirSync(SRC).filter(name => name.endsWith(".ts") && !name.endsWith(".test.ts") && name !== "i18n.ts")) {
    const source = readFileSync(join(SRC, file), "utf8");
    for (const pattern of CALLS) for (const match of source.matchAll(pattern)) for (const literal of match.slice(1)) keys.add(JSON.parse(`"${literal}"`));
  }
  return keys;
}
const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map(match => match[1]).sort();
const tags = (text: string) => [...text.matchAll(/<\/?([a-z][\w-]*)/gi)].map(match => match[0].toLowerCase()).sort();

afterEach(() => setLanguage("en"));

describe("message catalogs", () => {
  const keys = sourceKeys();

  it("finds the workspace's messages", () => {
    expect(keys.size).toBeGreaterThan(0);
  });

  for (const [name, catalog] of Object.entries({zh, es, ko}) as [string, Record<string, string>][]) {
    describe(name, () => {
      it("translates every message in the sources", () => {
        expect([...keys].filter(key => !(key in catalog))).toEqual([]);
      });
      it("keeps no messages the sources no longer use", () => {
        expect(Object.keys(catalog).filter(key => !keys.has(key))).toEqual([]);
      });
      it("keeps each message's placeholders and markup", () => {
        const broken = Object.entries(catalog).filter(([key, text]) =>
          !text.trim() || placeholders(key).join() !== placeholders(text).join() || tags(key).join() !== tags(text).join() || (text.includes('"') && !key.includes('"')));
        expect(broken).toEqual([]);
      });
    });
  }
});

describe("t and tn", () => {
  it("fall back to the English text and fill placeholders", () => {
    expect(t("Not a real message {name}", {name: "x"})).toBe("Not a real message x");
    expect(t("Keeps {unknown} as it is")).toBe("Keeps {unknown} as it is");
    expect(tn("{count} item", "{count} items", 1)).toBe("1 item");
    expect(tn("{count} item", "{count} items", 3)).toBe("3 items");
    expect(msg("Later")).toBe("Later");
  });

  it("use the chosen language's catalog", () => {
    const [key, text] = Object.entries(zh as Record<string, string>).find(([key]) => !placeholders(key).length) ?? ["", ""];
    setLanguage("zh");
    expect(language()).toBe("zh");
    expect(locale()).toBe("zh-CN");
    expect(document.documentElement.lang).toBe("zh-CN");
    if (key) expect(t(key)).toBe(text);
  });
});

describe("restoreLanguage", () => {
  it("brings back the language this browser used last, or English", () => {
    setLanguage("es");
    expect(window.localStorage.getItem("schedule-everything-language")).toBe("es");
    window.localStorage.setItem("schedule-everything-language", "ko");
    expect(restoreLanguage()).toBe("ko");
    window.localStorage.removeItem("schedule-everything-language");
    expect(restoreLanguage()).toBe("en");
  });
});

describe("normalizeLanguage", () => {
  it("reads the spellings a settings file or browser may use", () => {
    expect(["zh", "zh-CN", "zh_TW", "cn", "Chinese", "中文"].map(normalizeLanguage)).toEqual(Array(6).fill("zh"));
    expect(["es", "es-MX", "Spanish", "Español"].map(normalizeLanguage)).toEqual(Array(4).fill("es"));
    expect(["ko", "ko-KR", "kr", "Korean", "한국어"].map(normalizeLanguage)).toEqual(Array(5).fill("ko"));
    expect(["en", "en-GB", "English"].map(normalizeLanguage)).toEqual(Array(3).fill("en"));
    expect([undefined, 7, "", "fr"].map(normalizeLanguage)).toEqual([null, null, null, null]);
    expect(setLanguage("fr")).toBe("en");
  });
});
