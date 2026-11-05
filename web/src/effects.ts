// Motion helpers: celebratory bursts and pointer-driven tilt, skipped when reduced motion is requested.

export function reducedMotion(): boolean {
  try { return !!window.matchMedia?.("(prefers-reduced-motion: reduce)")?.matches; } catch { return false; }
}

const BURST_COLORS = ["blue", "orange", "aqua", "yellow", "magenta", "green", "violet", "red"];

/** Confetti burst centered on an element; particles live outside the app root so redraws keep them. */
export function burst(origin: Element, count = 22): void {
  if (reducedMotion()) return;
  const rect = origin.getBoundingClientRect();
  const layer = document.createElement("div");
  layer.className = "burst";
  layer.setAttribute("aria-hidden", "true");
  layer.style.left = `${rect.left + rect.width / 2}px`;
  layer.style.top = `${rect.top + rect.height / 2}px`;
  for (let i = 0; i < count; i++) {
    const piece = document.createElement("i");
    const angle = (i / count) * Math.PI * 2 + Math.random() * 0.5;
    const distance = 40 + Math.random() * 70;
    piece.className = `sw-${BURST_COLORS[i % BURST_COLORS.length]} ${i % 3 === 0 ? "round" : ""}`;
    piece.style.setProperty("--dx", `${Math.cos(angle) * distance}px`);
    piece.style.setProperty("--dy", `${Math.sin(angle) * distance - 30}px`);
    piece.style.setProperty("--rot", `${Math.round(Math.random() * 720 - 360)}deg`);
    layer.append(piece);
  }
  document.body.append(layer);
  window.setTimeout(() => layer.remove(), 1100);
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
