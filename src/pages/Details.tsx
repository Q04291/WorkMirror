import { Component, createSignal, createEffect, Show, For } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { List, RefreshCw } from "lucide-solid";

function todayDateStr(): string {
  return new Date().toISOString().slice(0, 10);
}

function formatDuration(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

const Details: Component = () => {
  const [date, setDate] = createSignal(todayDateStr());
  const [data, setData] = createSignal<any>(null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  async function fetchDate(d: string) {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke("get_daily_summary", { date: d });
      setData(result);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  createEffect(() => { fetchDate(date()); });

  const breakdown = (): [string, number][] => {
    const b = data()?.app_breakdown;
    if (!b) return [];
    return Object.entries(b as Record<string, number>).sort((a, b) => b[1] - a[1]);
  };

  return (
    <div class="space-y-6 text-zinc-100 max-w-3xl">

      {/* Date picker */}
      <div class="flex items-center gap-4">
        <h2 class="text-sm font-semibold flex items-center gap-2">
          <List size={16} class="text-indigo-400" />
          详细数据
        </h2>
        <input
          type="date"
          value={date()}
          onInput={(e) => setDate(e.currentTarget.value)}
          class="px-2 py-1 rounded bg-zinc-800 border border-zinc-700 text-xs text-zinc-100"
        />
        <button
          onClick={() => fetchDate(date())}
          disabled={loading()}
          class="flex items-center gap-1 px-2 py-1 rounded text-xs bg-zinc-800 hover:bg-zinc-700 text-zinc-400"
        >
          <RefreshCw size={11} class={loading() ? "animate-spin" : ""} />
          刷新
        </button>
      </div>

      {/* Error */}
      <Show when={error()}>
        {(e) => (
          <div class="rounded-xl bg-red-900/30 border border-red-800 p-4 text-xs text-red-400">
            {e()}
            <button onClick={() => fetchDate(date())} class="ml-2 underline">重试</button>
          </div>
        )}
      </Show>

      {/* Stats */}
      <Show when={data() && !error()}>
        <div class="grid grid-cols-3 gap-4">
          <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-4">
            <div class="text-xs text-zinc-500 mb-1">活跃时长</div>
            <div class="text-2xl font-mono font-bold text-white">
              {formatDuration((data() as any)?.total_active_seconds ?? 0)}
            </div>
          </div>
          <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-4">
            <div class="text-xs text-zinc-500 mb-1">深度工作</div>
            <div class="text-2xl font-mono font-bold text-white">
              {formatDuration((data() as any)?.deep_work_seconds ?? 0)}
            </div>
          </div>
          <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-4">
            <div class="text-xs text-zinc-500 mb-1">切换次数</div>
            <div class="text-2xl font-mono font-bold text-white">
              {(data() as any)?.switch_count ?? 0}
            </div>
          </div>
        </div>

        {/* App list */}
        <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-4">
          <h3 class="text-xs text-zinc-500 mb-3">应用使用时间</h3>
          <Show
            when={breakdown().length > 0}
            fallback={<div class="text-xs text-zinc-600">当天无记录</div>}
          >
            <table class="w-full text-xs">
              <thead>
                <tr class="text-zinc-500 border-b border-zinc-800">
                  <th class="text-left py-2 font-medium">应用</th>
                  <th class="text-right py-2 font-medium">时长</th>
                  <th class="text-right py-2 font-medium">占比</th>
                </tr>
              </thead>
              <tbody>
                <For each={breakdown()}>
                  {([app, secs], i) => {
                    const total = (data() as any)?.total_active_seconds ?? 1;
                    const pct = Math.round((secs / total) * 100);
                    return (
                      <tr class="border-b border-zinc-800/50">
                        <td class="py-2 text-zinc-300">{i() + 1}. {app}</td>
                        <td class="py-2 text-right font-mono text-zinc-300">{formatDuration(secs)}</td>
                        <td class="py-2 text-right text-zinc-500">{pct}%</td>
                      </tr>
                    );
                  }}
                </For>
              </tbody>
            </table>
          </Show>
        </div>
      </Show>

      {/* Empty state */}
      <Show when={!data() && !loading() && !error()}>
        <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-8 text-center text-xs text-zinc-500">
          请选择一个日期查看数据
        </div>
      </Show>

    </div>
  );
};

export default Details;
