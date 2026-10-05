"""
Test suite for CLI commands in the reminder module.
Tests the update, view, and status commands using the new OOP architecture.
"""

import json
import sys
import pytest
from unittest.mock import patch, MagicMock
from pathlib import Path
from datetime import datetime, time, date

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))
import schedule_management.reminder as reminder
import schedule_management.runner as schedule_runner
from schedule_management.synced_schedule import (
    SyncedDaySchedule,
    apply_synced_schedule,
    save_synced_schedule,
)

# Import test configuration paths
from conftest import TEST_CONFIG_DIR


class TestUpdateCommand:
    """Test the update command functionality."""

    @patch("schedule_management.commands.service._restart_reminder_service")
    @patch("schedule_management.commands.service._has_git_metadata")
    @patch("schedule_management.commands.service._resolve_config_dir")
    @patch("schedule_management.commands.service.subprocess.run")
    def test_update_success(
        self,
        mock_subprocess,
        mock_resolve_config_dir,
        mock_has_git_metadata,
        mock_restart_service,
    ):
        """Test successful update command for a git-managed config."""
        mock_config_dir = MagicMock()
        mock_config_dir.exists.return_value = True
        mock_config_dir.__str__.return_value = "/tmp/config"
        mock_resolve_config_dir.return_value = mock_config_dir
        mock_has_git_metadata.return_value = True
        mock_restart_service.return_value = (True, "")
        mock_subprocess.return_value = MagicMock(
            returncode=0, stdout="Already up to date", stderr=""
        )

        args = MagicMock()
        result = reminder.update_command(args)

        assert result == 0
        mock_subprocess.assert_called_once_with(
            ["git", "-C", "/tmp/config", "pull", "--rebase"],
            capture_output=True,
            text=True,
            check=False,
        )
        mock_restart_service.assert_called_once_with()

    @patch("schedule_management.commands.service._restart_reminder_service")
    @patch("schedule_management.commands.service._has_git_metadata")
    @patch("schedule_management.commands.service._resolve_config_dir")
    @patch("schedule_management.commands.service.subprocess.run")
    def test_update_invalid_config(
        self,
        mock_subprocess,
        mock_resolve_config_dir,
        mock_has_git_metadata,
        mock_restart_service,
    ):
        """Test update command when git pull fails."""
        mock_config_dir = MagicMock()
        mock_config_dir.exists.return_value = True
        mock_config_dir.__str__.return_value = "/tmp/config"
        mock_resolve_config_dir.return_value = mock_config_dir
        mock_has_git_metadata.return_value = True
        mock_subprocess.return_value = MagicMock(
            returncode=1, stdout="", stderr="error: failed to pull"
        )

        args = MagicMock()
        result = reminder.update_command(args)

        assert result == 1
        mock_restart_service.assert_not_called()

    @patch("schedule_management.commands.service._resolve_config_dir")
    def test_update_missing_files(self, mock_resolve_config_dir):
        """Test update command with missing configuration files."""
        mock_config_dir = MagicMock()
        mock_config_dir.exists.return_value = False
        mock_resolve_config_dir.return_value = mock_config_dir

        args = MagicMock()
        result = reminder.update_command(args)

        assert result == 1

    @patch("schedule_management.commands.service._restart_reminder_service")
    @patch("schedule_management.commands.service._has_git_metadata")
    @patch("schedule_management.commands.service._resolve_config_dir")
    @patch("schedule_management.commands.service.subprocess.run")
    def test_update_non_git_config_skips_pull(
        self,
        mock_subprocess,
        mock_resolve_config_dir,
        mock_has_git_metadata,
        mock_restart_service,
    ):
        """Test update command when config is local-only and not git-managed."""
        mock_config_dir = MagicMock()
        mock_config_dir.exists.return_value = True
        mock_config_dir.__str__.return_value = "/tmp/config"
        mock_resolve_config_dir.return_value = mock_config_dir
        mock_has_git_metadata.return_value = False
        mock_restart_service.return_value = (False, "No installer restart script found.")

        args = MagicMock()
        result = reminder.update_command(args)

        assert result == 0
        mock_subprocess.assert_not_called()
        mock_restart_service.assert_called_once_with()

    @patch("schedule_management.commands.service._restart_reminder_service")
    @patch("schedule_management.commands.service._has_git_metadata")
    @patch("schedule_management.commands.service._resolve_config_dir")
    def test_update_restart_failure(
        self,
        mock_resolve_config_dir,
        mock_has_git_metadata,
        mock_restart_service,
    ):
        """Test update command when service restart fails."""
        mock_config_dir = MagicMock()
        mock_config_dir.exists.return_value = True
        mock_resolve_config_dir.return_value = mock_config_dir
        mock_has_git_metadata.return_value = False
        mock_restart_service.return_value = (False, "launchctl failed")

        args = MagicMock()
        result = reminder.update_command(args)

        assert result == 1


class TestSwitchCommand:
    """Test the switch command functionality."""

    @patch("schedule_management.commands.service._restart_reminder_service")
    @patch("schedule_management.commands.service._resolve_config_dir")
    def test_switch_updates_active_config_and_restarts_service(
        self,
        mock_resolve_config_dir,
        mock_restart_service,
        tmp_path,
    ):
        config_root = tmp_path / "config"
        (config_root / "user_config_0").mkdir(parents=True)
        (config_root / "user_config_1").mkdir(parents=True)
        mock_resolve_config_dir.return_value = config_root
        mock_restart_service.return_value = (True, "")

        args = MagicMock(config_id="1")
        result = reminder.switch_command(args)

        assert result == 0
        assert (config_root / ".active_config").read_text(encoding="utf-8").strip() == "1"
        mock_restart_service.assert_called_once_with()

    @patch("schedule_management.commands.service._restart_reminder_service")
    @patch("schedule_management.commands.service._resolve_config_dir")
    def test_switch_rejects_invalid_config_id(
        self,
        mock_resolve_config_dir,
        mock_restart_service,
        tmp_path,
    ):
        config_root = tmp_path / "config"
        (config_root / "user_config_0").mkdir(parents=True)
        (config_root / "user_config_2").mkdir(parents=True)
        mock_resolve_config_dir.return_value = config_root

        args = MagicMock(config_id="5")
        with patch("builtins.print") as mock_print:
            result = reminder.switch_command(args)

        assert result == 1
        mock_restart_service.assert_not_called()
        printed = "\n".join(str(call.args[0]) for call in mock_print.call_args_list)
        assert "Invalid config id: 5" in printed
        assert "Valid config ids: 0, 2" in printed


class TestReportCommand:
    """Test the manual report command."""

    @patch("schedule_management.report.generate_manual_report")
    def test_report_weekly_success(self, mock_generate_manual_report):
        """Test successful weekly report generation."""
        mock_generate_manual_report.return_value = Path("/tmp/weekly_report.pdf")

        args = MagicMock(type="weekly", date="2024-02-01", days=7)
        result = reminder.report_command(args)

        assert result == 0
        mock_generate_manual_report.assert_called_once_with(
            "weekly",
            target_date=date(2024, 2, 1),
        )

    @patch("schedule_management.report.generate_manual_report")
    def test_report_monthly_success(self, mock_generate_manual_report):
        """Test successful monthly report generation."""
        mock_generate_manual_report.return_value = Path("/tmp/monthly_report.pdf")

        args = MagicMock(type="monthly", date="2024-02-01", days=None)
        result = reminder.report_command(args)

        assert result == 0
        mock_generate_manual_report.assert_called_once_with(
            "monthly",
            target_date=date(2024, 2, 1),
        )

    def test_report_rejects_invalid_date(self):
        """Test report command with an invalid date string."""
        args = MagicMock(type="weekly", date="2024/02/01", days=None)

        with patch("builtins.print") as mock_print:
            result = reminder.report_command(args)

        assert result == 1
        printed = "\n".join(str(call.args[0]) for call in mock_print.call_args_list)
        assert "Invalid date format" in printed

    def test_report_rejects_custom_weekly_day_range(self):
        """Test report command rejecting unsupported custom weekly ranges."""
        args = MagicMock(type="weekly", date=None, days=14)

        with patch("builtins.print") as mock_print:
            result = reminder.report_command(args)

        assert result == 1
        printed = "\n".join(str(call.args[0]) for call in mock_print.call_args_list)
        assert "Custom day ranges are not supported for weekly reports." in printed

    def test_report_rejects_days_for_monthly(self):
        """Test report command rejecting --days for monthly reports."""
        args = MagicMock(type="monthly", date=None, days=7)

        with patch("builtins.print") as mock_print:
            result = reminder.report_command(args)

        assert result == 1
        printed = "\n".join(str(call.args[0]) for call in mock_print.call_args_list)
        assert "'--days' is not supported for monthly reports." in printed

    @patch("schedule_management.commands.service.open_file")
    @patch("schedule_management.report.generate_manual_report")
    def test_report_opens_file_cross_platform(
        self, mock_generate_manual_report, mock_open_file
    ):
        """rmd report opens the generated PDF via open_file regardless of platform."""
        mock_generate_manual_report.return_value = Path("/tmp/weekly_report.pdf")
        mock_open_file.return_value = True

        args = MagicMock(type="weekly", date=None, days=7)
        result = reminder.report_command(args)

        assert result == 0
        mock_open_file.assert_called_once_with(Path("/tmp/weekly_report.pdf"))


class TestOpenFileHelper:
    """Test the cross-platform open_file helper."""

    @patch("schedule_management.platform.subprocess.Popen")
    @patch("schedule_management.platform.shutil.which")
    @patch("schedule_management.platform.get_platform", return_value="macos")
    def test_open_file_uses_open_on_macos(
        self, mock_platform, mock_which, mock_popen
    ):
        from schedule_management.platform import open_file

        mock_which.return_value = "/usr/bin/open"
        assert open_file("/some/file.pdf") is True
        mock_popen.assert_called_once_with(["open", "/some/file.pdf"])

    @patch("schedule_management.platform.subprocess.Popen")
    @patch("schedule_management.platform.shutil.which")
    @patch("schedule_management.platform.get_platform", return_value="linux")
    def test_open_file_uses_xdg_open_on_linux(
        self, mock_platform, mock_which, mock_popen
    ):
        from schedule_management.platform import open_file

        mock_which.return_value = "/usr/bin/xdg-open"
        assert open_file(Path("/some/file.pdf")) is True
        mock_popen.assert_called_once_with(["xdg-open", "/some/file.pdf"])

    @patch("schedule_management.platform.shutil.which", return_value=None)
    @patch("schedule_management.platform.get_platform", return_value="linux")
    def test_open_file_returns_false_when_no_opener(self, mock_platform, mock_which):
        from schedule_management.platform import open_file

        assert open_file("/some/file.pdf") is False


class TestPlatformSoundDefault:
    """Test the platform-aware default notification sound."""

    @patch("schedule_management.platform.get_platform", return_value="macos")
    def test_sound_file_default_macos(self, _mock_platform):
        from schedule_management.config import ScheduleConfig, _DEFAULT_SOUND_MACOS

        config = ScheduleConfig.__new__(ScheduleConfig)
        config.settings = {}
        assert config.sound_file == _DEFAULT_SOUND_MACOS

    @patch("schedule_management.platform.get_platform", return_value="linux")
    def test_sound_file_default_linux(self, _mock_platform):
        from schedule_management.config import ScheduleConfig, _DEFAULT_SOUND_LINUX

        config = ScheduleConfig.__new__(ScheduleConfig)
        config.settings = {}
        assert config.sound_file == _DEFAULT_SOUND_LINUX

    def test_sound_file_respects_explicit_setting(self):
        from schedule_management.config import ScheduleConfig

        config = ScheduleConfig.__new__(ScheduleConfig)
        config.settings = {"sound_file": "/custom/sound.wav"}
        assert config.sound_file == "/custom/sound.wav"


class TestRunnerSoundExpansion:
    """Test that the macOS sound-path expansion is gated to darwin."""

    @patch.object(schedule_runner, "sys")
    def test_short_sound_not_expanded_on_linux(self, mock_sys):
        from schedule_management.runner import ScheduleRunner

        # Simulate a non-macOS platform
        mock_sys.platform = "linux"

        runner = ScheduleRunner.__new__(ScheduleRunner)
        runner.config = MagicMock()
        runner.config.sound_file = "/usr/share/sounds/freedesktop/stereo/complete.oga"
        runner.config.alarm_interval = 5
        runner.config.max_alarm_duration = 10

        captured: dict = {}
        original_thread = schedule_runner.threading.Thread

        class FakeThread:
            def __init__(self, target=None, args=(), kwargs=None, daemon=False):
                captured["sound_file"] = args[2]

            def start(self):
                pass

        schedule_runner.threading.Thread = FakeThread
        try:
            runner._trigger_alarm("title", "message", sound="Glass")
        finally:
            schedule_runner.threading.Thread = original_thread

        # On Linux the short name must NOT be rewritten to a /System/Library path
        assert captured["sound_file"] == "Glass"

    @patch.object(schedule_runner, "sys")
    def test_short_sound_expanded_on_macos(self, mock_sys):
        from schedule_management.runner import ScheduleRunner

        # Simulate macOS
        mock_sys.platform = "darwin"

        runner = ScheduleRunner.__new__(ScheduleRunner)
        runner.config = MagicMock()
        runner.config.alarm_interval = 5
        runner.config.max_alarm_duration = 10

        captured: dict = {}
        original_thread = schedule_runner.threading.Thread

        class FakeThread:
            def __init__(self, target=None, args=(), kwargs=None, daemon=False):
                captured["sound_file"] = args[2]

            def start(self):
                pass

        schedule_runner.threading.Thread = FakeThread
        try:
            runner._trigger_alarm("title", "message", sound="Glass")
        finally:
            schedule_runner.threading.Thread = original_thread

        assert captured["sound_file"] == "/System/Library/Sounds/Glass.aiff"


class TestViewCommand:
    """Test the view command functionality."""

    @patch("schedule_management.commands.status._schedule_visualizer_class")
    @patch("schedule_management.commands.status.WeeklySchedule")
    @patch("schedule_management.commands.status.ScheduleConfig")
    @patch("schedule_management.commands.status.open_file")
    def test_view_success(
        self, mock_open_file, mock_config, mock_weekly, mock_visualizer_class
    ):
        """Test successful view command."""
        mock_visualizer = MagicMock()
        mock_visualizer_instance = MagicMock()
        mock_visualizer.return_value = mock_visualizer_instance
        mock_visualizer_class.return_value = mock_visualizer
        mock_open_file.return_value = True

        # Configure the mock config to return a proper config_dir
        mock_config_instance = MagicMock()
        mock_config_instance.config_dir = TEST_CONFIG_DIR
        mock_config.return_value = mock_config_instance

        args = MagicMock()
        result = reminder.view_command(args)

        assert result == 0
        mock_visualizer_instance.visualize.assert_called_once()
        mock_open_file.assert_called_once()

    @patch("schedule_management.commands.status._schedule_visualizer_class")
    @patch("schedule_management.commands.status.WeeklySchedule")
    @patch("schedule_management.commands.status.ScheduleConfig")
    def test_view_visualization_error(
        self, mock_config, mock_weekly, mock_visualizer_class
    ):
        """Test view command when visualization fails."""
        mock_visualizer = MagicMock()
        mock_visualizer.side_effect = Exception("Visualization error")
        mock_visualizer_class.return_value = mock_visualizer

        # Configure the mock config to return a proper config_dir
        mock_config_instance = MagicMock()
        mock_config_instance.config_dir = TEST_CONFIG_DIR
        mock_config.return_value = mock_config_instance

        args = MagicMock()
        result = reminder.view_command(args)

        assert result == 1


