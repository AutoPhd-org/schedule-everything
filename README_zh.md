<p align="center">
  <img src="documentation/static/img/logo.svg" alt="" width="88">
</p>

<h1 align="center">Schedule Everything · 晨钟暮鼓</h1>

<p align="center">
  <strong>让你的 AI 智能体规划一周，让持续的提醒帮你坚持下去。</strong><br>
  本地优先的周计划、任务清单与专注时间块提醒工具，配有色彩丰富的浏览器工作区和 <code>rmd</code> 命令行。
</p>

<p align="center">
  <a href="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/logic-tests.yml"><img src="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/logic-tests.yml/badge.svg" alt="Logic tests"></a>
  <a href="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/cli-tests.yml"><img src="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/cli-tests.yml/badge.svg" alt="CLI tests"></a>
  <a href="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/browser-tests.yml"><img src="https://github.com/AutoPhd-org/schedule-everything/actions/workflows/browser-tests.yml/badge.svg" alt="Browser tests"></a>
  <a href="https://pypi.org/project/schedule-management"><img src="https://badge.fury.io/py/schedule-management.svg" alt="PyPI version"></a>
  <a href="https://opensource.org/licenses/MIT"><img src="https://img.shields.io/badge/License-MIT-6250d6.svg" alt="MIT license"></a>
</p>

<p align="center">
  <a href="#快速开始">快速开始</a> ·
  <a href="#工作区一览">工作区一览</a> ·
  <a href="#常用命令">常用命令</a> ·
  <a href="https://autophd-org.github.io/schedule-everything/zh/">文档</a> ·
  <a href="README.md">English</a>
</p>

<p align="center">
  <img src="documentation/static/img/hero.webp" alt="Schedule Everything 浏览器工作区：J 模式概览、深色的每周概览，以及手机上的 P 模式" width="100%">
</p>

## 为什么选择 Schedule Everything

<table>
  <tr>
    <td width="33%" valign="top">
      <h3>🧠 智能体负责规划</h3>
      把你的生活讲给 Claude Code、Codex 或 Antigravity 听。<code>build-schedule</code> 技能会先了解你，再写出你的周计划；<code>plan-day</code> 会把今天的专注块安排成具体任务。
    </td>
    <td width="33%" valign="top">
      <h3>⏰ 不会被忽略的提醒</h3>
      后台服务会在每个时间块开始和结束时响铃并弹窗，直到你回应。截止日期、习惯和紧急任务也各有提醒。
    </td>
    <td width="33%" valign="top">
      <h3>🔒 本地纯文本</h3>
      周计划是 TOML，任务和历史是 JSON，全部保存在你的电脑上。应用本身不调用任何 AI，你的智能体使用它自己的模型服务。
    </td>
  </tr>
</table>

<details>
<summary><strong>健康的默认原则，而不只是把日程塞满</strong></summary>

当你的偏好还不完整时，`build-schedule` 会优先保护这些基于研究的默认原则：每晚至少 7 小时的睡眠机会并保持规律作息；每周 150–300 分钟中等强度活动，并在两天或以上安排力量训练；在长时间久坐中插入短暂活动休息；把高强度工作放在你最专注的时段；尽量在白天早些时候接触日光。这些只是排程上的启发式原则，不构成医疗建议，你的真实约束始终优先。

