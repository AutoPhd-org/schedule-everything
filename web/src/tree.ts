// Activity tree: task history along a vertical time axis, newest at the top.
// Each event is a node on a branch off the trunk. Every task type keeps its own lane and color, and the
// trunk takes the color of whichever type dominates the events around it. Layout and markup are pure;
// bindTree adds the scroll effects: a focus band at the viewport's center, a minimap, and keyboard steps.
import { leadHours, STATUS_ICON, STATUS_LABEL, statusOf } from "./activity";
import { reducedMotion } from "./effects";
import type { HistoryItem } from "./types";
import { addDays, dayDiff, e, formatDate, formatHours, icons, parseLocal, sw, typeSwatch, type Swatch } from "./ui";

/** Minimum distance between two events on the axis, in pixels. */
export const ROW = 64;
/** Half-height of the band at the viewport's center whose events count as fully in focus. */
const LENS = 40;
/** Events on each side of an event that decide the trunk's color around it. */
const NEIGHBORS = 3;
/** Header offsets above an event: its day label, a month label above that, and a quiet-gap note on top. */
const DAY_OFFSET = 44;
const MONTH_OFFSET = 80;
const QUIET_OFFSET = 30;

export type TreeLane = {id: string | null; name: string; tone: Swatch | null; lane: number; side: -1 | 1; depth: number};
export type AxisTone = {type: string | null; tone: Swatch | null; share: number};
export type TreeMark = {y: number; kind: "day" | "month" | "quiet"; label: string};
export type TreeNode = {item: HistoryItem; y: number; day: string; lane: TreeLane; axis: AxisTone; marks: TreeMark[]};
export type TreeLayout = {nodes: TreeNode[]; lanes: TreeLane[]; top: number; bottom: number; height: number};

const typeRank = (taskTypes: Record<string, string>) => {
  const order = Object.keys(taskTypes).sort((a, b) => a.localeCompare(b, undefined, {numeric: true}));
  return (id: string | null) => id === null ? order.length + 1 : order.includes(id) ? order.indexOf(id) : order.length;
};

/** One lane per task type in configured order, alternating left and right of the trunk and moving outward. */
export function treeLanes(items: HistoryItem[], taskTypes: Record<string, string>): TreeLane[] {
  const rank = typeRank(taskTypes);
  const ids = [...new Set(items.map(item => item.type || null))]
    .sort((a, b) => rank(a) - rank(b) || String(a).localeCompare(String(b), undefined, {numeric: true}));
  const single = ids.length === 1;
  return ids.map((id, lane) => ({
    id, lane, tone: typeSwatch(taskTypes, id),
    name: id === null ? "Other" : taskTypes[id] ?? items.find(item => item.type === id)?.typeName ?? id,
    side: single || lane % 2 ? 1 : -1,
    depth: single ? 1 : Math.floor(lane / 2) + 1
  }));
}

/** For each event, the type that dominates its neighborhood and how strongly (0–1).
 *  Nearer events weigh more, by position and by time, and ties go to the event's own type. */
export function axisTones(items: HistoryItem[], taskTypes: Record<string, string>, radius = NEIGHBORS): AxisTone[] {
  const times = items.map(item => parseLocal(item.endedAt).getTime());
  return items.map((item, i) => {
    const weights = new Map<string, number>();
    let total = 0;
    for (let j = Math.max(0, i - radius); j <= Math.min(items.length - 1, i + radius); j++) {
      const days = Math.abs(times[i] - times[j]) / 86400000;
      const weight = (radius + 1 - Math.abs(i - j)) / (1 + days / 7);
      const key = items[j].type || "";
      weights.set(key, (weights.get(key) ?? 0) + weight);
      total += weight;
    }
    let best = item.type || "";
    for (const [key, weight] of weights) if (weight > weights.get(best)!) best = key;
    return {type: best || null, tone: typeSwatch(taskTypes, best || null), share: weights.get(best)! / total};
  });
}

const dayLabel = (day: string, today: string) => day === today ? "Today" : day === addDays(today, -1) ? "Yesterday"
  : formatDate(day, {weekday: "short", month: "short", day: "numeric", ...(day.slice(0, 4) === today.slice(0, 4) ? {} : {year: "numeric"})});
const quietLabel = (days: number) => days >= 60 ? `About ${Math.round(days / 30)} quiet months` : `${days} quiet days`;

/** Place events (newest first) on the axis. Events are at least one row apart, longer pauses add space,
 *  and a new day leaves room for its label. y is measured down from the newest event. */
