import { t } from "./i18n";
import type { BridgeClient, BridgeResponse } from "./types";

/** Whether this tab runs on the computer that serves it, rather than on a paired phone. */
function onThisComputer(): boolean {
  return location.hostname === "127.0.0.1";
}

function reconnectHint(): string {
  return onThisComputer() ? t("Launch the browser workspace again.") : t("Run {command} on your computer and scan the new code.", {command: "`rmd web --phone`"});
}

export async function connect(): Promise<EventSource> {
  const token = new URLSearchParams(location.hash.slice(1)).get("token");
  if (token) {
    const response = await fetch("/session", {method: "POST", headers: {Authorization: `Bearer ${token}`, "Content-Type": "application/json"}, body: "{}"});
    if (!response.ok) throw new Error(t("Could not connect. {hint}", {hint: reconnectHint()}));
    history.replaceState(null, "", location.pathname);
  }
  return new EventSource("/events");
}

export const browserClient: BridgeClient = {
  async send<T>(command: string, payload: Record<string, unknown>): Promise<T> {
    const result = await fetch("/api", {method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify({command, payload})});
    if (!result.ok) throw new Error(result.status === 403 ? t("Connection expired. {hint}", {hint: reconnectHint()}) : onThisComputer() ? t("The local server is unavailable. Try again.") : t("Your computer is unreachable. Check that it is awake and on the same network, then try again."));
    const response: BridgeResponse<T> = await result.json();
    if (!response.ok) throw new Error(response.error.message);
    return response.data;
  }
};
