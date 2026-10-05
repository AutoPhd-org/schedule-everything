"""Tests for the `rmd setup` command flow and helpers."""

import json
import os
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

import schedule_management.commands.setup as setup_cmd_module
from schedule_management.commands.setup import (
    LLMConfig,
    has_completed_configuration,
    load_llm_config,
    save_llm_config,
    setup_command,
)
from schedule_management.commands.setup_prompts import (
    BUILD_SYSTEM_PROMPT,
    MODIFY_SYSTEM_PROMPT,
    render_build_user_prompt,
    render_modify_user_prompt,
    render_retry_user_prompt,
)
from schedule_management.cli import create_parser


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def test_has_completed_configuration_true(tmp_path):
    _write(
        tmp_path / "settings.toml",
        """
[settings]

[time_blocks]

[time_points]

[tasks]

[paths]
""".strip()
        + "\n",
    )
    _write(
        tmp_path / "odd_weeks.toml",
        """
[monday]
"09:00" = "focus"

[common]
""".strip()
        + "\n",
    )
    _write(
        tmp_path / "even_weeks.toml",
        """
[monday]
"09:00" = "focus"

[common]
""".strip()
        + "\n",
    )
    _write(tmp_path / "habits.toml", '[habits]\n1 = "Read"\n')

    complete, reason = has_completed_configuration(tmp_path)

    assert complete is True
    assert reason == "ok"


def test_has_completed_configuration_missing_file(tmp_path):
    _write(tmp_path / "settings.toml", "[settings]\n")

    complete, reason = has_completed_configuration(tmp_path)

    assert complete is False
    assert "Missing" in reason


def test_has_completed_configuration_invalid_toml(tmp_path):
    _write(tmp_path / "settings.toml", "[settings]\n")
    _write(tmp_path / "odd_weeks.toml", "[monday]\n")
    _write(tmp_path / "even_weeks.toml", "[monday]\n")
    _write(tmp_path / "habits.toml", "[habits\n")

    complete, reason = has_completed_configuration(tmp_path)

    assert complete is False
    assert "Invalid TOML" in reason


def test_llm_config_round_trip(tmp_path):
    config_path = tmp_path / "llm.toml"
    expected = LLMConfig(model="gpt-4.1-mini")

    save_llm_config(config_path, expected)
    loaded = load_llm_config(config_path)

    assert loaded == expected


def test_llm_config_round_trip_none_model(tmp_path):
    config_path = tmp_path / "llm.toml"

    save_llm_config(config_path, LLMConfig(model=None))
    loaded = load_llm_config(config_path)

    assert loaded == LLMConfig(model=None)


def test_load_llm_config_ignores_legacy_vendor_fields(tmp_path):
    config_path = tmp_path / "llm.toml"
    config_path.write_text(
        'vendor = "openai"\n'
        'model = "gpt-4.1-mini"\n'
        'api_key = "secret"\n'
        'base_url = "https://example.com/v1"\n',
        encoding="utf-8",
    )

    loaded = load_llm_config(config_path)

    assert loaded == LLMConfig(model="gpt-4.1-mini")


def test_profile_markdown_round_trip(tmp_path):
    setup_cmd_module._write_profile_markdown(
        tmp_path,
        "# Basic Information\n- Role: CS PhD student",
    )

    loaded = setup_cmd_module._load_profile_markdown(tmp_path)

    assert loaded == "# Basic Information\n- Role: CS PhD student"