export function layoutTree(items: HistoryItem[], taskTypes: Record<string, string>, today: string): TreeLayout {
  const lanes = treeLanes(items, taskTypes);
  const laneOf = new Map(lanes.map(lane => [lane.id, lane]));
  const tones = axisTones(items, taskTypes);
  const nodes: TreeNode[] = [];
  let y = 0;
  items.forEach((item, i) => {
    const day = item.endedAt.slice(0, 10);
    const previous = items[i - 1];
    const previousDay = previous?.endedAt.slice(0, 10);
    const newDay = day !== previousDay;
    const newMonth = !previous || day.slice(0, 7) !== previousDay.slice(0, 7);
    const quiet = previous ? dayDiff(day, previousDay) - 1 : 0;
    const header = newDay ? (newMonth ? MONTH_OFFSET : DAY_OFFSET) + (quiet >= 2 ? QUIET_OFFSET : 0) : 0;
    if (previous) {
      const hours = Math.max(0, (parseLocal(previous.endedAt).getTime() - parseLocal(item.endedAt).getTime()) / 3600000);
      y += Math.max(ROW + Math.min(110, Math.round(14 * Math.log1p(hours / 6))), newDay ? ROW / 2 + header + 16 : 0);
    }
    const marks: TreeMark[] = [];
    if (quiet >= 2) marks.push({y: y - header, kind: "quiet", label: quietLabel(quiet)});
    if (newMonth) marks.push({y: y - MONTH_OFFSET, kind: "month", label: formatDate(day, {month: "long", year: "numeric"})});
    if (newDay) marks.push({y: y - DAY_OFFSET, kind: "day", label: dayLabel(day, today)});
    nodes.push({item, y, day, lane: laneOf.get(item.type || null)!, axis: tones[i], marks});
  });
  const firstHeader = nodes[0]?.marks[0]?.y ?? 0;
  return {nodes, lanes, top: firstHeader - 58, bottom: y + 70, height: y};
}

const toneColor = (tone: AxisTone) => {
  const strength = Math.round(tone.share * 10) / 10;
  return `color-mix(in oklab, ${tone.tone ? `var(--c-${tone.tone})` : "var(--muted)"} ${Math.round(30 + 70 * strength)}%, var(--surface-sunk))`;
};

/** A gradient along the axis with each event's neighborhood color at its height; runs of one color share two stops. */
export function axisGradient(nodes: TreeNode[], from: number, to: number): string {
  const colors = nodes.map(node => toneColor(node.axis));
  const span = Math.max(1, to - from);
  const stops = colors.flatMap((color, i) => i > 0 && i < colors.length - 1 && colors[i - 1] === color && colors[i + 1] === color ? [] : [`${color} ${((nodes[i].y - from) / span * 100).toFixed(2)}%`]);
  if (!stops.length) return "var(--line-strong)";
  return stops.length === 1 ? colors[0] : `linear-gradient(to bottom, ${stops.join(", ")})`;
}

function nodeHtml(node: TreeNode, index: number, order: number): string {
  const {item, lane} = node;
  const status = statusOf(item);
  const when = formatDate(item.endedAt, {weekday: "short", month: "short", day: "numeric", hour: "numeric", minute: "2-digit"});
  return `<div class="tree-node side-${lane.side < 0 ? "l" : "r"} status-${status} ${sw(lane.tone)} ${item.priority >= 8 ? "major" : ""}" data-y="${node.y}" data-day="${node.day}" data-tone="${lane.tone ?? ""}" data-type-name="${e(lane.name)}" style="--y:${node.y}px; --side:${lane.side}; --depth:${lane.depth}; --lane:${lane.lane}; --s:${10 + Math.max(1, Math.min(10, item.priority))}px; --i:${Math.min(order, 14)}">
    <svg class="tn-branch" viewBox="0 0 100 24" preserveAspectRatio="none" aria-hidden="true"><path d="M0 24 C 45 24 55 0 100 0" vector-effect="non-scaling-stroke"/></svg>
    <button type="button" class="tn-card" data-history="${index}" aria-haspopup="dialog" aria-label="${e(`${item.description} · ${lane.name} · ${STATUS_LABEL[status]} ${when}`)}"><span class="tn-dot" aria-hidden="true"></span><span class="tn-body"><strong>${e(item.description)}</strong><small><time datetime="${e(item.endedAt)}">${e(formatDate(item.endedAt, {hour: "numeric", minute: "2-digit"}))}</time><span>${icons.hourglass}${formatHours(leadHours(item))}</span>${status === "completed" ? "" : `<span class="tn-status">${STATUS_ICON[status]}${STATUS_LABEL[status]}</span>`}</small></span></button>
  </div>`;
}

