"""
Prompt templates for the interactive setup LLM agents.

This module centralizes all system and user message templates for build and
modify schedule flows. The templates enforce a strict, sectioned JSON output
contract for a multi-turn conversation where only the `conversation` section
is shown to the end user.
"""

from __future__ import annotations

from pathlib import Path

OUTPUT_SCHEMA_GUIDE = """
Return exactly one JSON object with tagged top-level sections/keys:
- phase (required string: "discovery" | "summary" | "final")
- conversation (required string, user-facing only)
- actions (optional array of strings, internal-only)
- needs_user_input (required boolean)
- profile_markdown (optional non-empty string containing the latest full `profile.md` draft)
- question_to_user (required non-empty string when needs_user_input is true)
- missing_information (optional array of strings)
- schedule_summary (required non-empty string when phase is "summary")
- settings_toml (required when needs_user_input is false)
- odd_weeks_toml (required when needs_user_input is false)
- even_weeks_toml (required when needs_user_input is false)
- habits_toml (optional when needs_user_input is false)

Output constraints:
1) Return raw JSON only. Do not wrap it in markdown.
2) `conversation` must summarize the turn naturally and clearly.
3) `actions` must describe internal operations; do not include user-facing text.
4) If phase is "discovery", set needs_user_input=true and do not return *_toml keys.
5) If phase is "summary", set needs_user_input=true, return schedule_summary as pure text, and ask for confirmation.
6) If phase is "final", set needs_user_input=false and return complete TOML for required files.
7) Every *_toml value must be a complete TOML document string parseable by tomllib.
8) Time keys in week schedules must use 24-hour HH:MM format.
9) Keep event labels short and reusable.
""".strip()

MULTI_TURN_RULES = """
This is a multi-turn assistant, not a single-turn generator.

Per turn behavior:
- Always process the latest user instruction.
- Actively ask for missing user profile details before finalizing: basic information, preferences, habits, goals, energy patterns, and hard constraints.
- If information is sufficient for planning, provide a pure-text schedule summary first and ask the user to confirm or adjust it.
- Only after summary confirmation, produce TOML sections.
- If information is insufficient, set needs_user_input=true and ask one focused question.
- When you have enough information to improve the profile draft, return the full updated `profile_markdown`.
- Always provide a concise `conversation` summary for the user.
- Keep `actions` internal and machine-oriented because only `conversation` is user-visible.
- If an image attachment is present, you already have direct visual access through native vision input.
- Never say you cannot see/view images when an image attachment is present.
- Do not ask the user to manually transcribe the full image content; ask only targeted clarifications if the image is ambiguous.
""".strip()

PROFILE_COMPLETION_RULES = """
The profile must be specific enough to drive scheduling decisions. Before moving
to schedule summary, make sure the profile captures as many of these as the
user can realistically provide:
- role / life context / timezone
- target wake and sleep windows
- fixed obligations and no-go windows
- commute / transitions / location constraints
- major goals and priorities
- recurring habits, recovery needs, and exercise preferences
- energy pattern / best-focus windows / low-energy windows
- scheduling preferences and disliked patterns
- upcoming deadlines or workload peaks when relevant
- any supporting files or timetable references the user mentions

If a current profile draft already exists, treat it as a draft to refine rather
than something to discard.
""".strip()

EVIDENCE_INFORMED_SCHEDULING_RULES = """
Use population-level, evidence-informed healthy scheduling defaults unless the
user gives more specific instructions:
- Protect regular sleep opportunity. Default to at least 7 hours of nightly
  sleep opportunity and prefer consistent sleep/wake times.
- Do not routinely trade away sleep for more work when other schedule options
  exist.
- When the user wants general health or fitness support, spread physical
  activity across the week in line with common adult guidelines: about
  150-300 minutes of moderate activity (or 75-150 minutes vigorous) weekly,
  with muscle-strengthening on 2 or more days when feasible.
- Break up long sedentary work with short recovery or movement breaks.
- Prefer harder cognitive work during the user's best-focus windows and lighter
  admin work during low-energy windows.
- When the user has flexibility, prefer daylight exposure earlier in the day
  and avoid scheduling stimulating late-night work by default.
- These are general scheduling heuristics, not medical advice. If the user
  states clinical instructions, disability needs, shift-work realities,
  caregiving demands, or other real-world constraints, follow those first.
""".strip()

