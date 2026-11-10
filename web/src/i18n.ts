// Interface language for the browser workspace: English (the source text), Simplified Chinese, Spanish, and Korean.
// The language is `[settings] language` in settings.toml, the same setting the CLI and reminder popups follow; the
// workspace reads it at start-up and the Settings page changes it. The English text is the message key: t() looks
// it up in the active catalog (locales/*.json) and falls back to the English. Keys are always string literals at
// the call site (t, tn, or msg) so i18n.test.ts can check that every catalog covers every key.
import es from "./locales/es.json";
import ko from "./locales/ko.json";
import zh from "./locales/zh.json";

export type Language = "en" | "zh" | "es" | "ko";
type Vars = Record<string, string | number>;

/** Each language with its `language = "…"` value in settings.toml, its name in that language, and the BCP 47 tag
 *  dates and numbers are formatted with. */
export const LANGUAGES: readonly {id: Language; name: string; locale: string}[] = [
  {id: "en", name: "English", locale: "en-US"},
  {id: "zh", name: "中文", locale: "zh-CN"},
  {id: "es", name: "Español", locale: "es-ES"},
  {id: "ko", name: "한국어", locale: "ko-KR"}
];
const CATALOGS: Record<Exclude<Language, "en">, Record<string, string>> = {zh, es, ko};

let current: Language = "en";
const REMEMBERED = "schedule-everything-language";

/** The language a setting or browser tag names, or null: `zh`, `zh-CN`, `cn`, `chinese`, `中文`; `es`, `es-MX`,
 *  `spanish`, `español`; `ko`, `ko-KR`, `kr`, `korean`, `한국어`; `en`, `en-GB`, `english`. */
export function normalizeLanguage(value: unknown): Language | null {
  if (typeof value !== "string") return null;
  const tag = value.trim().toLowerCase().replace(/_/g, "-");
  const base = tag.split("-")[0];
  if (["zh", "cn", "chinese", "中文", "简体中文"].includes(base)) return "zh";
  if (["es", "spanish", "español", "espanol"].includes(base)) return "es";
  if (["ko", "kr", "korean", "한국어"].includes(base)) return "ko";
  if (["en", "english"].includes(base)) return "en";
  return null;
}

/** Use `value` (any spelling normalizeLanguage knows; anything else means English) for every later t(), and tell
 *  the page so screen readers, fonts, and line breaking follow. The browser remembers it for restoreLanguage(). */
export function setLanguage(value: unknown): Language {
  current = normalizeLanguage(value) ?? "en";
  if (typeof document !== "undefined") document.documentElement.lang = locale();
  try { window.localStorage.setItem(REMEMBERED, current); } catch { /* Keep working without storage. */ }
  return current;
}
/** The language this browser last used, until the workspace says which one is set: so the loading screen and a
 *  failed first connection already speak it. */
export function restoreLanguage(): Language {
  let remembered: string | null = null;
  try { remembered = window.localStorage.getItem(REMEMBERED); } catch { /* No storage: English. */ }
  return setLanguage(remembered);
}
export const language = (): Language => current;
/** The BCP 47 tag of the active language, for Intl and toLocale* calls. English keeps the browser's own English
 *  (so en-GB still gets day-month dates); every other language uses its standard tag. */
export function locale(): string {
  const browser = typeof navigator === "undefined" ? "" : navigator.language;
  if (current === "en" && normalizeLanguage(browser) === "en") return browser;
  return LANGUAGES.find(entry => entry.id === current)!.locale;
}

/** Fill `{name}` placeholders; a name without a value stays as it is. */
function fill(text: string, vars?: Vars): string {
  return vars ? text.replace(/\{(\w+)\}/g, (whole, name: string) => name in vars ? String(vars[name]) : whole) : text;
}

/** `key` (the English text) in the active language, with its `{name}` placeholders filled from `vars`. Values
 *  go in as given, so escape user text first, as everywhere else. */
export function t(key: string, vars?: Vars): string {
  return fill(current === "en" ? key : CATALOGS[current][key] ?? key, vars);
}

/** A count with its noun: `one` when `count` is 1, else `other`, both English keys translated like t(). `{count}`
 *  is filled in along with `vars`. */
export function tn(one: string, other: string, count: number, vars?: Vars): string {
  return t(count === 1 ? one : other, {count, ...vars});
}

/** Marks English text to translate later, where it is used: for labels kept in module-level constants, which
 *  are built before the language is known. Pass the value through t() when drawing. */
export const msg = (key: string): string => key;
