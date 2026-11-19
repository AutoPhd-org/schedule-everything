// Tests for Settings → Reminders: what the card shows, the patch each control sends, and the card on the Settings page.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { bindReminders, isReminders, remindersCard, type RemindersData } from "./reminders";
import { renderApp } from "./render";
import type { BridgeClient, Snapshot } from "./types";

const data = (): RemindersData => ({
  on: {block_start: true, block_end: false, time_points: true, urgent_tasks: true, deadlines: true, daily_summary: true, habit_prompt: false, weekly_report: true, monthly_report: true},
  times: {urgent_tasks: ["08:31", "14:01"], deadlines: [], daily_summary: "23:35", habit_prompt: null, weekly_report: {day: "sunday", time: "20:00"}, monthly_report: {day: 31, time: "20:00"}},
  skipDays: ["sunday"],
  urgentImportance: 8,
  deadlineDays: 3,
  blocks: [{name: "pomodoro", minutes: 25, on: true}, {name: "meeting", minutes: 50, on: false}],
  points: [{name: "lunch", message: "Lunch <now>", on: true}],
  limits: {maxDeadlineDays: 60}
});

describe("the Reminders card", () => {
  let root: HTMLElement;
  let save: ReturnType<typeof vi.fn>;
  const draw = (value = data(), paused = false) => {
    root.innerHTML = remindersCard(value, {paused});
    bindReminders(root, value, save);
  };
  const q = <T extends Element = HTMLElement>(selector: string) => root.querySelector<T>(selector)!;
  beforeEach(() => {
    root = document.createElement("div");
    document.body.append(root);
    save = vi.fn();
  });
  afterEach(() => root.remove());

  it("shows each reminder's switch and times, and what rings", () => {
    draw();
    expect(q<HTMLInputElement>('[data-reminder-on="block_start"]').checked).toBe(true);
    expect(q<HTMLInputElement>('[data-reminder-on="block_end"]').checked).toBe(false);
    expect(q('[data-reminder-row="habit_prompt"]').classList.contains("is-off")).toBe(true);
    expect([...root.querySelectorAll('[data-reminder-row="urgent_tasks"] .time-chip span')].map(chip => chip.textContent)).toEqual(["08:31", "14:01"]);
    expect(q('[data-reminder-row="deadlines"]').textContent).toContain("No times yet");
    expect(q<HTMLInputElement>('[data-reminder-time="daily_summary"]').value).toBe("23:35");
    expect(q<HTMLSelectElement>('[data-reminder-day="weekly_report"]').value).toBe("sunday");
    expect(q('[data-reminder-row="monthly_report"]').textContent).toContain("Months without that day are skipped.");
    expect(q('[data-reminder-block="meeting"]').getAttribute("aria-pressed")).toBe("false");
    // Messages are text from the user's files.
    expect(q('[data-reminder-point="lunch"]').innerHTML).toContain("Lunch &lt;now&gt;");
    expect(q('[data-quiet-day="sunday"]').getAttribute("aria-pressed")).toBe("true");
    expect(root.textContent).not.toContain("P mode pauses");
    draw(data(), true);
    expect(root.textContent).toContain("P mode pauses these until you switch back to J mode.");
  });

  it("sends one change at a time", () => {
    draw();
    const box = q<HTMLInputElement>('[data-reminder-on="block_end"]');
    box.checked = true; box.dispatchEvent(new Event("change"));
    expect(save).toHaveBeenLastCalledWith({on: {block_end: true}}, "“When a block ends” is on.");

    q<HTMLButtonElement>('[data-reminder-remove="urgent_tasks"][data-time="08:31"]').click();
    expect(save).toHaveBeenLastCalledWith({times: {urgent_tasks: ["14:01"]}}, expect.any(String));
    q<HTMLInputElement>('[data-reminder-new="deadlines"]').value = "09:15";
    q<HTMLButtonElement>('[data-reminder-add="deadlines"]').click();
    expect(save).toHaveBeenLastCalledWith({times: {deadlines: ["09:15"]}}, expect.any(String));

    const day = q<HTMLSelectElement>('[data-reminder-day="weekly_report"]');
    day.value = "friday"; day.dispatchEvent(new Event("change"));
    expect(save).toHaveBeenLastCalledWith({times: {weekly_report: {day: "friday", time: "20:00"}}}, expect.any(String));
    const time = q<HTMLInputElement>('[data-reminder-time="monthly_report"]');
    time.value = "07:45"; time.dispatchEvent(new Event("change"));
    expect(save).toHaveBeenLastCalledWith({times: {monthly_report: {day: 31, time: "07:45"}}}, expect.any(String));

    const importance = q<HTMLSelectElement>("[data-reminder-importance]");
    importance.value = "6"; importance.dispatchEvent(new Event("change"));
    expect(save).toHaveBeenLastCalledWith({urgentImportance: 6}, expect.any(String));

    q<HTMLButtonElement>('[data-reminder-block="pomodoro"]').click();
    expect(save).toHaveBeenLastCalledWith({blocks: {pomodoro: false}}, "“pomodoro” stays quiet now.");
    q<HTMLButtonElement>('[data-reminder-point="lunch"]').click();
    expect(save).toHaveBeenLastCalledWith({points: {lunch: false}}, "“Lunch <now>” stays quiet now.");
    q<HTMLButtonElement>('[data-quiet-day="saturday"]').click();
    expect(save).toHaveBeenLastCalledWith({skipDays: ["saturday", "sunday"]}, "Saturday is a quiet day now.");
    q<HTMLButtonElement>('[data-quiet-day="sunday"]').click();
    expect(save).toHaveBeenLastCalledWith({skipDays: []}, "Reminders ring on Sunday again.");
  });

  it("sends nothing for an empty or repeated time, or days out of range", () => {
    draw();
    q<HTMLButtonElement>('[data-reminder-add="urgent_tasks"]').click();
    q<HTMLInputElement>('[data-reminder-new="urgent_tasks"]').value = "14:01";
    q<HTMLButtonElement>('[data-reminder-add="urgent_tasks"]').click();
    const lead = q<HTMLInputElement>("[data-reminder-lead]");
    lead.value = "61"; lead.dispatchEvent(new Event("change"));
    expect(lead.value).toBe("3");
    expect(save).not.toHaveBeenCalled();
    lead.value = "7"; lead.dispatchEvent(new Event("change"));
    expect(save).toHaveBeenCalledWith({deadlineDays: 7}, expect.any(String));
  });

  it("knows an answer it can show", () => {
    expect(isReminders(data())).toBe(true);
    expect([null, {}, {on: {}, times: {}}].map(isReminders)).toEqual([false, false, false]);
  });
});

