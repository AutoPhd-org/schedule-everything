"""
Schedule Visualizer - PDF generation for schedule visualization.

This module provides the ScheduleVisualizer class which generates
beautiful PDF visualizations of weekly schedules using Matplotlib.

The visualizer creates a multi-page PDF containing:
- Page 1: Odd week schedule (color-coded blocks by activity type)
- Page 2: Even week schedule
- Page 3: Statistics comparison (work hours, pomodoros, etc.)

Features:
- Modern, vibrant color palette for different activity types
- Automatic duration detection from config or defaults
- Time-of-day grid layout (6 AM to midnight)
- Weekly statistics summary

Example Usage:
    >>> from schedule_management.visualizer import ScheduleVisualizer
    >>> from schedule_management.config import ScheduleConfig, WeeklySchedule
    >>>
    >>> config = ScheduleConfig('settings.toml')
    >>> weekly = WeeklySchedule('odd_weeks.toml', 'even_weeks.toml')
    >>> visualizer = ScheduleVisualizer(config, weekly.odd_data, weekly.even_data)
    >>> visualizer.visualize()  # Creates PDF on Desktop

Dependencies:
    - matplotlib: Required for PDF generation
    - matplotlib.backends.backend_pdf: For multi-page PDF support
"""

from pathlib import Path
from typing import Any

# Conditional import for matplotlib (may not be available)
try:
    import matplotlib.pyplot as plt
    import matplotlib.patches as patches
    from matplotlib.backends.backend_pdf import PdfPages

    MATPLOTLIB_AVAILABLE = True
except ImportError:
    MATPLOTLIB_AVAILABLE = False


