// Hover cards: an item's details float beside it while the pointer rests on it (or it has keyboard focus),
// without opening a dialog. One card lives on <body>, so no scrolling area or card can clip it; it follows
// its item while that item scrolls and glides from item to item as the pointer moves along.

type Content = (item: HTMLElement) => string | null;
type Side = "right" | "left" | "below" | "above";

const GAP = 14;
const MARGIN = 12;
let card: HTMLElement | null = null;
let anchor: HTMLElement | null = null;
let hideTimer = 0;
let frame = 0;
let following = false;

function ensureCard(): HTMLElement {
  if (card?.isConnected) return card;
  card = document.createElement("div");
  card.className = "peek";
  card.setAttribute("aria-hidden", "true");
  document.body.append(card);
  if (!following) {
    following = true;
    // Capture scrolls of any element, such as the tree's own viewport.
    document.addEventListener("scroll", follow, {capture: true, passive: true});
    window.addEventListener("resize", follow, {passive: true});
  }
  return card;
}

/** Whether the item can be seen: inside the window and inside every scrolling area around it. */
function visible(item: HTMLElement): boolean {
  if (!item.isConnected) return false;
  const box = item.getBoundingClientRect();
  const middle = box.top + box.height / 2;
  if (middle < 0 || middle > window.innerHeight) return false;
  for (let parent = item.parentElement; parent && parent !== document.body; parent = parent.parentElement) {
    const style = getComputedStyle(parent);
    if (!/(auto|scroll|hidden|clip)/.test(style.overflowY + style.overflowX)) continue;
    const clip = parent.getBoundingClientRect();
    if (middle < clip.top + 8 || middle > clip.bottom - 8 || box.right < clip.left || box.left > clip.right) return false;
  }
  return true;
}

function place(): void {
  if (!card || !anchor) return;
  if (!visible(anchor)) { hidePeek(); return; }
  // Stay clear of the whole item: a tree event's dot and label, or a gallery card.
  const box = anchor.getBoundingClientRect();
  const width = card.offsetWidth;
  const height = card.offsetHeight;
  const right = window.innerWidth;
  const bottom = window.innerHeight;
  const side: Side = box.right + GAP + width <= right - MARGIN ? "right"
    : box.left - GAP - width >= MARGIN ? "left"
    : box.bottom + GAP + height <= bottom - MARGIN ? "below" : "above";
  const clamp = (value: number, low: number, high: number) => Math.max(low, Math.min(high, value));
  let x: number;
  let y: number;
  if (side === "right" || side === "left") {
    x = side === "right" ? box.right + GAP : box.left - GAP - width;
    y = clamp(box.top + box.height / 2 - height / 2, MARGIN, bottom - MARGIN - height);
    card.style.setProperty("--arrow", `${clamp(box.top + box.height / 2 - y, 22, height - 22)}px`);
  } else {
    x = clamp(box.left + box.width / 2 - width / 2, MARGIN, right - MARGIN - width);
    y = side === "below" ? box.bottom + GAP : box.top - GAP - height;
    card.style.setProperty("--arrow", `${clamp(box.left + box.width / 2 - x, 22, width - 22)}px`);
  }
  card.dataset.side = side;
  card.style.transform = `translate3d(${Math.round(x)}px, ${Math.round(y)}px, 0)`;
}

function follow(): void {
  if (!anchor || frame) return;
  frame = requestAnimationFrame(() => { frame = 0; place(); });
}

function show(item: HTMLElement, html: string): void {
  window.clearTimeout(hideTimer);
  const peek = ensureCard();
  const moving = peek.classList.contains("visible") && anchor !== item;
  if (anchor !== item || !peek.classList.contains("visible")) {
    peek.innerHTML = html;
    anchor = item;
  }
  // Glide between neighbors; a fresh card appears in place.
  peek.classList.toggle("glide", moving);
  place();
  if (!anchor) return;
  if (moving) {
    peek.classList.remove("swap");
    void peek.offsetWidth;
    peek.classList.add("swap");
  } else if (!peek.classList.contains("visible")) {
    peek.classList.add("visible");
  }
}

/** Hide the card now, or after `delay` ms so the pointer can cross the gap to a neighboring item. */
export function hidePeek(delay = 0): void {
  window.clearTimeout(hideTimer);
  const hide = () => {
    anchor = null;
    card?.classList.remove("visible", "glide", "swap");
  };
  if (delay > 0) hideTimer = window.setTimeout(hide, delay);
  else hide();
}

/** Show `content` for any [data-history] item in `scope` on hover or keyboard focus. Touch keeps tap-to-open. */
export function bindPeek(scope: HTMLElement | null, content: Content): void {
  hidePeek();
  if (!scope) return;
  const itemOf = (target: EventTarget | null) => target instanceof Element ? target.closest<HTMLElement>("[data-history]") : null;
  const open = (item: HTMLElement) => {
    const html = content(item);
    if (html) show(item, html);
  };
  scope.addEventListener("pointerover", event => {
    if (event.pointerType === "touch") return;
    const item = itemOf(event.target);
    if (item && scope.contains(item)) open(item);
  });
  scope.addEventListener("pointerout", event => {
    const item = itemOf(event.target);
    if (item && itemOf(event.relatedTarget) !== item) hidePeek(110);
  });
  scope.addEventListener("focusin", event => {
    const item = itemOf(event.target);
    let keyboard = true;
    try { keyboard = (event.target as Element).matches(":focus-visible"); } catch { /* Older engines: treat as keyboard focus. */ }
    if (item && keyboard) open(item);
  });
  scope.addEventListener("focusout", event => { if (!itemOf(event.relatedTarget)) hidePeek(60); });
  scope.addEventListener("click", () => hidePeek());
  scope.addEventListener("keydown", event => {
    if (event.key !== "Escape" || !anchor) return;
    event.preventDefault();
    hidePeek();
  });
}