const markHtml = (mark: TreeMark) => `<div class="tree-mark mark-${mark.kind}" style="--y:${mark.y}px"><span>${e(mark.label)}</span></div>`;

/** Rows for labels centered on `x` within `span` pixels: each label takes the first row where it clears its
 *  left neighbor by `gap`, and labels near the edges shift inward to stay `edge` pixels inside. */
export function packLabels(labels: {x: number; width: number}[], span: number, gap = 8, edge = 10): {placements: {row: number; shift: number}[]; rows: number} {
  const ends: number[] = [];
  const placements = labels.map(() => ({row: 0, shift: 0}));
  const order = labels.map((_, i) => i).sort((a, b) => labels[a].x - labels[b].x);
  for (const i of order) {
    const {x, width} = labels[i];
    const left = Math.max(edge, Math.min(span - edge - width, x - width / 2));
    let row = ends.findIndex(end => left >= end + gap);
    if (row < 0) { row = ends.length; ends.push(0); }
    ends[row] = left + width;
    placements[i] = {row, shift: Math.round(left - (x - width / 2))};
  }
  return {placements, rows: Math.max(1, ends.length)};
}

/** Stack the lane labels in as many rows as they need, each centered over its lane and kept inside the tree. */
function arrangeLanes(band: HTMLElement): void {
  const heads = [...band.querySelectorAll<HTMLElement>(".tree-lane-head")];
  const span = band.clientWidth;
  if (!span || !heads.length) return;
  const labels = heads.map(head => ({x: parseFloat(getComputedStyle(head).left), width: head.offsetWidth}));
  if (labels.some(label => !Number.isFinite(label.x) || !label.width)) return;
  // Narrow trees show each lane as a dot (see the container query in styles.css). Labels keep clear of the
  // stage's rounded top corners.
  const dots = labels.every(label => label.width <= 12);
  const {placements, rows} = packLabels(labels, span, dots ? 3 : 8, dots ? 8 : 22);
  placements.forEach(({row, shift}, i) => {
    heads[i].style.setProperty("--row", String(row));
    heads[i].style.setProperty("--shift", `${shift}px`);
  });
  band.style.setProperty("--rows", String(rows));
}