def test_llm_client_uses_pi_cli_and_streams_stdout():
    llm_client = setup_cmd_module.LLMClient(LLMConfig(model="gpt-4.1-mini"))

    with (
        patch.object(
            setup_cmd_module.LLMClient,
            "_resolve_pi_bin",
            return_value="/usr/local/bin/pi",
        ),
        patch("subprocess.run") as run_mock,
    ):
        event = {
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": '{"ok": true}'}],
            },
        }
        run_mock.return_value = subprocess.CompletedProcess(
            args=["pi"],
            returncode=0,
            stdout=json.dumps(event) + "\n",
            stderr="",
        )

        streamed_chunks: list[str] = []
        result = llm_client.generate("system", "user", on_text=streamed_chunks.append)

    assert result == '{"ok": true}'
    assert streamed_chunks == ['{"ok": true}']

    command = run_mock.call_args.args[0]
    assert command[0] == "/usr/local/bin/pi"
    assert command[1:3] == ["--mode", "json"]
    assert "--print" in command
    assert command[command.index("--tools") + 1] == "read,grep,find,ls"
    # model is passed through as-is, no provider prefixing
    assert "--model" in command
    idx = command.index("--model")
    assert command[idx + 1] == "gpt-4.1-mini"
    assert "--system-prompt" in command
    assert "system" in command
    assert command[-1] == "user"

    # pi owns credentials: we never inject provider API keys / base URL
    env = run_mock.call_args.kwargs["env"]
    for key in (
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "OPENAI_BASE_URL",
        "OPENCODE_CLIENT",
    ):
        assert key not in env or env[key] == os.environ.get(key), (
            f"{key} must not be injected by the client"
        )


def test_llm_client_omits_model_flag_when_unset():
    llm_client = setup_cmd_module.LLMClient(LLMConfig(model=None))

    with (
        patch.object(
            setup_cmd_module.LLMClient,
            "_resolve_pi_bin",
            return_value="/usr/local/bin/pi",
        ),
        patch("subprocess.run") as run_mock,
    ):
        event = {
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": '{"ok": true}'}],
            },
        }
        run_mock.return_value = subprocess.CompletedProcess(
            args=["pi"],
            returncode=0,
            stdout=json.dumps(event) + "\n",
            stderr="",
        )

        llm_client.generate("system", "user")

    command = run_mock.call_args.args[0]
    assert "--model" not in command


def test_local_file_tools_can_read_and_edit(tmp_path):
    tools = setup_cmd_module.LocalFileTools(allowed_roots=[tmp_path])
    source = tmp_path / "notes.txt"
    source.write_text("alpha\nbeta\n", encoding="utf-8")

    read_result = tools.execute(
        "read_file",
        {
            "path": str(source),
            "start_line": 2,
            "end_line": 2,
        },
    )
    assert read_result["ok"] is True
    assert read_result["content"] == "beta"

    replace_result = tools.execute(
        "replace_in_file",
        {
            "path": str(source),
            "old_text": "beta",
            "new_text": "gamma",
        },
    )
    assert replace_result["ok"] is True
    assert "gamma" in source.read_text(encoding="utf-8")

    write_result = tools.execute(
        "write_file",
        {
            "path": str(tmp_path / "created.txt"),
            "content": "hello",
        },
    )
    assert write_result["ok"] is True
    assert (tmp_path / "created.txt").read_text(encoding="utf-8") == "hello"

    blocked = tools.execute("read_file", {"path": "/etc/hosts"})
    assert blocked["ok"] is False


def test_llm_client_passes_attachment_as_positional_at_path(tmp_path):
    llm_client = setup_cmd_module.LLMClient(LLMConfig(model="gpt-4.1-mini"))

    attachment = setup_cmd_module.SourceAttachment(
        path=tmp_path / "plan.png",
        mime_type="image/png",
        image_base64="aGVsbG8=",
    )

    with (
        patch.object(
            setup_cmd_module.LLMClient,
            "_resolve_pi_bin",
            return_value="/usr/local/bin/pi",
        ),
        patch("subprocess.run") as run_mock,
    ):
        event = {
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [{"type": "text", "text": '{"ok": true}'}],
            },
        }
        run_mock.return_value = subprocess.CompletedProcess(
            args=["pi"],
            returncode=0,
            stdout=json.dumps(event) + "\n",
            stderr="",
        )

        result = llm_client.generate("system", "user", attachment=attachment)

    assert result == '{"ok": true}'
    command = run_mock.call_args.args[0]
    assert f"@{attachment.path}" in command
    assert "--file" not in command


