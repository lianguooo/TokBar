import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

export type Lang = "zh" | "en";

const en = {
  // App shell
  "app.subtitle": "AI Usage Dashboard",
  "nav.overview": "Overview",
  "nav.trends": "Trends",
  "nav.sessions": "Sessions",
  "nav.models": "Models",
  "nav.blocks": "Billing Blocks",
  "nav.settings": "Settings",
  "app.refresh": "Refresh",
  "app.scanning": "Scanning…",
  "app.scanProgress": "Scanning {done}/{total}",
  "app.upToDate": "Up to date",
  "app.refreshed": "Updated {n} entries",
  "update.available": "New version {v} available",
  "update.action": "Update & Restart",
  "update.later": "Later",
  "update.downloading": "Downloading… {pct}%",
  "update.installing": "Installing…",
  "update.check": "Check for updates",
  "update.checking": "Checking…",
  "update.upToDate": "TokBar is up to date.",
  "update.checkFailed": "Couldn't check for updates: {error}",
  "update.installFailed": "Couldn't install the update: {error}",
  "update.currentVersion": "Current version {v}",
  "update.automatic": "TokBar checks automatically while it stays running.",
  "update.desktopOnly": "Update checks are available in the desktop app.",
  "range.today": "Today",
  "range.7d": "7D",
  "range.30d": "30D",
  "range.90d": "90D",
  "range.all": "All",

  // Overview
  "overview.totalCost": "Total Cost",
  "overview.totalTokens": "Total Tokens",
  "overview.requests": "Requests",
  "overview.sessions": "Sessions",
  "overview.activeDays": "{n} active days",
  "overview.inOut": "in {in} / out {out}",
  "overview.cacheRead": "cache read {n}",
  "overview.agents": "{n} agents",
  "overview.costTrend": "Cost Trend",
  "overview.costByModel": "Cost by Model",
  "overview.costByAgent": "Cost by Agent",
  "overview.topProjects": "Top Projects",
  "overview.projectStats": "{cost} · {tokens} tok · {sessions} sessions",
  "common.empty": "No usage data in this period",
  "common.loadFailed": "Failed to load data",
  "common.retry": "Retry",
  "blocks.remainHM": "{h}h {m}m left",
  "blocks.remainM": "{m}m left",
  "unit.perMin": "/min",
  "unit.perHour": "/h",
  "unit.tok": "tok",

  // Table heads
  "th.date": "Date",
  "th.time": "Time",
  "th.input": "Input",
  "th.output": "Output",
  "th.cacheWrite": "Cache Write",
  "th.cacheRead": "Cache Read",
  "th.requests": "Requests",
  "th.cost": "Cost",
  "th.project": "Project",
  "th.agent": "Agent",
  "th.models": "Models",
  "th.lastActivity": "Last Activity",
  "th.tokens": "Tokens",
  "th.model": "Model",

  // Trends
  "trends.byDay": "Daily",
  "trends.byWeek": "Weekly",
  "trends.byMonth": "Monthly",
  "trends.dailyCost": "Daily Cost",
  "trends.dailyTokens": "Daily Tokens by Type",
  "trends.dailyBreakdown": "Daily Breakdown",
  "chart.input": "Input",
  "chart.output": "Output",
  "chart.cacheRead": "Cache Read",
  "chart.cacheWrite": "Cache Write",

  // Sessions
  "sessions.title": "Sessions",
  "sessions.empty": "No sessions in this period",
  "sessions.search": "Search project / session / model…",
  "sessions.allAgents": "All",
  "sessions.retentionHint":
    "Session details are kept for 30 days. Older token and cost history remains available in Trends.",

  // Models
  "models.costDist": "Cost Distribution",
  "models.tokenDist": "Token Distribution",
  "models.all": "All Models",

  // Blocks
  "blocks.desc":
    "5-hour billing blocks: usage is grouped into 5-hour windows aligned to the hour, matching Claude's session billing window. Last 14 days.",
  "blocks.empty": "No billing blocks in the last 14 days",
  "blocks.active": "Active block",
  "blocks.completed": "Completed",
  "blocks.live": "LIVE",
  "blocks.cost": "Cost",
  "blocks.tokens": "Tokens",
  "blocks.requests": "Requests",
  "blocks.burnRate": "Burn rate",
  "blocks.costRate": "Cost rate",

  // Settings
  "settings.language": "Language",
  "settings.languageDesc": "Display language for the interface.",
  "settings.general": "General",
  "settings.softwareUpdate": "Software Update",
  "settings.autostart": "Launch at login",
  "settings.autostartDesc": "Start TokBar automatically when you log in.",
  "settings.appearance": "Appearance",
  "settings.themeMode": "Theme",
  "settings.theme.dark": "Dark",
  "settings.theme.light": "Light",
  "settings.accentColor": "Accent color",
  "settings.trayDisplay": "Menu Bar",
  "settings.trayDisplayDesc":
    "What to show next to the menu bar icon at the top right of your screen.",
  "settings.tray.cost": "Today's cost",
  "settings.tray.tokens": "Today's tokens",
  "settings.tray.off": "Icon only",
  "settings.dataSources": "Data Sources",
  "settings.notDetected": "Not detected on this machine",
  "settings.sources.inactive": "Not detected ({n})",
  "settings.files": "{n} files",
  "settings.lastScan":
    "Last scan: {parsed} of {total} files re-parsed, {entries} entries updated in {ms} ms.",
  "settings.retention": "Data Retention",
  "settings.retentionDesc":
    "Remove Claude Code and Codex session logs whose last activity was over 30 days ago. Daily token and cost history is archived first.",
  "settings.retentionPolicy": "Full session details: 30 days",
  "settings.retentionPreview": "Preview cleanup",
  "settings.retentionPreviewing": "Checking…",
  "settings.retentionSummary": "{sessions} sessions · {files} files · {size}",
  "settings.retentionPreserve":
    "TokBar will preserve {tokens} tokens and {cost} in daily usage history.",
  "settings.retentionSkipped":
    "{n} older sessions use shared or unsupported sources and will be skipped.",
  "settings.retentionEmpty": "No supported sessions older than 30 days.",
  "settings.retentionDelete": "Delete old sessions",
  "settings.retentionConfirmTitle": "Delete original session logs?",
  "settings.retentionConfirmDesc":
    "The conversations will no longer be available in Claude Code or Codex. Token and cost history will remain in TokBar.",
  "settings.retentionCancel": "Cancel",
  "settings.retentionConfirm": "Delete permanently",
  "settings.retentionDeleting": "Archiving and deleting…",
  "settings.retentionSuccess":
    "Archived {sessions} sessions and deleted {files} source files.",
  "settings.retentionPending":
    "Usage was archived, but {n} changed or locked files could not be deleted.",
  "settings.retentionFailed": "Cleanup failed: {error}",
  "settings.costMode": "Cost Mode",
  "settings.mode.auto": "Auto",
  "settings.mode.autoDesc":
    "Use the log's costUSD when present, otherwise calculate from tokens.",
  "settings.mode.calculate": "Calculate",
  "settings.mode.calculateDesc":
    "Always calculate from token counts using LiteLLM pricing.",
  "settings.mode.display": "Display",
  "settings.mode.displayDesc": "Only show pre-computed costUSD from logs.",
  "settings.pricing": "Pricing Data",
  "settings.pricingDesc":
    "Cost is calculated from LiteLLM's community model pricing database. Rates refresh automatically once a day (checked at launch). New rates apply to new usage going forward — historical costs keep the rates in effect when they were recorded.",
  "settings.pricingRefresh": "Update pricing now",
  "settings.pricingRefreshing": "Updating…",
  "settings.pricingRefreshed":
    "Updated — {count} models; active usage re-priced, archived history unchanged.",
  "settings.pricingRefreshFailed": "Update failed: {error}",

  // Quick panel (menu bar popover)
  "quick.todayCost": "Today's Cost",
  "quick.todayTokens": "Tokens Today",
  "quick.todayRequests": "Requests Today",
  "quick.monthCost": "This Month",
  "quick.activeBlock": "Active Billing Block",
  "quick.noActiveBlock": "No active billing block",
  "quick.burnRate": "Burn rate",
  "quick.costRate": "Cost rate",
  "quick.blockEnds": "Block ends",
  "quick.openDashboard": "Open Dashboard",

  // Subscription ROI (Overview)
  "roi.title": "Subscription ROI",
  "roi.subtitle": "This month · at API prices",
  "roi.apiValue": "API-priced value",
  "roi.youPay": "you pay",
  "roi.saved": "Saved {amount}",
  "roi.notRecouped": "{amount} to break even",
  "roi.multiple": "{x}× back",
  "roi.noUsage": "no usage",
  "roi.untitled": "Untitled",
  "roi.hint":
    "When plans share an agent, its usage is split across them by fee — never double-counted. Real savings are usually higher, since plans also cover usage TokBar can't see.",

  // Settings — subscriptions
  "settings.subscriptions": "Subscriptions",
  "settings.subscriptionsDesc":
    "Track flat-rate plans (Claude Max, ChatGPT Pro…). TokBar prices the agents they cover at API rates and shows your real ROI on the Overview.",
  "settings.sub.namePlaceholder": "Plan name",
  "settings.sub.agent": "Covers",
  "settings.sub.pickAgents": "Pick agents",
  "settings.sub.addAgent": "Add",
  "settings.sub.perMonth": "/mo",
  "settings.sub.quickAdd": "Quick add",
  "settings.sub.custom": "Custom",
  "settings.sub.add": "Add subscription",
  "settings.sub.remove": "Remove",
  "settings.sub.confirmRemove": "Confirm delete",
  "settings.sub.empty":
    "No subscriptions yet — pick one below to see your ROI on the Overview.",

  // Advanced (opt-in) features
  "settings.advanced": "Advanced",
  "settings.advancedDesc":
    "Off by default. These two write outside TokBar’s own data: one edits your Codex config and login, the other deletes source log files.",
  "settings.codexSwitch": "Codex account & provider switch",
  "settings.codexSwitchDesc":
    "Switch the Codex account and model provider from here. Edits $CODEX_HOME/config.toml and swaps auth.json.",
  "settings.sessionDelete": "Delete individual sessions",
  "settings.sessionDeleteDesc":
    "Adds a delete action to the sessions table. Removes the session’s source log; daily totals are kept.",

  // Codex switch
  "codexSwitch.title": "Codex Account & Provider",
  "codexSwitch.home": "Codex home: {path}",
  "codexSwitch.restartHint": "Changes take effect the next time Codex starts.",
  "codexSwitch.accounts": "Accounts",
  "codexSwitch.providers": "Providers",
  "codexSwitch.official": "Official ChatGPT",
  "codexSwitch.officialDesc": "Send requests straight to ChatGPT.",
  "codexSwitch.current": "Current",
  "codexSwitch.signedIn": "Signed in",
  "codexSwitch.signedInHint":
    "Still the signed-in account — selecting a provider only changes where requests go.",
  "codexSwitch.use": "Use",
  "codexSwitch.working": "Working…",
  "codexSwitch.edit": "Edit",
  "codexSwitch.delete": "Delete",
  "codexSwitch.confirmDelete": "Confirm delete",
  "codexSwitch.cancel": "Cancel",
  "codexSwitch.save": "Save",
  "codexSwitch.addProvider": "Add provider",
  "codexSwitch.addAccount": "Add account",
  "codexSwitch.import": "Import from CodexPlusPlus",
  "codexSwitch.importHint":
    "{n} account(s) found in CodexPlusPlus’s store. Importing copies their logins here; ones already saved are skipped.",
  "codexSwitch.saveCurrent": "Save current account",
  "codexSwitch.saveCurrentDesc":
    "A Codex login was found but is not saved yet. Name it so you can switch back to it later.",
  "codexSwitch.name": "Name",
  "codexSwitch.baseUrl": "Base URL",
  "codexSwitch.token": "Bearer token",
  "codexSwitch.model": "Model",
  "codexSwitch.modelHint": "Written to the top-level model in config.toml when selected.",
  "codexSwitch.currentAccountName": "Name for the account being signed out",
  "codexSwitch.addAccountWarning":
    "Adding an account signs the current one out: its auth.json is archived here, then removed. Restart Codex, sign in as the new account, and it is picked up automatically.",
  "codexSwitch.pending": "{name} · waiting for sign-in",
  "codexSwitch.pendingHint":
    "Restart Codex and sign in as {name}. This panel adopts it automatically.",
  "codexSwitch.captured": "Account {name} was signed in and saved",
  "codexSwitch.noChange": "Already selected — nothing was changed.",
  "codexSwitch.empty": "No accounts or providers yet.",
  "codexSwitch.noCodex": "No Codex config found at this path.",
  "codexSwitch.loadFailed": "Could not read the Codex config",

  // Session delete
  "sessions.delete": "Delete",
  "sessions.deleteConfirm": "Delete log?",
  "sessions.deleteTitle": "Delete this session’s log file?",
  "sessions.deleteBody":
    "{size} on disk will be removed. Daily cost and token totals are kept; the conversation itself is gone for good.",
  "sessions.deleteShared":
    "Skipped: this log file also holds other sessions, so deleting it would take them too.",
  "sessions.deleteStale":
    "Skipped: the log file changed since the last scan. Refresh and try again.",
  "sessions.deleted": "Deleted — {size} freed",
  "sessions.deleteFailed": "Delete failed",
  // Delete button inside Codex itself
  "settings.codexInject": "Show the delete button inside Codex",
  "settings.codexInjectDesc":
    "Adds the same delete action to Codex’s own sidebar. Codex can only be scripted when it is started with a debug port, so turning this on quits a running Codex and reopens it once. After that TokBar never launches Codex on its own — quit it and it stays quit.",
  "settings.codexInjectPlatforms":
    "Supported on macOS and Windows. Microsoft Store installations are detected automatically.",
  "settings.codexInjectAttached": "Attached to Codex · port {port}",
  "settings.codexInjectWaiting": "Not attached to Codex",
  "settings.codexInjectNoAutoLaunch":
    "TokBar will not reopen Codex by itself. Start Codex from here when you want the delete button back.",
  "settings.codexInjectRelaunch": "Relaunch Codex",
  "settings.codexInjectApp": "Codex app",
  "settings.codexInjectAppPlaceholder": "Auto-detected; set a path to override",
  "settings.codexInjectConflict":
    "If CodexPlusPlus is installed, quit it first — both use debug port 9229 and would fight over the same Codex window.",

  // Advanced page
  "nav.advanced": "Advanced",
  "advanced.desc":
    "Everything here writes outside TokBar’s own data: two opt-in features that change your Codex config and login, and a sweep that deletes source logs.",
  "advanced.enable": "Enable",
  "advanced.retention": "Delete sessions older than 30 days",
};

