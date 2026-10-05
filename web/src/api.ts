import type { BridgeClient, BridgeResponse } from "./types";

export async function connect(): Promise<EventSource> {
  const token = new URLSearchParams(location.hash.slice(1)).get("token");
  if (token) {
    const response = await fetch("/session", {method: "POST", headers: {Authorization: `Bearer ${token}`, "Content-Type": "application/json"}, body: "{}"});
    if (!response.ok) throw new Error("Could not connect. Launch the browser workspace again.");
    history.replaceState(null, "", location.pathname);
  }
  return new EventSource("/events");
}

export const browserClient: BridgeClient = {
  async send<T>(command: string, payload: Record<string, unknown>): Promise<T> {
    const result = await fetch("/api", {method: "POST", headers: {"Content-Type": "application/json"}, body: JSON.stringify({command, payload})});
    if (!result.ok) throw new Error(result.status === 403 ? "Connection expired. Launch the browser workspace again." : "The local server is unavailable. Try again.");
    const response: BridgeResponse<T> = await result.json();
    if (!response.ok) throw new Error(response.error.message);
    return response.data;
  }
};