def test_llm_client_raises_when_pi_fails():
    llm_client = setup_cmd_module.LLMClient(
        LLMConfig(model="gpt-4.1-mini")
    )

    with (
        patch.object(
            setup_cmd_module.LLMClient,
            "_resolve_pi_bin",
            return_value="/usr/local/bin/pi",
        ),
        patch("subprocess.run") as run_mock,
    ):
        run_mock.return_value = subprocess.CompletedProcess(
            args=["pi"],
            returncode=1,
            stdout="",
            stderr="provider auth failed",
        )

        with pytest.raises(RuntimeError) as exc:
            llm_client.generate("system", "user")

    assert "pi CLI execution failed" in str(exc.value)


def test_llm_client_raises_when_pi_reports_error_event():
    llm_client = setup_cmd_module.LLMClient(
        LLMConfig(model="gpt-4.1-mini")
    )

    with (
        patch.object(
            setup_cmd_module.LLMClient,
            "_resolve_pi_bin",
            return_value="/usr/local/bin/pi",
        ),
        patch("subprocess.run") as run_mock,
    ):
        event = {
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [],
                "stopReason": "error",
                "errorMessage": "Invalid provider API key",
            },
        }
        run_mock.return_value = subprocess.CompletedProcess(
            args=["pi"],
            returncode=0,
            stdout=json.dumps(event) + "\n",
            stderr="",
        )

        with pytest.raises(RuntimeError) as exc:
            llm_client.generate("system", "user")

    assert "pi CLI reported an error" in str(exc.value)
    assert "Invalid provider API key" in str(exc.value)


def test_llm_client_does_not_accept_text_before_pi_error():
    llm_client = setup_cmd_module.LLMClient(LLMConfig())
    events = [
        {
            "type": "message_end",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "partial"}]},
        },
        {"type": "error", "error": "request failed"},
    ]
    with (
        patch.object(setup_cmd_module.LLMClient, "_resolve_pi_bin", return_value="pi"),
        patch("subprocess.run") as run_mock,
    ):
        run_mock.return_value = subprocess.CompletedProcess(
            args=["pi"], returncode=0,
            stdout="\n".join(json.dumps(event) for event in events), stderr="",
        )
        with pytest.raises(RuntimeError, match="request failed"):
            llm_client.generate("system", "user")


def test_llm_client_rejects_json_events_without_assistant_message():
    llm_client = setup_cmd_module.LLMClient(LLMConfig())
    with (
        patch.object(setup_cmd_module.LLMClient, "_resolve_pi_bin", return_value="pi"),
        patch("subprocess.run") as run_mock,
    ):
        run_mock.return_value = subprocess.CompletedProcess(
            args=["pi"], returncode=0,
            stdout=json.dumps({"type": "agent_start"}), stderr="",
        )
        with pytest.raises(RuntimeError, match="no assistant message"):
            llm_client.generate("system", "user")


def test_llm_client_raises_when_stdout_empty_but_stderr_present():
    llm_client = setup_cmd_module.LLMClient(
        LLMConfig(model="gpt-4.1-mini")
    )

    with (
        patch.object(
            setup_cmd_module.LLMClient,
            "_resolve_pi_bin",
            return_value="/usr/local/bin/pi",
        ),
        patch("subprocess.run") as run_mock,
    ):
        run_mock.return_value = subprocess.CompletedProcess(
            args=["pi"],
            returncode=0,
            stdout="",
            stderr="rate limit exceeded",
        )

        with pytest.raises(RuntimeError) as exc:
            llm_client.generate("system", "user")

    assert "pi CLI stderr" in str(exc.value)
    assert "rate limit exceeded" in str(exc.value)