class TestStatusCommand:
    """Test the status command functionality."""

    @patch("schedule_management.commands.status.get_today_schedule_for_status")
    @patch("schedule_management.commands.status.Console")
    def test_status_normal_day(self, mock_console_class, mock_get_schedule):
        """Test status command on a normal day."""
        mock_get_schedule.return_value = (
            {
                "09:00": "pomodoro",
                "10:00": {"block": "pomodoro", "title": "Focus Task"},
                "21:00": "summary",
            },
            "odd",
            False,
            MagicMock(time_blocks={"pomodoro": 25}, time_points={"summary": "done"}),
        )

        mock_console = MagicMock()
        mock_console_class.return_value = mock_console

        args = MagicMock(verbose=False)
        result = reminder.status_command(args)

        assert result == 0
        # Verify console.print was called
        assert mock_console.print.called

        # Check the calls contain expected content
        print_calls = [str(call) for call in mock_console.print.call_args_list]

        # Look for "Odd Week" in the calls (should be in one of the print calls)
        odd_week_found = any("Odd Week" in call for call in print_calls)
        assert odd_week_found, f"Expected 'Odd Week' in print calls, got: {print_calls}"

    @patch("schedule_management.commands.status.get_today_schedule_for_status")
    @patch("schedule_management.commands.status.Console")
    def test_status_skip_day(self, mock_console_class, mock_get_schedule):
        """Test status command on a skipped day."""
        mock_get_schedule.return_value = ({}, "odd", True, MagicMock())

        mock_console = MagicMock()
        mock_console_class.return_value = mock_console

        args = MagicMock(verbose=False)
        result = reminder.status_command(args)

        assert result == 0
        # Verify console.print was called
        assert mock_console.print.called

        # Verify that a Panel object was printed (indicating skipped day message)
        panel_calls = [
            call
            for call in mock_console.print.call_args_list
            if len(call[0]) > 0
            and hasattr(call[0][0], "__class__")
            and "Panel" in str(type(call[0][0]))
        ]
        assert len(panel_calls) > 0, (
            "Expected a Panel to be printed for skipped day message"
        )

    @patch("schedule_management.commands.status.get_today_schedule_for_status")
    @patch("schedule_management.commands.status.Console")
    def test_status_no_schedule(self, mock_console_class, mock_get_schedule):
        """Test status command when no schedule exists."""
        mock_get_schedule.return_value = (
            {},
            "odd",
            False,
            MagicMock(time_blocks={}, time_points={}),
        )

        mock_console = MagicMock()
        mock_console_class.return_value = mock_console

        args = MagicMock(verbose=False)
        result = reminder.status_command(args)

        assert result == 0
        # Verify console.print was called
        assert mock_console.print.called

        # Verify that a Panel object was printed (status panel)
        panel_calls = [
            call
            for call in mock_console.print.call_args_list
            if len(call[0]) > 0
            and hasattr(call[0][0], "__class__")
            and "Panel" in str(type(call[0][0]))
        ]
        assert len(panel_calls) > 0, "Expected a Panel to be printed for status message"


class TestSyncedScheduleHelpers:
    """Test date-scoped synced schedule helpers."""

    def test_apply_synced_schedule_uses_matching_overlay(self, tmp_path, monkeypatch):
        synced_path = tmp_path / "synced_schedule.toml"
        monkeypatch.setenv("REMINDER_SYNCED_SCHEDULE_PATH", str(synced_path))

        today = date.today()
        weekday = today.strftime("%A").lower()
        save_synced_schedule(
            SyncedDaySchedule(
                target_date=today.isoformat(),
                parity="odd",
                weekday=weekday,
                assignments={
                    "09:00": {"block": "pomodoro", "title": "Write release notes"}
                },
            ),
            synced_path,
        )

        merged = apply_synced_schedule(
            {"09:00": "pomodoro", "12:00": "lunch"},
            target_date=today,
            parity="odd",
            weekday=weekday,
        )

        assert merged["09:00"] == {
            "block": "pomodoro",
            "title": "Write release notes",
        }
        assert merged["12:00"] == "lunch"

    @patch("schedule_management.commands.status.apply_synced_schedule")
    @patch("schedule_management.commands.status.ScheduleConfig")
    @patch("schedule_management.commands.status.WeeklySchedule")
    @patch("schedule_management.commands.status.get_week_parity")
    def test_get_today_schedule_for_status_can_skip_overlay(
        self,
        mock_parity,
        mock_weekly,
        mock_config,
        mock_apply_synced,
    ):
        mock_config_instance = MagicMock()
        mock_config_instance.should_skip_today.return_value = False
        mock_config.return_value = mock_config_instance

        mock_weekly_instance = MagicMock()
        mock_weekly_instance.get_today_schedule.return_value = {"09:00": "pomodoro"}
        mock_weekly.return_value = mock_weekly_instance
        mock_parity.return_value = "odd"

        schedule, parity, is_skipped, _ = reminder.get_today_schedule_for_status(
            apply_sync=False
        )

        assert schedule == {"09:00": "pomodoro"}
        assert parity == "odd"
        assert is_skipped is False
        mock_apply_synced.assert_not_called()


class TestHelperFunctions:
    """Test helper functions used by the CLI commands."""

    def test_get_current_and_next_events_with_schedule(self):
        """Test get_current_and_next_events with a populated schedule."""
        test_schedule = {
            "08:00": "early_task",
            "10:00": {"block": "pomodoro", "title": "Focus Task"},
        }

        current, next_event, time_to_next = reminder.get_current_and_next_events(
            test_schedule
        )

        # Since we don't control the current time in this test,
        # we'll just verify the function doesn't crash and returns expected types
        assert isinstance(current, (str, type(None)))
        assert isinstance(next_event, (str, type(None)))
        assert isinstance(time_to_next, (str, type(None)))

    def test_get_current_and_next_events_empty_schedule(self):
        """Test get_current_and_next_events with empty schedule."""
        current, next_event, time_to_next = reminder.get_current_and_next_events({})

        assert current is None
        assert next_event is None
        assert time_to_next is None

    @patch("schedule_management.commands.status.parse_time")
    @patch("schedule_management.commands.status.datetime")
    def test_get_current_and_next_events_specific_times(
        self, mock_datetime, mock_parse_time
    ):
        """Test get_current_and_next_events with controlled time."""
        # Mock current time as 09:30
        mock_now = MagicMock()
        mock_now.time.return_value = time(9, 30)
        mock_now.strftime.return_value = "09:30"
        mock_now.date.return_value = date(2024, 1, 1)
        mock_datetime.now.return_value = mock_now
        mock_datetime.combine = datetime.combine

        # Mock parse_time to return specific times
        def mock_parse(time_str):
            if time_str == "08:00":
                return time(8, 0)
            elif time_str == "10:00":
                return time(10, 0)
            elif time_str == "14:00":
                return time(14, 0)
            return time(0, 0)

        mock_parse_time.side_effect = mock_parse

        test_schedule = {
            "08:00": "morning_task",
            "10:00": "focus_session",
            "14:00": "afternoon_meeting",
        }

        current, next_event, time_to_next = reminder.get_current_and_next_events(
            test_schedule
        )

        # No current event (non-block events are only active for the trigger minute)
        assert current is None
        assert next_event == "focus_session at 10:00"
        assert time_to_next == "30m"

    @patch("schedule_management.commands.status.parse_time")
    @patch("schedule_management.commands.status.datetime")
    def test_get_current_and_next_events_idle_between_blocks(
        self, mock_datetime, mock_parse_time
    ):
        """Test get_current_and_next_events returns idle between time blocks."""
        # Mock current time as 09:30
        mock_now = MagicMock()
        mock_now.time.return_value = time(9, 30)
        mock_datetime.now.return_value = mock_now
        mock_datetime.combine = datetime.combine

        def mock_parse(time_str):
            if time_str == "08:00":
                return time(8, 0)
            if time_str == "10:00":
                return time(10, 0)
            return time(0, 0)

        mock_parse_time.side_effect = mock_parse

        test_schedule = {
            "08:00": "block_a",
            "10:00": "block_b",
        }
        config = MagicMock(time_blocks={"block_a": 60, "block_b": 30}, time_points={})

        current, next_event, time_to_next = reminder.get_current_and_next_events(
            test_schedule, config
        )

        assert current is None
        assert next_event == "block_b at 10:00"
        assert time_to_next == "30m"

    @patch("schedule_management.commands.status.parse_time")
    @patch("schedule_management.commands.status.datetime")
    def test_get_current_and_next_events_formats_block_titles(
        self, mock_datetime, mock_parse_time
    ):
        """Test status labels include block type and task title."""
        mock_now = MagicMock()
        mock_now.time.return_value = time(9, 30)
        mock_datetime.now.return_value = mock_now
        mock_datetime.combine = datetime.combine

        def mock_parse(time_str):
            if time_str == "10:00":
                return time(10, 0)
            return time(0, 0)

        mock_parse_time.side_effect = mock_parse

        current, next_event, time_to_next = reminder.get_current_and_next_events(
            {"10:00": {"block": "pomodoro", "title": "Finish proposal draft"}},
            MagicMock(time_blocks={"pomodoro": 25}, time_points={}),
        )

        assert current is None
        assert next_event == "pomodoro: Finish proposal draft at 10:00"
        assert time_to_next == "30m"

    @patch("schedule_management.commands.status.ScheduleConfig")
    @patch("schedule_management.commands.status.WeeklySchedule")
    @patch("schedule_management.commands.status.get_week_parity")
    def test_get_today_schedule_for_status_normal(
        self, mock_parity, mock_weekly, mock_config
    ):
        """Test get_today_schedule_for_status on a normal day."""
        mock_config_instance = MagicMock()
        mock_config_instance.should_skip_today.return_value = False
        mock_config_instance.config_dir = TEST_CONFIG_DIR
        mock_config.return_value = mock_config_instance

        mock_weekly_instance = MagicMock()
        mock_weekly_instance.get_today_schedule.return_value = {"09:00": "pomodoro"}
        mock_weekly.return_value = mock_weekly_instance

        mock_parity.return_value = "odd"
        schedule, parity, is_skipped, config = reminder.get_today_schedule_for_status()

        assert schedule == {"09:00": "pomodoro"}
        assert parity == "odd"
        assert is_skipped is False
        assert config is mock_config_instance

    @patch("schedule_management.commands.status.ScheduleConfig")
    @patch("schedule_management.commands.status.WeeklySchedule")
    @patch("schedule_management.commands.status.get_week_parity")
    def test_get_today_schedule_for_status_skipped(
        self, mock_parity, mock_weekly, mock_config
    ):
        """Test get_today_schedule_for_status on a skipped day."""
        mock_config_instance = MagicMock()
        mock_config_instance.should_skip_today.return_value = True
        mock_config_instance.config_dir = TEST_CONFIG_DIR
        mock_config.return_value = mock_config_instance

        mock_parity.return_value = "even"
        schedule, parity, is_skipped, config = reminder.get_today_schedule_for_status()

        assert schedule == {}
        assert parity == "even"
        assert is_skipped is True
        assert config is mock_config_instance


class TestMainFunction:
    """Test the main entry point function."""

    @patch("schedule_management.reminder.status_command")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_with_status_command(self, mock_parse_args, mock_status_command):
        """Test main function with status command."""
        mock_args = MagicMock()
        mock_args.command = "status"
        mock_args.func = mock_status_command
        mock_parse_args.return_value = mock_args
        mock_status_command.return_value = 0

        result = reminder.main()

        assert result == 0
        mock_status_command.assert_called_once_with(mock_args)

    @patch("argparse.ArgumentParser.print_help")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_no_command(self, mock_parse_args, mock_print_help):
        """Test main function with no command specified."""
        mock_args = MagicMock()
        mock_args.command = None
        mock_parse_args.return_value = mock_args

        result = reminder.main()

        assert result == 1
        mock_print_help.assert_called_once()

    @patch("schedule_management.reminder.update_command")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_keyboard_interrupt(self, mock_parse_args, mock_update_command):
        """Test main function handling keyboard interrupt."""
        mock_args = MagicMock()
        mock_args.command = "update"
        mock_args.func = mock_update_command
        mock_parse_args.return_value = mock_args
        mock_update_command.side_effect = KeyboardInterrupt()

        result = reminder.main()

        assert result == 1

    @patch("schedule_management.reminder.add_task")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_with_add_command(self, mock_parse_args, mock_add_task):
        """Test main function with add command."""
        mock_args = MagicMock()
        mock_args.command = "add"
        mock_args.func = mock_add_task
        mock_parse_args.return_value = mock_args
        mock_add_task.return_value = 0

        # Use configured test paths
        result = reminder.main()

        assert result == 0
        mock_add_task.assert_called_once_with(mock_args)

    @patch("schedule_management.reminder.delete_task")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_with_rm_command(self, mock_parse_args, mock_delete_task):
        """Test main function with rm command."""
        mock_args = MagicMock()
        mock_args.command = "rm"
        mock_args.func = mock_delete_task
        mock_parse_args.return_value = mock_args
        mock_delete_task.return_value = 0

        # Use configured test paths
        result = reminder.main()

        assert result == 0
        mock_delete_task.assert_called_once_with(mock_args)

    @patch("schedule_management.reminder.show_tasks")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_with_ls_command(self, mock_parse_args, mock_show_tasks):
        """Test main function with ls command."""
        mock_args = MagicMock()
        mock_args.command = "ls"
        mock_args.func = mock_show_tasks
        mock_parse_args.return_value = mock_args
        mock_show_tasks.return_value = 0

        # Use configured test paths
        result = reminder.main()

        assert result == 0
        mock_show_tasks.assert_called_once_with(mock_args)

    @patch("schedule_management.reminder.view_command")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_with_view_command(self, mock_parse_args, mock_view_command):
        """Test main function with view command."""
        mock_args = MagicMock()
        mock_args.command = "view"
        mock_args.func = mock_view_command
        mock_parse_args.return_value = mock_args
        mock_view_command.return_value = 0

        # Use configured test paths
        result = reminder.main()

        assert result == 0
        mock_view_command.assert_called_once_with(mock_args)

    @patch("schedule_management.reminder.sync_command")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_with_sync_command(self, mock_parse_args, mock_sync_command):
        """Test main function with sync command."""
        mock_args = MagicMock()
        mock_args.command = "sync"
        mock_args.func = mock_sync_command
        mock_parse_args.return_value = mock_args
        mock_sync_command.return_value = 0

        result = reminder.main()

        assert result == 0
        mock_sync_command.assert_called_once_with(mock_args)

    @patch("schedule_management.reminder.completion_command")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_with_completion_command(
        self, mock_parse_args, mock_completion_command
    ):
        """Test main function with completion command."""
        mock_args = MagicMock()
        mock_args.command = "completion"
        mock_args.func = mock_completion_command
        mock_parse_args.return_value = mock_args
        mock_completion_command.return_value = 0

        result = reminder.main()

        assert result == 0
        mock_completion_command.assert_called_once_with(mock_args)