class ScheduleVisualizer:
    """
    Generates beautiful PDF visualizations of weekly schedules.

    This class creates a multi-page PDF with visual representations
    of odd and even week schedules, along with statistics.

    Attributes:
        config: ScheduleConfig instance with time block durations
        odd_schedule: Dictionary containing odd week schedule data
        even_schedule: Dictionary containing even week schedule data

    Color Palette:
        The visualizer uses a modern pastel palette:
        - Pomodoro: Soft Red (#FF6B6B)
        - Potato: Deeper Pink (#EE5253)
        - Long Break: Bright Teal (#1DD1A1)
        - Short Break: Light Blue (#48DBFB)
        - Napping: Blue (#54A0FF)
        - Meeting: Orange (#FF9F43)
        - Exercise: Yellow (#FECA57)
        - Lunch: Deep Purple (#5F27CD)
        - Other: Blue Grey (#8395A7)
    """

    # ==========================================================================
    # COLOR CONFIGURATION
    # ==========================================================================

    # Modern, vibrant pastel palette for activity blocks
    COLORS = {
        "pomodoro": "#FF6B6B",  # Soft Red - focused work sessions
        "potato": "#EE5253",  # Deeper Red/Pink - longer work blocks
        "long_break": "#1DD1A1",  # Bright Teal - extended rest periods
        "short_break": "#48DBFB",  # Light Blue - quick breaks
        "napping": "#54A0FF",  # Blue - rest/nap time
        "meeting": "#FF9F43",  # Orange - meetings/calls
        "exercise": "#Feca57",  # Yellow - physical activity
        "lunch": "#5F27CD",  # Deep Purple - meal time
        "summary_time": "#C8D6E5",  # Light Grey - review/planning
        "go_to_bed": "#576574",  # Dark Grey - bedtime
        "other": "#8395A7",  # Blue Grey - default/other
        "deep_work": "#0ABDE3",  # Cyan - deep focus sessions
    }

    # Text colors for contrast (white for dark backgrounds, dark for light)
    TEXT_COLORS = {
        "exercise": "#333333",  # Dark text on yellow background
        "summary_time": "#333333",  # Dark text on light grey
        "default": "#FFFFFF",  # White text for most blocks
    }

    # Default durations (minutes) for common activity types
    # Used when config doesn't specify duration
    DEFAULT_DURATIONS = {
        "potato": 50,  # Long work session
        "pomodoro": 25,  # Standard pomodoro
        "long_break": 15,  # Extended break
        "short_break": 5,  # Quick break
        "lunch": 60,  # Lunch hour
        "napping": 20,  # Power nap
    }

    # Days of the week in order (Monday first)
    DAYS_ORDER = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ]

    def __init__(self, config, odd_schedule: dict, even_schedule: dict):
        """
        Initialize the visualizer with schedule data.

        Args:
            config: ScheduleConfig instance containing time_blocks durations
            odd_schedule: Dict with odd week schedule data (by day)
            even_schedule: Dict with even week schedule data (by day)
        """
        self.config = config
        self.odd_schedule = odd_schedule
        self.even_schedule = even_schedule

    # ==========================================================================
    # ACTIVITY PARSING HELPERS
    # ==========================================================================

    def _extract_activity_name(self, activity: Any) -> str:
        """
        Extract the display name from an activity definition.

        Activities can be:
        - String: Activity name directly (e.g., 'pomodoro')
        - Dict: {'block': 'pomodoro', 'title': 'Morning Focus'}

        Args:
            activity: Activity definition (string or dict)

        Returns:
            The activity name for display
        """
        if isinstance(activity, str):
            return activity
        elif isinstance(activity, dict) and "block" in activity:
            # Use custom title if provided, otherwise use block type
            return activity.get("title", activity["block"])
        else:
            return str(activity)

    def _get_activity_duration(self, activity_name: str) -> float:
        """
        Get the duration of an activity in HOURS.

        Looks up duration from multiple sources:
        1. Config time_blocks (exact match)
        2. Default durations (partial match)
        3. Generic fallback (30 minutes)

        Args:
            activity_name: Name of the activity

        Returns:
            Duration in hours (e.g., 0.5 for 30 minutes)
        """
        # 1. Check config for exact match
        if activity_name in self.config.time_blocks:
            return self.config.time_blocks[activity_name] / 60.0

        # 2. Check default durations (case-insensitive partial match)
        lower_name = activity_name.lower()
        for key, minutes in self.DEFAULT_DURATIONS.items():
            if key in lower_name:
                return minutes / 60.0

        # 3. Generic fallback ensures visibility in chart
        return 0.5  # 30 minutes

    def _get_color(self, activity_name: str) -> str:
        """
        Get the background color for an activity block.

        Args:
            activity_name: Name of the activity

        Returns:
            Hex color code (e.g., '#FF6B6B')
        """
        lower_name = activity_name.lower()
        for key, color in self.COLORS.items():
            if key in lower_name:
                return color
        return self.COLORS["other"]

    def _get_text_color(self, activity_name: str) -> str:
        """
        Get the text color for activity labels (for contrast).

        Args:
            activity_name: Name of the activity

        Returns:
            Hex color code for text
        """
        lower_name = activity_name.lower()
        for key, color in self.TEXT_COLORS.items():
            if key in lower_name:
                return color
        return self.TEXT_COLORS["default"]

    # ==========================================================================
    # CHART GENERATION
    # ==========================================================================

    def _create_chart(self, ax, schedule_data: dict, title: str) -> None:
        """
        Create a weekly schedule chart on the given axes.

        Draws a time-based grid with colored blocks for each activity.
        Days are columns, hours are rows.

        Args:
            ax: Matplotlib axes to draw on
            schedule_data: Schedule dict with 'common' and day-specific entries
            title: Title for the chart
        """
        if not MATPLOTLIB_AVAILABLE:
            return

        # Setup aesthetics - pure white background
        ax.set_facecolor("#FFFFFF")
        used_activities = set()

        # Draw alternating day column backgrounds
        for i in range(0, len(self.DAYS_ORDER), 2):
            ax.axvspan(i - 0.5, i + 0.5, color="#F7F9FA", zorder=0)

        # Draw activity blocks for each day
        for day_idx, day in enumerate(self.DAYS_ORDER):
            # Merge common schedule with day-specific overrides
            day_schedule = {}
            if "common" in schedule_data:
                day_schedule.update(schedule_data["common"])
            if day in schedule_data:
                day_schedule.update(schedule_data[day])

            # Draw each scheduled activity
            for time_str, activity in day_schedule.items():
                activity_name = self._extract_activity_name(activity)
                used_activities.add(activity_name)

                # Parse time string to decimal hours
                time_parts = time_str.split(":")
                hour = int(time_parts[0])
                minute = int(time_parts[1])
                time_decimal = hour + minute / 60.0

                # Get block dimensions and colors
                duration_hours = self._get_activity_duration(activity_name)
                bg_color = self._get_color(activity_name)
                txt_color = self._get_text_color(activity_name)

                # Draw the main colored block
                rect_width = 0.85
                rect_x = day_idx - (rect_width / 2)

                rect = patches.Rectangle(
                    (rect_x, time_decimal),
                    rect_width,
                    duration_hours,
                    linewidth=0,
                    facecolor=bg_color,
                    alpha=0.9,
                    zorder=2,
                )
                ax.add_patch(rect)

                # Add left accent border for depth effect
                rect_accent = patches.Rectangle(
                    (rect_x, time_decimal),
                    0.04,
                    duration_hours,
                    linewidth=0,
                    facecolor="black",
                    alpha=0.1,
                    zorder=3,
                )
                ax.add_patch(rect_accent)

                # Add text label if block is large enough
                if duration_hours >= 0.3:
                    font_size = 8 if duration_hours > 0.5 else 6
                    display_name = activity_name.replace("_", " ").title()

                    # Truncate for very short blocks
                    if duration_hours < 0.4:
                        display_name = display_name[:3]

                    ax.text(
                        day_idx,
                        time_decimal + (duration_hours / 2),
                        display_name,
                        ha="center",
                        va="center",
                        color=txt_color,
                        fontsize=font_size,
                        fontweight="bold",
                        zorder=4,
                    )

        # Configure axes limits (6 AM to midnight, top to bottom)
        ax.set_xlim(-0.5, len(self.DAYS_ORDER) - 0.5)
        ax.set_ylim(24, 6)

        # Day labels on X axis
        ax.set_xticks(range(len(self.DAYS_ORDER)))
        ax.set_xticklabels(
            [d.upper()[:3] for d in self.DAYS_ORDER],
            fontsize=11,
            weight="bold",
            color="#57606f",
        )
        ax.tick_params(axis="x", pad=10)

        # Hour labels on Y axis
        hour_ticks = list(range(6, 25))
        ax.set_yticks(hour_ticks)
        ax.set_yticklabels(
            [f"{h:02d}:00" for h in hour_ticks],
            fontsize=9,
            color="#a4b0be",
            family="monospace",
        )

        # Grid styling
        ax.grid(True, axis="y", linestyle=":", alpha=0.4, color="#dfe4ea", zorder=1)
        ax.grid(False, axis="x")

        # Remove spines for clean look
        for spine in ax.spines.values():
            spine.set_visible(False)
        ax.axvline(-0.5, color="#dfe4ea", linewidth=1)

        # Title
        ax.set_title(
            title.upper(),
            fontsize=20,
            weight="heavy",
            pad=25,
            color="#2f3542",
            loc="left",
        )

        # Legend with activity colors
        legend_elements = [
            patches.Patch(
                facecolor=self._get_color(act),
                label=act.replace("_", " ").title(),
                edgecolor="none",
            )
            for act in sorted(used_activities)
        ]

        if legend_elements:
            ax.legend(
                handles=legend_elements,
                loc="upper center",
                bbox_to_anchor=(0.5, -0.08),
                ncol=min(len(used_activities), 6),
                frameon=False,
                fontsize=8,
            )

    # ==========================================================================
    # STATISTICS CALCULATION
    # ==========================================================================

    def _calculate_weekly_stats(self, schedule_data: dict) -> dict[str, Any]:
        """
        Calculate statistics for a week's schedule.

        Args:
            schedule_data: Schedule dictionary for one week

        Returns:
            Dict with keys: pomodoro_count, potato_count, work_hours
        """
        pomodoro_count = 0
        potato_count = 0
        work_hours = 0.0

        work_activities = {"pomodoro", "potato", "deep_work", "meeting"}

        for day in self.DAYS_ORDER:
            day_schedule = {}
            if "common" in schedule_data:
                day_schedule.update(schedule_data["common"])
            if day in schedule_data:
                day_schedule.update(schedule_data[day])

            for _, activity in day_schedule.items():
                activity_name = self._extract_activity_name(activity)
                duration_hours = self._get_activity_duration(activity_name)

                # Count pomodoros and potatoes
                if "pomodoro" in activity_name.lower():
                    pomodoro_count += 1
                if "potato" in activity_name.lower():
                    potato_count += 1

                # Accumulate work hours
                if (
                    activity_name in work_activities
                    or "pomodoro" in activity_name.lower()
                    or "potato" in activity_name.lower()
                ):
                    work_hours += duration_hours

        return {
            "pomodoro_count": pomodoro_count,
            "work_hours": work_hours,
            "potato_count": potato_count,
        }

    def _create_stats_page(self, ax, odd_stats: dict, even_stats: dict) -> None:
        """
        Create a statistics comparison page.

        Shows side-by-side stats for odd and even weeks.

        Args:
            ax: Matplotlib axes to draw on
            odd_stats: Statistics dict for odd week
            even_stats: Statistics dict for even week
        """
        ax.axis("off")
        ax.set_xlim(0, 1)
        ax.set_ylim(0, 1)

        # Decorative background circle
        circle = patches.Circle((0.5, 0.5), 0.4, color="#F7F9FA", zorder=0)
        ax.add_patch(circle)

        # Title
        ax.text(
            0.5,
            0.92,
            "WEEKLY STATISTICS",
            ha="center",
            va="center",
            fontsize=22,
            weight="bold",
            color="#2f3542",
        )
        ax.plot([0.3, 0.7], [0.89, 0.89], color="#ff6b6b", linewidth=2)

        def draw_stat_column(x_pos: float, title: str, stats: dict, color_theme: str):
            """Draw a column of statistics."""
            # Header
            ax.text(
                x_pos,
                0.78,
                title,
                ha="center",
                fontsize=16,
                weight="bold",
                color=color_theme,
            )

            # Work Hours (large)
            ax.text(
                x_pos,
                0.65,
                f"{stats['work_hours']:.1f}h",
                ha="center",
                fontsize=45,
                weight="heavy",
                color="#2f3542",
            )
            ax.text(
                x_pos,
                0.58,
                "Work Time",
                ha="center",
                fontsize=12,
                color="#a4b0be",
                weight="medium",
            )

            # Pomodoro count
            ax.text(
                x_pos,
                0.45,
                f"{stats['pomodoro_count']}",
                ha="center",
                fontsize=45,
                weight="heavy",
                color="#2f3542",
            )
            ax.text(
                x_pos,
                0.38,
                "Pomodoros",
                ha="center",
                fontsize=12,
                color="#a4b0be",
                weight="medium",
            )

            # Potato count
            ax.text(
                x_pos,
                0.25,
                f"{stats['potato_count']}",
                ha="center",
                fontsize=45,
                weight="heavy",
                color="#2f3542",
            )
            ax.text(
                x_pos,
                0.18,
                "Potatoes",
                ha="center",
                fontsize=12,
                color="#a4b0be",
                weight="medium",
            )

        draw_stat_column(0.25, "ODD WEEK", odd_stats, "#ff6b6b")
        draw_stat_column(0.75, "EVEN WEEK", even_stats, "#54a0ff")

        # Vertical divider
        ax.plot([0.5, 0.5], [0.15, 0.8], color="#dfe4ea", linewidth=1, linestyle="--")

    # ==========================================================================
    # MAIN VISUALIZATION METHOD
    # ==========================================================================

    def visualize(self, output_path: Path | None = None) -> None:
        """
        Generate the complete schedule visualization PDF.

        Creates a multi-page PDF on the Desktop containing:
        - Page 1: Odd week schedule
        - Page 2: Even week schedule
        - Page 3: Statistics comparison

        The PDF is saved to ~/Desktop/schedule_visualization.pdf
        """
        if not MATPLOTLIB_AVAILABLE:
            print(
                "❌ matplotlib is not available. Please install it: pip install matplotlib"
            )
            return

        import platform

        # Determine output path
        if platform.system() == "Windows":
            desktop_path = Path.home() / "Desktop"
        else:
            desktop_path = Path.home() / "Desktop"

        pdf_filename = output_path or desktop_path / "schedule_visualization.pdf"
        pdf_filename.parent.mkdir(parents=True, exist_ok=True)

        # Configure fonts
        plt.rcParams["font.family"] = "sans-serif"
        plt.rcParams["font.sans-serif"] = ["Arial", "Helvetica", "DejaVu Sans"]

        # Generate multi-page PDF
        with PdfPages(pdf_filename) as pdf:
            # Page 1: Odd Week
            fig1, ax1 = plt.subplots(figsize=(14, 9))
            self._create_chart(ax1, self.odd_schedule, "Odd Week Schedule")
            plt.tight_layout()
            pdf.savefig(fig1, dpi=300, bbox_inches="tight")
            plt.close(fig1)

            # Page 2: Even Week
            fig2, ax2 = plt.subplots(figsize=(14, 9))
            self._create_chart(ax2, self.even_schedule, "Even Week Schedule")
            plt.tight_layout()
            pdf.savefig(fig2, dpi=300, bbox_inches="tight")
            plt.close(fig2)

            # Page 3: Statistics
            fig3 = plt.figure(figsize=(14, 9))
            ax3 = fig3.add_subplot(111)
            odd_stats = self._calculate_weekly_stats(self.odd_schedule)
            even_stats = self._calculate_weekly_stats(self.even_schedule)
            self._create_stats_page(ax3, odd_stats, even_stats)
            plt.tight_layout()
            pdf.savefig(fig3, dpi=300, bbox_inches="tight")
            plt.close(fig3)

        # Print completion message
        print(f"Schedule visualization saved as '{pdf_filename}'")
        print("\nSchedule visualization complete!")
        print("Generated file:")
        print("- schedule_visualization.pdf (on Desktop)")
        print("  - Page 1: Odd Week Schedule")
        print("  - Page 2: Even Week Schedule")
        print("  - Page 3: Statistics")