def test_resolve_pi_bin_prefers_env_override(monkeypatch):
    monkeypatch.setenv("REMINDER_PI_BIN", "/custom/pi")

    with patch("shutil.which") as which_mock:
        resolved = setup_cmd_module.LLMClient._resolve_pi_bin()

    assert resolved == "/custom/pi"
    which_mock.assert_not_called()


def test_resolve_pi_bin_uses_path_lookup(monkeypatch):
    monkeypatch.delenv("REMINDER_PI_BIN", raising=False)

    with patch("shutil.which", return_value="/usr/local/bin/pi"):
        resolved = setup_cmd_module.LLMClient._resolve_pi_bin()

    assert resolved == "/usr/local/bin/pi"


def test_resolve_pi_bin_errors_with_npm_install_hint(monkeypatch):
    monkeypatch.delenv("REMINDER_PI_BIN", raising=False)

    with patch("shutil.which", return_value=None):
        with pytest.raises(RuntimeError) as exc:
            setup_cmd_module.LLMClient._resolve_pi_bin()

    assert "npm install -g @earendil-works/pi-coding-agent" in str(exc.value)
    assert "REMINDER_PI_BIN" in str(exc.value)


def test_load_source_attachment_detects_image_without_image_extension(tmp_path):
    raw_png = b"\x89PNG\r\n\x1a\n" + b"x" * 16
    path = tmp_path / "upload.bin"
    path.write_bytes(raw_png)

    attachment, error = setup_cmd_module._load_source_attachment(path)

    assert error is None
    assert attachment is not None
    assert attachment.image_base64 is not None
    assert attachment.mime_type == "image/png"


def test_resolve_source_path_input_finds_common_downloads(tmp_path, monkeypatch):
    home = tmp_path / "home"
    downloads = home / "Downloads"
    downloads.mkdir(parents=True)

    target = downloads / "50499A1ABA0EFC793F82A8F910B3FC3C.png"
    target.write_bytes(b"\x89PNG\r\n\x1a\n" + b"x" * 16)

    workdir = tmp_path / "workspace"
    workdir.mkdir(parents=True)
    monkeypatch.chdir(workdir)

    with patch("pathlib.Path.home", return_value=home):
        resolved = setup_cmd_module._resolve_source_path_input(target.name)

    assert resolved == target


def test_turn_requests_manual_image_transcription_detection():
    turn = setup_cmd_module.AgentTurn(
        phase="discovery",
        conversation="I'm sorry, but I cannot see images.",
        needs_user_input=True,
        question_to_user=(
            "Could you describe the changes you'd like to make based on the image?"
        ),
        missing_information=["Details of the schedule changes from the image"],
    )

    assert setup_cmd_module._turn_requests_manual_image_transcription(turn) is True

    quality_turn = setup_cmd_module.AgentTurn(
        phase="discovery",
        conversation="I can see the image but the text is blurry in two cells.",
        needs_user_input=True,
        question_to_user="Could you confirm the Tuesday 14:00 course name?",
        missing_information=["Tuesday 14:00 course label"],
    )

    assert (
        setup_cmd_module._turn_requests_manual_image_transcription(quality_turn)
        is False
    )


def test_setup_command_routes_to_modify_when_config_exists(tmp_path):
    llm = LLMConfig(model="gpt-4.1-mini")

    with (
        patch(
            "schedule_management.commands.setup.ensure_llm_config",
            return_value=llm,
        ),
        patch(
            "schedule_management.commands.setup._resolve_config_dir",
            return_value=tmp_path,
        ),
        patch(
            "schedule_management.commands.setup.has_completed_configuration",
            return_value=(True, "ok"),
        ),
        patch("schedule_management.commands.setup._ask_yes_no", return_value=True),
        patch(
            "schedule_management.commands.setup.modify_schedule_agent",
            return_value=0,
        ) as modify_mock,
    ):
        result = setup_command(MagicMock())

    assert result == 0
    modify_mock.assert_called_once_with(llm, tmp_path)