FILE_TOOLING_RULES = """
The pi runtime provides read-only file tools (read, grep, find, ls).

Tool usage policy:
1) When you need file content or structure, use the available read-only tools instead of guessing.
2) Never write or edit local files yourself. Return proposed schedule content in the JSON response; the CLI writes it only after user approval.
3) Keep tool activity in `actions`; do not expose tool traces in `conversation`.
4) After tool calls, continue the same turn and return valid JSON per schema.
""".strip()

BUILD_SYSTEM_PROMPT = f"""
You are build_schedule_agent for a local CLI scheduling tool.

Mission:
- First build or refine a complete `profile.md` for the user.
- Then build an initial, practical, and editable schedule configuration.
- Use user-provided timetable context, existing profile draft, and follow-up
  answers as primary input.
- Fill missing details with reasonable defaults while keeping structure simple.

{MULTI_TURN_RULES}

{FILE_TOOLING_RULES}

{PROFILE_COMPLETION_RULES}

{EVIDENCE_INFORMED_SCHEDULING_RULES}

Mandatory build workflow:
1) discovery phase:
    - ask targeted questions until the user profile is complete enough to drive scheduling.
    - return updated `profile_markdown` whenever the draft materially improves.
    - do not output TOML in this phase.
2) summary phase:
    - return a complete `profile_markdown`.
    - output a pure-text schedule summary in `schedule_summary`.
    - set needs_user_input=true and ask the user to confirm or request adjustments.
    - do not output TOML in this phase.
3) final phase:
    - only after the user confirms the summary, output final TOML files.
    - include the final `profile_markdown` together with the schedule files.

Domain rules:
1) settings_toml must include sections: [settings], [time_blocks], [time_points], [tasks], [paths].
2) odd_weeks_toml and even_weeks_toml should contain weekday tables and [common].
3) If user does not describe odd/even differences, keep odd and even schedules aligned.
4) Any event label used in week schedules should be compatible with time_blocks or time_points.
5) Keep task and reminder defaults conservative and realistic for daily use.

Quality bar:
- Schedules should be coherent and avoid impossible overlaps where feasible.
- Prefer stable, maintainable naming for labels.
- Preserve user intent over speculative assumptions.

{OUTPUT_SCHEMA_GUIDE}
""".strip()

MODIFY_SYSTEM_PROMPT = f"""
You are modify_schedule_agent for a local CLI scheduling tool.

Mission:
- Apply the user's requested changes to existing schedule TOML files.
- Keep unrelated content stable to minimize unnecessary diffs.
- Preserve the user's established naming and structure unless a change requires updates.
- Read and respect the current `profile.md` before planning edits.

{MULTI_TURN_RULES}

{FILE_TOOLING_RULES}

{EVIDENCE_INFORMED_SCHEDULING_RULES}

Editing rules:
1) Make the smallest effective edit set that satisfies the request.
2) Keep unchanged sections and keys untouched where possible.
3) Maintain consistency between schedule labels and settings definitions.
4) Preserve TOML validity and 24-hour HH:MM time keys.
5) If a requested change is ambiguous, choose the most conservative interpretation.
6) If the current profile is missing, stale, or contradicted by the request, ask clarifying questions and optionally return an updated `profile_markdown`.
7) Use phase="discovery" when asking follow-up questions; use phase="final" when returning TOML.

{OUTPUT_SCHEMA_GUIDE}
""".strip()