const zh: Record<keyof typeof en, string> = {
  "app.subtitle": "AI 用量仪表盘",
  "nav.overview": "总览",
  "nav.trends": "趋势",
  "nav.sessions": "会话",
  "nav.models": "模型",
  "nav.blocks": "计费块",
  "nav.settings": "设置",
  "app.refresh": "刷新",
  "app.scanning": "扫描中…",
  "app.scanProgress": "扫描中 {done}/{total}",
  "app.upToDate": "已是最新",
  "app.refreshed": "已更新 {n} 条",
  "update.available": "发现新版本 {v}",
  "update.action": "更新并重启",
  "update.later": "稍后",
  "update.downloading": "下载中… {pct}%",
  "update.installing": "安装中…",
  "update.check": "检查更新",
  "update.checking": "正在检查…",
  "update.upToDate": "TokBar 已是最新版本。",
  "update.checkFailed": "检查更新失败：{error}",
  "update.installFailed": "安装更新失败：{error}",
  "update.currentVersion": "当前版本 {v}",
  "update.automatic": "TokBar 会在运行期间自动检查新版本。",
  "update.desktopOnly": "仅桌面应用支持检查更新。",
  "range.today": "今日",
  "range.7d": "7天",
  "range.30d": "30天",
  "range.90d": "90天",
  "range.all": "全部",

  "overview.totalCost": "总成本",
  "overview.totalTokens": "总 Token",
  "overview.requests": "请求数",
  "overview.sessions": "会话数",
  "overview.activeDays": "{n} 个活跃天",
  "overview.inOut": "输入 {in} / 输出 {out}",
  "overview.cacheRead": "缓存读取 {n}",
  "overview.agents": "{n} 个 Agent",
  "overview.costTrend": "成本趋势",
  "overview.costByModel": "按模型分布",
  "overview.costByAgent": "按 Agent 分布",
  "overview.topProjects": "项目排行",
  "overview.projectStats": "{cost} · {tokens} tok · {sessions} 个会话",
  "common.empty": "该时间段内暂无使用数据",
  "common.loadFailed": "数据加载失败",
  "common.retry": "重试",
  "blocks.remainHM": "还剩 {h} 小时 {m} 分",
  "blocks.remainM": "还剩 {m} 分钟",
  "unit.perMin": "/分钟",
  "unit.perHour": "/小时",
  "unit.tok": "tok",

  "th.date": "日期",
  "th.time": "时间段",
  "th.input": "输入",
  "th.output": "输出",
  "th.cacheWrite": "缓存写入",
  "th.cacheRead": "缓存读取",
  "th.requests": "请求数",
  "th.cost": "成本",
  "th.project": "项目",
  "th.agent": "Agent",
  "th.models": "模型",
  "th.lastActivity": "最近活动",
  "th.tokens": "Token",
  "th.model": "模型",

  "trends.byDay": "按日",
  "trends.byWeek": "按周",
  "trends.byMonth": "按月",
  "trends.dailyCost": "每日成本",
  "trends.dailyTokens": "每日 Token(按类型)",
  "trends.dailyBreakdown": "每日明细",
  "chart.input": "输入",
  "chart.output": "输出",
  "chart.cacheRead": "缓存读取",
  "chart.cacheWrite": "缓存写入",

  "sessions.title": "会话",
  "sessions.empty": "该时间段内暂无会话",
  "sessions.search": "搜索项目 / 会话 / 模型…",
  "sessions.allAgents": "全部",
  "sessions.retentionHint":
    "会话详情保留 30 天；更早的 Token 与金额历史仍可在趋势页查看。",

  "models.costDist": "成本分布",
  "models.tokenDist": "Token 分布",
  "models.all": "全部模型",

  "blocks.desc":
    "5 小时计费块:用量按整点对齐的 5 小时窗口分组,对应 Claude 的会话计费窗口。显示最近 14 天。",
  "blocks.empty": "最近 14 天内没有计费块",
  "blocks.active": "进行中",
  "blocks.completed": "已结束",
  "blocks.live": "进行中",
  "blocks.cost": "成本",
  "blocks.tokens": "Token",
  "blocks.requests": "请求数",
  "blocks.burnRate": "燃烧率",
  "blocks.costRate": "成本速率",

  "settings.language": "语言",
  "settings.languageDesc": "界面显示语言。",
  "settings.general": "通用",
  "settings.softwareUpdate": "软件更新",
  "settings.autostart": "开机自启",
  "settings.autostartDesc": "登录系统时自动启动 TokBar。",
  "settings.appearance": "外观",
  "settings.themeMode": "主题",
  "settings.theme.dark": "深色",
  "settings.theme.light": "浅色",
  "settings.accentColor": "主题色",
  "settings.trayDisplay": "菜单栏",
  "settings.trayDisplayDesc": "屏幕右上角菜单栏图标旁显示的内容。",
  "settings.tray.cost": "今日成本",
  "settings.tray.tokens": "今日 Token",
  "settings.tray.off": "仅图标",
  "settings.dataSources": "数据源",
  "settings.notDetected": "本机未检测到",
  "settings.sources.inactive": "未检测到 ({n})",
  "settings.files": "{n} 个文件",
  "settings.lastScan":
    "上次扫描:重新解析 {parsed}/{total} 个文件,更新 {entries} 条记录,耗时 {ms} 毫秒。",
  "settings.retention": "数据保留",
  "settings.retentionDesc":
    "删除最后活动时间超过 30 天的 Claude Code 和 Codex 会话日志，删除前会先归档每日 Token 与金额。",
  "settings.retentionPolicy": "完整会话详情：保留 30 天",
  "settings.retentionPreview": "预览可清理内容",
  "settings.retentionPreviewing": "正在检查…",
  "settings.retentionSummary": "{sessions} 个会话 · {files} 个文件 · {size}",
  "settings.retentionPreserve":
    "TokBar 将在每日用量历史中保留 {tokens} Token 和 {cost}。",
  "settings.retentionSkipped":
    "另有 {n} 个旧会话来自共享或暂不支持的数据源，本次会跳过。",
  "settings.retentionEmpty": "当前没有超过 30 天且支持清理的会话。",
  "settings.retentionDelete": "删除旧会话",
  "settings.retentionConfirmTitle": "确定删除原始会话日志？",
  "settings.retentionConfirmDesc":
    "删除后将无法在 Claude Code 或 Codex 中查看这些对话，但 TokBar 会继续保留 Token 与金额历史。",
  "settings.retentionCancel": "取消",
  "settings.retentionConfirm": "永久删除",
  "settings.retentionDeleting": "正在归档并删除…",
  "settings.retentionSuccess": "已归档 {sessions} 个会话并删除 {files} 个源文件。",
  "settings.retentionPending":
    "用量已归档，但仍有 {n} 个已变化或被占用的文件无法删除。",
  "settings.retentionFailed": "清理失败：{error}",
  "settings.costMode": "成本模式",
  "settings.mode.auto": "自动",
  "settings.mode.autoDesc":
    "日志中有 costUSD 时优先使用,否则按 Token 计算。",
  "settings.mode.calculate": "计算",
  "settings.mode.calculateDesc": "始终按 Token 数 × LiteLLM 价格重新计算。",
  "settings.mode.display": "展示",
  "settings.mode.displayDesc": "只显示日志中预先计算好的 costUSD。",
  "settings.pricing": "定价数据",
  "settings.pricingDesc":
    "成本基于 LiteLLM 社区模型价格库计算。价格每天自动更新一次(启动时检查),新价格只用于之后的新增用量 —— 历史成本保持记录时的价格不变。",
  "settings.pricingRefresh": "立即更新价格",
  "settings.pricingRefreshing": "更新中…",
  "settings.pricingRefreshed":
    "已更新 —— 共 {count} 个模型；近 30 天用量已重新计价，归档历史保持不变。",
  "settings.pricingRefreshFailed": "更新失败:{error}",

  "quick.todayCost": "今日成本",
  "quick.todayTokens": "今日 Token",
  "quick.todayRequests": "今日请求",
  "quick.monthCost": "本月成本",
  "quick.activeBlock": "活跃计费块",
  "quick.noActiveBlock": "当前没有活跃计费块",
  "quick.burnRate": "燃烧率",
  "quick.costRate": "成本速率",
  "quick.blockEnds": "块结束于",
  "quick.openDashboard": "打开主面板",

  // 订阅回本(总览)
  "roi.title": "订阅回本",
  "roi.subtitle": "本月 · 按 API 价",
  "roi.apiValue": "按 API 计价",
  "roi.youPay": "实付",
  "roi.saved": "省了 {amount}",
  "roi.notRecouped": "还差 {amount} 回本",
  "roi.multiple": "{x}× 回本",
  "roi.noUsage": "暂无用量",
  "roi.untitled": "未命名",
  "roi.hint":
    "多个套餐覆盖同一 agent 时,其用量按月费占比分摊,不会重复计价。实际节省通常更多 —— 套餐还覆盖了 TokBar 看不到的用量。",

  // 设置 — 订阅
  "settings.subscriptions": "订阅",
  "settings.subscriptionsDesc":
    "记录你的固定月费套餐(Claude Max、ChatGPT Pro…)。TokBar 会按 API 价给它们覆盖的 agent 计价,在总览展示你真实的回本情况。",
  "settings.sub.namePlaceholder": "套餐名称",
  "settings.sub.agent": "覆盖",
  "settings.sub.pickAgents": "选择 Agent",
  "settings.sub.addAgent": "添加",
  "settings.sub.perMonth": "/月",
  "settings.sub.quickAdd": "快速添加",
  "settings.sub.custom": "自定义",
  "settings.sub.add": "添加订阅",
  "settings.sub.remove": "删除",
  "settings.sub.confirmRemove": "确认删除",
  "settings.sub.empty": "还没有订阅 —— 在下方选一个即可在总览看到回本情况。",

  // Advanced (opt-in) features
  "settings.advanced": "高级功能",
  "settings.advancedDesc":
    "默认关闭。这两项会写入 TokBar 自身数据以外的位置：一个修改 Codex 配置与登录，一个删除源日志文件。",
  "settings.codexSwitch": "Codex 账号与供应商切换",
  "settings.codexSwitchDesc":
    "在这里切换 Codex 账号和模型供应商。会修改 $CODEX_HOME/config.toml 并替换 auth.json。",
  "settings.sessionDelete": "删除单条会话",
  "settings.sessionDeleteDesc":
    "在会话列表里增加删除操作。会删掉该会话的源日志，每日用量统计保留。",

  // Codex switch
  "codexSwitch.title": "Codex 账号与供应商",
  "codexSwitch.home": "Codex 目录：{path}",
  "codexSwitch.restartHint": "修改在 Codex 下次启动时生效。",
  "codexSwitch.accounts": "账号",
  "codexSwitch.providers": "供应商",
  "codexSwitch.official": "官方 ChatGPT",
  "codexSwitch.officialDesc": "请求直接发往 ChatGPT。",
  "codexSwitch.current": "当前",
  "codexSwitch.signedIn": "已登录",
  "codexSwitch.signedInHint":
    "仍是当前登录账号 —— 选供应商只改变请求通道。",
  "codexSwitch.use": "切换",
  "codexSwitch.working": "处理中…",
  "codexSwitch.edit": "编辑",
  "codexSwitch.delete": "删除",
  "codexSwitch.confirmDelete": "确认删除",
  "codexSwitch.cancel": "取消",
  "codexSwitch.save": "保存",
  "codexSwitch.addProvider": "添加供应商",
  "codexSwitch.addAccount": "添加账号",
  "codexSwitch.import": "从 CodexPlusPlus 导入",
  "codexSwitch.importHint":
    "在 CodexPlusPlus 的存储里找到 {n} 个账号。导入会把它们的登录复制过来，已有的会自动跳过。",
  "codexSwitch.saveCurrent": "收录当前账号",
  "codexSwitch.saveCurrentDesc":
    "检测到一个 Codex 登录，但还没收录。给它起个名字，以后才能切回来。",
  "codexSwitch.name": "名称",
  "codexSwitch.baseUrl": "Base URL",
  "codexSwitch.token": "Bearer Token",
  "codexSwitch.model": "模型",
  "codexSwitch.modelHint": "切换到该项时写入 config.toml 的顶层 model。",
  "codexSwitch.currentAccountName": "即将退出的账号名称",
  "codexSwitch.addAccountWarning":
    "添加账号会退出当前登录：先把 auth.json 冷备份到本地，再删除它。随后重启 Codex 并登录新账号，这里会自动收录。",
  "codexSwitch.pending": "{name} · 等待登录",
  "codexSwitch.pendingHint": "请重启 Codex 并登录 {name}，此处会自动收录。",
  "codexSwitch.captured": "账号 {name} 已登录并保存",
  "codexSwitch.noChange": "已经是当前项 —— 未做任何修改。",
  "codexSwitch.empty": "还没有账号或供应商。",
  "codexSwitch.noCodex": "该路径下没有找到 Codex 配置。",
  "codexSwitch.loadFailed": "读取 Codex 配置失败",

  // Session delete
  "sessions.delete": "删除",
  "sessions.deleteConfirm": "确认删除？",
  "sessions.deleteTitle": "删除这条会话的日志文件？",
  "sessions.deleteBody":
    "将删除磁盘上的 {size}。每日花费与 Token 统计会保留，但对话内容无法恢复。",
  "sessions.deleteShared":
    "已跳过：该日志文件里还有其他会话，删了会一并带走。",
  "sessions.deleteStale":
    "已跳过：日志文件在上次扫描后发生了变化，请刷新后重试。",
  "sessions.deleted": "已删除 —— 释放 {size}",
  "sessions.deleteFailed": "删除失败",
  // Delete button inside Codex itself
  "settings.codexInject": "在 Codex 界面内显示删除按钮",
  "settings.codexInjectDesc":
    "在 Codex 自己的侧边栏加上相同的删除操作。Codex 只有带调试端口启动才能被注入，所以打开此项会退出已在运行的 Codex 并重新打开一次。之后 TokBar 不会再自动拉起 Codex —— 你退出它就保持退出。",
  "settings.codexInjectPlatforms":
    "支持 macOS 和 Windows；Windows Microsoft Store 安装会自动检测。",
  "settings.codexInjectAttached": "已接入 Codex · 端口 {port}",
  "settings.codexInjectWaiting": "未接入 Codex",
  "settings.codexInjectNoAutoLaunch":
    "TokBar 不会自动把 Codex 拉回来。需要删除按钮时，从这里启动 Codex。",
  "settings.codexInjectRelaunch": "重启 Codex",
  "settings.codexInjectApp": "Codex 应用",
  "settings.codexInjectAppPlaceholder": "自动检测；填路径可指定",
  "settings.codexInjectConflict":
    "如果装了 CodexPlusPlus，请先退出它 —— 两者都用 9229 调试端口，会互抢同一个 Codex 窗口。",

  // Advanced page
  "nav.advanced": "高级",
  "advanced.desc":
    "这里的功能都会写入 TokBar 自身数据以外的位置：两项需手动开启的功能会改动 Codex 配置与登录，另一项会删除源日志。",
  "advanced.enable": "启用",
  "advanced.retention": "删除 30 天前的会话",
};

