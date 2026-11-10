// Motion helpers: celebratory bursts, the way a task leaves the board, and pointer-driven tilt. All of it is
// skipped when reduced motion is requested.
import { msg, t } from "./i18n";
import { icons } from "./ui";

export function reducedMotion(): boolean {
  try { return !!window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches; } catch { return false; }
}

const BURST_COLORS = ["blue", "orange", "aqua", "yellow", "magenta", "green", "violet", "red"];

type Point = {x: number; y: number};
const centerOf = (element: Element): Point => {
  const rect = element.getBoundingClientRect();
  return {x: rect.left + rect.width / 2, y: rect.top + rect.height / 2};
};
/** A fixed layer on <body>, outside the app root, so redraws of the app keep it until it removes itself. */
function layer(className: string, at: Point, lifetime: number): HTMLElement {
  const element = document.createElement("div");
  element.className = className;
  element.setAttribute("aria-hidden", "true");
  element.style.left = `${at.x}px`;
  element.style.top = `${at.y}px`;
  document.body.append(element);
  window.setTimeout(() => element.remove(), lifetime);
  return element;
}

/** Confetti burst centered on an element. */
export function burst(origin: Element, count = 22): void {
  if (reducedMotion()) return;
  const confetti = layer("burst", centerOf(origin), 1100);
  for (let i = 0; i < count; i++) {
    const piece = document.createElement("i");
    const angle = (i / count) * Math.PI * 2 + Math.random() * 0.5;
    const distance = 40 + Math.random() * 70;
    piece.className = `sw-${BURST_COLORS[i % BURST_COLORS.length]} ${i % 3 === 0 ? "round" : ""}`;
    piece.style.setProperty("--dx", `${Math.cos(angle) * distance}px`);
    piece.style.setProperty("--dy", `${Math.sin(angle) * distance - 30}px`);
    piece.style.setProperty("--rot", `${Math.round(Math.random() * 720 - 360)}deg`);
    confetti.append(piece);
  }
}

/** Two rings that spread out from a point, like a ripple. */
function shockwave(at: Point): void {
  const rings = layer("shockwave", at, 900);
  rings.innerHTML = "<i></i><i></i>";
}

/** A soft puff of dust that drifts up and away from a point, or from along a line `width` wide. */
function puff(at: Point, count = 12, width = 0): void {
  const dust = layer("burst dust", at, 1000);
  for (let i = 0; i < count; i++) {
    const mote = document.createElement("i");
    const angle = (i / count) * Math.PI * 2 + Math.random() * 0.6;
    const distance = 18 + Math.random() * 38;
    const x = (Math.random() - 0.5) * width;
    mote.style.left = `${x}px`;
    mote.style.setProperty("--dx", `${Math.cos(angle) * distance + x * 0.3}px`);
    mote.style.setProperty("--dy", `${Math.sin(angle) * distance * 0.7 - 16}px`);
    mote.style.setProperty("--s", `${6 + Math.round(Math.random() * 8)}px`);
    dust.append(mote);
  }
}

/** How a task leaves: completed (the server calls it "deleted"), cancelled as added by mistake, or dropped. */
export type Farewell = "deleted" | "cancelled" | "dropped";
export type Exit = {
  /** Settles once the item has left and the space it took has closed. */
  done: Promise<void>;
  /** Put the item back as it was, when the change it stood for did not go through. */
  revert: () => void;
};

const TAGS: Record<Farewell, {label: string; icon: string}> = {
  deleted: {label: msg("Done"), icon: icons.check},
  cancelled: {label: msg("Cancelled"), icon: icons.ban},
  dropped: {label: msg("Dropped"), icon: icons.dropped}
};

/** A small label that floats up from a point and fades, naming what just happened. */
function tag(at: Point, kind: Farewell): void {
  const label = layer(`float-tag tone-${kind}`, at, 1200);
  label.innerHTML = `${TAGS[kind].icon}<span>${t(TAGS[kind].label)}</span>`;
}

type Step = {keyframes: Keyframe[]; delay: number; duration: number; easing: string};
/** Each kind's exit, after the item's look has changed (styles.css, under .farewell). Where the first keyframe
 *  is left out, the exit starts wherever the item is, hovered or not. `close` is when its space closes. */
