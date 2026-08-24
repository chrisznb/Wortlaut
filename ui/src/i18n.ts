// Tiny locale layer, no i18n library.
//
// Every JSON file in ./locales is a flat key to string map, loaded eagerly via
// Vite's import.meta.glob. Adding a language means adding a file, no code
// change. English is the fallback for any key a translation is missing.
// The "_label" key holds the display name for the picker and is never a
// translatable UI string.

export type Lang = string;

const files = import.meta.glob("./locales/*.json", { eager: true }) as Record<
  string,
  { default: Record<string, string> }
>;

const locales: Record<string, Record<string, string>> = {};
for (const [path, mod] of Object.entries(files)) {
  const code = path.replace(/^\.\/locales\//, "").replace(/\.json$/, "");
  locales[code] = mod.default;
}

const en: Record<string, string> = locales["en"] ?? {};

export function resolveLang(setting: string | undefined): Lang {
  if (setting && setting !== "system" && locales[setting]) return setting;
  const sys = typeof navigator !== "undefined" ? navigator.language || "" : "";
  if (locales[sys]) return sys;
  const base = sys.split("-")[0]?.toLowerCase();
  const match = Object.keys(locales).find((code) => code.toLowerCase() === base);
  if (match) return match;
  return "en";
}

export function availableLangs(): { code: string; label: string }[] {
  return Object.entries(locales).map(([code, dict]) => ({
    code,
    label: dict["_label"] || code,
  }));
}

let lang: Lang = "en";

export function setLang(l: Lang) {
  lang = l;
}

export function getLang(): Lang {
  return lang;
}

export function t(key: string, vars?: Record<string, string | number>): string {
  const dict = locales[lang] ?? en;
  let s = dict[key] ?? en[key] ?? key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      s = s.split(`{${k}}`).join(String(v));
    }
  }
  return s;
}
