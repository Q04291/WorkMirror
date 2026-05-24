import { Component, createSignal, createEffect, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { FileText, Download, RefreshCw } from "lucide-solid";

const Report: Component = () => {
  const [html, setHtml] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  async function fetchReport() {
    setLoading(true);
    setError(null);
    try {
      const report = await invoke("get_weekly_report");
      setHtml(JSON.stringify(report, null, 2));
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }

  createEffect(() => { fetchReport(); });

  return (
    <div class="space-y-6 text-zinc-100 max-w-4xl">

      {/* Header */}
      <div class="flex items-center justify-between">
        <h2 class="text-sm font-semibold flex items-center gap-2">
          <FileText size={16} class="text-indigo-400" />
          周报
        </h2>
        <div class="flex items-center gap-2">
          <button
            onClick={fetchReport}
            disabled={loading()}
            class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-800 hover:bg-zinc-700 text-zinc-300 disabled:opacity-50"
          >
            <RefreshCw size={12} class={loading() ? "animate-spin" : ""} />
            {loading() ? "生成中..." : "刷新"}
          </button>
          <Show when={html()}>
            <button
              onClick={async () => {
                try {
                  const path = await invoke("export_report", { format: "json" });
                  alert("报告已导出: " + path);
                } catch (e) {
                  alert("导出失败: " + e);
                }
              }}
              class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-indigo-600 hover:bg-indigo-500 text-white"
            >
              <Download size={12} />
              导出 JSON
            </button>
          </Show>
        </div>
      </div>

      {/* Error */}
      <Show when={error()}>
        {(e) => (
          <div class="rounded-xl bg-red-900/30 border border-red-800 p-4 text-xs text-red-400">
            {e()}
            <button onClick={fetchReport} class="ml-2 underline">重试</button>
          </div>
        )}
      </Show>

      {/* Content */}
      <Show
        when={html()}
        fallback={
          <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-8 text-center">
            <p class="text-xs text-zinc-500">
              {loading() ? "正在生成报告..." : "暂无数据。追踪一段时间后回到这里查看周报。"}
            </p>
          </div>
        }
      >
        <pre class="rounded-xl bg-zinc-900 border border-zinc-800 p-4 text-xs text-zinc-300 overflow-auto max-h-[600px] leading-relaxed whitespace-pre-wrap font-mono">
          {html()}
        </pre>
      </Show>

    </div>
  );
};

export default Report;
