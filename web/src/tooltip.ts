// Tooltips for [data-tip] elements. One tooltip lives on <body> above everything, so scrolling areas and
// cards with hidden overflow never cut it off, and it stays inside the window near the edges.

const MARGIN = 8;

export function bindTooltips(signal: AbortSignal): void {
  let tip: HTMLElement | null = null;
  let target: Element | null = null;
  const tipOf = (node: EventTarget | null) => node instanceof Element ? node.closest("[data-tip]") : null;

  function show(element: Element) {
    const text = element.getAttribute("data-tip");
    if (!text) return;
    if (!tip?.isConnected) {
      tip = document.createElement("div");
      tip.className = "float-tip";
      tip.setAttribute("aria-hidden", "true");
      document.body.append(tip);
    }
    target = element;
    tip.textContent = text;
    const box = element.getBoundingClientRect();
    const width = tip.offsetWidth;
    const height = tip.offsetHeight;
    const x = Math.max(MARGIN, Math.min(window.innerWidth - MARGIN - width, box.left + box.width / 2 - width / 2));
    const below = box.top - 8 - height < MARGIN;
    const y = below ? box.bottom + 8 : box.top - 8 - height;
    tip.style.left = `${Math.round(x)}px`;
    tip.style.top = `${Math.round(y)}px`;
    tip.classList.toggle("below", below);
    tip.classList.remove("visible");
    void tip.offsetWidth;
    tip.classList.add("visible");
  }
  function hide() {
    target = null;
    tip?.classList.remove("visible");
  }

  document.addEventListener("pointerover", event => {
    const element = tipOf(event.target);
    if (element && element !== target) show(element);
    else if (!element && target) hide();
  }, {signal});
  document.addEventListener("pointerout", event => {
    if (target && tipOf(event.target) === target && tipOf(event.relatedTarget) !== target) hide();
  }, {signal});
  document.addEventListener("focusin", event => {
    const element = tipOf(event.target);
    let keyboard = true;
    try { keyboard = (event.target as Element).matches(":focus-visible"); } catch { /* Older engines: treat as keyboard focus. */ }
    if (element && keyboard) show(element);
  }, {signal});
  document.addEventListener("focusout", () => { if (target) hide(); }, {signal});
  document.addEventListener("pointerdown", hide, {signal});
  document.addEventListener("scroll", () => { if (target) hide(); }, {capture: true, passive: true, signal});
  signal.addEventListener("abort", () => tip?.remove());
}
