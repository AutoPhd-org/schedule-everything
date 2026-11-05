/// <reference types="vitest/jsdom" />

// Use browser storage even on Node versions that also expose localStorage.
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: jsdom.window.localStorage
});