const dicts = { en, zh };

export type I18nKey = keyof typeof en;

interface I18nValue {
  lang: Lang;
  setLang: (lang: Lang) => void;
  t: (key: I18nKey, vars?: Record<string, string | number>) => string;
}

const I18nContext = createContext<I18nValue>({
  lang: "en",
  setLang: () => {},
  t: (k) => k,
});

const STORAGE_KEY = "tokbar-lang";

function detectLang(): Lang {
  const saved = localStorage.getItem(STORAGE_KEY);
  if (saved === "zh" || saved === "en") return saved;
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectLang);

  const setLang = useCallback((l: Lang) => {
    localStorage.setItem(STORAGE_KEY, l);
    setLangState(l);
  }, []);

  // Keep the main window and the menu-bar quick panel in sync:
  // storage events fire in the other window when one changes language.
  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === STORAGE_KEY && (e.newValue === "zh" || e.newValue === "en")) {
        setLangState(e.newValue);
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);

  const t = useCallback(
    (key: I18nKey, vars?: Record<string, string | number>) => {
      let s: string = dicts[lang][key] ?? en[key] ?? key;
      if (vars) {
        for (const [k, v] of Object.entries(vars)) {
          s = s.replace(`{${k}}`, String(v));
        }
      }
      return s;
    },
    [lang],
  );

  return (
    <I18nContext.Provider value={{ lang, setLang, t }}>
      {children}
    </I18nContext.Provider>
  );
}

export function useI18n() {
  return useContext(I18nContext);
}
