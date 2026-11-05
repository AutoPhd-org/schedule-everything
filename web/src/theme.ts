// Browser-local appearance preferences, with the system theme as the default.
export type ThemePreference = "system" | "light" | "dark";
const storageKey = "schedule-everything-theme";

export function themePreference(): ThemePreference {
  const current = document.documentElement.dataset.themePreference;
  if (current === "light" || current === "dark" || current === "system") return current;
  try {
    const value = window.localStorage.getItem(storageKey);
    if (value === "light" || value === "dark") return value;
  } catch { /* Appearance still works when browser storage is unavailable. */ }
  return "system";
}

export function applyTheme(preference = themePreference()): void {
  const dark = preference === "dark" || (preference === "system" && window.matchMedia?.("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  document.documentElement.dataset.themePreference = preference;
}

export function setThemePreference(preference: ThemePreference): void {
  try { window.localStorage.setItem(storageKey, preference); } catch { /* Keep this session usable. */ }
  applyTheme(preference);
}

export function watchSystemTheme(): void {
  applyTheme();
  window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener("change", () => {
    if (themePreference() === "system") applyTheme();
  });
}