def test_modify_schedule_agent_creates_next_version_after_acceptance(tmp_path):
    config_dir = tmp_path / "user_config_0"
    _write(
        config_dir / "settings.toml",
        """
[settings]

[time_blocks]
focus = 50

[time_points]

[tasks]

[paths]
""".strip()
        + "\n",
    )
    _write(
        config_dir / "odd_weeks.toml",
        """
[monday]
"09:00" = "focus"

[common]
""".strip()
        + "\n",
    )
    _write(
        config_dir / "even_weeks.toml",
        """
[monday]
"09:00" = "focus"

[common]
""".strip()
        + "\n",
    )
    _write(config_dir / "habits.toml", '[habits]\n1 = "Read"\n')
    _write(config_dir / "profile.md", "# Before\n")
    _write(config_dir / "ddl.json", "{}\n")

    turn = setup_cmd_module.AgentTurn(
        phase="final",
        conversation="I moved lunch later and kept the rest stable.",
        needs_user_input=False,
        profile_markdown="# After\n- Keep afternoons lighter",
        bundle={
            "settings.toml": (
                "[settings]\n\n"
                "[time_blocks]\nfocus = 60\n\n"
                "[time_points]\n\n"
                "[tasks]\n\n"
                "[paths]\n"
            ),
            "odd_weeks.toml": '[monday]\n"10:00" = "focus"\n\n[common]\n',
            "even_weeks.toml": '[monday]\n"10:00" = "focus"\n\n[common]\n',
            "habits.toml": '[habits]\n1 = "Read"\n',
        },
    )

    status_mock = MagicMock()
    status_mock.__enter__.return_value = None
    status_mock.__exit__.return_value = False

    with (
        patch(
            "schedule_management.commands.setup_agent.workflow._request_agent_turn",
            return_value=(turn, None),
        ),
        patch(
            "schedule_management.commands.setup_agent.workflow._prompt_non_empty",
            side_effect=["Move lunch later"],
        ),
        patch(
            "schedule_management.commands.setup_agent.workflow._ask_yes_no",
            side_effect=[True, False],
        ),
        patch(
            "schedule_management.commands.setup_agent.workflow._reload_runner_after_config_change",
            return_value=(True, ""),
        ),
        patch.object(
            setup_cmd_module.CONSOLE,
            "status",
            return_value=status_mock,
        ),
    ):
        result = setup_cmd_module.modify_schedule_agent(
            LLMConfig(model="gpt-4.1-mini"),
            config_dir,
        )

    assert result == 0
    assert (tmp_path / ".active_config").read_text(encoding="utf-8").strip() == "1"
    assert (tmp_path / "user_config_1" / "ddl.json").read_text(encoding="utf-8") == "{}\n"
    assert (
        tmp_path / "user_config_1" / "profile.md"
    ).read_text(encoding="utf-8") == "# After\n- Keep afternoons lighter\n"
    assert (
        tmp_path / "user_config_1" / "settings.toml"
    ).read_text(encoding="utf-8") == turn.bundle["settings.toml"]
    assert (
        tmp_path / "user_config_0" / "settings.toml"
    ).read_text(encoding="utf-8") != turn.bundle["settings.toml"]


def test_setup_command_routes_to_build_when_config_missing(tmp_path):
    llm = LLMConfig(model="gpt-4.1-mini")

    with (
        patch(
            "schedule_management.commands.setup.ensure_llm_config",
            return_value=llm,
        ),
        patch(
            "schedule_management.commands.setup._resolve_config_dir",
            return_value=tmp_path,
        ),
        patch(
            "schedule_management.commands.setup.has_completed_configuration",
            return_value=(False, "Missing settings.toml"),
        ),
        patch("schedule_management.commands.setup._ask_yes_no", return_value=True),
        patch(
            "schedule_management.commands.setup.build_schedule_agent",
            return_value=0,
        ) as build_mock,
    ):
        result = setup_command(MagicMock())

    assert result == 0
    build_mock.assert_called_once_with(llm, tmp_path)


