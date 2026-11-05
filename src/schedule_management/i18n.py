"""
Internationalization (i18n) - Bilingual (Chinese & English) support.

This module provides translation support for the schedule management system.
The active language is determined by:
1. Environment variable REMINDER_LANG (e.g., 'zh' or 'en')
2. Configuration setting 'language' under [settings] in settings.toml
3. Default fallback is 'en' (English).
"""

import os
import tomllib
from pathlib import Path
from schedule_management.config_layout import resolve_runtime_paths


def get_language() -> str:
    """
    Determine the active language ('en' or 'zh').

    Priority:
    1. Environment variable REMINDER_LANG
    2. Configuration option 'language' in settings.toml
    3. Default to 'en'
    """
    # 1. Environment variable override (handy for testing)
    if env_lang := os.getenv("REMINDER_LANG"):
        if env_lang.lower() in ("zh", "cn", "chinese"):
            return "zh"
        return "en"

    # 2. Check settings.toml
    try:
        settings_path = resolve_runtime_paths().settings_path
        if settings_path.exists():
            with open(settings_path, "rb") as f:
                config = tomllib.load(f)
                lang = config.get("settings", {}).get("language", "en")
                if isinstance(lang, str) and lang.lower() in ("zh", "cn", "chinese"):
                    return "zh"
    except Exception:
        pass

    return "en"