function exitSteps(kind: Farewell, axis: "x" | "y"): {steps: Step[]; close: {delay: number; duration: number}} {
  if (kind === "cancelled") return {
    steps: [
      // A quick “no” shake, then the card crumples into itself and blurs away.
      {keyframes: [{transform: "translateX(-7px)"}, {transform: "translateX(6px)"}, {transform: "translateX(-4px)"}, {transform: "translateX(2px)"}, {transform: "translateX(0px)"}], delay: 0, duration: 300, easing: "ease-out"},
      {keyframes: [{offset: 0.25, transform: "scale(0.96) rotate(-3deg)", filter: "blur(0px)", opacity: 1}, {transform: "scale(0.2) rotate(9deg)", filter: "blur(5px)", opacity: 0}], delay: 300, duration: 420, easing: "cubic-bezier(.5, 0, .7, .6)"}
    ],
    close: {delay: 560, duration: 300}
  };
  if (kind === "dropped") return {
    steps: [
      // The card comes loose at one corner, swings, and falls off the board.
      {keyframes: [
        {offset: 0, transform: "translateY(0px) rotate(0deg)", opacity: 1, easing: "cubic-bezier(.3, 1.6, .55, 1)"},
        {offset: 0.3, transform: "translateY(0px) rotate(10deg)", easing: "ease-in-out"},
        {offset: 0.42, transform: "translateY(0px) rotate(6.5deg)", easing: "ease-in-out"},
        {offset: 0.52, transform: "translateY(0px) rotate(8deg)", opacity: 1, easing: "cubic-bezier(.5, 0, .9, .5)"},
        {offset: 1, transform: "translateY(170px) rotate(26deg)", opacity: 0}
      ], delay: 0, duration: 920, easing: "linear"}
    ],
    close: {delay: 600, duration: 320}
  };
  // Completed: a beat to see it checked off, then it lifts away, up and to the right (up only in a row of timers).
  const away = axis === "x" ? "translate(0px, -26px) rotate(0deg) scale(0.86)" : "translate(64px, -14px) rotate(3deg) scale(0.9)";
  return {
    steps: [{keyframes: [{offset: 0.25, transform: "translate(-4px, -4px) rotate(-1deg) scale(1.03)", opacity: 1}, {transform: away, opacity: 0}], delay: 420, duration: 480, easing: "cubic-bezier(.45, 0, .3, 1)"}],
    close: {delay: 680, duration: 300}
  };
}

/** Close the space an item took: its height in a list (`y`), its width in a row (`x`), and the gap after it. */
function closeUp(item: HTMLElement, axis: "x" | "y"): Keyframe[] {
  const style = getComputedStyle(item);
  const parent = item.parentElement ? getComputedStyle(item.parentElement) : null;
  const gap = parseFloat((axis === "y" ? parent?.rowGap : parent?.columnGap) ?? "") || 0;
  if (axis === "y") return [
    {height: `${item.offsetHeight}px`, minHeight: "0px", paddingTop: style.paddingTop, paddingBottom: style.paddingBottom, borderTopWidth: style.borderTopWidth, borderBottomWidth: style.borderBottomWidth, marginBottom: style.marginBottom},
    {height: "0px", minHeight: "0px", paddingTop: "0px", paddingBottom: "0px", borderTopWidth: "0px", borderBottomWidth: "0px", marginBottom: `${-gap}px`}
  ];
  return [
    {flexBasis: `${item.offsetWidth}px`, flexGrow: 0, flexShrink: 0, minWidth: "0px", paddingLeft: style.paddingLeft, paddingRight: style.paddingRight, borderLeftWidth: style.borderLeftWidth, borderRightWidth: style.borderRightWidth, marginRight: style.marginRight},
    {flexBasis: "0px", flexGrow: 0, flexShrink: 0, minWidth: "0px", paddingLeft: "0px", paddingRight: "0px", borderLeftWidth: "0px", borderRightWidth: "0px", marginRight: `${-gap}px`}
  ];
}

const STILL: Exit = {done: Promise.resolve(), revert: () => {}};