def test_setup_command_cancels_when_input_closes(tmp_path):
    with (
        patch("schedule_management.commands.setup.ensure_llm_config", return_value=LLMConfig()),
        patch("schedule_management.commands.setup._resolve_config_dir", return_value=tmp_path),
        patch("schedule_management.commands.setup.has_completed_configuration", return_value=(False, "missing")),
        patch("schedule_management.commands.setup_agent.configuration.CONSOLE.input", side_effect=EOFError),
        patch("schedule_management.commands.setup.build_schedule_agent") as build_mock,
    ):
        assert setup_command(MagicMock()) == 1
    build_mock.assert_not_called()


def test_prompt_non_empty_stops_on_eof():
    with patch("schedule_management.commands.setup_agent.configuration.CONSOLE.input", side_effect=EOFError) as input_mock:
        with pytest.raises(EOFError, match="Setup input closed"):
            setup_cmd_module._prompt_non_empty("What changed? ")
    input_mock.assert_called_once()


def test_modify_preview_shows_actual_file_diff(tmp_path):
    from schedule_management.commands.setup_agent import interaction

    _write(tmp_path / "settings.toml", "[settings]\nfocus = 25\n")
    with patch.object(interaction.CONSOLE, "print") as print_mock:
        interaction._render_bundle_preview(
            tmp_path, {"settings.toml": "[settings]\nfocus = 50\n"}
        )
    preview = print_mock.call_args.args[0].renderable.plain
    assert "-focus = 25" in preview
    assert "+focus = 50" in preview


def test_setup_command_is_registered_in_parser():
    parser = create_parser()

    assert parser.prog == "rmd"

    args = parser.parse_args(["setup"])

    assert args.command == "setup"
    assert args.func.__name__ == "setup_command"


def test_prompt_templates_include_required_keys():
    for prompt in (BUILD_SYSTEM_PROMPT, MODIFY_SYSTEM_PROMPT):
        assert "phase" in prompt
        assert "conversation" in prompt
        assert "needs_user_input" in prompt
        assert "profile_markdown" in prompt
        assert "question_to_user" in prompt
        assert "schedule_summary" in prompt
        assert "settings_toml" in prompt
        assert "odd_weeks_toml" in prompt
        assert "even_weeks_toml" in prompt
        assert "read-only file tools" in prompt
        assert "Never write or edit local files yourself" in prompt

    assert "Never say you cannot see/view images" in BUILD_SYSTEM_PROMPT
    assert "at least 7 hours" in BUILD_SYSTEM_PROMPT
    assert "150-300 minutes of moderate activity" in BUILD_SYSTEM_PROMPT


def test_prompt_renderers_include_context_sections(tmp_path):
    build_prompt = render_build_user_prompt(
        tmp_path,
        description="Classes on monday and wednesday mornings.",
        attachment_name="timetable.png",
        conversation_history="Assistant: Got it\nUser: Please keep evenings free.",
        profile_context="# Basic Information\n- Role: Student",
    )
    assert "Target config directory" in build_prompt
    assert "Profile file path" in build_prompt
    assert "timetable.png" in build_prompt
    assert "Image vision input status" in build_prompt
    assert "Current profile draft" in build_prompt
    assert "Conversation history" in build_prompt

    summary_gate_prompt = render_build_user_prompt(
        tmp_path,
        description="draft",
        attachment_name=None,
        summary_presented=False,
        summary_confirmed=False,
    )
    assert "Do NOT output any *_toml fields yet." in summary_gate_prompt

    final_gate_prompt = render_build_user_prompt(
        tmp_path,
        description="draft",
        attachment_name=None,
        summary_presented=True,
        summary_confirmed=True,
        latest_summary="Weekday deep-work mornings.",
    )
    assert "Return complete TOML now." in final_gate_prompt

    modify_prompt = render_modify_user_prompt(
        "Move gym to evenings",
        "[settings.toml]\n[settings]\n",
        profile_context="# Preferences\n- Keep mornings for research.",
        conversation_history="Assistant: Do you prefer weekdays?\nUser: Tuesday/Thursday.",
    )
    assert "Change request" in modify_prompt
    assert "Current profile draft" in modify_prompt
    assert "Current configuration files" in modify_prompt
    assert "Conversation history" in modify_prompt

    retry_prompt = render_retry_user_prompt(
        original_user_prompt="Original task",
        parse_error="missing key",
        previous_response='{"foo": "bar"}',
    )
    assert "Parse error" in retry_prompt
    assert "Return corrected JSON only." in retry_prompt