describe("Settings → Reminders", () => {
  const snapshot: Snapshot = {
    config: {rootDir: "/local/config", activeId: 0, activeConfigDir: "/local/config/user_config_0", tasksPath: "", deadlinesPath: "", habitsPath: "", recordsPath: ""},
    today: {date: "2026-10-05", weekday: "monday", parity: "even"},
    schedule: {isSkipped: false, current: null, next: null, timeToNext: null, events: [], hasSyncedOverlay: false},
    taskTypes: {"1": "Coding"}, tasks: [], deadlines: [], habits: [], history: []
  };
  let root: HTMLElement;
  const flush = () => new Promise(resolve => setTimeout(resolve, 0));
  beforeEach(() => { root = document.createElement("div"); document.body.append(root); });
  afterEach(() => root.remove());

  it("loads the card with the page, saves a change, and shows what the server saved", async () => {
    let saved = data();
    const send = vi.fn(async (command: string, payload: Record<string, unknown> = {}) => {
      if (command === "status_snapshot") return structuredClone(snapshot);
      if (command === "workspace_info") return {configIds: [0], activeId: 0, mode: "j", rootDir: "/local/config"};
      if (command === "reminders_read") return structuredClone(saved);
      if (command === "reminders_save") {
        if ((payload.on as Record<string, boolean> | undefined)?.deadlines === false) throw new Error("failed to save settings: read-only");
        saved = {...saved, on: {...saved.on, ...(payload.on as object)}};
        return structuredClone(saved);
      }
      return {};
    });
    const client: BridgeClient = {send: send as BridgeClient["send"]};
    await renderApp(root, client);
    root.querySelector<HTMLButtonElement>('[data-page="settings"]')!.click();
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("reminders_read", {});
    const box = () => root.querySelector<HTMLInputElement>('#reminders-card [data-reminder-on="block_end"]')!;
    expect(box().checked).toBe(false);

    box().checked = true; box().dispatchEvent(new Event("change"));
    await flush(); await flush();
    expect(send).toHaveBeenCalledWith("reminders_save", {on: {block_end: true}});
    expect(box().checked).toBe(true);
    expect(root.querySelector(".notice-text")?.textContent).toBe("“When a block ends” is on.");

    // A save that fails says why, and the switch goes back to what is saved.
    const deadlines = () => root.querySelector<HTMLInputElement>('#reminders-card [data-reminder-on="deadlines"]')!;
    deadlines().checked = false; deadlines().dispatchEvent(new Event("change"));
    await flush(); await flush();
    expect(root.querySelector(".notice-text")?.textContent).toBe("failed to save settings: read-only");
    expect(deadlines().checked).toBe(true);
  });
});
