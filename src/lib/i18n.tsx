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
  "settings.files": "{n} files",
  "settings.lastScan":
    "Last scan: {parsed} of {total} files re-parsed, {entries} entries updated in {ms} ms.",
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
  "settings.files": "{n} 个文件",
  "settings.lastScan":
    "上次扫描:重新解析 {parsed}/{total} 个文件,更新 {entries} 条记录,耗时 {ms} 毫秒。",
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