- Watson NF 等. Recommended Amount of Sleep for a Healthy Adult. [AASM 共识](https://aasm.org/resources/pdf/pressroom/adult-sleep-duration-consensus.pdf)
- Sletten TL 等. The importance of sleep regularity. [Sleep Health, 2023](https://doi.org/10.1016/j.sleh.2023.07.016)
- 世界卫生组织. 成人身体活动建议. [WHO](https://www.who.int/initiatives/behealthy/physical-activity)
- Albulescu P 等. "Give me a break!" [PLOS ONE, 2022](https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0272460)
- Figueiro MG 等. The impact of daytime light exposures on sleep and mood in office workers. [Sleep Health, 2017](https://doi.org/10.1016/j.sleh.2017.03.005)

</details>

## 工作原理

<p align="center">
  <img src="documentation/static/img/diagrams/how-it-works.svg" alt="你向 AI 编程智能体提问；Schedule Everything 插件调用 rmd 命令；rmd 校验并写入本地文件，再驱动提醒和浏览器工作区" width="100%">
</p>

插件里的技能负责思考，`rmd` 命令行负责它们需要的确定性操作：`rmd check`
校验配置并给出路径，`rmd sync --json` 描述今天的专注块和未完成任务，
`rmd sync --apply` 保存今天的计划。后台服务把结果变成提醒，`rmd web`
在浏览器中呈现一切。

## 快速开始

### 1. 安装 Schedule Everything

```bash
curl -fsSL https://raw.githubusercontent.com/AutoPhd-org/schedule-everything/main/install.sh | bash
```

或者从克隆的仓库安装：

```bash
git clone https://github.com/AutoPhd-org/schedule-everything.git
cd schedule-everything
./install.sh
```

安装脚本会准备 Python 环境，在 `~/.schedule_management/config` 中生成配置，
并注册提醒服务（macOS 上为 LaunchAgent，Linux 上为 systemd）。已有配置会保留。

### 2. 为你的 AI 智能体添加插件

<table>
  <tr><th>Claude Code</th><th>OpenAI Codex</th><th>Google Antigravity</th></tr>
  <tr>
    <td valign="top"><pre lang="bash">claude plugin marketplace add AutoPhd-org/schedule-everything
claude plugin install schedule-everything@schedule-everything</pre></td>
    <td valign="top"><pre lang="bash">codex plugin marketplace add AutoPhd-org/schedule-everything
codex plugin add schedule-everything@schedule-everything</pre></td>
    <td valign="top"><pre lang="bash">git clone https://github.com/AutoPhd-org/schedule-everything.git
agy plugin install ./schedule-everything/plugins/schedule-everything</pre></td>
  </tr>
</table>

在 Claude Code 会话中也可以用 `/plugin marketplace add` 和 `/plugin install`
完成相同步骤。安装脚本也会打印这些命令，浏览器的 **Schedule** 页面提供一键复制。

### 3. 告诉你的智能体

| 你说 | 会发生什么 |
| --- | --- |
| **“帮我制定每周日程”** | `build-schedule` 会询问你的工作、约束、精力和习惯，先给出文字摘要，经你确认后才写入 `settings.toml`、`odd_weeks.toml`、`even_weeks.toml`、`habits.toml` 和 `profile.md`，再用 `rmd check` 校验并重载提醒。 |
| **“帮我规划今天的专注块”** | `plan-day` 读取 `rmd sync --json`，把最重要的任务和最近的截止日期安排进今天的 pomodoro 和 potato 时间块，与你确认后用 `rmd sync --apply` 保存。 |
| **“把运动挪到晚上”** | 修改会保存为新的 `user_config_<n>` 版本，用 `rmd check --dir` 校验后通过 `rmd switch <n>` 启用，旧版本保留可随时回滚。 |

### 4. 打开工作区

```bash
rmd web
```

浏览器工作区只监听 `127.0.0.1`，与命令行读取同一套文件，再次启动时会复用已打开的标签页。

## 工作区一览

### 两种过一天的方式

<p align="center">
  <img src="documentation/static/img/modes.webp" alt="J 模式显示状态、当前时间块、截止日期、任务和时间线；P 模式只显示按类型分组的任务板" width="100%">
</p>

**J 模式**是有结构的一天：逾期和今日到期数量、高重要度任务、习惯进度、当前和下一个时间块、即将到来的截止日期以及今日时间线。**P 模式**适合更灵活的日子：部分事件提醒会暂停，概览只保留按类型分组、自由排布的任务。点击页眉的 **J / P** 开关，或运行 `rmd mode j|p` 切换。两种模式下，重要工作更多的分组都排在前面（**紧凑**布局），也可以切换到**分散**布局，按工作量摆放分组：最重的在中间，越轻的离得越远。你在页面上工作时分组保持原位，下次打开页面时再重新排列。可以直接在分组里添加任务，点击 **Fold** 可把每个分组折叠成重要度、进度和数量的概览。每个任务都有进度条，也可以拆成子任务，由子任务的完成情况自动计算进度。看板上方带表盘的计时器会为今天的限时任务倒计时（逾期任务显示为红色），也会为最后 24 小时内的截止日期倒计时，顶部一条进度条汇总今天限时任务的整体进度，并可按重要度加权。**每日概览**汇总你的一天，主题配色会随**工作量**变化：工作量是每个未完成任务的重要度乘以其剩余部分之和。

### 和智能体一起规划

<p align="center">
  <img src="documentation/static/img/screens/schedule-agents.webp" alt="Schedule 页面：今日时间线、今日计划，以及 Claude Code、Codex、Antigravity 的插件安装命令" width="100%">
</p>

今日时间线会显示每个已规划的时间块及其任务。Schedule 页面还提供插件安装命令的复制按钮、示例提问，以及清除今日计划的按钮。

### 一年尽收眼底

<p align="center">
  <img src="documentation/static/img/screens/calendar.webp" alt="可交互的年历：彩色的截止日期、打开的 11 月 21 日卡片，以及可拖动的即将到来的截止日期" width="82%">
</p>

**Routines & deadlines** 把所有截止日期放在年历上。点击某一天即可添加，可设为每周、每两周或每月重复，为它选择八种标记颜色之一，在日期卡片中改色或编辑，把右侧的卡片拖到另一天即可改期。重复事件会显示每一次的日期，并自动顺延到下一次。打开 **AI conferences** 后，年历会按你的时区显示 NeurIPS、ICLR、ICML、CVPR、ACL、ICRA 等主要会议今年的投稿截止日期、rebuttal（作者回复）期和会期。今日习惯也在这个页面。

### 积少成多

<p align="center">
  <img src="documentation/static/img/screens/activity-gallery.webp" alt="Activity 页面：每周概览入口、每日完成情况的节奏格、筛选条和带有生成图案的已完成任务画廊" width="100%">
</p>

**Activity** 把你完成、取消或放弃的所有任务排在一棵可滚动的时间树上：每种任务类型有自己的车道和颜色，树干的颜色随附近任务中占主导的类型变化，中间的高亮带标出你正在看的位置。也可以切换到画廊，每张卡片都带有与任务类型同色的图案。可以按结果或类型筛选、搜索，或点击节奏格中的某一天。把鼠标悬停在任意任务上，就能看到它何时开始、用了多久，以及与其他任务相比如何。

<table>
  <tr>
    <td width="50%" valign="top"><img src="documentation/static/img/screens/lightbox-dark.webp" alt="深色模式下的任务详情：添加和完成时间，以及用时与其他任务的比较"></td>
    <td width="50%" valign="top"><img src="documentation/static/img/screens/weekly-dark.webp" alt="深色模式下的每周概览：完成数量与八周趋势、习惯圆环、连续天数、每日节奏、各类型投入、结果、习惯打卡、节奏与亮点"></td>
  </tr>
  <tr>
    <td valign="top"><strong>每个任务都有故事。</strong>悬停在任意任务上查看它的时间线和节奏，点击可看完整详情。</td>
    <td valign="top"><strong>你的一周，色彩分明。</strong>每周概览用图表把本周与上周以及最近几周的平均水平对比。</td>
  </tr>
</table>

### 手机上也能用

<table>
  <tr>
    <td width="33%"><img src="documentation/static/img/screens/mobile-overview.webp" alt="手机上的 J 模式概览"></td>
    <td width="33%"><img src="documentation/static/img/screens/mobile-activity.webp" alt="手机上的 Activity 页面"></td>
    <td width="33%"><img src="documentation/static/img/screens/mobile-overview-p-dark.webp" alt="手机上深色主题的 P 模式"></td>
  </tr>
</table>

支持浅色、深色或跟随系统主题，并按浏览器记住你的选择；动效会遵循系统的“减少动态效果”设置。

## 每日节奏

<p align="center">
  <img src="documentation/static/img/diagrams/daily-loop.svg" alt="先搭建一次周计划，然后每天添加任务、规划今天、专注并接受提醒、回顾" width="100%">
</p>

## 今天的日程如何生成

<p align="center">
  <img src="documentation/static/img/diagrams/todays-schedule.svg" alt="按 ISO 周数选择单周或双周文件，与 common 条目合并，再由今日计划为专注块加上任务标题" width="100%">
</p>

```toml
# odd_weeks.toml（even_weeks.toml 结构相同）
[wednesday]
"09:00" = "pomodoro"                                  # [time_blocks] 名称：开始和结束时都会提醒
"14:00" = { block = "meeting", title = "Advisor 1:1" }

[common]                                              # 每天都生效；同一时间的工作日条目优先
"12:30" = "lunch_time"                                # [time_points] 名称：提醒一次
"20:00" = "potato"
```

未命名的 `pomodoro` 和 `potato` 时间块就是每日计划可以填充的专注块。计划只影响今天，你的每周文件不会改变。

## 常用命令

| 命令 | 作用 |
| --- | --- |
| `rmd web` | 打开浏览器工作区，复用已有标签页 |
| `rmd status [-v]` | 显示当前和下一项安排，以及今天计划的任务标题 |
| `rmd add "任务" 8 [类型] [天数]` | 添加重要度 1–10 的任务，可选类型和延后提醒天数 |
| `rmd ls` · `rmd rm` · `rmd cancel` · `rmd drop` | 列出任务；完成任务（计为已完成），或作为误加、放弃移除 |
| `rmd history [n]` | 最近完成、取消和放弃的任务 |
| `rmd ddl` · `rmd ddl add <事件> <月.日> [--repeat weekly\|biweekly\|monthly]` · `rmd ddl rm` | 管理截止日期，包括重复事件 |
| `rmd track` | 记录今天的习惯 |
| `rmd sync [--json \| --apply FILE \| --clear]` | 显示今天的专注块和计划；`plan-day` 使用 `--json` 和 `--apply` |
| `rmd check [--dir DIR] [--json]` | 校验配置目录并给出路径 |
| `rmd mode [j\|p]` | 查看或切换 J / P 模式 |
| `rmd switch <id>` · `rmd update` · `rmd stop` | 启用某个 `user_config_<n>` 版本、重载提醒或停止提醒 |
| `rmd settings` | 在交互式终端界面中编辑 `settings.toml` |
| `rmd view` · `rmd report weekly\|monthly` | 导出日程 PDF 或效率报告 |

## 配置结构

```text
~/.schedule_management/config/
├── user_config_0/            # 一个完整的周计划版本
│   ├── settings.toml         # 时间块、时间点、提醒时间、任务类型
│   ├── odd_weeks.toml        # 单周（ISO 周数为奇数）日程
│   ├── even_weeks.toml       # 双周日程
│   ├── habits.toml           # 每日习惯
│   ├── ddl.json              # 截止日期（可带标记颜色）
│   └── profile.md            # 智能体对你的了解
├── user_config_1/            # build-schedule 生成的新版本
├── synced_schedule.toml      # 今日计划
└── tasks/                    # 任务、历史、习惯记录、模式
```

每个文件都可以手动编辑、在浏览器的配置编辑器中修改，或通过 `rmd settings` 编辑。手动修改后请运行 `rmd check`。

## 文档

- [简介](https://autophd-org.github.io/schedule-everything/zh/docs/intro)
- [AI 智能体插件](https://autophd-org.github.io/schedule-everything/docs/ai-agents)
- [快速开始](https://autophd-org.github.io/schedule-everything/docs/quick-start)
- [浏览器工作区](https://autophd-org.github.io/schedule-everything/docs/browser)
- [配置](https://autophd-org.github.io/schedule-everything/docs/configuration/overview)
- [命令行参考](https://autophd-org.github.io/schedule-everything/docs/cli/overview)

## 开发

```bash
uv venv .venv && source .venv/bin/activate
uv pip install -e .
uv run pytest -q        # Python 测试

bun install
bun run test            # 浏览器工作区测试（Vitest；直接运行 `bun test` 会调用 Bun 自带的测试器）
bun run build           # 重新构建内置的浏览器资源
```

JavaScript 依赖由 [Bun](https://bun.sh) 管理（`bun.lock`），相关工具运行在 Node.js 20 或更新版本上。浏览器前端位于 `web/`，`bun run build` 会把打包结果写入 `src/schedule_management/web/static/`，并随 Python 包一起发布。智能体插件位于 [`plugins/schedule-everything`](plugins/schedule-everything)。

## 许可证

采用 **MIT License** 发布，详见 [LICENSE](LICENSE)。