/** The tree for events sorted newest first. `total` counts every matching event, so a partial tree offers to grow. */
export function treeView(options: {items: HistoryItem[]; total: number; indexOf: (item: HistoryItem) => number; taskTypes: Record<string, string>; today: string; step: number}): string {
  const {items, total, taskTypes, today} = options;
  const layout = layoutTree(items, taskTypes, today);
  const {nodes, lanes, top, bottom, height} = layout;
  const last = nodes[nodes.length - 1];
  const mapHeight = Math.max(1, height);
  const lanePlace = (lane: TreeLane) => `--side:${lane.side}; --depth:${lane.depth}; --lane:${lane.lane}`;
  // Month names on the minimap, skipping any that would crowd the one above.
  let lastMonth = -Infinity;
  const months = nodes.flatMap(node => node.marks.filter(mark => mark.kind === "month").map(() => node)).filter(node => {
    const at = node.y / mapHeight;
    if (at - lastMonth < 0.07) return false;
    lastMonth = at;
    return true;
  });
  const remaining = total - items.length;
  const lead = nodes[0] ? `${e(dayLabel(nodes[0].day, today))}` : "";
  // Lanes share the room beside the trunk, however many types there are; see --step in styles.css.
  const depths = Math.max(1, ...lanes.map(lane => lane.depth));
  return `<section class="tree ${lanes.length === 1 ? "single" : ""}" data-tree aria-label="Task history tree, newest first" style="--depths:${depths}; --lane-gaps:${Math.max(1, lanes.length - 1)}">
    <header class="tree-head"><p class="tree-readout"><i class="tr-dot" aria-hidden="true"></i><strong data-tree-range>${lead}</strong><span data-tree-summary>${total} ${total === 1 ? "event" : "events"}</span></p><p class="tree-hint">Hover over an event for details · scroll to travel back in time · ↑ ↓ step through events · drag the map to jump</p></header>
    <div class="tree-body">
      <div class="tree-stage">
        <div class="tree-lanes" data-tree-lanes aria-hidden="true" style="--rows:${Math.min(2, lanes.length)}">${lanes.map(lane => `<span class="tree-lane-head ${sw(lane.tone)}" data-tip="${e(lane.name)}" style="${lanePlace(lane)}; --row:${(lane.depth - 1) % 2}"><b>${e(lane.name)}</b></span>`).join("")}</div>
        <div class="tree-window">
          <div class="tree-lens" aria-hidden="true"><i class="tree-glow"></i></div>
          <div class="tree-viewport" data-tree-viewport>
            <div class="tree-canvas" data-total="${height}" style="--h:${height}px">
              ${lanes.map(lane => `<i class="tree-lane-guide ${sw(lane.tone)}" aria-hidden="true" style="${lanePlace(lane)}; --from:${top + 30}px; --len:${bottom - top - 60}px"></i>`).join("")}
              <div class="tree-trunk" aria-hidden="true" style="--from:${top}px; --len:${bottom - top}px; background:${axisGradient(nodes, top, bottom)}"></div>
              <div class="tree-crown ${sw(nodes[0]?.axis.tone ?? null)}" aria-hidden="true" style="--y:${top}px"><span>${icons.sprout}</span></div>
              ${nodes.map((node, i) => node.marks.map(markHtml).join("") + nodeHtml(node, options.indexOf(node.item), i)).join("")}
              <div class="tree-root ${sw(last?.axis.tone ?? null)}" style="--y:${bottom}px"><svg class="tr-roots" viewBox="0 0 120 34" aria-hidden="true"><path d="M60 0 C 60 16 38 20 14 30 M60 0 C 60 14 60 24 60 32 M60 0 C 60 16 82 20 106 30"/></svg>${remaining > 0
                ? `<button type="button" class="button small" data-tree-more>${icons.plus}<span>Grow ${Math.min(options.step, remaining)} older ${Math.min(options.step, remaining) === 1 ? "event" : "events"}</span></button>`
                : last ? `<span class="tr-label">Earliest · ${e(formatDate(last.day, {month: "short", day: "numeric", year: "numeric"}))}</span>` : ""}</div>
            </div>
          </div>
        </div>
      </div>
      <div class="tree-map" data-tree-map aria-hidden="true"><div class="tm-inner">
        <span class="tm-trunk" style="background:${axisGradient(nodes, 0, mapHeight)}"></span>
        ${months.map(node => `<span class="tm-month" style="--t:${(node.y / mapHeight * 100).toFixed(2)}%">${e(formatDate(node.day, {month: "short"}))}</span>`).join("")}
        <svg class="tm-ticks" viewBox="0 0 100 ${mapHeight}" preserveAspectRatio="none">${nodes.map(node => {
          const width = 100 / lanes.length;
          return `<line class="${sw(node.lane.tone)}" x1="${(node.lane.lane * width + 8).toFixed(1)}" x2="${((node.lane.lane + 1) * width - 8).toFixed(1)}" y1="${node.y}" y2="${node.y}" vector-effect="non-scaling-stroke"/>`;
        }).join("")}</svg>
        <span class="tm-window" data-tree-window></span>
      </div></div>
    </div>
  </section>`;
}

// Scrolling the tree is animated here rather than with `behavior: "smooth"`: Chromium cancels one smooth scroll
// when another starts (the page scrolling to the tree, for example), and some embedded browsers never animate it.
const glides = new WeakMap<HTMLElement, number>();
const stopGlide = (viewport: HTMLElement) => { cancelAnimationFrame(glides.get(viewport) ?? 0); glides.delete(viewport); };
function scrollToY(viewport: HTMLElement, top: number): void {
  stopGlide(viewport);
  const from = viewport.scrollTop;
  const distance = top - from;
  if (reducedMotion() || typeof requestAnimationFrame !== "function" || Math.abs(distance) < 2) { viewport.scrollTop = top; return; }
  const duration = Math.min(720, 240 + Math.sqrt(Math.abs(distance)) * 7);
  let start: number | undefined;
  const step = (now: number) => {
    if (!viewport.isConnected) { glides.delete(viewport); return; }
    start ??= now;
    const t = Math.min(1, (now - start) / duration);
    viewport.scrollTop = from + distance * (1 - (1 - t) ** 3);
    if (t < 1) glides.set(viewport, requestAnimationFrame(step)); else glides.delete(viewport);
  };
  glides.set(viewport, requestAnimationFrame(step));
}
const shortDay = (day: string) => formatDate(day, {month: "short", day: "numeric", ...(day.slice(0, 4) === String(new Date().getFullYear()) ? {} : {year: "numeric"})});