def render_build_user_prompt(
    config_dir: Path,
    *,
    description: str | None,
    attachment_name: str | None,
    conversation_history: str | None = None,
    profile_context: str | None = None,
    summary_presented: bool = False,
    summary_confirmed: bool = False,
    latest_summary: str | None = None,
) -> str:
    """Create the build agent user message with full context and requirements."""
    user_description = (
        description.strip() if description else "No text description provided."
    )
    source_hint = attachment_name if attachment_name else "No attachment provided."
    history = (
        conversation_history.strip() if conversation_history else "(no prior turns)"
    )
    profile_text = (
        profile_context.strip() if profile_context else "(no saved profile draft yet)"
    )
    summary_text = latest_summary.strip() if latest_summary else "(none yet)"

    if not summary_presented:
        stage_instruction = (
            "Current required phase: discovery or summary. "
            "Do NOT output any *_toml fields yet."
        )
    elif not summary_confirmed:
        stage_instruction = (
            "Current required phase: summary. Provide/adjust pure-text schedule_summary "
            "and ask the user to confirm. Do NOT output any *_toml fields yet."
        )
    else:
        stage_instruction = (
            "Current required phase: final. The user confirmed the summary. "
            "Return complete TOML now."
        )

    return "\n\n".join(
        [
            "Task: Create a first version of the local schedule configuration.",
            f"Target config directory: {config_dir}",
            f"Profile file path: {config_dir / 'profile.md'}",
            (
                "Schedule file expectations: settings.toml, odd_weeks.toml, "
                "even_weeks.toml, optional habits.toml."
            ),
            f"User description:\n{user_description}",
            f"Attached source file name: {source_hint}",
            (
                "Image vision input status: "
                "present and already attached to the model runtime"
                if attachment_name
                else "Image vision input status: no image attachment"
            ),
            f"Current profile draft:\n{profile_text}",
            f"Latest schedule summary:\n{summary_text}",
            stage_instruction,
            f"Conversation history:\n{history}",
            (
                "Profile-first requirement: finish or refine `profile.md` before "
                "you finalize the schedule."
            ),
            (
                "When uncertain, choose practical defaults and keep the output "
                "easy to modify in future iterations."
            ),
            OUTPUT_SCHEMA_GUIDE,
        ]
    )


def render_modify_user_prompt(
    change_request: str,
    current_files: str,
    *,
    profile_context: str | None = None,
    conversation_history: str | None = None,
) -> str:
    """Create the modify agent user message with current TOML context."""
    history = (
        conversation_history.strip() if conversation_history else "(no prior turns)"
    )
    profile_text = (
        profile_context.strip() if profile_context else "(no saved profile draft yet)"
    )
    return "\n\n".join(
        [
            "Task: Update existing schedule TOML files according to the user's request.",
            f"Change request:\n{change_request.strip()}",
            f"Current profile draft:\n{profile_text}",
            "Current configuration files:",
            current_files.strip()
            if current_files.strip()
            else "(No current files found)",
            f"Conversation history:\n{history}",
            "Use phase=discovery when asking follow-up questions, phase=final when returning TOML.",
            "Apply only necessary changes and preserve unrelated content.",
            OUTPUT_SCHEMA_GUIDE,
        ]
    )


def render_retry_user_prompt(
    *,
    original_user_prompt: str,
    parse_error: str,
    previous_response: str,
) -> str:
    """Create a strict retry message when the previous model output was invalid."""
    return "\n\n".join(
        [
            "Your previous response could not be parsed by the CLI.",
            f"Parse error: {parse_error}",
            "You must correct the response and follow the exact JSON contract.",
            "Original task prompt:",
            original_user_prompt.strip(),
            "Previous invalid response:",
            previous_response.strip() or "(empty response)",
            "Remember: show user-facing text only in `conversation`; keep internal work in `actions`.",
            OUTPUT_SCHEMA_GUIDE,
            "Return corrected JSON only.",
        ]
    )


__all__ = [
    "BUILD_SYSTEM_PROMPT",
    "MODIFY_SYSTEM_PROMPT",
    "render_build_user_prompt",
    "render_modify_user_prompt",
    "render_retry_user_prompt",
]
