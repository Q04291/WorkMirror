import { Component, createSignal, createEffect, onCleanup, Show, For, createMemo } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { RefreshCw, Sparkles } from "lucide-solid";

// ── Types ─────────────────────────────────────────────

interface DailyNote {
  date: string;
  total_active_hours: number;
  deep_work_hours: number;
  learning_hours: number;
  focus_score: number;
  streak_days: number;
  what_you_did: string;
  what_you_learned: string;
  good_habit: string;
  tip: string;
}

// ── Helpers ────────────────────────────────────────────

function todayStr(): string {
  return new Date().toISOString().slice(0, 10);
}

function hoursToStr(h: number): string {
  const hr = Math.floor(h);
  const min = Math.round((h - hr) * 60);
  return hr > 0 ? `${hr}h ${min}m` : `${min}m`;
}

// ── Preset fallback quotes ─────────────────────────────

const QUOTES = [
  "今天也在变好的路上 🌱",
  "每一步都算数 👣",
  "学习是件温柔的事 📖",
  "你比昨天又厉害了一点 💪",
  "保持节奏，慢慢来 🐢",
  "今天你学了不少东西 👏",
  "进步藏在每一个专注的瞬间 ✨",
  "小小的坚持，大大的变化 🌟",
  "今天也是充实的一天 🎯",
  "别忘了给自己点个赞 👍",
];

function randomQuote(): string {
  return QUOTES[Math.floor(Math.random() * QUOTES.length)];
}

// ── Timeline color blocks (mocked from categories) ────

interface TimelineBlock {
  label: string;
  color: string;
  hours: number;
}

const CATEGORY_META: Record<string, { label: string; color: string }> = {
  code: { label: "💻 编程", color: "#22c55e" },
  study: { label: "📚 学习", color: "#eab308" },
  writing: { label: "✍️ 写作", color: "#6366f1" },
  browsing: { label: "🌐 浏览", color: "#06b6d4" },
  reading: { label: "📖 阅读", color: "#a855f7" },
  design: { label: "🎨 设计", color: "#f97316" },
  meeting: { label: "💬 会议", color: "#ef4444" },
  other: { label: "🔄 其他", color: "#52525b" },
};

// ── Component ──────────────────────────────────────────

