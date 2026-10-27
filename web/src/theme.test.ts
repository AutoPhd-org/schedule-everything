import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { applyTheme, setThemePreference, themePreference, watchSystemTheme } from "./theme";

beforeEach(() => {
  window.localStorage.clear();
  delete document.documentElement.dataset.theme;
  delete document.documentElement.dataset.themePreference;
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it('responds to system changes only while following the system', () => {
  const media = {matches: false, addEventListener: vi.fn<(event: string, handler: () => void) => void>()};
  vi.stubGlobal('matchMedia', vi.fn(() => media));
  watchSystemTheme();
  expect(document.documentElement.dataset.theme).toBe('light');
  const onChange = media.addEventListener.mock.calls[0][1] as () => void;
  media.matches = true;onChange();
  expect(document.documentElement.dataset.theme).toBe('dark');
  setThemePreference('light');onChange();
  expect(document.documentElement.dataset.theme).toBe('light');
  setThemePreference('system');
  expect(document.documentElement.dataset.theme).toBe('dark');
});

it('keeps theme controls usable when browser storage is unavailable', () => {
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {throw new Error('Storage blocked');});
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {throw new Error('Storage blocked');});
  vi.stubGlobal('matchMedia', vi.fn(() => ({matches: false})));
  expect(themePreference()).toBe('system');
  setThemePreference('dark');applyTheme();
  expect(themePreference()).toBe('dark');
  expect(document.documentElement.dataset.theme).toBe('dark');
});