def test_parse_agent_turn_requires_conversation_and_toml_when_ready():
    response = (
        "{"
        '"conversation": "I applied your update and generated files.", '
        '"needs_user_input": false, '
        '"actions": ["analyzed request", "prepared output"], '
        '"settings_toml": "[settings]\\n\\n[time_blocks]\\n\\n[time_points]\\n\\n[tasks]\\n\\n[paths]\\n", '
        '"odd_weeks_toml": "[monday]\\n\\"09:00\\" = \\"study\\"\\n\\n[common]\\n", '
        '"even_weeks_toml": "[monday]\\n\\"09:00\\" = \\"study\\"\\n\\n[common]\\n"'
        "}"
    )

    turn, error = setup_cmd_module._parse_agent_turn(response)

    assert error is None
    assert turn is not None
    assert turn.conversation.startswith("I applied")
    assert turn.needs_user_input is False
    assert turn.bundle is not None
    assert "settings.toml" in turn.bundle


def test_request_agent_turn_keeps_image_attachment_on_retry(tmp_path):
    attachment = setup_cmd_module.SourceAttachment(
        path=tmp_path / "plan.png",
        mime_type="image/png",
        image_base64="aGVsbG8=",
    )

    class FakeClient:
        def __init__(self):
            self.calls = 0
            self.attachments: list[setup_cmd_module.SourceAttachment | None] = []

        def generate(self, _sys, _usr, attachment=None, **_kwargs):
            self.calls += 1
            self.attachments.append(attachment)
            if self.calls == 1:
                return "not json"
            return (
                "{"
                '"phase": "discovery", '
                '"conversation": "Need one more detail.", '
                '"needs_user_input": true, '
                '"question_to_user": "What is your commute time?"'
                "}"
            )

    fake_client = FakeClient()
    turn, error = setup_cmd_module._request_agent_turn(
        fake_client,
        "system",
        "user",
        attachment=attachment,
    )

    assert error is None
    assert turn is not None
    assert fake_client.calls == 2
    assert fake_client.attachments == [attachment, attachment]


def test_parse_agent_turn_supports_missing_information_questions():
    response = (
        "{"
        '"conversation": "I need one more detail before I can finish.", '
        '"needs_user_input": true, '
        '"profile_markdown": "# Goals\\n- Finish dissertation", '
        '"question_to_user": "Which days should stay completely free?", '
        '"missing_information": ["free days preference"], '
        '"actions": ["validated constraints"]'
        "}"
    )

    turn, error = setup_cmd_module._parse_agent_turn(response)

    assert error is None
    assert turn is not None
    assert turn.needs_user_input is True
    assert turn.bundle is None
    assert turn.profile_markdown == "# Goals\n- Finish dissertation"
    assert turn.question_to_user == "Which days should stay completely free?"
    assert turn.missing_information == ["free days preference"]


def test_parse_agent_turn_requires_schedule_summary_in_summary_phase():
    response = (
        "{"
        '"phase": "summary", '
        '"conversation": "Here is the draft plan overview.", '
        '"needs_user_input": true, '
        '"question_to_user": "Do you want me to generate the TOML files now?"'
        "}"
    )

    turn, error = setup_cmd_module._parse_agent_turn(response)

    assert turn is None
    assert error is not None
    assert "schedule_summary" in error
