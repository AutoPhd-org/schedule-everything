// Display preferences kept in this browser, set on the Settings page (the board's layout switch sets one too):
// how the task board is laid out, whether the theme follows the workload, whether today's progress weighs tasks
// by importance, and whether the daily overview is folded. Without storage the defaults apply.
export type BoardLayout = "compact" | "spread";
export type Prefs = {layout: BoardLayout; workloadTheme: boolean; weighted: boolean; dayFolded: boolean};

const KEY = "schedule-everything-display";
export const DEFAULT_PREFS: Prefs = {layout: "compact", workloadTheme: true, weighted: false, dayFolded: false};

export function loadPrefs(): Prefs {
  try {
    const saved = JSON.parse(window.localStorage.getItem(KEY) ?? "null");
    if (saved && typeof saved === "object") {
      return {
        layout: saved.layout === "spread" ? "spread" : "compact",
        workloadTheme: saved.workloadTheme !== false,
        weighted: saved.weighted === true,
        dayFolded: saved.dayFolded === true
      };
    }
  } catch { /* Fall back to the defaults when storage is unavailable. */ }
  return {...DEFAULT_PREFS};
}

export function savePrefs(prefs: Prefs): void {
  try { window.localStorage.setItem(KEY, JSON.stringify(prefs)); } catch { /* Keep working without storage. */ }
}