# Centralized dictionary mapping English source strings to Chinese translations.
ZH_TRANSLATIONS = {
    # -------------------------------------------------------------------------
    # CLI Help & Global Messages
    # -------------------------------------------------------------------------
    "rmd CLI": "rmd 命令行工具",
    "Manage your schedule management system": "管理您的日程管理系统",
    "Configuration root:": "配置根目录：",
    "Active config:": "当前活动配置：",
    "Available commands": "可用命令",
    "❌ Operation cancelled by user": "❌ 操作已被用户取消",
    "❌ Unexpected error: {e}": "❌ 非预期错误：{e}",
    "Please install the 'rich' library: pip install rich": "请安装 'rich' 库：pip install rich",

    # -------------------------------------------------------------------------
    # Tasks Command (tasks.py)
    # -------------------------------------------------------------------------
    "❌ Error: the following arguments are required: {missing}": "❌ 错误：以下参数是必填的：{missing}",
    "Let's add a new task to your schedule! 🚀\n": "让我们为您的日程添加一个新任务！🚀\n",
    "I'll guide you through a couple of quick questions. 🌟": "我将引导您完成几个快速问题。🌟",
    "Almost there! Let's complete the details for your task. ✨\n": "快好了！让我们为您的任务补充细节。✨\n",
    "Task: {task_description}": "任务: {task_description}",
    "Task Creator Wizard": "任务创建向导",
    "✍️  What task would you like to add? ": "✍️ 您想添加什么任务？",
    "⚠️  Oops! The task description can't be empty. Let's try that again! 😊": "⚠️ 哎呀！任务描述不能为空。让我们再试一次！😊",
    "🔢 What is the priority level for this task (1-10)? ": "🔢 这个任务的优先级是多少 (1-10)？",
    "⚠️  Oops! Priority must be between 1 and 10. Let's try that again! 🌟": "⚠️ 哎呀！优先级必须在 1 到 10 之间。让我们再试一次！🌟",
    "⚠️  Oops! Priority needs to be a valid number. Please enter a number between 1 and 10! 🔢": "⚠️ 哎呀！优先级必须是一个有效的数字。请输入 1 到 10 之间的数字！🔢",
    "🔢 Select a task type:": "🔢 选择任务类型：",
    "Enter Task Type Number: ": "请输入任务类型编号：",
    "💡 Tip: enter a number not listed above to create a new task type.": "💡 提示：输入上面未列出的数字可创建新的任务类型。",
    "⚠️  Oops! Please enter a positive number (an existing one or a new one to create a type).": "⚠️ 哎呀！请输入一个正整数（已有的编号，或一个新编号来创建类型）。",
    "✨ New task type! Let's create type {type_id}.": "✨ 新的任务类型！让我们创建类型 {type_id}。",
    "🏷️  Enter a name for task type {type_id}: ": "🏷️ 请为任务类型 {type_id} 输入名称：",
    "⚠️  Oops! Task type name can't be empty. Let's try that again! 😊": "⚠️ 哎呀！任务类型名称不能为空。让我们再试一次！😊",
    "⚠️  Oops! A task type with that name already exists. Let's try that again! 😊": "⚠️ 哎呀！该名称的任务类型已存在。让我们再试一次！😊",
    "✅ Task type '{type_name}' (number {type_id}) created and saved!": "✅ 任务类型 '{type_name}'（编号 {type_id}）已创建并保存！",
    "⚠️  Could not save the new task type to settings.toml; using it for this task only.": "⚠️ 无法将新任务类型保存到 settings.toml；仅为本次任务使用。",
    "⚠️  Oops! Invalid selection. Please choose a valid number from the list.": "⚠️ 哎呀！选择无效。请从列表中选择一个有效的编号。",
    "❌ Error: Invalid task type. Choose from: {choices}": "❌ 错误：无效的任务类型。可选范围：{choices}",
    "🗓️  Postpone the daily urgent alarm? Enter days from now (0 or empty = start today): ": "🗓️ 要推迟每日紧急提醒吗？请输入从今天起的天数（0 或留空 = 今天开始）：",
    "Postpone days (default 0): ": "推迟天数（默认 0）：",
    "⚠️  Oops! Postpone days must be a non-negative integer. Let's try that again! 🌟": "⚠️ 哎呀！推迟天数必须是非负整数。让我们再试一次！🌟",
    "⚠️  Oops! Postpone days needs to be a valid number. Please enter a non-negative integer! 🔢": "⚠️ 哎呀！推迟天数必须是一个有效的数字。请输入非负整数！🔢",
    "Legend:": "图例：",
    "👋 Operation cancelled. Have a great day! ✨": "👋 操作已取消。祝您度过美好的一天！✨",
    "❌ Error: Priority must be a positive integer": "❌ 错误：优先级必须是正整数",
    "✅ Task '{task_description}' updated! Priority changed from {old_priority} to {priority}": "✅ 任务 '{task_description}' 已更新！优先级从 {old_priority} 修改为 {priority}",
    "⚠️  Warning: Could not log task update: {e}": "⚠️ 警告：无法记录任务更新日志：{e}",
    "✅ Task '{task_description}' added successfully with priority {priority}!": "✅ 任务 '{task_description}' 添加成功，优先级为 {priority}！",
    "⚠️  Warning: Could not log task addition: {e}": "⚠️ 警告：无法记录任务添加日志：{e}",
    "❌ Error saving task: {e}": "❌ 保存任务错误：{e}",
    "⚠️  No tasks found to delete": "⚠️ 未找到要删除的任务",
    "❌ Invalid task ID: {task_id}. Please use a number between 1 and {length}": "❌ 无效的任务 ID：{task_id}。请使用 1 到 {length} 之间的数字",
    "❌ Task '{task_description}' not found": "❌ 未找到任务 '{task_description}'",
    "⚠️  Warning: Could not log task deletion: {e}": "⚠️ 警告：无法记录任务删除日志：{e}",
    "✅ {deletion} deleted successfully!": "✅ {deletion} 删除成功！",
    "✅ {count} sets of tasks deleted successfully:": "✅ {count} 组任务删除成功：",
    "🚫 {deletion} cancelled (not counted as done).": "🚫 {deletion} 已取消（不计入完成）。",
    "🚫 {count} sets of tasks cancelled (not counted as done):": "🚫 {count} 组任务已取消（不计入完成）：",
    "🏳️ {deletion} dropped (gave up).": "🏳️ {deletion} 已放弃（放弃处理）。",
    "🏳️ {count} sets of tasks dropped (gave up):": "🏳️ {count} 组任务已放弃（放弃处理）：",
    "❌ Error saving tasks: {e}": "❌ 保存任务错误：{e}",
    # History status tags shown next to non-completed activities
    "cancelled": "已取消",
    "dropped": "已放弃",
    "Task '{task_description}'": "任务 '{task_description}'",
    "{deleted_count} tasks with description '{task_description}'": "{deleted_count} 个描述为 '{task_description}' 的任务",
    " (deferred today)": " (今天已延期)",
    " (1 day overdue)": " (逾期 1 天)",
    " ({age_days} days overdue)": " (逾期 {age_days} 天)",
    " (coming tomorrow)": " (明天开始)",
    " (coming in {days_left} days)": " ({days_left} 天后开始)",
    "📋 No tasks found": "📋 未找到任务",
    "Current Task List": "当前任务列表",
    "ID": "ID",
    "Priority": "优先级",
    "Description": "描述",
    "Total tasks: {count}": "任务总数：{count}",
    "❌ Unknown task type '{type_filter}'. Valid types: {valid}": "❌ 未知的任务类型 '{type_filter}'。有效类型：{valid}",
    "📋 No tasks found for type '{type_name}'": "📋 未找到类型为 '{type_name}' 的任务",
    "Showing {shown} of {total} tasks (type: {type_name})": "显示 {shown} / {total} 个任务（类型：{type_name}）",

    # -------------------------------------------------------------------------
    # Deadlines Command (deadlines.py)
    # -------------------------------------------------------------------------
    "❌ Error: Date must be in format M.D or MM.DD (e.g., 7.4 or 07.04)": "❌ 错误：日期格式必须为 M.D 或 MM.DD (例如 7.4 或 07.04)",
    "❌ Error: Month must be between 1 and 12": "❌ 错误：月份必须在 1 到 12 之间",
    "❌ Error: Day must be between 1 and 31": "❌ 错误：日期必须在 1 到 31 之间",
    "❌ Error: Invalid date format - {e}": "❌ 错误：无效的日期格式 - {e}",
    "✅ Deadline '{event}' added successfully for {date}!": "✅ 截止日期 '{event}' 已成功添加至 {date}！",
    "❌ Error saving deadline: {e}": "❌ 保存截止日期错误：{e}",
    "✅ Deadline for '{event_name}' updated from {old_date} to {deadline_str}": "✅ '{event_name}' 的截止日期已从 {old_date} 更新为 {deadline_str}",
    "⚠️ OVERDUE": "⚠️ 已过期",
    "🔴 TODAY": "🔴 今天",
    "🔴 URGENT": "🔴 紧急",
    "🟡 SOON": "🟡 临近",
    "🟢 OK": "🟢 正常",
    "Deadline '{event}'": "截止日期 '{event}'",
    "{deleted_count} deadlines with name '{event}'": "{deleted_count} 个名为 '{event}' 的截止日期",
    "[bold red]❌ Error saving pruned deadlines: {e}[/bold red]": "[bold red]❌ 保存修剪后的截止日期错误：{e}[/bold red]",
    "[bold yellow]📅 No deadlines found[/bold yellow]": "[bold yellow]📅 未找到截止日期[/bold yellow]",
    "Current Deadlines": "当前截止日期",
    "Event": "事件",
    "Due Date": "截止日期",
    "Days Left": "剩余天数",
    "Urgent": "紧急",
    "Passed!": "已过期！",
    "Today!": "今天！",
    "Tomorrow!": "明天！",
    "{days} days left": "剩余 {days} 天",
    "Total deadlines: {count}": "截止日期总数：{count}",
    "⚠️  No deadlines found to delete": "⚠️ 未找到要删除的截止日期",
    "❌ Deadline '{event}' not found": "❌ 未找到截止日期 '{event}'",
    "✅ {deletion} deleted successfully!": "✅ {deletion} 删除成功！",
    "✅ {count} deadlines deleted successfully:": "✅ {count} 个截止日期已成功删除：",
    "❌ Error saving deadlines: {e}": "❌ 保存截止日期错误：{e}",

    # -------------------------------------------------------------------------
    # Habits Command (habits.py)
    # -------------------------------------------------------------------------
    "Habits for today:": "今日习惯：",
    "❌ Error: No habits configured. Please create config/habits.toml": "❌ 错误：未配置习惯。请创建 config/habits.toml",
    "⚠️  Warning: Invalid habit IDs: {ids}": "⚠️ 警告：无效的习惯 ID：{ids}",
    "Available habits: {habits}": "可用的习惯：{habits}",
    "✅ Updated habit record for {today}": "✅ 已更新 {today} 的习惯记录",
    "Previously completed: {count} habits": "之前已完成：{count} 个习惯",
    "✅ Recorded habit tracking for {today}": "✅ 已记录 {today} 的习惯追踪",
    "Completed habits today: {count}": "今天已完成习惯：{count}",
    "❌ Error saving habit records: {e}": "❌ 保存习惯记录错误：{e}",
    "Did you complete this habit today?": "今天你完成这个习惯了吗？",
    "Habit Tracker ({i}/{total_habits})": "习惯追踪 ({i}/{total_habits})",
    "Enter completed habit IDs (space-separated), or press Enter for none: ": "输入已完成的习惯 ID（用空格分隔），或者按回车键表示没有：",
    "❌ Could not open a habit prompt window. Provide habit IDs, e.g. `rmd track 1 2`.": "❌ 无法打开习惯选择窗口。请提供习惯 ID，例如 `rmd track 1 2`。",

    # -------------------------------------------------------------------------
    # Status & View Commands (status.py)
    # -------------------------------------------------------------------------
    "❌ Currently in p mode. Switch back to j mode to execute this command.": "❌ 当前处于 p mode，若想使用则切换到 j mode",
    "📊 Generating schedule visualizations...": "📊 正在生成日程可视化...",
    "\n📁 Visualization file generated:": "\n📁 可视化文件已生成：",
    "\n🖼️  Opening visualization...": "\n🖼️ 正在打开可视化...",
    "⚠️  Could not open file: {e}": "⚠️ 无法打开文件：{e}",
    "❌ Error generating visualizations: {e}": "❌ 生成可视化错误：{e}",
    "📅 {parity} Week": "📅 {parity}周",
    "Odd": "单数",
    "Even": "双数",
    "⏭️  Today is a skipped day - enjoy your time off!": "⏭️ 今天是跳过的休息日 - 享受您的闲暇时光！",
    "[bold green]🔔 NOW:[/bold green]  {event}": "[bold green]🔔 当前:[/bold green]  {event}",
    "[bold yellow]🟡 IDLE[/bold yellow]": "[bold yellow]🟡 空闲[/bold yellow]",
    " (in {time_until})": " (在 {time_until} 后)",
    "[bold blue]⏰ NEXT:[/bold blue] {event}{time_str}": "[bold blue]⏰ 下一个:[/bold blue] {event}{time_str}",
    "[dim]📭 No upcoming events[/dim]": "[dim]📭 无后续事件[/dim]",
    "Status": "状态",
    "Time": "时间",
    "Activity": "活动",
    "Morning": "上午",
    "Afternoon": "下午",
    "Evening": "晚上",
    "Total events: {count}": "事件总数：{count}",
    "[bold red]❌ Error checking status:[/bold red] {e}": "[bold red]❌ 检查状态错误:[/bold red] {e}",
    "{event} at {time}": "{time} {event}",
    "{hours}h {minutes}m": "{hours}小时 {minutes}分钟",
    "{minutes}m": "{minutes}分钟",
    "[bold yellow]🟡 IDLE[/bold yellow]": "[bold yellow]🟡 空闲[/bold yellow]",
    "[bold blue]⏰ NEXT:[/bold blue] {event}{time_str}": "[bold blue]⏰ 下一个:[/bold blue] {event}{time_str}",
    "[dim]📭 No upcoming events[/dim]": "[dim]📭 无后续事件[/dim]",
    "Status": "状态",
    "Time": "时间",
    "Activity": "活动",
    "Morning": "上午",
    "Afternoon": "下午",
    "Evening": "晚上",
    "Total events: {count}": "事件总数：{count}",

    # -------------------------------------------------------------------------
    # -------------------------------------------------------------------------
    # Service Command (service.py)
    # -------------------------------------------------------------------------
    "📥 Updating schedule files...": "📥 正在更新日程文件...",
    "❌ Config directory not found: {config_dir}": "❌ 未找到配置目录：{config_dir}",
    "❌ Git not found. Please install git to use update.": "❌ 未找到 Git。请安装 git 以使用 update 命令。",
    "✅ Successfully pulled latest changes": "✅ 成功拉取最新更改",
    "   Already up to date.": "   已经是最新的。",
    "❌ Git pull failed:": "❌ Git 拉取失败：",
    "ℹ️  Config directory is not a git repository: {config_dir}": "ℹ️ 配置目录不是 git 仓库：{config_dir}",
    "   Skipping git pull and using local schedule files as-is.": "   跳过 git pull，直接使用本地日程文件。",
    "✅ Reminder service restarted": "✅ 提醒服务已重启",
    "ℹ️  No installer restart script found.": "ℹ️ 未找到安装器重启脚本。",
    "   Restart the reminder service manually if it is already running.": "   如果服务已在运行，请手动重启提醒服务。",
    "❌ Reminder service restart failed:": "❌ 提醒服务重启失败：",
    "✅ Update finished": "✅ 更新完成",
    "❌ Reminder service is not running": "❌ 提醒服务未运行",
    "✅ Successfully stopped reminder service": "✅ 成功停止提醒服务",
    "❌ Failed to stop reminder service:": "❌ 停止提醒服务失败：",
    "❌ Failed to switch configuration: {e}": "❌ 切换配置失败：{e}",
    "✅ Successfully switched active configuration to {config_id}": "✅ 成功将活动配置切换为 {config_id}",
    "✅ Reminder service reloaded with new configuration": "✅ 提醒服务已重载新配置",
    "📊 Generating weekly report for {date}...": "📊 正在生成 {date} 的周报...",
    "📊 Generating monthly report for {date}...": "📊 正在生成 {date} 的月报...",
    "✅ Report generated successfully at {path}!": "✅ 报告生成成功，位于 {path}！",
    "❌ Error generating report: {e}": "❌ 生成报告错误：{e}",
    "✅ Mode switched to {mode} mode successfully!": "✅ 成功切换至 {mode} 模式！",
    "Current mode is {mode} mode": "当前模式为 {mode} 模式",
    "❌ Invalid file type: {file}. Choose from: settings, odd, even, deadlines, habits": "❌ 无效的文件类型：{file}。可选项有：settings, odd, even, deadlines, habits",
    "📝 Opening {file} configuration in editor...": "📝 正在编辑器中打开 {file} 配置...",
    "❌ Error opening editor: {e}": "❌ 打开编辑器错误：{e}",
    "❌ No config sets found under: {config_root_dir}": "❌ 未在 {config_root_dir} 下找到配置集",
    "   Create or migrate a schedule first so user_config_0 exists.": "   请先创建或迁移日程，以便存在 user_config_0。",
    "❌ Invalid config id: {config_id}": "❌ 无效的配置 ID：{config_id}",
    "   Valid config ids: {ids}": "   有效的配置 ID：{ids}",
    "✅ Switched to user_config_{requested_id}": "✅ 已切换至 user_config_{requested_id}",
    "   Active config directory: {directory}": "   当前活动配置目录：{directory}",
    "🛑 Stopping reminder service...": "🛑 正在停止提醒服务...",
    "⚠️  No running reminder-runner process found.": "⚠️ 未找到正在运行的 reminder-runner 进程。",
    "✅ Stopped reminder-runner (PID: {pid})": "✅ 已停止 reminder-runner (PID: {pid})",
    "⚠️  Could not stop PID {pid}: {e}": "⚠️ 无法停止 PID {pid}：{e}",
    "⚠️  No reminder-runner processes were stopped.": "⚠️ 没有 reminder-runner 进程被停止。",
    "   Total processes stopped: {count}": "   共停止进程数：{count}",
    "❌ 'pgrep' command not found.": "❌ 未找到 'pgrep' 命令。",
    "   Try finding the process manually with 'ps aux | grep reminder-runner'": "   请尝试使用 'ps aux | grep reminder-runner' 手动查找进程",
    "❌ Error stopping service: {e}": "❌ 停止服务错误：{e}",
    "📊 Generating report...": "📊 正在生成报告...",
    "❌ Invalid date format: {date}": "❌ 无效的日期格式：{date}",
    "   Use YYYY-MM-DD format (e.g., 2024-01-15)": "   使用 YYYY-MM-DD 格式 (例如 2024-01-15)",
    "❌ Unsupported report type: {type}": "❌ 不支持的报告类型：{type}",
    "❌ Custom day ranges are not supported for weekly reports.": "❌ 周报不支持自定义天数范围。",
    "   Use '--days 7' or omit the flag.": "   请使用 '--days 7' 或省略该标志。",
    "❌ '--days' is not supported for monthly reports.": "❌ 月报不支持 '--days' 标志。",
    "   Report type: {type}": "   报告类型：{type}",
    "   Target date: {date}": "   目标日期：{date}",
    "✅ Report generated: {path}": "✅ 报告已生成：{path}",
    "⚠️  Report generation completed but no file was created.": "⚠️ 报告生成已完成，但未创建任何文件。",
    "❌ Missing dependency for report generation: {e}": "❌ 报告生成缺少依赖项：{e}",
    "❌ Unknown file: {file}": "❌ 未知文件：{file}",
    "   Available: {files}": "   可用文件：{files}",
    "⚠️  File does not exist: {path}": "⚠️ 文件不存在：{path}",
    "   Creating empty file...": "   正在创建空文件...",
    "❌ No editor found. Set $EDITOR environment variable.": "❌ 未找到编辑器。请设置 $EDITOR 环境变量。",
    "   File path: {path}": "   文件路径：{path}",
    "📝 Opening {file} in {editor}...": "📝 正在 {editor} 中打开 {file}...",
    "❌ Could not open editor: {e}": "❌ 无法打开编辑器：{e}",
    "❌ Invalid mode: {mode}. Only 'j' or 'p' is supported.": "❌ 无效的模式：{mode}。仅支持 'j' 或 'p'。",
    "Already in {mode} mode": "当前已处于 {mode} 模式",
    "❌ Failed to switch mode: {e}": "❌ 切换模式失败：{e}",
    "❌ Error updating: {e}": "❌ 更新错误：{e}",

    # -------------------------------------------------------------------------
    # Sync Command (sync.py): today's focus-block plan
    # -------------------------------------------------------------------------
    "Failed to load today's schedule:": "加载今日日程失败：",
    "Today is a skipped day. Nothing to sync.": "今天是跳过的休息日。无需同步。",
    "No untitled pomodoro/potato blocks need syncing today.": "今天没有未命名的 pomodoro/potato 专注时间块需要规划。",
    "Currently in p mode. Switch back to j mode to execute this command.": "当前处于 p mode，若想使用则切换到 j mode",
    "Today's focus blocks": "今日专注时间块",
    "Block": "时间块",
    "Planned task": "计划任务",
    "No plan yet. Ask your AI agent to plan today with the Schedule Everything plugin, or apply a plan with `rmd sync --apply plan.json`.": "还没有计划。请让你的 AI 智能体通过 Schedule Everything 插件规划今天，或使用 `rmd sync --apply plan.json` 应用计划。",
    "Saved today's plan with {count} focus blocks to {path}.": "已将包含 {count} 个专注时间块的今日计划保存至 {path}。",
    "Cleared today's plan.": "已清除今日计划。",
    "There was no saved plan.": "没有已保存的计划。",

    # -------------------------------------------------------------------------
    # Popups, Reminders, Platform & Run Wizards
    # -------------------------------------------------------------------------
    "📋 Daily Completed Tasks\n\n✨ No tasks completed today. Keep it up tomorrow!": "📋 今日完成任务\n\n✨ 今天没有完成的任务哦，明天继续加油！",
    "{index}. {description} (Priority: {priority})": "{index}. {description} (优先级: {priority})",
    "📋 Daily Completed Tasks Summary\n\n🎉 Completed {count} tasks today:\n\n": "📋 今日完成任务总结\n\n🎉 今天完成了 {count} 个任务：\n\n",
    "Unknown Task": "未知任务",
    "Reminder": "提醒",
    "Dismiss": "忽略",
    "Daily Review": "每日总结",
    "Weekly Review": "周度总结",
    "Monthly Review": "月度总结",
    "Urgent Task Reminders": "紧急任务提醒",
    "Deadline Reminders": "截止日期提醒",
    "Habit Tracking Prompt": "习惯追踪提示",
    "Do you want to dismiss?": "您要忽略此提醒吗？",
    "{title} ⏱️ ({duration}min)": "{title} ⏱️ ({duration}分钟)",
    "Start": "开始",
    "{title} finished! Take a break 🎉": "{title} 结束！休息一下 🎉",
    "Deferred today": "今天已延期",
    "Procrastinated for 1 day": "已拖延 1 天",
    "Procrastinated for {age_days} days": "已拖延 {age_days} 天",
    "Did you complete this task?": "你完成这个任务了吗？",
    "Priority: {priority}": "优先级: {priority}",
    "Urgent Task ({index}/{total_tasks})": "紧急任务 ({index}/{total_tasks})",
    "Unknown Event": "未知事件",
    "⚠️ {event} - {deadline_str} (Overdue {days_left} days)": "⚠️ {event} - {deadline_str} (已逾期 {days_left} 天)",
    "🔴 {event} - {deadline_str} (Due today)": "🔴 {event} - {deadline_str} (今天截止)",
    "🚨 {event} - {deadline_str} ({days_left} days left)": "🚨 {event} - {deadline_str} (剩余 {days_left} 天)",
    "📅 Urgent Deadline Reminder\n\n": "📅 紧急DDL提醒\n\n",
    "Urgent Deadlines": "紧急截止日期",
    "End Reminder": "结束提醒",
    "Input stream closed; using default answer.": "输入流已关闭；正在使用默认答案。",
    "Please answer with 'y' or 'n'.": "请使用 'y' 或 'n' 回答。",
    "{config_name} is missing. Create it from {template_name}?": "{config_name} 丢失。是否从 {template_name} 创建它？",
    "Cannot continue without {config_name}.": "没有 {config_name}，无法继续。",
    "Created {config_path}": "已创建 {config_path}",
    "Invalid TOML in {settings_path}: {exc}": "{settings_path} 中有无效的 TOML：{exc}",
    "settings_template.toml is unavailable; cannot auto-repair settings.toml": "settings_template.toml 不可用；无法自动修复 settings.toml",
    "Replace settings.toml with settings_template.toml and continue?": "是否使用 settings_template.toml 替换 settings.toml 并继续？",
    "Configuration check complete. No missing keys found.": "配置检查完成。未发现丢失的键。",
    "Found {count} missing configuration value(s).": "发现了 {count} 个丢失的配置值。",
    "Missing [{section}].{key}; using default {default_display}": "丢失 [{section}].{key}；正在使用默认值 {default_display}",
    "Enter value for [{section}].{key} (press Enter for {default_display}): ": "输入 [{section}].{key} 的值 (按回车键使用 {default_display}): ",
    "Invalid value: {exc}": "无效的值：{exc}",
    "Updated {settings_path} with {count} missing value(s).": "已更新 {settings_path}，包含 {count} 个丢失的值。",
    "Interactive prompts require a TTY. Re-run with --yes to accept defaults.": "交互式提示符需要一个 TTY。请携带 --yes 重新运行以接受默认值。",
    "Missing required template: {path}": "缺少必要的模板：{path}",
    "Failed to load settings template: {exc}": "加载设置模板失败：{exc}",
    "settings_template.toml not found; skipping missing-key checks.": "未找到 settings_template.toml；跳过丢失键检查。",
}


def _t(text: str) -> str:
    """
    Translate English source text to the active language (Chinese 'zh' or original English 'en').
    """
    if get_language() == "zh":
        return ZH_TRANSLATIONS.get(text, text)
    return text