/** See a task off the board, each kind its own way. A completed task is checked off with confetti and a ripple,
 *  struck through, and lifts away; a cancelled one shakes, crumples, and turns to dust; a dropped one comes
 *  loose and falls. A label floats up to say which happened, and the space the item took closes, so what was
 *  below it slides up instead of jumping once the board is redrawn.
 *
 *  `items` are the copies of the task on the page (its card on the board, its timer), the first one carrying the
 *  label; `origin` is the button that was pressed. Without animation support, or with reduced motion requested,
 *  nothing moves and `done` settles at once. */
export function farewell(items: HTMLElement[], kind: Farewell, origin?: Element): Exit {
  const shown = items.filter(item => item.isConnected);
  if (reducedMotion() || !shown.length || shown.some(item => typeof item.animate !== "function")) return STILL;
  const animations: Animation[] = [];
  const timers: number[] = [];
  const later = (delay: number, run: () => void) => { timers.push(window.setTimeout(run, delay)); };
  let longest = 0;
  for (const item of shown) {
    const axis = item.parentElement && getComputedStyle(item.parentElement).display.includes("flex") && getComputedStyle(item.parentElement).flexDirection.startsWith("row") ? "x" : "y";
    const {steps, close} = exitSteps(kind, axis);
    const collapse = closeUp(item, axis);
    item.classList.add("farewell", `farewell-${kind}`);
    for (const step of steps) animations.push(item.animate(step.keyframes, {delay: step.delay, duration: step.duration, easing: step.easing, fill: "forwards"}));
    animations.push(item.animate(collapse, {delay: close.delay, duration: close.duration, easing: "cubic-bezier(.4, 0, .2, 1)", fill: "forwards"}));
    longest = Math.max(longest, close.delay + close.duration, ...steps.map(step => step.delay + step.duration));
    // Dust where a cancelled card crumples, and where a dropped one comes loose.
    if (kind === "cancelled") {
      const wide = item.getBoundingClientRect().width * 0.4;
      later(540, () => { if (item.isConnected) puff(centerOf(item), 18, wide); });
    }
    if (kind === "dropped") later(160, () => {
      if (!item.isConnected) return;
      const rect = item.getBoundingClientRect();
      puff({x: rect.left + 16, y: rect.top + 10}, 8);
    });
  }
  const first = shown[0].getBoundingClientRect();
  tag({x: first.left + first.width / 2, y: first.top + 6}, kind);
  if (kind === "deleted") {
    const from = origin?.isConnected ? origin : shown[0];
    burst(from, 28);
    shockwave(centerOf(from));
  }
  // Animations in a page that is not being painted may never finish; the board moves on regardless.
  const settled = Promise.all(animations.map(animation => animation.finished.then(() => {}, () => {})));
  const done = Promise.race([settled, new Promise<void>(resolve => later(longest + 250, resolve))]).then(() => {});
  return {
    done,
    revert: () => {
      timers.forEach(timer => window.clearTimeout(timer));
      animations.forEach(animation => animation.cancel());
      shown.forEach(item => item.classList.remove("farewell", `farewell-${kind}`));
    }
  };
}

/** Tilt cards toward the pointer for a tactile gallery feel. */
export function bindTilt(scope: ParentNode, selector: string): void {
  if (reducedMotion()) return;
  scope.querySelectorAll<HTMLElement>(selector).forEach(card => {
    card.addEventListener("pointermove", event => {
      if (event.pointerType === "touch") return;
      const rect = card.getBoundingClientRect();
      if (!rect.width || !rect.height) return;
      const x = (event.clientX - rect.left) / rect.width - 0.5;
      const y = (event.clientY - rect.top) / rect.height - 0.5;
      card.style.setProperty("--rx", `${(-y * 8).toFixed(2)}deg`);
      card.style.setProperty("--ry", `${(x * 10).toFixed(2)}deg`);
      card.style.setProperty("--mx", `${((x + 0.5) * 100).toFixed(1)}%`);
      card.style.setProperty("--my", `${((y + 0.5) * 100).toFixed(1)}%`);
    });
    card.addEventListener("pointerleave", () => {
      card.style.removeProperty("--rx");
      card.style.removeProperty("--ry");
    });
  });
}
