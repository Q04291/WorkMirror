import { Component, createSignal, createEffect, onCleanup, Show, For, createMemo } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { Activity, Clock, Focus, ArrowLeftRight, TrendingUp, TrendingDown } from "lucide-solid";

function formatDuration(totalSeconds: number): string {
  if (totalSeconds <= 0) return "0m";
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

function todayDateStr(): string {
  return new Date().toISOString().slice(0, 10);
}

function yesterdayDateStr(): string {
  const d = new Date();
  d.setDate(d.getDate() - 1);
  return d.toISOString().slice(0, 10);
}

const PIE_COLORS = ["#6366f1", "#22c55e", "#f59e0b", "#ef4444", "#06b6d4", "#a855f7", "#f97316", "#84cc16"];

const Dashboard: Component = () => {
  const [tracking, setTracking] = createSignal(false);
  const [active, setActive] = createSignal<string>("—");
  const [todaySecs, setTodaySecs] = createSignal(0);
  const [deepSecs, setDeepSecs] = createSignal(0);
  const [switches, setSwitches] = createSignal(0);
  const [breakdown, setBreakdown] = createSignal<Record<string, number>>({});
  const [yesterdaySecs, setYesterdaySecs] = createSignal<number | null>(null);
  const [error, setError] = createSignal<string | null>(null);

  async function refresh() {
    try {
      const s = await invoke("get_tracking_status");
      setTracking(Boolean(s));
    } catch { /* ignore */ }

    try {
      const act = await invoke("get_current_activity");
      if (act) setActive(String(act));
    } catch { /* ignore */ }

    try {
      const summary = await invoke("get_daily_summary", { date: todayDateStr() });
      if (summary) {
        const s: any = summary;
        setTodaySecs(s.total_active_seconds ?? 0);
        setDeepSecs(s.deep_work_seconds ?? 0);
        setSwitches(s.switch_count ?? 0);
        setBreakdown(s.app_breakdown ?? {});
      }
    } catch { /* ignore */ }

    try {
      const y = await invoke("get_daily_summary", { date: yesterdayDateStr() });
      if (y) setYesterdaySecs((y as any).total_active_seconds ?? null);
    } catch { /* ignore */ }
  }

  createEffect(() => {
    refresh();
    const id = setInterval(refresh, 30000);
    onCleanup(() => clearInterval(id));
  });

  const totalHours = () => (todaySecs() / 3600).toFixed(1);
  const deepHours = () => (deepSecs() / 3600).toFixed(1);
  const deepPct = () => todaySecs() > 0 ? Math.round((deepSecs() / todaySecs()) * 100) : 0;

  const yesterdayChange = createMemo(() => {
    if (yesterdaySecs() == null || yesterdaySecs() === 0) return null;
    return Math.round(((todaySecs() - yesterdaySecs()!) / yesterdaySecs()!) * 100);
  });

  const pieData = createMemo(() => {
    const entries = Object.entries(breakdown()).sort((a, b) => b[1] - a[1]);
    const top = entries.slice(0, 5);
    const rest = entries.slice(5).reduce((sum, [, v]) => sum + v, 0);
    const result = top.map(([label, value], i) => ({ label, value, color: PIE_COLORS[i % PIE_COLORS.length] }));
    if (rest > 0) result.push({ label: "其他", value: rest, color: "#52525b" });
    return result;
  });

  return (
    <div class="space-y-6 text-zinc-100">

      {/* Error */}
      <Show when={error()}>
        {(e) => (
          <div class="rounded-xl bg-red-900/30 border border-red-800 p-4 text-xs text-red-400 flex items-center justify-between">
            <span>{e()}</span>
            <button onClick={() => { setError(null); refresh(); }} class="underline">重试</button>
          </div>
        )}
      </Show>

      {/* Tracking indicator */}
      <div class="flex items-center gap-2 text-xs text-zinc-500">
        <span class={`w-2 h-2 rounded-full ${tracking() ? "bg-green-400 animate-pulse" : "bg-zinc-600"}`} />
        {tracking() ? "正在追踪" : "追踪已停止"}
      </div>

      {/* Stats cards */}
      <div class="grid grid-cols-3 gap-4">
        {[
          { icon: Clock, label: "活跃时长", value: `${totalHours()}h`, warn: todaySecs() > 28800 },
          { icon: Focus, label: "深度工作", value: `${deepHours()}h (${deepPct()}%)`, warn: deepPct() < 20 },
          { icon: ArrowLeftRight, label: "切换次数", value: `${switches()} 次` },
        ].map((card) => (
          <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-4">
            <div class="flex items-center gap-2 text-xs text-zinc-500 mb-2">
              <card.icon size={14} class="text-indigo-400" />
              {card.label}
            </div>
            <div class={`text-2xl font-mono font-bold ${card.warn ? "text-orange-400" : "text-white"}`}>
              {card.value}
            </div>
          </div>
        ))}
      </div>

      {/* Yesterday comparison */}
      <Show when={yesterdayChange() != null}>
        <div class="flex items-center gap-2 text-xs text-zinc-500">
          {yesterdayChange()! >= 0 ? (
            <><TrendingUp size={14} class="text-green-400" /><span class="text-green-400">+{yesterdayChange()}%</span></>
          ) : (
            <><TrendingDown size={14} class="text-red-400" /><span class="text-red-400">{yesterdayChange()}%</span></>
          )}
          <span>vs 昨天</span>
        </div>
      </Show>

      {/* Current activity */}
      <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-4">
        <div class="flex items-center gap-2 text-xs text-zinc-500 mb-2">
          <Activity size={14} class="text-indigo-400" />
          当前应用
        </div>
        <div class="text-sm font-mono">{active()}</div>
      </div>

      {/* Pie chart + breakdown */}
      <div class="grid grid-cols-2 gap-4">
        <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-4">
          <h3 class="text-xs text-zinc-500 mb-3">应用分布</h3>
          <Show when={pieData().length > 0} fallback={<div class="text-xs text-zinc-600">暂无数据</div>}>
            <svg width="160" height="160" viewBox="0 0 160 160" class="mx-auto">
              <For each={pieData()}>
                {(slice, idx) => {
                  const total = pieData().reduce((s, d) => s + d.value, 0);
                  if (total === 0) return null;
                  let angle = -90;
                  for (let i = 0; i < idx(); i++) angle += (pieData()[i].value / total) * 360;
                  const a = (slice.value / total) * 360;
                  const sr = ((angle) * Math.PI) / 180;
                  const er = ((angle + a) * Math.PI) / 180;
                  const x1 = 80 + 60 * Math.cos(sr);
                  const y1 = 80 + 60 * Math.sin(sr);
                  const x2 = 80 + 60 * Math.cos(er);
                  const y2 = 80 + 60 * Math.sin(er);
                  return <path d={`M 80 80 L ${x1} ${y1} A 60 60 0 ${a > 180 ? 1 : 0} 1 ${x2} ${y2} Z`} fill={slice.color} />;
                }}
              </For>
            </svg>
            <div class="mt-3 space-y-1">
              <For each={pieData()}>
                {(d) => (
                  <div class="flex items-center gap-2 text-xs">
                    <span class="w-2 h-2 rounded-full" style={`background:${d.color}`} />
                    <span class="text-zinc-400 flex-1 truncate">{d.label}</span>
                    <span class="font-mono text-zinc-300">{formatDuration(d.value)}</span>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </div>

        {/* App ranking */}
        <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-4">
          <h3 class="text-xs text-zinc-500 mb-3">应用排行</h3>
          <Show when={pieData().length > 0} fallback={<div class="text-xs text-zinc-600">暂无数据</div>}>
            <div class="space-y-3">
              <For each={pieData()}>
                {(d, i) => {
                  const maxVal = pieData()[0]?.value || 1;
                  const pct = Math.round((d.value / maxVal) * 100);
                  return (
                    <div>
                      <div class="flex justify-between text-xs mb-1">
                        <span class="text-zinc-400">{i() + 1}. {d.label}</span>
                        <span class="font-mono text-zinc-300">{formatDuration(d.value)}</span>
                      </div>
                      <div class="h-1.5 rounded-full bg-zinc-800 overflow-hidden">
                        <div class="h-full rounded-full transition-all duration-500" style={`width:${pct}%;background:${d.color}`} />
                      </div>
                    </div>
                  );
                }}
              </For>
            </div>
          </Show>
        </div>
      </div>

      {/* Refresh button */}
      <button onClick={refresh} class="text-xs text-zinc-600 hover:text-zinc-400 underline">
        刷新数据
      </button>

    </div>
  );
};

export default Dashboard;