export type TreeViewRange = {from: string; to: string} | null;

/** Scroll effects. The viewport's center is the focus point, and content y equals scrollTop: events near the
 *  center light up, the readout and minimap follow the view, and `onView` reports the days in view. */
export function bindTree(scope: ParentNode, onView?: (range: TreeViewRange) => void): void {
  const found = scope.querySelector<HTMLElement>("[data-tree]");
  if (!found) return;
  const tree: HTMLElement = found;
  const viewport = tree.querySelector<HTMLElement>("[data-tree-viewport]")!;
  const canvas = tree.querySelector<HTMLElement>(".tree-canvas")!;
  const map = tree.querySelector<HTMLElement>(".tm-inner");
  const frame = tree.querySelector<HTMLElement>("[data-tree-window]");
  const rangeText = tree.querySelector<HTMLElement>("[data-tree-range]")!;
  const summaryText = tree.querySelector<HTMLElement>("[data-tree-summary]")!;
  const nodes = [...tree.querySelectorAll<HTMLElement>(".tree-node")];
  const ys = nodes.map(node => Number(node.dataset.y));
  const height = Number(canvas.dataset.total) || 0;
  let pending = 0;
  let focused: HTMLElement | null = null;
  let reported = "";

  // Index of the first event at or below y.
  const firstAt = (y: number) => {
    let lo = 0, hi = ys.length;
    while (lo < hi) { const mid = (lo + hi) >> 1; if (ys[mid] < y) lo = mid + 1; else hi = mid; }
    return lo;
  };

  function update() {
    pending = 0;
    const view = viewport.clientHeight;
    if (!view || !nodes.length) return;
    const half = view / 2;
    const center = viewport.scrollTop;
    let nearest: HTMLElement | null = null;
    let nearestDistance = LENS + 20;
    let count = 0;
    let newest = "";
    let oldest = "";
    const weights = new Map<string, {weight: number; tone: string}>();
    for (let i = firstAt(center - half - ROW), end = firstAt(center + half + ROW); i < end; i++) {
      const node = nodes[i];
      const distance = Math.abs(ys[i] - center);
      const t = distance <= LENS ? 1 : Math.max(0, 1 - (distance - LENS) / Math.max(1, half - LENS));
      const focus = t * t * (3 - 2 * t);
      node.style.setProperty("--f", focus.toFixed(3));
      if (distance < nearestDistance) { nearest = node; nearestDistance = distance; }
      if (distance > half) continue;
      count++;
      newest ||= node.dataset.day!;
      oldest = node.dataset.day!;
      const name = node.dataset.typeName!;
      const entry = weights.get(name) ?? {weight: 0, tone: node.dataset.tone!};
      entry.weight += 0.25 + focus;
      weights.set(name, entry);
    }
    if (nearest !== focused) { focused?.classList.remove("is-focus"); nearest?.classList.add("is-focus"); focused = nearest; }

    // The type that dominates the view tints the focus band, the minimap window, and the readout.
    const ranked = [...weights].sort((a, b) => b[1].weight - a[1].weight);
    const totalWeight = ranked.reduce((sum, [, entry]) => sum + entry.weight, 0);
    const [leader, lead] = ranked[0] ?? ["", {weight: 0, tone: ""}];
    tree.style.setProperty("--view-c", lead.tone ? `var(--c-${lead.tone})` : "var(--accent)");
    if (count) {
      rangeText.textContent = newest === oldest ? formatDate(newest, {weekday: "short", month: "short", day: "numeric"}) : `${shortDay(oldest)} – ${shortDay(newest)}`;
      summaryText.textContent = `${count} ${count === 1 ? "event" : "events"} · ${ranked.length === 1 ? `all ${leader}` : lead.weight / totalWeight >= 0.5 ? `mostly ${leader}` : `a mix, led by ${leader}`}`;
    } else {
      const below = Math.min(nodes.length - 1, firstAt(center));
      const above = Math.max(0, below - 1);
      rangeText.textContent = `${shortDay(nodes[below].dataset.day!)} – ${shortDay(nodes[above].dataset.day!)}`;
      summaryText.textContent = "A quiet stretch";
    }
    if (frame) {
      const from = Math.min(height, Math.max(0, center - half));
      const to = Math.min(height, Math.max(0, center + half));
      frame.style.top = `${height ? from / height * 100 : 0}%`;
      frame.style.height = `${height ? (to - from) / height * 100 : 100}%`;
    }
    const range = count ? `${oldest}|${newest}` : "";
    if (range !== reported) { reported = range; onView?.(count ? {from: oldest, to: newest} : null); }
  }
  const schedule = () => { if (!pending) pending = requestAnimationFrame(update); };
  // Half a viewport of room above the newest and below the oldest event lets every event reach the center.
  const band = tree.querySelector<HTMLElement>("[data-tree-lanes]");
  const fit = () => {
    if (!viewport.isConnected) { observer?.disconnect(); return; }
    canvas.style.setProperty("--pad", `${Math.round(viewport.clientHeight / 2)}px`);
    if (band) arrangeLanes(band);
    update();
  };
  const observer = typeof ResizeObserver === "function" ? new ResizeObserver(fit) : null;
  observer?.observe(viewport);
  fit();
  viewport.addEventListener("scroll", schedule, {passive: true});
  // The person scrolling always wins over an animated jump.
  for (const type of ["wheel", "touchstart", "pointerdown"]) viewport.addEventListener(type, () => stopGlide(viewport), {passive: true});

  const center = (index: number) => scrollToY(viewport, ys[index]);
  const steps: Record<string, (index: number) => number> = {ArrowDown: i => i + 1, ArrowUp: i => i - 1, Home: () => 0, End: () => nodes.length - 1};
  viewport.addEventListener("keydown", event => {
    const node = (event.target as Element).closest<HTMLElement>(".tree-node");
    if (!node || !steps[event.key]) return;
    event.preventDefault();
    const next = Math.max(0, Math.min(nodes.length - 1, steps[event.key](nodes.indexOf(node))));
    nodes[next].querySelector<HTMLElement>(".tn-card")?.focus({preventScroll: true});
    center(next);
  });
  // Keyboard focus brings an event into the focus band; pointer clicks leave the view where it is.
  viewport.addEventListener("focusin", event => {
    const node = (event.target as Element).closest<HTMLElement>(".tree-node");
    let keyboard = true;
    try { keyboard = (event.target as Element).matches(":focus-visible"); } catch { /* Older engines: treat as keyboard focus. */ }
    if (node && keyboard) center(nodes.indexOf(node));
  });

  // The minimap works like a scrollbar: press or drag to jump, and the wheel scrolls the tree.
  const box = tree.querySelector<HTMLElement>("[data-tree-map]");
  if (!map || !box) return;
  const jump = (event: PointerEvent) => {
    stopGlide(viewport);
    const rect = map.getBoundingClientRect();
    if (rect.height) viewport.scrollTop = Math.max(0, Math.min(1, (event.clientY - rect.top) / rect.height)) * height;
  };
  box.addEventListener("pointerdown", event => { box.setPointerCapture?.(event.pointerId); box.classList.add("dragging"); jump(event); });
  box.addEventListener("pointermove", event => { if (box.hasPointerCapture?.(event.pointerId)) jump(event); });
  const release = () => box.classList.remove("dragging");
  box.addEventListener("pointerup", release);
  box.addEventListener("pointercancel", release);
  box.addEventListener("wheel", event => { event.preventDefault(); stopGlide(viewport); viewport.scrollTop += event.deltaY; }, {passive: false});
}

/** Bring the newest event on or before `day` to the focus band and flash that day's events. */
export function scrollTreeTo(scope: ParentNode, day: string): boolean {
  const tree = scope.querySelector<HTMLElement>("[data-tree]");
  const nodes = [...(tree?.querySelectorAll<HTMLElement>(".tree-node") ?? [])];
  if (!tree || !nodes.length) return false;
  const target = nodes.find(node => node.dataset.day! <= day) ?? nodes[nodes.length - 1];
  scrollToY(tree.querySelector<HTMLElement>("[data-tree-viewport]")!, Number(target.dataset.y));
  const box = tree.getBoundingClientRect();
  if (box.top < 0 || box.bottom > window.innerHeight) tree.scrollIntoView?.({block: "nearest", behavior: reducedMotion() ? "auto" : "smooth"});
  for (const node of nodes) if (node.dataset.day === target.dataset.day) {
    node.classList.remove("flash");
    void node.offsetWidth;
    node.classList.add("flash");
    window.setTimeout(() => node.classList.remove("flash"), 1700);
  }
  return true;
}
