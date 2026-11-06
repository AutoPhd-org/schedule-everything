import tomllib

from schedule_management.install_config_wizard import run_wizard


def _write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def test_run_wizard_creates_required_files(tmp_path):
    config_dir = tmp_path / "config"
    template_dir = tmp_path / "templates"

    _write(
        template_dir / "settings_template.toml",
        """
[settings]
sound_file = "/tmp/ping.aiff"
alarm_interval = 5

[paths]
reports_path = "~/Desktop/reports"
""".strip()
        + "\n",
    )
    _write(
        template_dir / "week_schedule_template.toml",
        """
[monday]
"09:00" = "pomodoro"
""".strip()
        + "\n",
    )
    _write(
        template_dir / "habits_template.toml",
        """
[habits]
1 = "Read"
""".strip()
        + "\n",
    )

    success = run_wizard(config_dir, template_dir, auto_yes=True)

    assert success
    assert (config_dir / "settings.toml").exists()
    assert (config_dir / "odd_weeks.toml").exists()
    assert (config_dir / "even_weeks.toml").exists()
    assert (config_dir / "habits.toml").exists()


def test_run_wizard_prompts_for_missing_settings(tmp_path):
    config_dir = tmp_path / "config"
    template_dir = tmp_path / "templates"

    _write(
        config_dir / "settings.toml",
        """
[settings]
sound_file = "/tmp/ping.aiff"
""".strip()
        + "\n",
    )
    _write(config_dir / "odd_weeks.toml", '[monday]\n"09:00" = "pomodoro"\n')
    _write(config_dir / "even_weeks.toml", '[monday]\n"09:00" = "pomodoro"\n')
    _write(config_dir / "habits.toml", '[habits]\n1 = "Read"\n')

    _write(
        template_dir / "settings_template.toml",
        """
[settings]
sound_file = "/tmp/ping.aiff"
alarm_interval = 5

[paths]
reports_path = "~/Desktop/reports"
""".strip()
        + "\n",
    )
    _write(
        template_dir / "week_schedule_template.toml",
        """
[monday]
"09:00" = "pomodoro"
""".strip()
        + "\n",
    )
    _write(
        template_dir / "habits_template.toml",
        """
[habits]
1 = "Read"
""".strip()
        + "\n",
    )

    responses = iter(["7", "~/Desktop/custom-reports"])
    success = run_wizard(
        config_dir,
        template_dir,
        auto_yes=False,
        input_func=lambda _: next(responses),
    )

    assert success

    with open(config_dir / "settings.toml", "rb") as handle:
        data = tomllib.load(handle)

    assert data["settings"]["alarm_interval"] == 7
    assert data["paths"]["reports_path"] == "~/Desktop/custom-reports"


def _write_config_and_templates(config_dir, template_dir, settings, template_settings):
    _write(config_dir / "settings.toml", settings.strip() + "\n")
    _write(config_dir / "odd_weeks.toml", '[monday]\n"09:00" = "potato"\n')
    _write(config_dir / "even_weeks.toml", '[monday]\n"09:00" = "potato"\n')
    _write(config_dir / "habits.toml", '[habits]\n1 = "Read"\n')
    _write(template_dir / "settings_template.toml", template_settings.strip() + "\n")
    _write(template_dir / "week_schedule_template.toml", '[monday]\n"09:00" = "pomodoro"\n')
    _write(template_dir / "habits_template.toml", '[habits]\n1 = "Read"\n')


EXAMPLE_TEMPLATE = """
[settings]
sound_file = "/tmp/ping.aiff"
alarm_interval = 5

[time_blocks]
pomodoro = 25
potato = 50

[time_points]
go_to_bed = "Time for bed"

[task_types]
1 = "read papers"
2 = "gym work"
3 = "coding"
4 = "other"
"""


def test_run_wizard_does_not_bring_back_removed_user_defined_entries(tmp_path):
    config_dir = tmp_path / "config"
    template_dir = tmp_path / "templates"
    # Task types 2 and 4, the pomodoro block, and the bedtime reminder were all removed on purpose.
    _write_config_and_templates(
        config_dir,
        template_dir,
        """
[settings]
sound_file = "/tmp/ping.aiff"

[time_blocks]
potato = 50

[time_points]
lunch = "Lunch time"

[task_types]
1 = "TA"
3 = "diary"
18 = "Intern"
""",
        EXAMPLE_TEMPLATE,
    )

    prompts = []
    responses = iter(["7"])

    def answer(prompt):
        prompts.append(prompt)
        return next(responses)

    assert run_wizard(config_dir, template_dir, auto_yes=False, input_func=answer)

    # Only the missing fixed setting is asked for.
    assert len(prompts) == 1
    assert "[settings].alarm_interval" in prompts[0]
    with open(config_dir / "settings.toml", "rb") as handle:
        data = tomllib.load(handle)
    assert data["settings"]["alarm_interval"] == 7
    assert data["task_types"] == {"1": "TA", "3": "diary", "18": "Intern"}
    assert data["time_blocks"] == {"potato": 50}
    assert data["time_points"] == {"lunch": "Lunch time"}


def test_run_wizard_offers_examples_for_a_missing_user_defined_section(tmp_path):
    config_dir = tmp_path / "config"
    template_dir = tmp_path / "templates"
    _write_config_and_templates(
        config_dir,
        template_dir,
        """
[settings]
sound_file = "/tmp/ping.aiff"
alarm_interval = 5

[time_blocks]
potato = 50

[time_points]
lunch = "Lunch time"
""",
        EXAMPLE_TEMPLATE,
    )

    assert run_wizard(config_dir, template_dir, auto_yes=True)

    with open(config_dir / "settings.toml", "rb") as handle:
        data = tomllib.load(handle)
    assert data["task_types"] == {"1": "read papers", "2": "gym work", "3": "coding", "4": "other"}
    assert data["time_blocks"] == {"potato": 50}


def test_run_wizard_aborts_when_required_file_creation_declined(tmp_path):
    config_dir = tmp_path / "config"
    template_dir = tmp_path / "templates"

    _write(
        template_dir / "settings_template.toml",
        """
[settings]
sound_file = "/tmp/ping.aiff"
""".strip()
        + "\n",
    )
    _write(template_dir / "week_schedule_template.toml", "[monday]\n")
    _write(template_dir / "habits_template.toml", '[habits]\n1 = "Read"\n')

    responses = iter(["n"])
    success = run_wizard(
        config_dir,
        template_dir,
        auto_yes=False,
        input_func=lambda _: next(responses),
    )

    assert not success
    assert not (config_dir / "settings.toml").exists()