const Growth: Component = () => {
  // ── State ──
  const [note, setNote] = createSignal<DailyNote | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [refreshing, setRefreshing] = createSignal(false);
  const [failed, setFailed] = createSignal(false);

  // Derived
  const focusScore = () => note()?.focus_score ?? 0;
  const streakDays = () => note()?.streak_days ?? 0;
  const totalHours = () => note()?.total_active_hours ?? 0;
  const learnHours = () => note()?.learning_hours ?? 0;

  // ── Data fetch ──
  async function load() {
    try {
      const data = await invoke<DailyNote>("generate_daily_note", { date: todayStr() });
      setNote(data);
      setFailed(false);
    } catch {
      setFailed(true);
      setNote(null);
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }

  createEffect(() => {
    load();
  });

  function handleRefresh() {
    setRefreshing(true);
    setLoading(true);
    load();
  }

  // ── Timeline blocks (derived from note data) ──
  // We don't have per-时段 breakdown from the API, so we create
  // placeholder blocks proportional to total hours using the categories
  // from what_you_did / learning_hours heuristic.
  const timelineBlocks = createMemo<TimelineBlock[]>(() => {
    const n = note();
    if (!n || n.total_active_hours <= 0) return [];
    const total = n.total_active_hours;
    const blocks: TimelineBlock[] = [];

    // Heuristic distribution based on what's available
    if (n.learning_hours > 0) {
      blocks.push({
        label: CATEGORY_META.study.label,
        color: CATEGORY_META.study.color,
        hours: n.learning_hours,
      });
    }
    if (n.deep_work_hours > 0 && n.deep_work_hours !== n.learning_hours) {
      blocks.push({
        label: "🎯 深度工作",
        color: CATEGORY_META.code.color,
        hours: n.deep_work_hours,
      });
    }
    const otherHours = total - blocks.reduce((s, b) => s + b.hours, 0);
    if (otherHours > 0.3) {
      blocks.push({
        label: CATEGORY_META.other.label,
        color: CATEGORY_META.other.color,
        hours: Math.round(otherHours * 10) / 10,
      });
    }
    return blocks;
  });

  // ── Today's quote ──
  const quote = createMemo(() => {
    const n = note();
    if (!n) return randomQuote();
    // Generate a personalized quote based on data
    if (n.focus_score >= 80) return "今天的专注力爆棚 🔥🔥";
    if (n.learning_hours >= 3) return "今天学了不少东西 👏";
    if (n.streak_days >= 7) return `连续活跃 ${n.streak_days} 天，这个节奏太棒了 🎯`;
    if (n.total_active_hours >= 6) return "充实的一天，给自己鼓个掌 🌟";
    return randomQuote();
  });

  // ── Loading / Empty ──
  if (loading()) {
    return (
      <div class="space-y-6 animate-pulse">
        <div class="h-10 w-3/4 rounded-xl bg-zinc-800" />
        <div class="grid grid-cols-4 gap-4">
          {Array.from({ length: 4 }).map(() => (
            <div class="h-24 rounded-2xl bg-zinc-800" />
          ))}
        </div>
        <div class="h-20 rounded-2xl bg-zinc-800" />
        <div class="h-16 rounded-2xl bg-zinc-800" />
        <div class="grid grid-cols-2 gap-4">
          <div class="h-32 rounded-2xl bg-zinc-800" />
          <div class="h-32 rounded-2xl bg-zinc-800" />
        </div>
      </div>
    );
  }

  // ── Empty state ──
  if (failed || !note()) {
    return (
      <div class="flex flex-col items-center justify-center py-20 text-center space-y-4">
        <span class="text-6xl">🌱</span>
        <h2 class="text-lg font-semibold text-zinc-300">还没有成长记录</h2>
        <p class="text-sm text-zinc-500 max-w-md leading-relaxed">
          开始追踪后，这里会显示你的成长记录 🌱<br />
          每一天的专注、学习、坚持，都值得被看见。
        </p>
        <button
          onClick={handleRefresh}
          class="flex items-center gap-2 px-5 py-2.5 rounded-xl bg-emerald-700/40 text-emerald-300 text-sm border border-emerald-700/50 hover:bg-emerald-700/60 transition-colors"
        >
          <RefreshCw size={14} />
          刷新
        </button>
      </div>
    );
  }

  // ── Main render ──
  return (
    <div class="space-y-7 text-zinc-100">

      {/* ── 顶部：今日一句话 ── */}
      <div class="flex items-start justify-between">
        <div class="space-y-1">
          <p class="text-2xl font-bold tracking-tight leading-snug text-emerald-200">
            {quote()}
          </p>
          <p class="text-xs text-zinc-600">
            {note()?.date ?? todayStr()} · 成长日记
          </p>
        </div>
        <button
          onClick={handleRefresh}
          title="刷新"
          class="p-2 rounded-xl text-zinc-500 hover:text-emerald-400 hover:bg-zinc-800 transition-colors"
          classList={{ "animate-spin": refreshing }}
        >
          <RefreshCw size={18} />
        </button>
      </div>

      {/* ── 统计卡片（4 个横向排列） ── */}
      <div class="grid grid-cols-4 gap-4">
        {[
          { icon: "🔥", label: "连续活跃", value: `${streakDays()} 天`, color: "text-orange-400" },
          { icon: "📚", label: "学习时间", value: `${hoursToStr(learnHours())}`, color: "text-yellow-400" },
          { icon: "🎯", label: "专注度", value: `${focusScore()}/100`, color: "text-emerald-400" },
          { icon: "⭐", label: "总活跃天数", value: `${streakDays()} 天`, color: "text-amber-400" },
        ].map((card) => (
          <div class="rounded-2xl bg-zinc-900/80 border border-zinc-800/60 p-5 hover:border-zinc-700/60 transition-colors">
            <div class="text-2xl mb-2">{card.icon}</div>
            <div class="text-xs text-zinc-500 mb-1">{card.label}</div>
            <div class={`text-xl font-bold font-mono ${card.color}`}>{card.value}</div>
          </div>
        ))}
      </div>

      {/* ── 时间线：今天各时段做了啥 ── */}
      <Show when={timelineBlocks().length > 0}>
        <div class="rounded-2xl bg-zinc-900/80 border border-zinc-800/60 p-5">
          <h3 class="text-sm font-medium text-zinc-400 mb-3">⏳ 时间分布</h3>
          <div class="flex h-10 rounded-xl overflow-hidden border border-zinc-700/50">
            <For each={timelineBlocks()}>
              {(block) => {
                const pct = Math.round((block.hours / Math.max(note()!.total_active_hours, 0.1)) * 100);
                return (
                  <div
                    class="flex items-center justify-center text-[10px] font-medium text-white/90 transition-all"
                    style={{
                      width: `${Math.max(pct, 5)}%`,
                      "background-color": block.color,
                    }}
                    title={`${block.label}: ${hoursToStr(block.hours)}`}
                  >
                    {pct > 12 ? block.label : ""}
                  </div>
                );
              }}
            </For>
          </div>
          <div class="flex flex-wrap gap-3 mt-3">
            <For each={timelineBlocks()}>
              {(b) => (
                <span class="flex items-center gap-1.5 text-xs text-zinc-500">
                  <span class="w-2.5 h-2.5 rounded-sm" style={{ background: b.color }} />
                  {b.label}
                  <span class="text-zinc-600 font-mono">{hoursToStr(b.hours)}</span>
                </span>
              )}
            </For>
          </div>
        </div>
      </Show>

      {/* ── "今天你做了什么" AI 总结 ── */}
      <Show when={note()?.what_you_did || note()?.what_you_learned}>
        <div class="rounded-2xl bg-gradient-to-br from-emerald-950/60 to-zinc-900/80 border border-emerald-900/40 p-6">
          <div class="flex items-center gap-2 text-sm text-emerald-400/80 mb-3">
            <Sparkles size={16} />
            <span>AI 总结</span>
          </div>
          <p class="text-lg leading-relaxed text-zinc-200">
            {note()?.what_you_did}
          </p>
          <Show when={note()?.what_you_learned}>
            <div class="mt-3 pt-3 border-t border-emerald-900/30">
              <span class="text-xs text-emerald-500/70 font-medium">📖 学到了什么</span>
              <p class="text-base text-zinc-300 mt-1">{note()?.what_you_learned}</p>
            </div>
          </Show>
        </div>
      </Show>

      {/* ── "做得好的地方" + "小建议" 并排卡片 ── */}
      <div class="grid grid-cols-2 gap-5">
        {/* 夸奖 */}
        <Show when={note()?.good_habit}>
          <div class="rounded-2xl bg-zinc-900/80 border-l-4 border-emerald-500 p-5">
            <div class="flex items-center gap-2 mb-2">
              <span class="text-lg">🌟</span>
              <h3 class="text-sm font-semibold text-emerald-400">做得好的地方</h3>
            </div>
            <p class="text-sm text-zinc-300 leading-relaxed">{note()?.good_habit}</p>
          </div>
        </Show>

        {/* 建议 */}
        <Show when={note()?.tip}>
          <div class="rounded-2xl bg-zinc-900/80 border-l-4 border-orange-500 p-5">
            <div class="flex items-center gap-2 mb-2">
              <span class="text-lg">💡</span>
              <h3 class="text-sm font-semibold text-orange-400">小建议</h3>
            </div>
            <p class="text-sm text-zinc-300 leading-relaxed">{note()?.tip}</p>
          </div>
        </Show>
      </div>

    </div>
  );
};

export default Growth;
