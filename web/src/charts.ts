// Small SVG and HTML chart primitives: progress rings, organic blob art, and comparison chips.
// Charts use CSS custom properties for color so they follow the light and dark themes.
import { e, hash, icons, sw, type Swatch } from "./ui";

/** Seeded pseudo-random generator so generated art is stable between renders. */
export function seeded(seed: number): () => number {
  let state = seed || 1;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** A smooth closed blob path through jittered polar points (Catmull-Rom converted to cubic Béziers). */
export function blobPath(random: () => number, cx: number, cy: number, radius: number, points = 7, variance = 0.28): string {
  const vertices = Array.from({length: points}, (_, i) => {
    const angle = (i / points) * Math.PI * 2;
    const r = radius * (1 - variance / 2 + random() * variance);
    return [cx + Math.cos(angle) * r, cy + Math.sin(angle) * r];
  });
  const at = (i: number) => vertices[(i + points) % points];
  let path = `M${at(0)[0].toFixed(1)},${at(0)[1].toFixed(1)}`;
  for (let i = 0; i < points; i++) {
    const [p0, p1, p2, p3] = [at(i - 1), at(i), at(i + 1), at(i + 2)];
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    path += `C${c1[0].toFixed(1)},${c1[1].toFixed(1)} ${c2[0].toFixed(1)},${c2[1].toFixed(1)} ${p2[0].toFixed(1)},${p2[1].toFixed(1)}`;
  }
  return `${path}Z`;
}

/** Generative artwork for a task: blobs, orbits, and dots seeded by the task text. */
export function artwork(seedText: string, swatch: Swatch | null, muted = false): string {
  const seed = hash(seedText);
  const random = seeded(seed);
  const id = `art${seed.toString(36)}`;
  const accent = ["violet", "magenta", "aqua", "yellow", "orange", "blue"][seed % 6];
  const style = random() > 0.5 ? "orbit" : "bloom";
  const dots = Array.from({length: 5 + Math.floor(random() * 6)}, () => `<circle cx="${(random() * 200).toFixed(0)}" cy="${(random() * 160).toFixed(0)}" r="${(1.5 + random() * 3.5).toFixed(1)}" class="art-dot"/>`).join("");
  const shapes = style === "orbit"
    ? `<path d="${blobPath(random, 70 + random() * 60, 80, 62, 8, .34)}" fill="url(#${id}a)"/><circle cx="${(120 + random() * 50).toFixed(0)}" cy="${(40 + random() * 30).toFixed(0)}" r="${(18 + random() * 14).toFixed(0)}" class="art-accent sw-${accent}"/><ellipse cx="100" cy="82" rx="${(78 + random() * 14).toFixed(0)}" ry="${(24 + random() * 12).toFixed(0)}" transform="rotate(${(-25 + random() * 50).toFixed(0)} 100 82)" class="art-orbit"/>`
    : `<path d="${blobPath(random, 60 + random() * 30, 70 + random() * 30, 52, 6, .4)}" class="art-accent sw-${accent}" opacity=".85"/><path d="${blobPath(random, 120 + random() * 30, 80, 66, 7, .3)}" fill="url(#${id}a)" class="art-blend"/>`;
  return `<svg class="art ${sw(swatch)} ${muted ? "muted" : ""}" data-art-style="${style}" viewBox="0 0 200 160" preserveAspectRatio="xMidYMid slice" aria-hidden="true"><defs><linearGradient id="${id}a" x1="0" y1="0" x2="1" y2="1"><stop offset="0" class="art-stop-a"/><stop offset="1" class="art-stop-b"/></linearGradient></defs><rect width="200" height="160" class="art-bg"/><g class="art-spin">${shapes}</g>${dots}</svg>`;
}

/** A progress ring with an optional inner comparison ring (e.g. this week vs last week). */
export function ring(options: {value: number | null; compare?: number | null; size?: number; label: string; center?: string; tone?: string}): string {
  const {value, compare, size = 120, label, center, tone = "sw-green"} = options;
  const pct = (fraction: number | null | undefined) => Math.max(0, Math.min(100, Math.round((fraction ?? 0) * 100)));
  const outer = `<circle cx="60" cy="60" r="50" class="ring-track"/><circle cx="60" cy="60" r="50" pathLength="100" class="ring-value" style="--pct:${pct(value)}" stroke-dasharray="${pct(value)} 100"/>`;
  const inner = compare === undefined ? "" : `<circle cx="60" cy="60" r="36" class="ring-track inner"/><circle cx="60" cy="60" r="36" pathLength="100" class="ring-compare" style="--pct:${pct(compare)}" stroke-dasharray="${pct(compare)} 100"/>`;
  return `<figure class="ring ${tone}" style="--size:${size}px" role="img" aria-label="${e(label)}"><svg viewBox="0 0 120 120">${outer}${inner}</svg><figcaption>${center ?? (value === null ? "–" : `${pct(value)}%`)}</figcaption></figure>`;
}

/** Signed comparison chip with an icon, so direction is never conveyed by color alone. */
export function delta(current: number | null, reference: number | null, options: {label: string; unit?: string; digits?: number; invert?: boolean; format?: (value: number) => string}): string {
  if (current === null || reference === null) return `<span class="delta flat">${icons.zap}<span>No ${e(options.label)} yet</span></span>`;
  const diff = current - reference;
  const digits = options.digits ?? 0;
  const rounded = Math.round(diff * 10 ** digits) / 10 ** digits;
  if (rounded === 0) return `<span class="delta flat"><b aria-hidden="true">=</b><span>Same as ${e(options.label)}</span></span>`;
  const better = options.invert ? rounded < 0 : rounded > 0;
  const amount = options.format ? options.format(Math.abs(diff)) : `${Math.abs(rounded)}${options.unit ?? ""}`;
  return `<span class="delta ${better ? "up" : "down"}">${rounded > 0 ? icons.up : icons.down}<span>${rounded > 0 ? "+" : "−"}${e(amount)} vs ${e(options.label)}</span></span>`;
}

export const pctText = (value: number | null) => value === null ? "–" : `${Math.round(value * 100)}%`;