class TestCompletionCommand:
    """Test shell completion command wiring and output."""

    def test_create_parser_with_completion_command(self):
        """Test parser wiring for the completion command."""
        parser = reminder.create_parser()

        args = parser.parse_args(["completion", "zsh"])

        assert args.command == "completion"
        assert args.shell == "zsh"
        assert args.func is reminder.completion_command

    def test_create_parser_completion_defaults_to_bash(self):
        """Test completion command default shell."""
        parser = reminder.create_parser()

        args = parser.parse_args(["completion"])

        assert args.command == "completion"
        assert args.shell == "bash"

    def test_create_parser_with_switch_command(self):
        """Test parser wiring for the switch command."""
        parser = reminder.create_parser()

        args = parser.parse_args(["switch", "2"])

        assert args.command == "switch"
        assert args.config_id == "2"
        assert args.func is reminder.switch_command

    def test_completion_command_prints_bash_script(self, capsys):
        """Test completion command emits a bash completion script."""
        args = MagicMock(shell="bash", parser_factory=reminder.create_parser)

        result = reminder.completion_command(args)
        captured = capsys.readouterr()

        assert result == 0
        assert "AUTOMATICALLY GENERATED by `shtab`" in captured.out
        assert "complete " in captured.out
        assert "rmd" in captured.out


class TestSyncCommand:
    """Test the LLM-backed sync command."""

    @patch("schedule_management.commands.sync._prompt_acceptance", return_value=True)
    @patch("schedule_management.commands.sync.CONSOLE")
    @patch("schedule_management.commands.sync.LLMClient")
    @patch("schedule_management.commands.sync.ensure_llm_config")
    @patch("schedule_management.commands.sync._load_ranked_tasks")
    @patch("schedule_management.commands.sync._get_base_today_schedule")
    def test_sync_command_accepts_preview_and_writes_overlay(
        self,
        mock_get_schedule,
        mock_load_ranked_tasks,
        mock_ensure_llm_config,
        mock_llm_client_class,
        mock_console,
        _mock_prompt_acceptance,
        tmp_path,
        monkeypatch,
    ):
        monkeypatch.setenv(
            "REMINDER_SYNCED_SCHEDULE_PATH", str(tmp_path / "synced_schedule.toml")
        )

        mock_console.status.return_value.__enter__.return_value = None
        mock_console.status.return_value.__exit__.return_value = False
        mock_get_schedule.return_value = (
            {
                "09:00": "pomodoro",
                "10:00": "potato",
                "12:00": "lunch",
            },
            "odd",
            False,
            MagicMock(time_blocks={"pomodoro": 25, "potato": 50}),
        )
        mock_load_ranked_tasks.return_value = [
            {"description": "Finish release notes", "priority": 9},
            {"description": "Review pull request", "priority": 7},
        ]
        mock_ensure_llm_config.return_value = MagicMock()
        mock_llm_client = MagicMock()
        mock_llm_client.generate.return_value = json.dumps(
            {
                "summary": "Urgent writing work goes first.",
                "assignments": {
                    "09:00": "Finish release notes",
                    "10:00": "Review pull request",
                },
            }
        )
        mock_llm_client_class.return_value = mock_llm_client

        result = reminder.sync_command(MagicMock())

        assert result == 0
        saved_text = (tmp_path / "synced_schedule.toml").read_text(encoding="utf-8")
        assert 'title = "Finish release notes"' in saved_text
        assert 'title = "Review pull request"' in saved_text

    @patch("schedule_management.commands.sync._prompt_rejection_reason")
    @patch("schedule_management.commands.sync._prompt_acceptance")
    @patch("schedule_management.commands.sync.CONSOLE")
    @patch("schedule_management.commands.sync.LLMClient")
    @patch("schedule_management.commands.sync.ensure_llm_config")
    @patch("schedule_management.commands.sync._load_ranked_tasks")
    @patch("schedule_management.commands.sync._get_base_today_schedule")
    def test_sync_command_retries_with_rejection_feedback(
        self,
        mock_get_schedule,
        mock_load_ranked_tasks,
        mock_ensure_llm_config,
        mock_llm_client_class,
        mock_console,
        mock_prompt_acceptance,
        mock_prompt_reason,
        tmp_path,
        monkeypatch,
    ):
        monkeypatch.setenv(
            "REMINDER_SYNCED_SCHEDULE_PATH", str(tmp_path / "synced_schedule.toml")
        )

        mock_console.status.return_value.__enter__.return_value = None
        mock_console.status.return_value.__exit__.return_value = False
        mock_get_schedule.return_value = (
            {
                "09:00": "pomodoro",
                "10:00": "pomodoro",
            },
            "odd",
            False,
            MagicMock(time_blocks={"pomodoro": 25}),
        )
        mock_load_ranked_tasks.return_value = [
            {"description": "Ship urgent bug fix", "priority": 10},
            {"description": "Clean backlog", "priority": 4},
        ]
        mock_ensure_llm_config.return_value = MagicMock()
        mock_prompt_acceptance.side_effect = [False, True]
        mock_prompt_reason.return_value = "Put the urgent bug fix first."

        mock_llm_client = MagicMock()
        mock_llm_client.generate.side_effect = [
            json.dumps(
                {
                    "summary": "First draft.",
                    "assignments": {
                        "09:00": "Clean backlog",
                        "10:00": "Ship urgent bug fix",
                    },
                }
            ),
            json.dumps(
                {
                    "summary": "Adjusted to prioritize the urgent work.",
                    "assignments": {
                        "09:00": "Ship urgent bug fix",
                        "10:00": "Clean backlog",
                    },
                }
            ),
        ]
        mock_llm_client_class.return_value = mock_llm_client

        result = reminder.sync_command(MagicMock())

        assert result == 0
        assert mock_llm_client.generate.call_count == 2
        second_user_prompt = mock_llm_client.generate.call_args_list[1].args[1]
        assert "Put the urgent bug fix first." in second_user_prompt

        saved_text = (tmp_path / "synced_schedule.toml").read_text(encoding="utf-8")
        assert 'title = "Ship urgent bug fix"' in saved_text
        assert 'title = "Clean backlog"' in saved_text


