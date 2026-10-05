import "./styles.css";
import { browserClient, connect } from "./api";
import { renderApp } from "./render";

const root = document.querySelector<HTMLElement>("#app")!;
window.addEventListener("hashchange", () => {
  if (new URLSearchParams(location.hash.slice(1)).has("token")) location.reload();
});
async function main() {
  try {
    const events = await connect();
    events.addEventListener("activate", () => {
      window.focus();
      document.title = "Your workspace is ready · Schedule Everything";
      root.dispatchEvent(new Event("workspace-activate"));
    });
    events.onerror = () => { document.title = "Reconnecting · Schedule Everything"; };
    events.addEventListener("ready", () => { document.title = "Schedule Everything"; });
    const refreshTimer = window.setInterval(() => root.dispatchEvent(new Event("workspace-refresh")), 30000);
    root.addEventListener("workspace-closed", () => { events.close(); window.clearInterval(refreshTimer); });
    await renderApp(root, browserClient);
  } catch (error) {
    root.textContent = error instanceof Error ? error.message : String(error);
  }
}
void main();