class TestTaskManagement:
    """Test the task management functionality."""

    def setup_method(self):
        """Set up test fixtures before each test method."""
        # Create a temporary tasks file for testing using helper function
        # Use the test config directory for the tasks file
        self.test_tasks_file = Path(TEST_CONFIG_DIR) / "tasks" / "tasks.json"
        self.original_tasks_content = None

        # Backup original tasks file if it exists
        if self.test_tasks_file.exists():
            with open(self.test_tasks_file, "r", encoding="utf-8") as f:
                self.original_tasks_content = f.read()

    def teardown_method(self):
        """Clean up after each test method."""
        # Restore original tasks file if it existed
        if self.original_tasks_content is not None:
            with open(self.test_tasks_file, "w", encoding="utf-8") as f:
                f.write(self.original_tasks_content)
        elif self.test_tasks_file.exists():
            # Remove the test file if it was created during testing
            self.test_tasks_file.unlink()

    def test_load_tasks_empty_file(self):
        """Test loading tasks from a non-existent or empty file."""
        # Ensure the file doesn't exist
        if self.test_tasks_file.exists():
            self.test_tasks_file.unlink()

        tasks = reminder.load_tasks()
        assert tasks == []

    def test_load_tasks_invalid_json(self):
        """Test loading tasks from a file with invalid JSON."""
        # Create a file with invalid JSON
        with open(self.test_tasks_file, "w", encoding="utf-8") as f:
            f.write('{"invalid": json}')

        tasks = reminder.load_tasks()
        assert tasks == []

    def test_save_and_load_tasks(self):
        """Test saving and then loading tasks."""
        test_tasks = [
            {"description": "Test task 1", "priority": 5},
            {"description": "Test task 2", "priority": 8},
        ]

        reminder.save_tasks(test_tasks)
        loaded_tasks = reminder.load_tasks()

        assert loaded_tasks == test_tasks

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_success(self, mock_save_tasks, mock_load_tasks):
        """Test adding a new task successfully."""
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.task = "Complete project"
        args.priority = 7

        # Use configured test paths
        result = reminder.add_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the task was added
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Complete project"
        assert saved_tasks[0]["priority"] == 7

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_with_type_success(self, mock_save_tasks, mock_load_tasks):
        """Test adding a task with a specific task_type successfully."""
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.task = "Workout"
        args.priority = 6
        args.task_type = 2

        result = reminder.add_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Workout"
        assert saved_tasks[0]["priority"] == 6
        assert saved_tasks[0]["type"] == "2"

    @patch("schedule_management.commands.tasks.show_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_add_task_does_not_display_list_by_default(
        self,
        mock_load_tasks,
        mock_save_tasks,
        mock_show_tasks,
        tmp_path,
        monkeypatch,
    ):
        """Test that adding a task keeps the current quiet output by default."""
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None
        monkeypatch.setattr(
            "schedule_management.commands.tasks.SETTINGS_PATH",
            str(tmp_path / "missing-settings.toml"),
        )

        args = MagicMock()
        args.task = "Complete project"
        args.priority = 7
        args.postpone = None

        result = reminder.add_task(args)

        assert result == 0
        mock_show_tasks.assert_not_called()

    @patch("schedule_management.commands.tasks.show_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_add_task_displays_list_when_config_enabled(
        self,
        mock_load_tasks,
        mock_save_tasks,
        mock_show_tasks,
        tmp_path,
        monkeypatch,
    ):
        """Test that add displays the task list after a successful save when enabled."""
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None
        settings_path = tmp_path / "settings.toml"
        settings_path.write_text(
            "[settings]\nshow_tasks_after_change = true\n",
            encoding="utf-8",
        )
        monkeypatch.setattr(
            "schedule_management.commands.tasks.SETTINGS_PATH",
            str(settings_path),
        )

        args = MagicMock()
        args.task = "Complete project"
        args.priority = 7
        args.postpone = None

        result = reminder.add_task(args)

        assert result == 0
        mock_show_tasks.assert_called_once_with(args)

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_duplicate(self, mock_save_tasks, mock_load_tasks):
        """Test adding a duplicate task updates the existing one."""
        existing_tasks = [
            {"description": "Complete project", "priority": 5},
            {"description": "Review code", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.task = "Complete project"
        args.priority = 9  # New priority

        # Use configured test paths
        result = reminder.add_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the task was updated
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 2
        updated_task = next(
            t for t in saved_tasks if t["description"] == "Complete project"
        )
        assert updated_task["priority"] == 9

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_negative_priority(self, mock_save_tasks, mock_load_tasks):
        """Test adding a task with negative priority fails."""
        args = MagicMock()
        args.task = "Complete project"
        args.priority = -1

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.add_task(args)

        assert result == 1
        mock_save_tasks.assert_not_called()
        mock_print.assert_called_once_with(
            "❌ Error: Priority must be a positive integer"
        )

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_zero_priority(self, mock_save_tasks, mock_load_tasks):
        """Test adding a task with zero priority fails."""
        args = MagicMock()
        args.task = "Complete project"
        args.priority = 0

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.add_task(args)

        assert result == 1
        mock_save_tasks.assert_not_called()
        mock_print.assert_called_once_with(
            "❌ Error: Priority must be a positive integer"
        )

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_save_error(self, mock_save_tasks, mock_load_tasks):
        """Test handling error when saving tasks fails."""
        mock_load_tasks.return_value = []
        mock_save_tasks.side_effect = Exception("Save failed")

        args = MagicMock()
        args.task = "Complete project"
        args.priority = 5

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.add_task(args)

        assert result == 1
        mock_print.assert_called_once()
        assert "❌ Error saving task:" in mock_print.call_args[0][0]

    def test_save_new_task_type_persists_to_settings(self, tmp_path):
        """_save_new_task_type writes a new type into settings.toml without clobbering it."""
        from schedule_management.commands.tasks import _save_new_task_type

        settings = tmp_path / "settings.toml"
        settings.write_text(
            '[settings]\nlanguage = "en"\n\n[task_types]\n1 = "read papers"\n',
            encoding="utf-8",
        )

        ok = _save_new_task_type(str(settings), "5", "writing")
        assert ok is True

        from schedule_management.config import ScheduleConfig

        config = ScheduleConfig(str(settings))
        assert config.task_types["1"] == "read papers"
        assert config.task_types["5"] == "writing"
        assert config.settings.get("language") == "en"

    def test_save_new_task_type_overwrites_existing_name(self, tmp_path):
        """Saving an existing type id replaces its name."""
        from schedule_management.commands.tasks import _save_new_task_type
        from schedule_management.config import ScheduleConfig

        settings = tmp_path / "settings.toml"
        settings.write_text(
            '[task_types]\n1 = "read papers"\n2 = "gym work"\n',
            encoding="utf-8",
        )

        ok = _save_new_task_type(str(settings), "2", "yoga")
        assert ok is True

        config = ScheduleConfig(str(settings))
        assert config.task_types["2"] == "yoga"

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_with_postpone_success(self, mock_save_tasks, mock_load_tasks):
        """Test adding a task with a valid positive postpone argument."""
        from datetime import datetime, timedelta
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.task = "Postponed project"
        args.priority = 9
        args.postpone = 2

        result = reminder.add_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Postponed project"
        assert saved_tasks[0]["priority"] == 9
        
        expected_date = (datetime.now().date() + timedelta(days=2)).isoformat()
        assert saved_tasks[0]["alarm_from"] == expected_date

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_with_negative_postpone(self, mock_save_tasks, mock_load_tasks):
        """Test adding a task with negative postpone fails validation."""
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.task = "Postponed project"
        args.priority = 9
        args.postpone = -1

        with patch("builtins.print") as mock_print:
            result = reminder.add_task(args)

        assert result == 1
        mock_save_tasks.assert_not_called()
        mock_print.assert_called_once_with(
            "❌ Error: Postpone days must be a non-negative integer"
        )

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_with_zero_postpone(self, mock_save_tasks, mock_load_tasks):
        """Test adding a task with postpone=0 works and does not set alarm_from."""
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.task = "Non-postponed project"
        args.priority = 9
        args.postpone = 0

        result = reminder.add_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert "alarm_from" not in saved_tasks[0]


    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    def test_add_task_missing_params_non_interactive(self, mock_isatty):
        """Test adding a task with missing parameters in non-interactive environment fails."""
        mock_isatty.return_value = False

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 1

    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_missing_params_interactive_success(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty
    ):
        """Test successfully prompting for missing parameters in interactive environment."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        # Simulate user entering task description first, priority, then task type
        mock_console_input.side_effect = ["Buy groceries", "5", "2"]

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 0

        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Buy groceries"
        assert saved_tasks[0]["priority"] == 5
        assert saved_tasks[0]["type"] == "2"

    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_missing_priority_interactive_success(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty
    ):
        """Test prompting only for priority when task description is provided."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        # Simulate user entering priority, then task type
        mock_console_input.side_effect = ["8", "1"]

        args = MagicMock()
        args.task = "Study science"
        args.priority = None

        result = reminder.add_task(args)
        assert result == 0

        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Study science"
        assert saved_tasks[0]["priority"] == 8
        assert saved_tasks[0]["type"] == "1"

    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    def test_add_task_interactive_user_cancel(self, mock_console_input, mock_isatty):
        """Test interactive prompting when user cancels (KeyboardInterrupt)."""
        mock_isatty.return_value = True
        mock_console_input.side_effect = KeyboardInterrupt()

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 1

    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_invalid_inputs_retry(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty
    ):
        """Test retrying on invalid input (empty description, invalid priority)."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        # 1. empty description, then valid description
        # 2. invalid priority (string), invalid priority (out of range), then valid priority
        # 3. non-numeric task type (rejected), then valid task type
        mock_console_input.side_effect = [
            "",
            "Valid Task",
            "abc",
            "12",
            "6",
            "x",
            "3"
        ]

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 0

        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Valid Task"
        assert saved_tasks[0]["priority"] == 6
        assert saved_tasks[0]["type"] == "3"


    @patch("schedule_management.commands.tasks._save_new_task_type")
    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_creates_new_task_type(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty,
        mock_save_new_type
    ):
        """Entering an unlisted number starts new-type creation and uses it."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None
        mock_save_new_type.return_value = True

        # description, priority, new type number 5, its name, then empty postpone (= 0)
        mock_console_input.side_effect = ["Write novel", "7", "5", "writing", ""]

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 0

        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert saved_tasks[0]["type"] == "5"

        # The new type should have been persisted under id "5" with the given name.
        mock_save_new_type.assert_called_once()
        save_args = mock_save_new_type.call_args[0]
        assert save_args[1] == "5"
        assert save_args[2] == "writing"

    @patch("schedule_management.commands.tasks._save_new_task_type")
    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_new_type_retries_empty_name(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty,
        mock_save_new_type
    ):
        """An empty type name is rejected; creation proceeds once a name is given."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None
        mock_save_new_type.return_value = True

        # type number 6, empty name (retry), then valid name, empty postpone (= 0)
        mock_console_input.side_effect = ["Read book", "5", "6", "", "reading", ""]

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 0

        saved_tasks = mock_save_tasks.call_args[0][0]
        assert saved_tasks[0]["type"] == "6"
        assert mock_save_new_type.call_args[0][2] == "reading"

    @patch("schedule_management.commands.tasks._save_new_task_type")
    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_new_type_cancel_falls_back_to_prompt(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty,
        mock_save_new_type
    ):
        """Cancelling new-type creation returns to the type selection prompt."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None
        mock_save_new_type.return_value = True

        # type number 7, then cancel name entry (KeyboardInterrupt),
        # then pick existing type 2, then empty postpone (= 0)
        mock_console_input.side_effect = ["Plan trip", "5", "7", KeyboardInterrupt(), "2", ""]

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 0

        saved_tasks = mock_save_tasks.call_args[0][0]
        assert saved_tasks[0]["type"] == "2"
        mock_save_new_type.assert_not_called()

    @patch("schedule_management.commands.tasks._save_new_task_type")
    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_new_type_persist_failure_uses_type_anyway(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty,
        mock_save_new_type
    ):
        """If the new type can't be saved, it is still applied to the current task."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None
        mock_save_new_type.return_value = False

        # type number 9, name, then empty postpone (= 0)
        mock_console_input.side_effect = ["Sketch", "5", "9", "drawing", ""]

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 0

        saved_tasks = mock_save_tasks.call_args[0][0]
        assert saved_tasks[0]["type"] == "9"

    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_new_type_rejects_non_numeric(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty
    ):
        """Non-numeric input at the type prompt is still rejected."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        # type "abc" (rejected), then valid type 1, then empty postpone (= 0)
        mock_console_input.side_effect = [" Jog", "5", "abc", "1", ""]

        args = MagicMock()
        args.task = None
        args.priority = None

        result = reminder.add_task(args)
        assert result == 0

        saved_tasks = mock_save_tasks.call_args[0][0]
        assert saved_tasks[0]["type"] == "1"


    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_postpone_default_no_alarm(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty
    ):
        """Empty postpone prompt keeps the task as a normal (non-future) task."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        # description, priority, task type, then empty postpone (= 0)
        mock_console_input.side_effect = ["Read book", "6", "1", ""]

        args = MagicMock()
        args.task = None
        args.priority = None
        args.postpone = None

        result = reminder.add_task(args)
        assert result == 0

        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert "alarm_from" not in saved_tasks[0]

    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_postpone_sets_alarm_from(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty
    ):
        """Entering a positive postpone day makes it a future task with alarm_from."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        # description, priority, task type, then postpone 2 days
        mock_console_input.side_effect = ["Future task", "7", "1", "2"]

        args = MagicMock()
        args.task = None
        args.priority = None
        args.postpone = None

        from datetime import datetime, timedelta
        expected_alarm_from = (datetime.now().date() + timedelta(days=2)).isoformat()

        result = reminder.add_task(args)
        assert result == 0

        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["alarm_from"] == expected_alarm_from

    @patch("schedule_management.commands.tasks.sys.stdin.isatty")
    @patch("rich.console.Console.input")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_add_task_interactive_postpone_retries_on_invalid_input(
        self, mock_save_tasks, mock_load_tasks, mock_console_input, mock_isatty
    ):
        """Invalid postpone input is retried until a valid value is entered."""
        mock_isatty.return_value = True
        mock_load_tasks.return_value = []
        mock_save_tasks.return_value = None

        # task type, then: non-numeric, negative, valid 3
        mock_console_input.side_effect = ["Plan trip", "5", "1", "abc", "-2", "3"]

        args = MagicMock()
        args.task = None
        args.priority = None
        args.postpone = None

        from datetime import datetime, timedelta
        expected_alarm_from = (datetime.now().date() + timedelta(days=3)).isoformat()

        result = reminder.add_task(args)
        assert result == 0

        saved_tasks = mock_save_tasks.call_args[0][0]
        assert saved_tasks[0]["alarm_from"] == expected_alarm_from

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_task_success(self, mock_save_tasks, mock_load_tasks):
        """Test deleting an existing task successfully."""
        existing_tasks = [
            {"description": "Complete project", "priority": 7},
            {"description": "Review code", "priority": 3},
            {"description": "Write documentation", "priority": 5},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Review code"]

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the task was removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 2
        assert not any(t["description"] == "Review code" for t in saved_tasks)

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_task_not_found(self, mock_save_tasks, mock_load_tasks):
        """Test deleting a non-existent task."""
        existing_tasks = [
            {"description": "Complete project", "priority": 7},
            {"description": "Review code", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Non-existent task"]

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_save_tasks.assert_not_called()
        mock_print.assert_called_once_with("❌ Task 'Non-existent task' not found")

    @patch("schedule_management.commands.tasks.load_tasks")
    def test_delete_task_empty_list(self, mock_load_tasks):
        """Test deleting a task from an empty list."""
        mock_load_tasks.return_value = []

        args = MagicMock()
        args.tasks = ["Any task"]

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_print.assert_called_once_with("⚠️  No tasks found to delete")

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_task_multiple_occurrences(self, mock_save_tasks, mock_load_tasks):
        """Test deleting multiple tasks with the same description."""
        existing_tasks = [
            {"description": "Review code", "priority": 7},
            {"description": "Write documentation", "priority": 3},
            {
                "description": "Review code",
                "priority": 5,
            },  # Same description, different priority
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Review code"]

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify both tasks with the same description were removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Write documentation"

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_task_save_error(self, mock_save_tasks, mock_load_tasks):
        """Test handling error when saving tasks fails after deletion."""
        existing_tasks = [{"description": "Complete project", "priority": 7}]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.side_effect = Exception("Save failed")

        args = MagicMock()
        args.tasks = ["Complete project"]

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_print.assert_called_once()
        assert "❌ Error saving tasks:" in mock_print.call_args[0][0]

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_task_by_id_success(self, mock_save_tasks, mock_load_tasks):
        """Test deleting a task by ID successfully."""
        existing_tasks = [
            {"description": "High priority task", "priority": 9},
            {"description": "Medium priority task", "priority": 5},
            {"description": "Low priority task", "priority": 2},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = [
            "2"
        ]  # ID 2 should be "Medium priority task" after sorting by priority

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the task was removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 2
        assert not any(t["description"] == "Medium priority task" for t in saved_tasks)
        # High priority task should still be there
        assert any(t["description"] == "High priority task" for t in saved_tasks)
        # Low priority task should still be there
        assert any(t["description"] == "Low priority task" for t in saved_tasks)

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_task_by_id_first_item(self, mock_save_tasks, mock_load_tasks):
        """Test deleting the first task (highest priority) by ID."""
        existing_tasks = [
            {"description": "High priority task", "priority": 9},
            {"description": "Medium priority task", "priority": 5},
            {"description": "Low priority task", "priority": 2},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["1"]  # ID 1 should be "High priority task" after sorting

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the task was removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 2
        assert not any(t["description"] == "High priority task" for t in saved_tasks)

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_task_by_id_last_item(self, mock_save_tasks, mock_load_tasks):
        """Test deleting the last task (lowest priority) by ID."""
        existing_tasks = [
            {"description": "High priority task", "priority": 9},
            {"description": "Medium priority task", "priority": 5},
            {"description": "Low priority task", "priority": 2},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["3"]  # ID 3 should be "Low priority task" after sorting

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the task was removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 2
        assert not any(t["description"] == "Low priority task" for t in saved_tasks)

    @patch("schedule_management.commands.tasks.load_tasks")
    def test_delete_task_by_id_invalid_id_too_high(self, mock_load_tasks):
        """Test deleting a task with ID that's too high."""
        existing_tasks = [
            {"description": "Task 1", "priority": 5},
            {"description": "Task 2", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks

        args = MagicMock()
        args.tasks = ["5"]  # Invalid ID

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_print.assert_called_once()
        assert "Invalid task ID: 5" in mock_print.call_args[0][0]
        assert "Please use a number between 1 and 2" in mock_print.call_args[0][0]

    @patch("schedule_management.commands.tasks.load_tasks")
    def test_delete_task_by_id_invalid_id_zero(self, mock_load_tasks):
        """Test deleting a task with ID of zero."""
        existing_tasks = [
            {"description": "Task 1", "priority": 5},
            {"description": "Task 2", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks

        args = MagicMock()
        args.tasks = ["0"]  # Invalid ID

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_print.assert_called_once()
        assert "Invalid task ID: 0" in mock_print.call_args[0][0]

    @patch("schedule_management.commands.tasks.load_tasks")
    def test_delete_task_by_id_invalid_id_negative(self, mock_load_tasks):
        """Test deleting a task with negative ID."""
        existing_tasks = [
            {"description": "Task 1", "priority": 5},
            {"description": "Task 2", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks

        args = MagicMock()
        args.tasks = ["-1"]  # Invalid ID

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_print.assert_called_once()
        assert "Invalid task ID: -1" in mock_print.call_args[0][0]

    @patch("schedule_management.commands.tasks.load_tasks")
    def test_delete_task_by_id_empty_list(self, mock_load_tasks):
        """Test deleting a task by ID from an empty list."""
        mock_load_tasks.return_value = []

        args = MagicMock()
        args.tasks = ["1"]

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_print.assert_called_once_with("⚠️  No tasks found to delete")

    @patch("schedule_management.commands.tasks.load_tasks")
    def test_delete_task_by_id_numeric_string_fallback(self, mock_load_tasks):
        """Test that numeric string descriptions are treated as IDs first, not descriptions."""
        existing_tasks = [
            {
                "description": "123",
                "priority": 9,
            },  # Task description is a numeric string
            {"description": "Regular task", "priority": 5},
        ]
        mock_load_tasks.return_value = existing_tasks

        args = MagicMock()
        args.tasks = [
            "123"
        ]  # This should be treated as an ID first (invalid in this case)

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        # Should treat "123" as an ID (invalid since there are only 2 tasks)
        mock_print.assert_called_once()
        assert "Invalid task ID: 123" in mock_print.call_args[0][0]
        assert "Please use a number between 1 and 2" in mock_print.call_args[0][0]

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_task_by_id_with_valid_id_as_string(
        self, mock_save_tasks, mock_load_tasks
    ):
        """Test that valid numeric IDs work even when passed as strings."""
        existing_tasks = [
            {"description": "High priority task", "priority": 9},
            {"description": "Regular task", "priority": 5},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["1"]  # Valid ID as string

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Should delete the task with ID 1 (highest priority)
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Regular task"

    # New tests for multi-argument delete functionality
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_multiple_tasks_success(self, mock_save_tasks, mock_load_tasks):
        """Test deleting multiple tasks successfully by description."""
        existing_tasks = [
            {"description": "Complete project", "priority": 9},
            {"description": "Review code", "priority": 7},
            {"description": "Write documentation", "priority": 5},
            {"description": "Test application", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Review code", "Write documentation"]

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the tasks were removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 2
        assert not any(t["description"] == "Review code" for t in saved_tasks)
        assert not any(t["description"] == "Write documentation" for t in saved_tasks)
        # Verify other tasks remain
        assert any(t["description"] == "Complete project" for t in saved_tasks)
        assert any(t["description"] == "Test application" for t in saved_tasks)

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_multiple_tasks_by_ids(self, mock_save_tasks, mock_load_tasks):
        """Test deleting multiple tasks successfully by IDs."""
        existing_tasks = [
            {"description": "High priority task", "priority": 9},
            {"description": "Medium priority task", "priority": 7},
            {"description": "Low priority task", "priority": 5},
            {"description": "Very low priority task", "priority": 2},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["1", "3"]  # Delete highest and lowest priority tasks

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the tasks were removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 2
        assert not any(t["description"] == "High priority task" for t in saved_tasks)
        assert not any(t["description"] == "Low priority task" for t in saved_tasks)
        # Verify other tasks remain
        assert any(t["description"] == "Medium priority task" for t in saved_tasks)
        assert any(t["description"] == "Very low priority task" for t in saved_tasks)

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_multiple_tasks_mixed_ids_and_descriptions(
        self, mock_save_tasks, mock_load_tasks
    ):
        """Test deleting multiple tasks using mixed IDs and descriptions."""
        existing_tasks = [
            {"description": "High priority task", "priority": 9},
            {"description": "Code review", "priority": 7},
            {"description": "Documentation", "priority": 5},
            {"description": "Testing", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["1", "Documentation", "4"]  # Mix of ID and description

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify the tasks were removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Code review"

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_multiple_tasks_partial_success(
        self, mock_save_tasks, mock_load_tasks
    ):
        """Test deleting multiple tasks with some successes and some failures."""
        existing_tasks = [
            {"description": "Complete project", "priority": 9},
            {"description": "Review code", "priority": 7},
            {"description": "Write documentation", "priority": 5},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Review code", "Non-existent task", "Complete project"]

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1  # Should return 1 due to some failures
        mock_save_tasks.assert_called_once()
        # Verify the valid tasks were removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Write documentation"
        # Verify error was printed for non-existent task
        mock_print.assert_any_call("❌ Task 'Non-existent task' not found")

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_multiple_tasks_all_fail(self, mock_save_tasks, mock_load_tasks):
        """Test deleting multiple tasks where none exist."""
        existing_tasks = [
            {"description": "Complete project", "priority": 9},
            {"description": "Review code", "priority": 7},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Non-existent task 1", "Non-existent task 2"]

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_save_tasks.assert_not_called()
        # Verify errors were printed for both non-existent tasks
        expected_calls = [
            "❌ Task 'Non-existent task 1' not found",
            "❌ Task 'Non-existent task 2' not found",
        ]
        for expected_call in expected_calls:
            mock_print.assert_any_call(expected_call)

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_multiple_tasks_invalid_ids(self, mock_save_tasks, mock_load_tasks):
        """Test deleting multiple tasks with some invalid IDs."""
        existing_tasks = [
            {"description": "Task 1", "priority": 5},
            {"description": "Task 2", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["1", "5", "-1", "2"]  # Mix of valid and invalid IDs

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1  # Should return 1 due to invalid IDs
        mock_save_tasks.assert_called_once()
        # Verify the valid tasks were removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 0
        # Verify errors were printed for invalid IDs
        mock_print.assert_any_call(
            "❌ Invalid task ID: 5. Please use a number between 1 and 2"
        )
        mock_print.assert_any_call(
            "❌ Invalid task ID: -1. Please use a number between 1 and 2"
        )

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_multiple_tasks_with_duplicate_occurrences(
        self, mock_save_tasks, mock_load_tasks
    ):
        """Test deleting multiple tasks when some have duplicate descriptions."""
        existing_tasks = [
            {"description": "Review code", "priority": 7},
            {"description": "Write documentation", "priority": 5},
            {"description": "Review code", "priority": 3},  # Duplicate description
            {"description": "Testing", "priority": 2},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Review code", "Testing"]

        # Use configured test paths
        result = reminder.delete_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        # Verify all tasks with matching descriptions were removed
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Write documentation"
        # Both "Review code" tasks should be removed

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.save_tasks")
    def test_delete_multiple_tasks_save_error(self, mock_save_tasks, mock_load_tasks):
        """Test handling error when saving tasks fails after multiple deletions."""
        existing_tasks = [
            {"description": "Task 1", "priority": 7},
            {"description": "Task 2", "priority": 5},
            {"description": "Task 3", "priority": 3},
        ]
        mock_load_tasks.return_value = existing_tasks
        mock_save_tasks.side_effect = Exception("Save failed")

        args = MagicMock()
        args.tasks = ["Task 1", "Task 3"]

        with patch("builtins.print") as mock_print:
            # Use configured test paths
            result = reminder.delete_task(args)

        assert result == 1
        mock_print.assert_called_once()
        assert "❌ Error saving tasks:" in mock_print.call_args[0][0]

    @patch("schedule_management.commands.tasks.load_tasks")
    def test_show_tasks_empty(self, mock_load_tasks):
        """Test showing tasks when there are no tasks."""
        mock_load_tasks.return_value = []

        args = MagicMock()

        with patch("schedule_management.commands.tasks.Console") as mock_console_class:
            mock_console = MagicMock()
            mock_console_class.return_value = mock_console

            # Use configured test paths
            result = reminder.show_tasks(args)

        assert result == 0
        # Verify console.print was called with the expected message
        mock_console.print.assert_called_once()
        call_args = mock_console.print.call_args[0][0]
        assert "No tasks found" in call_args or "📋" in call_args

    @patch("schedule_management.commands.tasks.load_tasks")
    def test_show_tasks_sorted_by_importance(self, mock_load_tasks):
        """Test that tasks are displayed sorted by importance (descending)."""
        test_tasks = [
            {"description": "Task 1", "priority": 3},
            {"description": "Task 2", "priority": 7},
            {"description": "Task 3", "priority": 1},
        ]
        mock_load_tasks.return_value = test_tasks

        args = MagicMock()

        with patch("schedule_management.commands.tasks.Console") as mock_console_class:
            mock_console = MagicMock()
            mock_console_class.return_value = mock_console

            # Use configured test paths
            result = reminder.show_tasks(args)

        assert result == 0
        # Verify console.print was called (should be called twice - once for table, once for total)
        assert mock_console.print.call_count >= 1

        # Verify that a Table object was printed (which should contain the tasks)
        table_calls = [
            call
            for call in mock_console.print.call_args_list
            if len(call[0]) > 0
            and hasattr(call[0][0], "__class__")
            and "Table" in str(type(call[0][0]))
        ]
        assert len(table_calls) > 0, "Expected a Table to be printed for task list"

        # Also verify the total tasks count was printed
        total_calls = [
            call
            for call in mock_console.print.call_args_list
            if len(call[0]) > 0 and "Total tasks" in str(call[0][0])
        ]
        assert len(total_calls) > 0, "Expected total tasks count to be printed"

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.load_procrastinate_records")
    @patch("schedule_management.commands.tasks.Table")
    def test_show_tasks_marks_procrastinated_tasks(
        self,
        mock_table_class,
        mock_load_procrastinate_records,
        mock_load_tasks,
    ):
        """Test that procrastinated tasks are prefixed and styled differently."""
        from datetime import datetime

        mock_load_tasks.return_value = [
            {"description": "Normal task", "priority": 9},
            {"description": "Delayed task", "priority": 8},
        ]
        mock_load_procrastinate_records.return_value = {
            "Delayed task": {
                "description": "Delayed task",
                "since": "2026-04-05",
            }
        }

        mock_table = MagicMock()
        mock_table_class.return_value = mock_table

        args = MagicMock()

        with patch("schedule_management.commands.tasks.Console") as mock_console_class:
            mock_console = MagicMock()
            mock_console_class.return_value = mock_console
            with patch("schedule_management.commands.tasks.datetime") as mock_datetime:
                mock_datetime.now.return_value = datetime(2026, 4, 8, 9, 0)
                result = reminder.show_tasks(args)

        assert result == 0
        description_cells = [
            call.args[2]
            for call in mock_table.add_row.call_args_list
            if len(call.args) >= 3
        ]

        procrastinated_cells = [
            cell
            for cell in description_cells
            if hasattr(cell, "plain") and cell.plain.startswith("⏳ ")
        ]
        assert len(procrastinated_cells) == 1
        # Strip zero-width type tag suffix for comparison
        clean_plain = procrastinated_cells[0].plain.split("\u2060")[0]
        assert clean_plain == "⏳ Delayed task (3 days overdue)"
        assert procrastinated_cells[0].style == "bold red"

    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.load_procrastinate_records")
    @patch("schedule_management.commands.tasks.Table")
    def test_show_tasks_marks_postponed_tasks(
        self,
        mock_table_class,
        mock_load_procrastinate_records,
        mock_load_tasks,
    ):
        """Test that future postponed tasks are prefixed and styled with sleep emoji and days left."""
        from datetime import datetime

        mock_load_tasks.return_value = [
            {"description": "Task tomorrow", "priority": 9, "alarm_from": "2026-04-09"},
            {"description": "Task in 2 days", "priority": 8, "alarm_from": "2026-04-10"},
            {"description": "Today or past task", "priority": 7, "alarm_from": "2026-04-08"},
        ]
        mock_load_procrastinate_records.return_value = {}

        mock_table = MagicMock()
        mock_table_class.return_value = mock_table

        args = MagicMock()

        with patch("schedule_management.commands.tasks.Console") as mock_console_class:
            mock_console = MagicMock()
            mock_console_class.return_value = mock_console
            with patch("schedule_management.commands.tasks.datetime") as mock_datetime:
                mock_datetime.now.return_value = datetime(2026, 4, 8, 9, 0)
                mock_datetime.strptime = datetime.strptime
                result = reminder.show_tasks(args)

        assert result == 0
        description_cells = [
            call.args[2]
            for call in mock_table.add_row.call_args_list
            if len(call.args) >= 3
        ]

        postponed_cells = [
            cell
            for cell in description_cells
            if hasattr(cell, "plain") and cell.plain.startswith("💤 ")
        ]
        assert len(postponed_cells) == 2
        # Highest priority first
        clean_plain_0 = postponed_cells[0].plain.split("\u2060")[0]
        clean_plain_1 = postponed_cells[1].plain.split("\u2060")[0]
        assert clean_plain_0 == "💤 Task tomorrow (coming tomorrow)"
        assert postponed_cells[0].style == "italic dim"
        assert clean_plain_1 == "💤 Task in 2 days (coming in 2 days)"
        assert postponed_cells[1].style == "italic dim"


    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.load_procrastinate_records")
    @patch("schedule_management.commands.tasks.Table")
    def test_show_tasks_sorted_by_section_and_priority(
        self,
        mock_table_class,
        mock_load_procrastinate_records,
        mock_load_tasks,
    ):
        """Test that tasks are ordered: procrastinated -> current -> incoming, and sorted by priority descending in each section."""
        from datetime import datetime

        # Task 1: Procrastinated, priority 5
        # Task 2: Current, priority 9
        # Task 3: Incoming, priority 10
        # Task 4: Procrastinated, priority 8
        # Task 5: Current, priority 3
        # Task 6: Incoming, priority 2
        mock_load_tasks.return_value = [
            {"description": "Task 1 (proc 5)", "priority": 5},
            {"description": "Task 2 (curr 9)", "priority": 9},
            {"description": "Task 3 (inc 10)", "priority": 10, "alarm_from": "2026-04-10"},
            {"description": "Task 4 (proc 8)", "priority": 8},
            {"description": "Task 5 (curr 3)", "priority": 3},
            {"description": "Task 6 (inc 2)", "priority": 2, "alarm_from": "2026-04-10"},
        ]
        # Set Procrastinated tasks
        mock_load_procrastinate_records.return_value = {
            "Task 1 (proc 5)": {"since": "2026-04-05"},
            "Task 4 (proc 8)": {"since": "2026-04-05"},
        }

        mock_table = MagicMock()
        mock_table_class.return_value = mock_table

        args = MagicMock()

        with patch("schedule_management.commands.tasks.Console") as mock_console_class:
            mock_console = MagicMock()
            mock_console_class.return_value = mock_console
            with patch("schedule_management.commands.tasks.datetime") as mock_datetime:
                mock_datetime.now.return_value = datetime(2026, 4, 8, 9, 0)
                mock_datetime.strptime = datetime.strptime
                result = reminder.show_tasks(args)

        assert result == 0

        # Extract the description strings that were added as rows and strip type tags
        added_rows = [
            call.args[2].plain.split("\u2060")[0]
            for call in mock_table.add_row.call_args_list
            if len(call.args) >= 3
        ]

        # The expected order should be:
        # Procrastinated tasks (prio 8 first, then 5):
        # 1. ⏳ Task 4 (proc 8) (3 days)
        # 2. ⏳ Task 1 (proc 5) (3 days)
        # Current tasks (prio 9 first, then 3):
        # 3. Task 2 (curr 9)
        # 4. Task 5 (curr 3)
        # Incoming tasks (prio 10 first, then 2):
        # 5. 💤 Task 3 (inc 10) (2 days left)
        # 6. 💤 Task 6 (inc 2) (2 days left)

        assert len(added_rows) == 6
        assert added_rows[0] == "⏳ Task 4 (proc 8) (3 days overdue)"
        assert added_rows[1] == "⏳ Task 1 (proc 5) (3 days overdue)"
        assert added_rows[2] == "Task 2 (curr 9)"
        assert added_rows[3] == "Task 5 (curr 3)"
        assert added_rows[4] == "💤 Task 3 (inc 10) (coming in 2 days)"
        assert added_rows[5] == "💤 Task 6 (inc 2) (coming in 2 days)"

    @patch("schedule_management.commands.tasks.Table")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_show_tasks_filter_by_type_id(self, mock_load_tasks, mock_table_class):
        """Test 'ls --type <id>' only shows rows of that type."""
        mock_load_tasks.return_value = [
            {"description": "Read paper", "priority": 8, "type": "1"},
            {"description": "Lift weights", "priority": 6, "type": "2"},
            {"description": "Review paper", "priority": 7, "type": "1"},
        ]
        mock_table = MagicMock()
        mock_table_class.return_value = mock_table

        args = MagicMock()
        args.task_type = "1"

        with patch("schedule_management.commands.tasks.Console") as mock_console_class:
            mock_console = MagicMock()
            mock_console_class.return_value = mock_console
            result = reminder.show_tasks(args)

        assert result == 0
        added_rows = [
            call.args[2].plain.split("\u2060")[0]
            for call in mock_table.add_row.call_args_list
            if len(call.args) >= 3
        ]
        # Only type '1' tasks are shown.
        assert len(added_rows) == 2
        assert added_rows[0] == "Read paper"
        assert added_rows[1] == "Review paper"

    @patch("schedule_management.commands.tasks.Table")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_show_tasks_filter_by_type_name_case_insensitive(self, mock_load_tasks, mock_table_class):
        """Test 'ls --type <name>' matches case-insensitively."""
        mock_load_tasks.return_value = [
            {"description": "Read paper", "priority": 8, "type": "1"},
            {"description": "Lift weights", "priority": 6, "type": "2"},
        ]
        mock_table = MagicMock()
        mock_table_class.return_value = mock_table

        args = MagicMock()
        # 'GYM WORK' should resolve to type id '2'
        args.task_type = "GYM WORK"

        with patch("schedule_management.commands.tasks.Console") as mock_console_class:
            mock_console = MagicMock()
            mock_console_class.return_value = mock_console
            result = reminder.show_tasks(args)

        assert result == 0
        added_rows = [
            call.args[2].plain.split("\u2060")[0]
            for call in mock_table.add_row.call_args_list
            if len(call.args) >= 3
        ]
        assert len(added_rows) == 1
        assert added_rows[0] == "Lift weights"

    @patch("schedule_management.commands.tasks.Table")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_show_tasks_filter_preserves_full_list_ids(self, mock_load_tasks, mock_table_class):
        """Filtered 'ls' keeps IDs aligned with the unfiltered list (what rm/cancel/drop resolve)."""
        # Sorted by priority desc: B(9, t1), C(7, t2), A(3, t1) -> full IDs: 1,2,3
        mock_load_tasks.return_value = [
            {"description": "Task A", "priority": 3, "type": "1"},
            {"description": "Task B", "priority": 9, "type": "1"},
            {"description": "Task C", "priority": 7, "type": "2"},
        ]
        mock_table = MagicMock()
        mock_table_class.return_value = mock_table

        args = MagicMock()
        # Filter to type 1 -> shows Task B (full id 1) and Task A (full id 3)
        args.task_type = "1"

        with patch("schedule_management.commands.tasks.Console") as mock_console_class:
            mock_console = MagicMock()
            mock_console_class.return_value = mock_console
            result = reminder.show_tasks(args)

        assert result == 0
        # Each add_row call: (id, priority_visual, description)
        row_ids = [call.args[0] for call in mock_table.add_row.call_args_list if len(call.args) >= 3]
        assert row_ids == ["1", "3"]

    @patch("schedule_management.commands.tasks.Console")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_show_tasks_filter_unknown_type_returns_error(self, mock_load_tasks, mock_console_class):
        """Test 'ls --type <unknown>' prints an error and returns 1."""
        mock_load_tasks.return_value = [{"description": "X", "priority": 5, "type": "1"}]

        args = MagicMock()
        args.task_type = "nonexistent"

        result = reminder.show_tasks(args)

        assert result == 1
        printed = " ".join(str(c.args[0]) for c in mock_console_class.return_value.print.call_args_list if c.args)
        assert "nonexistent" in printed

    @patch("schedule_management.commands.tasks.Console")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_show_tasks_filter_no_matching_tasks(self, mock_load_tasks, mock_console_class):
        """Test 'ls --type' shows a friendly empty message when no tasks match."""
        mock_load_tasks.return_value = [
            {"description": "Read paper", "priority": 8, "type": "1"},
        ]

        args = MagicMock()
        # Type 2 exists (default 'gym work') but no task uses it.
        args.task_type = "2"

        result = reminder.show_tasks(args)

        assert result == 0
        printed = " ".join(str(c.args[0]) for c in mock_console_class.return_value.print.call_args_list if c.args)
        assert "gym work" in printed

    def test_task_type_colors_unique_for_small_and_large_sets(self):
        """Type colors must not wrap/repeat once type count exceeds the palette."""
        from schedule_management.commands.tasks import (
            _TASK_TYPE_BASE_COLORS,
            _task_type_colors,
        )

        small_ids = [str(i) for i in range(1, 5)]
        small = _task_type_colors(small_ids)
        assert list(small.values()) == _TASK_TYPE_BASE_COLORS[:4]
        assert len(set(small.values())) == 4

        # Exactly palette-sized: still unique named colors, no wrap.
        exact_ids = [str(i) for i in range(1, len(_TASK_TYPE_BASE_COLORS) + 1)]
        exact = _task_type_colors(exact_ids)
        assert len(exact) == len(_TASK_TYPE_BASE_COLORS)
        assert len(set(exact.values())) == len(_TASK_TYPE_BASE_COLORS)

        # Beyond palette: HSL hex colors, all unique (old bug was idx % 10).
        large_n = len(_TASK_TYPE_BASE_COLORS) + 25
        large_ids = [str(i) for i in range(1, large_n + 1)]
        large = _task_type_colors(large_ids)
        assert len(large) == large_n
        assert len(set(large.values())) == large_n
        assert all(c.startswith("#") and len(c) == 7 for c in large.values())

    @patch("schedule_management.commands.tasks.Console")
    @patch("schedule_management.commands.tasks.load_tasks")
    @patch("schedule_management.commands.tasks.ScheduleConfig")
    def test_show_tasks_color_duplication(self, mock_config_class, mock_load_tasks, mock_console_class):
        """Test that all task types are assigned different colors without duplicates."""
        mock_config = MagicMock()
        mock_config.task_types = {str(i): f"type_{i}" for i in range(1, 41)}
        mock_config_class.return_value = mock_config

        mock_load_tasks.return_value = [
            {"description": f"Task {i}", "priority": 5, "type": str(i)}
            for i in range(1, 41)
        ]

        args = MagicMock()
        args.task_type = None

        with patch("schedule_management.commands.tasks.Table") as mock_table_class:
            mock_table = MagicMock()
            mock_table_class.return_value = mock_table
            result = reminder.show_tasks(args)

        assert result == 0

        colors = []
        for call in mock_table.add_row.call_args_list:
            if len(call.args) >= 2:
                prio_visual = call.args[1]
                if prio_visual.startswith("["):
                    color = prio_visual[1:prio_visual.find("]")]
                    colors.append(color)

        assert len(colors) == 40
        assert len(set(colors)) == 40

    def test_ls_parser_accepts_type_filter(self):
        """Test parser wires '-t/--type' and stores it as task_type."""
        parser = reminder.create_parser()

        args_short = parser.parse_args(["ls", "-t", "1"])
        assert args_short.task_type == "1"

        args_long = parser.parse_args(["ls", "--type", "coding"])
        assert args_long.task_type == "coding"

        args_none = parser.parse_args(["ls"])
        assert args_none.task_type is None
        assert args_none.func is reminder.show_tasks

    def test_resolve_task_type_id_helper(self):
        """Test the type-id resolution helper directly (id and name)."""
        from schedule_management.commands.tasks import _resolve_task_type_id

        types = {"1": "read papers", "2": "gym work"}
        assert _resolve_task_type_id("1", types) == "1"
        assert _resolve_task_type_id("gym work", types) == "2"
        assert _resolve_task_type_id("GYM WORK", types) == "2"  # case-insensitive
        assert _resolve_task_type_id("zzz", types) is None  # unknown

    @patch("schedule_management.commands.tasks.log_task_action")
    @patch("schedule_management.commands.tasks.save_procrastinate_list")
    @patch("schedule_management.commands.tasks.load_procrastinate_list")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_delete_task_removes_from_procrastinate_list(
        self,
        mock_load_tasks,
        mock_save_tasks,
        mock_load_procrastinate_list,
        mock_save_procrastinate_list,
        mock_log_task_action,
    ):
        """Test that deleting a task also removes it from procrastinate list."""
        mock_load_tasks.return_value = [
            {"description": "Review code", "priority": 9},
            {"description": "Write tests", "priority": 5},
        ]
        mock_load_procrastinate_list.return_value = {"Review code", "Write tests"}
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Review code"]

        result = reminder.delete_task(args)

        assert result == 0
        mock_log_task_action.assert_called_once()
        mock_save_tasks.assert_called_once()
        mock_save_procrastinate_list.assert_called_once()

        saved_list = mock_save_procrastinate_list.call_args[0][0]
        assert "Review code" not in saved_list
        assert "Write tests" in saved_list


class TestCancelDropCommands:
    """Test the 'cancel' and 'drop' commands.

    Both behave like 'rm' (remove the task, sync the procrastinate list) but
    record a distinct history action so they are NOT counted as completion.
    """

    @patch("schedule_management.commands.tasks.log_task_action")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_cancel_task_success(self, mock_load_tasks, mock_save_tasks, mock_log_task_action):
        """Test cancelling a task logs 'cancelled' (not 'deleted') and saves."""
        mock_load_tasks.return_value = [
            {"description": "Typo task", "priority": 5},
            {"description": "Real task", "priority": 8},
        ]
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Typo task"]

        result = reminder.cancel_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert not any(t["description"] == "Typo task" for t in saved_tasks)
        # Must record 'cancelled', never 'deleted'
        mock_log_task_action.assert_called_once()
        assert mock_log_task_action.call_args[0][0] == "cancelled"

    @patch("schedule_management.commands.tasks.log_task_action")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_drop_task_success(self, mock_load_tasks, mock_save_tasks, mock_log_task_action):
        """Test dropping a task logs 'dropped' (not 'deleted') and saves."""
        mock_load_tasks.return_value = [
            {"description": "Side project", "priority": 4},
            {"description": "Real task", "priority": 8},
        ]
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Side project"]

        result = reminder.drop_task(args)

        assert result == 0
        mock_save_tasks.assert_called_once()
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert not any(t["description"] == "Side project" for t in saved_tasks)
        mock_log_task_action.assert_called_once()
        assert mock_log_task_action.call_args[0][0] == "dropped"

    @patch("schedule_management.commands.tasks.log_task_action")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_cancel_drop_do_not_log_deleted(
        self, mock_load_tasks, mock_save_tasks, mock_log_task_action
    ):
        """Both commands must never record the 'deleted' (completion) action."""
        mock_load_tasks.return_value = [
            {"description": "Typo task", "priority": 5},
            {"description": "Side project", "priority": 4},
            {"description": "Other", "priority": 9},
        ]
        mock_save_tasks.return_value = None

        cancel_args = MagicMock()
        cancel_args.tasks = ["Typo task"]
        drop_args = MagicMock()
        drop_args.tasks = ["Side project"]

        reminder.cancel_task(cancel_args)
        reminder.drop_task(drop_args)

        actions = [call.args[0] for call in mock_log_task_action.call_args_list]
        assert "deleted" not in actions
        assert actions.count("cancelled") == 1
        assert actions.count("dropped") == 1

    @patch("schedule_management.commands.tasks.log_task_action")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_cancel_task_by_id(self, mock_load_tasks, mock_save_tasks, mock_log_task_action):
        """Test cancelling a task by ID resolves against the sorted list."""
        mock_load_tasks.return_value = [
            {"description": "High", "priority": 9},
            {"description": "Low", "priority": 2},
        ]
        mock_save_tasks.return_value = None

        args = MagicMock()
        # ID 1 = highest priority = "High"
        args.tasks = ["1"]

        result = reminder.cancel_task(args)

        assert result == 0
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "Low"

    @patch("schedule_management.commands.tasks.log_task_action")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_drop_task_by_id(self, mock_load_tasks, mock_save_tasks, mock_log_task_action):
        """Test dropping a task by ID resolves against the sorted list."""
        mock_load_tasks.return_value = [
            {"description": "High", "priority": 9},
            {"description": "Low", "priority": 2},
        ]
        mock_save_tasks.return_value = None

        args = MagicMock()
        # ID 2 = lowest priority = "Low"
        args.tasks = ["2"]

        result = reminder.drop_task(args)

        assert result == 0
        saved_tasks = mock_save_tasks.call_args[0][0]
        assert len(saved_tasks) == 1
        assert saved_tasks[0]["description"] == "High"

    @patch("builtins.print")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_cancel_task_not_found(self, mock_load_tasks, mock_save_tasks, mock_print):
        """Test cancelling a non-existent task returns 1 and prints an error."""
        mock_load_tasks.return_value = [{"description": "Real task", "priority": 8}]
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Ghost task"]

        result = reminder.cancel_task(args)

        assert result == 1
        mock_save_tasks.assert_not_called()
        printed = " ".join(str(c.args[0]) for c in mock_print.call_args_list)
        assert "Ghost task" in printed

    @patch("builtins.print")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_drop_task_no_tasks(self, mock_load_tasks, mock_print):
        """Test dropping when there are no tasks returns 1."""
        mock_load_tasks.return_value = []

        args = MagicMock()
        args.tasks = ["whatever"]

        result = reminder.drop_task(args)

        assert result == 1

    @patch("builtins.print")
    @patch("schedule_management.commands.tasks.log_task_action")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_cancel_task_save_error(self, mock_load_tasks, mock_save_tasks, mock_log_task_action, mock_print):
        """Test cancel returns 1 when saving fails."""
        mock_load_tasks.return_value = [{"description": "Typo", "priority": 5}]
        mock_save_tasks.side_effect = Exception("Save failed")

        args = MagicMock()
        args.tasks = ["Typo"]

        result = reminder.cancel_task(args)

        assert result == 1
        printed = " ".join(str(c.args[0]) for c in mock_print.call_args_list)
        assert "Save failed" in printed

    @patch("schedule_management.commands.tasks.log_task_action")
    @patch("schedule_management.commands.tasks.save_procrastinate_list")
    @patch("schedule_management.commands.tasks.load_procrastinate_list")
    @patch("schedule_management.commands.tasks.save_tasks")
    @patch("schedule_management.commands.tasks.load_tasks")
    def test_drop_task_removes_from_procrastinate_list(
        self,
        mock_load_tasks,
        mock_save_tasks,
        mock_load_procrastinate_list,
        mock_save_procrastinate_list,
        mock_log_task_action,
    ):
        """Test that dropping a task also removes it from the procrastinate list."""
        mock_load_tasks.return_value = [
            {"description": "Review code", "priority": 9},
            {"description": "Write tests", "priority": 5},
        ]
        mock_load_procrastinate_list.return_value = {"Review code", "Write tests"}
        mock_save_tasks.return_value = None

        args = MagicMock()
        args.tasks = ["Review code"]

        result = reminder.drop_task(args)

        assert result == 0
        mock_log_task_action.assert_called_once()
        assert mock_log_task_action.call_args[0][0] == "dropped"
        mock_save_tasks.assert_called_once()
        mock_save_procrastinate_list.assert_called_once()

        saved_list = mock_save_procrastinate_list.call_args[0][0]
        assert "Review code" not in saved_list
        assert "Write tests" in saved_list

    def test_cancel_parser_wiring(self):
        """Test parser routes 'cancel' to cancel_task."""
        parser = reminder.create_parser()
        args = parser.parse_args(["cancel", "some task"])

        assert args.command == "cancel"
        assert args.tasks == ["some task"]
        assert args.func is reminder.cancel_task

    def test_drop_parser_wiring(self):
        """Test parser routes 'drop' to drop_task."""
        parser = reminder.create_parser()
        args = parser.parse_args(["drop", "1", "2"])

        assert args.command == "drop"
        assert args.tasks == ["1", "2"]
        assert args.func is reminder.drop_task


class TestMainFunctions:
    """Test the main entry point function."""

    @patch("schedule_management.reminder.status_command")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_with_status_command(self, mock_parse_args, mock_status_command):
        """Test main function with status command."""
        mock_args = MagicMock()
        mock_args.command = "status"
        mock_args.func = mock_status_command
        mock_parse_args.return_value = mock_args
        mock_status_command.return_value = 0

        # Use configured test paths
        result = reminder.main()

        assert result == 0
        mock_status_command.assert_called_once_with(mock_args)

    @patch("argparse.ArgumentParser.print_help")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_no_command(self, mock_parse_args, mock_print_help):
        """Test main function with no command specified."""
        mock_args = MagicMock()
        mock_args.command = None
        mock_parse_args.return_value = mock_args

        # Use configured test paths
        result = reminder.main()

        assert result == 1
        mock_print_help.assert_called_once()

    @patch("schedule_management.reminder.update_command")
    @patch("argparse.ArgumentParser.parse_args")
    def test_main_keyboard_interrupt(self, mock_parse_args, mock_update_command):
        """Test main function handling keyboard interrupt."""
        mock_args = MagicMock()
        mock_args.command = "update"
        mock_args.func = mock_update_command
        mock_parse_args.return_value = mock_args
        mock_update_command.side_effect = KeyboardInterrupt()

        # Use configured test paths
        result = reminder.main()

        assert result == 1


class TestSettingsCommand:
    """Test the interactive settings TUI command wiring and data layer."""

    def test_settings_parser_wiring(self):
        """Test parser routes 'settings' to settings_command."""
        parser = reminder.create_parser()
        args = parser.parse_args(["settings"])

        assert args.command == "settings"
        assert args.func is reminder.settings_command

    def test_settings_model_load_and_save(self, tmp_path):
        """Test round-trip load → mutate → save → reload."""
        from schedule_management.commands.settings import SettingsModel

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            '[settings]\nalarm_interval = 5\nlanguage = "en"\n'
            "\n[time_blocks]\npomodoro = 25\n",
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)

        assert model.get("settings", "alarm_interval") == 5
        assert model.get("settings", "language") == "en"
        assert model.get("time_blocks", "pomodoro") == 25
        assert model.dirty is False

        # Mutate and save
        model.set("settings", "alarm_interval", 10)
        assert model.dirty is True

        model.save()
        assert model.dirty is False

        # Reload and verify persistence
        model2 = SettingsModel(settings_file)
        assert model2.get("settings", "alarm_interval") == 10
        assert model2.get("settings", "language") == "en"

    def test_settings_model_dirty_tracking(self, tmp_path):
        """Test dirty flag only flips on actual value changes."""
        from schedule_management.commands.settings import SettingsModel

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            '[settings]\nalarm_interval = 5\n', encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        assert model.dirty is False

        # Setting same value should not mark dirty
        model.set("settings", "alarm_interval", 5)
        assert model.dirty is False

        # Setting different value should mark dirty
        model.set("settings", "alarm_interval", 10)
        assert model.dirty is True

    def test_settings_model_delete(self, tmp_path):
        """Test key deletion and dirty tracking."""
        from schedule_management.commands.settings import SettingsModel

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            '[settings]\nalarm_interval = 5\nlanguage = "en"\n',
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        assert model.delete("settings", "language") is True
        assert model.dirty is True
        assert model.get("settings", "language") is None

        # Delete non-existent key
        assert model.delete("settings", "nonexistent") is False

    def test_settings_model_add_key(self, tmp_path):
        """Test adding a new key with default value."""
        from schedule_management.commands.settings import SettingsModel

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            "[time_blocks]\npomodoro = 25\n", encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        assert model.add_key("time_blocks", "study") is True
        assert model.dirty is True
        # time_blocks fallback is NUMBER with min_val=1
        assert model.get("time_blocks", "study") == 1

        # Adding duplicate returns False
        assert model.add_key("time_blocks", "study") is False

    def test_settings_model_sections_and_keys(self, tmp_path):
        """Test section and key listing."""
        from schedule_management.commands.settings import SettingsModel

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            "[settings]\na = 1\nb = 2\n\n[paths]\nc = 3\n",
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        assert "settings" in model.sections()
        assert "paths" in model.sections()
        assert "task_types" in model.sections()
        assert model.keys_in("settings") == ["a", "b"]
        assert model.keys_in("paths") == ["c"]
        assert model.keys_in("nonexistent") == []

    def test_toml_writer_round_trip(self, tmp_path):
        """Test the minimal TOML serializer handles all value types."""
        from schedule_management.toml_writer import dump_toml, load_toml_raw

        data = {
            "settings": {
                "sound": "/path/to/sound.aiff",
                "interval": 5,
                "enabled": True,
                "disabled": False,
                "days": ["monday", "friday"],
            },
            "blocks": {
                "pomodoro": 25,
            },
        }
        path = tmp_path / "test.toml"
        dump_toml(data, path)

        loaded = load_toml_raw(path)
        assert loaded == data

    def test_toml_writer_string_escaping(self, tmp_path):
        """Test TOML writer escapes special characters in strings."""
        from schedule_management.toml_writer import dump_toml, load_toml_raw

        data = {"section": {"msg": 'He said "hello" and \\n left'}}
        path = tmp_path / "escape.toml"
        dump_toml(data, path)

        loaded = load_toml_raw(path)
        assert loaded["section"]["msg"] == 'He said "hello" and \\n left'

    def test_settings_meta_coverage(self):
        """All keys in the template have metadata or a section fallback."""
        from schedule_management.commands.settings import _get_meta
        from schedule_management.toml_writer import load_toml_raw
        from conftest import TEST_CONFIG_DIR

        # Load the test config settings
        settings_path = TEST_CONFIG_DIR / "user_config_0" / "settings.toml"
        if not settings_path.exists():
            return  # skip if test fixture missing
        data = load_toml_raw(settings_path)

        for section, values in data.items():
            if not isinstance(values, dict):
                continue
            for key in values:
                meta = _get_meta(section, key)
                assert meta is not None, (
                    f"No metadata for ({section!r}, {key!r})"
                )

    def test_settings_tui_row_building(self, tmp_path):
        """Test TUI builds correct rows from model data with hierarchical browse."""
        from schedule_management.commands.settings import SettingsModel, SettingsTUI

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            "[settings]\na = 1\nb = 2\n\n[paths]\nc = 3\n",
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        tui = SettingsTUI(model)

        # Level 0: sections view — rows is empty (sections rendered separately)
        assert tui.browse_level == 0
        assert len(tui.rows) == 0
        sections = tui._sections_list()
        assert "settings" in sections
        assert "paths" in sections
        assert "task_types" in sections

        # Drill into "settings" section
        tui.section_cursor = sections.index("settings")
        tui._drill_into_section()
        assert tui.browse_level == 1
        assert tui.browse_section == "settings"
        assert len(tui.rows) == 2
        assert tui.rows[0].key == "a"
        assert tui.rows[1].key == "b"
        assert tui.cursor == 0

        # Go back to sections
        tui._go_back_to_sections()
        assert tui.browse_level == 0
        assert tui.section_cursor == sections.index("settings")
        assert len(tui.rows) == 0

        # Drill into "paths" section
        tui.section_cursor = sections.index("paths")
        tui._drill_into_section()
        assert tui.browse_level == 1
        assert tui.browse_section == "paths"
        assert len(tui.rows) == 1
        assert tui.rows[0].key == "c"

    @patch("sys.stdin")
    def test_settings_command_missing_file(self, mock_stdin, tmp_path, monkeypatch, capsys):
        """Test settings_command returns 1 when settings file is missing."""
        from schedule_management.commands.settings import settings_command

        mock_stdin.isatty.return_value = True

        # Point to an empty config dir
        empty_config = tmp_path / "config" / "user_config_0"
        empty_config.mkdir(parents=True)
        (tmp_path / "config" / ".active_config").write_text("0\n")
        monkeypatch.setenv("REMINDER_CONFIG_DIR", str(tmp_path / "config"))

        args = MagicMock()
        result = settings_command(args)
        assert result == 1

        captured = capsys.readouterr()
        assert "not found" in captured.out

    @pytest.mark.parametrize("exit_key", ["q", "e", "x"])
    @patch("schedule_management.commands.settings.Live")
    @patch("readchar.readkey")
    def test_settings_tui_run(self, mock_readkey, mock_live_class, exit_key, tmp_path):
        """Test the run loop of SettingsTUI updates Live and exits on exit_key."""
        from schedule_management.commands.settings import SettingsModel, SettingsTUI
        from unittest.mock import MagicMock

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text("[settings]\nlanguage = \"en\"\n", encoding="utf-8")

        model = SettingsModel(settings_file)
        tui = SettingsTUI(model)

        mock_readkey.return_value = exit_key

        # Mock the Live context manager instance
        mock_live_instance = MagicMock()
        mock_live_class.return_value = mock_live_instance

        # Run the TUI
        result = tui.run()

        assert result == 0
        mock_live_class.assert_called_once()
        # Verify the context manager entered
        mock_live_instance.__enter__.assert_called_once()

    def test_settings_tui_time_list_navigation(self, tmp_path):
        """Test that UP/DOWN/back keys work correctly in the TIME_LIST editor mode."""
        from schedule_management.commands.settings import SettingsModel, SettingsTUI, _Mode, Row
        import readchar

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text("[tasks]\ndaily_urgent = [\"10:00\", \"20:00\"]\n", encoding="utf-8")

        model = SettingsModel(settings_file)
        tui = SettingsTUI(model)

        # Set up TUI to edit "daily_urgent" in TIME_LIST mode
        tui.editing_row = Row("tasks", "daily_urgent")
        tui.mode = _Mode.TIME_LIST
        tui.time_list_values = ["10:00", "20:00"]
        tui.time_list_cursor = 0

        # Press DOWN key
        tui._handle_key(readchar.key.DOWN)
        assert tui.time_list_cursor == 1

        # Press DOWN key again (should clamp at index 1)
        tui._handle_key(readchar.key.DOWN)
        assert tui.time_list_cursor == 1

        # Press UP key
        tui._handle_key(readchar.key.UP)
        assert tui.time_list_cursor == 0

        # Press UP key again (should clamp at index 0)
        tui._handle_key(readchar.key.UP)
        assert tui.time_list_cursor == 0

        # Press LEFT key (back key)
        tui._handle_key(readchar.key.LEFT)
        assert tui.mode == _Mode.BROWSE
        assert model.get("tasks", "daily_urgent") == ["10:00", "20:00"]

        # Test back keys in PICKER mode
        tui.mode = _Mode.PICKER
        tui._handle_key(readchar.key.BACKSPACE)
        assert tui.mode == _Mode.BROWSE

    def test_settings_model_task_types_auto_populated(self, tmp_path):
        """Test that task_types section is auto-populated with defaults when missing."""
        from schedule_management.commands.settings import SettingsModel

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            "[settings]\nalarm_interval = 5\n", encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        assert "task_types" in model.sections()
        assert model.get("task_types", "1") == "read papers"
        assert model.get("task_types", "2") == "gym work"
        assert model.get("task_types", "3") == "coding"
        assert model.get("task_types", "4") == "other"
        assert model.dirty is False

    def test_settings_model_task_types_preserved_when_present(self, tmp_path):
        """Test that existing task_types section is preserved."""
        from schedule_management.commands.settings import SettingsModel

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            '[settings]\na = 1\n\n[task_types]\n1 = "custom type"\n',
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        assert model.get("task_types", "1") == "custom type"
        assert model.get("task_types", "2") is None

    def test_settings_tui_task_types_row_building(self, tmp_path):
        """Test TUI builds correct rows for task_types section."""
        from schedule_management.commands.settings import SettingsModel, SettingsTUI

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            "[settings]\na = 1\n\n[task_types]\n1 = \"read papers\"\n2 = \"coding\"\n",
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        tui = SettingsTUI(model)

        # Drill into task_types section
        sections = tui._sections_list()
        assert "task_types" in sections
        tui.section_cursor = sections.index("task_types")
        tui._drill_into_section()
        assert tui.browse_level == 1
        assert tui.browse_section == "task_types"
        assert len(tui.rows) == 2
        assert tui.rows[0].key == "1"
        assert tui.rows[1].key == "2"

    def test_settings_tui_task_types_add_prefill(self, tmp_path):
        """Test that adding a task type pre-fills the next available number."""
        from schedule_management.commands.settings import SettingsModel, SettingsTUI, _Mode

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            '[task_types]\n1 = "read papers"\n3 = "coding"\n',
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        tui = SettingsTUI(model)

        # Drill into task_types section
        tui.section_cursor = tui._sections_list().index("task_types")
        tui._drill_into_section()

        # Press 'a' to add
        tui._handle_key("a")
        assert tui.mode == _Mode.ADD_KEY
        assert tui.edit_buffer == "4"

    def test_settings_tui_task_types_add_auto_edit(self, tmp_path):
        """Test that adding a task type auto-transitions to inline editing."""
        from schedule_management.commands.settings import SettingsModel, SettingsTUI, _Mode
        import readchar

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            '[task_types]\n1 = "read papers"\n',
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        tui = SettingsTUI(model)

        # Drill into task_types section
        tui.section_cursor = tui._sections_list().index("task_types")
        tui._drill_into_section()

        # Press 'a' to add, then Enter to confirm key name
        tui._handle_key("a")
        assert tui.edit_buffer == "2"
        tui._handle_key(readchar.key.ENTER)

        # Should auto-transition to INLINE mode to edit the value
        assert tui.mode == _Mode.INLINE
        assert tui.editing_row.key == "2"

    def test_settings_tui_task_types_empty_name_validation(self, tmp_path):
        """Test that empty task type names are rejected."""
        from schedule_management.commands.settings import SettingsModel, SettingsTUI, _Mode, Row
        import readchar

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text(
            '[task_types]\n1 = "read papers"\n',
            encoding="utf-8",
        )

        model = SettingsModel(settings_file)
        tui = SettingsTUI(model)

        # Set up inline editing for a task type with empty buffer
        tui.editing_row = Row("task_types", "1")
        tui.mode = _Mode.INLINE
        tui.edit_buffer = ""

        # Try to confirm empty name
        tui._handle_key(readchar.key.ENTER)
        assert tui.mode == _Mode.INLINE
        assert "cannot be empty" in tui.message

    def test_settings_sections_list_includes_llm_first(self, tmp_path):
        """The synthetic Model Settings page is always present at the top."""
        from schedule_management.commands.settings import (
            LLM_SECTION,
            SettingsModel,
            SettingsTUI,
        )

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text("[settings]\nfoo = 1\n", encoding="utf-8")

        tui = SettingsTUI(SettingsModel(settings_file))
        sections = tui._sections_list()

        assert sections[0] == LLM_SECTION
        assert "settings" in sections

    def test_llm_model_adapter_round_trip(self, tmp_path):
        """_load_llm_model / _save_llm_model round-trip against llm.toml."""
        from schedule_management.commands import settings as settings_module

        llm_path = tmp_path / "llm.toml"
        monkey = patch.object(
            settings_module, "_resolve_llm_config_path", return_value=llm_path
        )
        with monkey:
            assert settings_module._load_llm_model() is None

            settings_module._save_llm_model("openai/gpt-4.1-mini")
            assert settings_module._load_llm_model() == "openai/gpt-4.1-mini"

            # Clearing persists a valid file that reloads as None
            settings_module._save_llm_model(None)
            assert settings_module._load_llm_model() is None

    def test_settings_llm_page_edit_and_clear(self, tmp_path):
        """Editing and clearing the model from the Model Settings page."""
        from schedule_management.commands import settings as settings_module
        from schedule_management.commands.settings import (
            LLM_CLEAR_KEY,
            LLM_SECTION,
            SettingsModel,
            SettingsTUI,
            _Mode,
        )
        import readchar

        llm_path = tmp_path / "llm.toml"
        settings_file = tmp_path / "settings.toml"
        settings_file.write_text("[settings]\nfoo = 1\n", encoding="utf-8")

        with patch.object(
            settings_module, "_resolve_llm_config_path", return_value=llm_path
        ):
            tui = SettingsTUI(SettingsModel(settings_file))
            assert tui.llm_model is None

            # Drill into the Model Settings page
            tui.section_cursor = tui._sections_list().index(LLM_SECTION)
            tui._drill_into_section()
            assert tui.browse_section == LLM_SECTION
            assert [r.key for r in tui.rows] == ["model", LLM_CLEAR_KEY]

            # Edit the model row
            tui.cursor = 0
            tui._handle_key(readchar.key.ENTER)
            assert tui.mode == _Mode.LLM_EDIT
            for ch in "openai/gpt-4.1-mini":
                tui._handle_key(ch)
            tui._handle_key(readchar.key.ENTER)
            assert tui.mode == _Mode.BROWSE
            assert tui.llm_model == "openai/gpt-4.1-mini"
            assert tui.llm_dirty is True

            # Persist via 's'
            tui._handle_key("s")
            assert tui.llm_dirty is False
            assert settings_module._load_llm_model() == "openai/gpt-4.1-mini"

            # Now clear it
            tui.cursor = 1  # clear row
            tui._handle_key(readchar.key.ENTER)
            assert tui.llm_model is None
            assert tui.llm_dirty is True
            tui._handle_key("s")
            assert settings_module._load_llm_model() is None

    def test_settings_llm_page_blank_edit_clears(self, tmp_path):
        """Submitting an empty model value clears the override."""
        from schedule_management.commands import settings as settings_module
        from schedule_management.commands.settings import (
            LLM_SECTION,
            SettingsModel,
            SettingsTUI,
            _Mode,
        )
        import readchar

        llm_path = tmp_path / "llm.toml"
        settings_file = tmp_path / "settings.toml"
        settings_file.write_text("[settings]\nfoo = 1\n", encoding="utf-8")

        with patch.object(
            settings_module, "_resolve_llm_config_path", return_value=llm_path
        ):
            # Seed an existing override
            settings_module._save_llm_model("anthropic/claude-sonnet")
            tui = SettingsTUI(SettingsModel(settings_file))

            tui.section_cursor = tui._sections_list().index(LLM_SECTION)
            tui._drill_into_section()
            tui.cursor = 0
            tui._handle_key(readchar.key.ENTER)
            assert tui.mode == _Mode.LLM_EDIT
            # Clear the buffer, then confirm blank
            tui.edit_buffer = ""
            tui._handle_key(readchar.key.ENTER)
            assert tui.llm_model is None
            assert tui.llm_dirty is True

    def test_settings_llm_dirty_blocks_quit(self, tmp_path):
        """Unsaved model changes route to confirm-quit instead of quitting."""
        from schedule_management.commands import settings as settings_module
        from schedule_management.commands.settings import (
            LLM_SECTION,
            SettingsModel,
            SettingsTUI,
            _Mode,
        )

        llm_path = tmp_path / "llm.toml"
        settings_file = tmp_path / "settings.toml"
        settings_file.write_text("[settings]\nfoo = 1\n", encoding="utf-8")

        with patch.object(
            settings_module, "_resolve_llm_config_path", return_value=llm_path
        ):
            tui = SettingsTUI(SettingsModel(settings_file))
            tui.llm_model = "openai/gpt-4.1-mini"
            tui.llm_dirty = True

            # On the llm page, 'q' should defer to confirm-quit, not quit
            tui.section_cursor = tui._sections_list().index(LLM_SECTION)
            tui._drill_into_section()
            result = tui._handle_key("q")
            assert result is None  # did not quit
            assert tui.mode == _Mode.CONFIRM_QUIT

            # 's' saves both and quits
            result = tui._handle_key("s")
            assert result == "quit"
            assert settings_module._load_llm_model() == "openai/gpt-4.1-mini"

    def test_settings_section_title_shows_unsaved_model_change(self, tmp_path):
        from schedule_management.commands.settings import SettingsModel, SettingsTUI

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text("[settings]\nfoo = 1\n", encoding="utf-8")
        tui = SettingsTUI(SettingsModel(settings_file))
        tui.llm_model = "test/model"
        tui.llm_dirty = True

        assert "modified" in tui._render_sections_view().title

    @patch("schedule_management.commands.settings.Live")
    @patch("readchar.readkey")
    def test_settings_ctrl_c_with_changes_opens_quit_confirmation(
        self, mock_readkey, mock_live_class, tmp_path
    ):
        from schedule_management.commands.settings import SettingsModel, SettingsTUI

        settings_file = tmp_path / "settings.toml"
        settings_file.write_text("[settings]\nfoo = 1\n", encoding="utf-8")
        tui = SettingsTUI(SettingsModel(settings_file))
        tui.model.set("settings", "foo", 2)
        mock_readkey.side_effect = [KeyboardInterrupt(), "q"]

        assert tui.run() == 0
        rendered = mock_live_class.return_value.__enter__.return_value.update.call_args.args[0]
        assert "Unsaved Changes" in rendered.title
        assert mock_readkey.call_count == 2



class TestHistoryCommand:
    """Test the history command functionality."""

    def test_history_parser_wiring(self):
        """Test parser routes 'history' to history_command with default count."""
        parser = reminder.create_parser()
        args = parser.parse_args(["history"])

        assert args.command == "history"
        assert args.count == 5
        assert args.func is reminder.history_command

    def test_history_parser_custom_count(self):
        """Test parser accepts a custom count for history."""
        parser = reminder.create_parser()
        args = parser.parse_args(["history", "10"])

        assert args.command == "history"
        assert args.count == 10

    @patch("schedule_management.commands.history.load_task_log")
    @patch("schedule_management.commands.history.Console")
    def test_history_empty_log(self, mock_console_class, mock_load_task_log):
        """Test history command with an empty task log."""
        mock_load_task_log.return_value = []
        mock_console = MagicMock()
        mock_console_class.return_value = mock_console

        args = MagicMock(count=5)
        result = reminder.history_command(args)

        assert result == 0
        mock_console.print.assert_called()

    @patch("schedule_management.commands.history.load_task_log")
    @patch("schedule_management.commands.history.Console")
    def test_history_no_completed_activities(
        self, mock_console_class, mock_load_task_log
    ):
        """Test history command when log has entries but no paired activities."""
        mock_load_task_log.return_value = [
            {
                "timestamp": "2026-06-01T09:00:00",
                "action": "added",
                "task": {"description": "Pending task", "priority": 5},
            }
        ]
        mock_console = MagicMock()
        mock_console_class.return_value = mock_console

        args = MagicMock(count=5)
        result = reminder.history_command(args)

        assert result == 0

    @patch("schedule_management.commands.history.load_task_log")
    @patch("schedule_management.commands.history.Console")
    def test_history_shows_completed_activities(
        self, mock_console_class, mock_load_task_log
    ):
        """Test history command displays paired activities correctly."""
        mock_load_task_log.return_value = [
            {
                "timestamp": "2026-06-01T09:00:00",
                "action": "added",
                "task": {"description": "Study math", "priority": 8},
            },
            {
                "timestamp": "2026-06-01T10:30:00",
                "action": "deleted",
                "task": {"description": "Study math", "priority": 8},
            },
            {
                "timestamp": "2026-06-02T14:00:00",
                "action": "added",
                "task": {"description": "Write report", "priority": 6},
            },
            {
                "timestamp": "2026-06-02T15:00:00",
                "action": "deleted",
                "task": {"description": "Write report", "priority": 6},
            },
        ]
        mock_console = MagicMock()
        mock_console_class.return_value = mock_console

        args = MagicMock(count=5)
        result = reminder.history_command(args)

        assert result == 0
        assert mock_console.print.called

    @patch("schedule_management.commands.history.load_task_log")
    @patch("schedule_management.commands.history.Console")
    def test_history_respects_count_limit(
        self, mock_console_class, mock_load_task_log
    ):
        """Test history command limits output to the requested count."""
        entries = []
        for i in range(10):
            entries.append({
                "timestamp": f"2026-06-01T{9 + i:02d}:00:00",
                "action": "added",
                "task": {"description": f"Task {i}", "priority": 5},
            })
            entries.append({
                "timestamp": f"2026-06-01T{9 + i:02d}:30:00",
                "action": "deleted",
                "task": {"description": f"Task {i}", "priority": 5},
            })
        mock_load_task_log.return_value = entries

        mock_console = MagicMock()
        mock_console_class.return_value = mock_console

        args = MagicMock(count=3)
        result = reminder.history_command(args)

        assert result == 0

    def test_pair_task_activities(self):
        """Test the activity pairing logic directly."""
        from schedule_management.commands.history import _pair_task_activities

        entries = [
            {
                "timestamp": "2026-06-01T09:00:00",
                "action": "added",
                "task": {"description": "Task A", "priority": 8},
            },
            {
                "timestamp": "2026-06-01T10:00:00",
                "action": "deleted",
                "task": {"description": "Task A", "priority": 8},
            },
            {
                "timestamp": "2026-06-01T11:00:00",
                "action": "added",
                "task": {"description": "Task B", "priority": 3},
            },
        ]

        activities = _pair_task_activities(entries)

        assert len(activities) == 1
        assert activities[0]["description"] == "Task A"
        assert activities[0]["priority"] == 8
        assert activities[0]["started_at"].hour == 9
        assert activities[0]["ended_at"].hour == 10

    def test_pair_task_activities_skips_unpaired(self):
        """Test that tasks with only 'added' and no 'deleted' are skipped."""
        from schedule_management.commands.history import _pair_task_activities

        entries = [
            {
                "timestamp": "2026-06-01T09:00:00",
                "action": "added",
                "task": {"description": "Orphan task", "priority": 5},
            },
            {
                "timestamp": "2026-06-01T10:00:00",
                "action": "updated",
                "task": {"description": "Orphan task", "priority": 6},
            },
        ]

        activities = _pair_task_activities(entries)
        assert len(activities) == 0

    def test_pair_task_activities_handles_re_added(self):
        """Test that re-added tasks are paired independently."""
        from schedule_management.commands.history import _pair_task_activities

        entries = [
            {
                "timestamp": "2026-06-01T09:00:00",
                "action": "added",
                "task": {"description": "Recycled task", "priority": 7},
            },
            {
                "timestamp": "2026-06-01T10:00:00",
                "action": "deleted",
                "task": {"description": "Recycled task", "priority": 7},
            },
            {
                "timestamp": "2026-06-02T09:00:00",
                "action": "added",
                "task": {"description": "Recycled task", "priority": 9},
            },
            {
                "timestamp": "2026-06-02T11:00:00",
                "action": "deleted",
                "task": {"description": "Recycled task", "priority": 9},
            },
        ]

        activities = _pair_task_activities(entries)

        assert len(activities) == 2
        assert activities[0]["priority"] == 7
        assert activities[1]["priority"] == 9

    def test_pair_task_activities_stamps_status(self):
        """Test that cancelled/dropped actions stamp a distinct status."""
        from schedule_management.commands.history import _pair_task_activities

        entries = [
            {
                "timestamp": "2026-06-01T09:00:00",
                "action": "added",
                "task": {"description": "Done task", "priority": 8},
            },
            {
                "timestamp": "2026-06-01T10:00:00",
                "action": "deleted",
                "task": {"description": "Done task", "priority": 8},
            },
            {
                "timestamp": "2026-06-01T09:00:00",
                "action": "added",
                "task": {"description": "Mistake task", "priority": 5},
            },
            {
                "timestamp": "2026-06-01T11:00:00",
                "action": "cancelled",
                "task": {"description": "Mistake task", "priority": 5},
            },
            {
                "timestamp": "2026-06-01T09:00:00",
                "action": "added",
                "task": {"description": "Give up task", "priority": 3},
            },
            {
                "timestamp": "2026-06-01T12:00:00",
                "action": "dropped",
                "task": {"description": "Give up task", "priority": 3},
            },
        ]

        activities = _pair_task_activities(entries)

        assert len(activities) == 3
        by_desc = {a["description"]: a for a in activities}
        assert by_desc["Done task"]["status"] == "completed"
        assert by_desc["Mistake task"]["status"] == "cancelled"
        assert by_desc["Give up task"]["status"] == "dropped"

    def test_activity_status_style_for_each_status(self):
        """Test that completed/cancelled/dropped map to distinct render styles."""
        from schedule_management.commands.history import _activity_status_style

        # completed: no special icon/tag/strike
        icon, tag, style, strike = _activity_status_style("completed")
        assert icon == ""
        assert tag == ""
        assert strike is False

        # cancelled: explicit icon + strike-through
        icon, tag, style, strike = _activity_status_style("cancelled")
        assert icon == "🚫"
        assert strike is True

        # dropped: explicit icon, no strike
        icon, tag, style, strike = _activity_status_style("dropped")
        assert icon == "🏳️"
        assert strike is False

    def test_format_duration(self):
        """Test duration formatting helper."""
        from schedule_management.commands.history import _format_duration

        assert _format_duration(
            datetime(2026, 6, 1, 9, 0), datetime(2026, 6, 1, 9, 30)
        ) == "30m"
        assert _format_duration(
            datetime(2026, 6, 1, 9, 0), datetime(2026, 6, 1, 11, 15)
        ) == "2h 15m"
        assert _format_duration(
            datetime(2026, 6, 1, 9, 0), datetime(2026, 6, 3, 9, 0)
        ) == "2d"
        assert _format_duration(
            datetime(2026, 6, 1, 9, 0), datetime(2026, 6, 1, 9, 0)
        ) == "< 1 min"
