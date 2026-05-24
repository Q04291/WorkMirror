import {
  Component,
  createSignal,
  createEffect,
  Show,
  onCleanup,
} from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import {
  Play,
  Square,
  Settings2,
  Brain,
  Trash2,
  AlertTriangle,
  Info,
  RefreshCw,
  CheckCircle2,
} from "lucide-solid";

const Settings: Component = () => {
  const [tracking, setTracking] = createSignal(false);
  const [pollInput, setPollInput] = createSignal("5");
  const [ollamaUrlInput, setOllamaUrlInput] = createSignal("http://localhost:11434");
  const [ollamaModelInput, setOllamaModelInput] = createSignal("llama3.2");
  const [toggling, setToggling] = createSignal(false);
  const [saving, setSaving] = createSignal(false);
  const [clearing, setClearing] = createSignal(false);
  const [clearConfirm, setClearConfirm] = createSignal(false);
  const [saveMessage, setSaveMessage] = createSignal<{ type: "success" | "error"; text: string } | null>(null);

  let saveTimer: number | undefined;

  // Try fetching data from backend; silently fail
  createEffect(() => {
    (async () => {
      try {
        const s = await invoke("get_tracking_status");
        setTracking(Boolean(s));
      } catch (e) {
        console.log("Settings: get_tracking_status failed", e);
      }
    })();
  });

  function showMessage(type: "success" | "error", text: string) {
    setSaveMessage({ type, text });
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => setSaveMessage(null), 3000);
  }

  async function handleToggleTracking() {
    setToggling(true);
    try {
      if (tracking()) {
        await invoke("stop_tracking");
        setTracking(false);
      } else {
        await invoke("start_tracking");
        setTracking(true);
      }
    } catch (err) {
      showMessage("error", String(err));
    } finally {
      setToggling(false);
    }
  }

  return (
    <div class="space-y-6 text-zinc-100 max-w-2xl">

      {/* Tracking toggle */}
      <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-5">
        <h2 class="text-sm font-semibold mb-3 flex items-center gap-2">
          <Play size={14} class="text-indigo-400" />
          追踪控制
        </h2>
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-2 text-xs">
            <span class={`w-2 h-2 rounded-full ${tracking() ? "bg-green-400" : "bg-zinc-600"}`} />
            <span>{tracking() ? "追踪中" : "已停止"}</span>
          </div>
          <button
            onClick={handleToggleTracking}
            disabled={toggling()}
            class={`flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              tracking()
                ? "bg-red-600 hover:bg-red-500 text-white"
                : "bg-green-600 hover:bg-green-500 text-white"
            } disabled:opacity-50 disabled:cursor-not-allowed`}
          >
            <Show when={tracking()} fallback={<Play size={16} />}>
              <Square size={16} />
            </Show>
            <span>{toggling() ? "处理中..." : tracking() ? "停止追踪" : "开始追踪"}</span>
          </button>
        </div>
      </div>

      {/* Detection interval */}
      <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-5 space-y-4">
        <h2 class="text-sm font-semibold flex items-center gap-2">
          <Settings2 size={14} class="text-indigo-400" />
          追踪设置
        </h2>
        <div class="flex items-center gap-4">
          <label class="text-xs text-zinc-400 w-32">检测间隔（秒）</label>
          <input
            type="number"
            min="1"
            max="3600"
            value={pollInput()}
            onInput={(e) => setPollInput(e.currentTarget.value)}
            class="w-20 px-2 py-1 rounded bg-zinc-800 border border-zinc-700 text-xs text-zinc-100"
          />
        </div>
        <div class="flex items-center gap-4">
          <label class="text-xs text-zinc-400 w-32">Ollama 地址</label>
          <input
            value={ollamaUrlInput()}
            onInput={(e) => setOllamaUrlInput(e.currentTarget.value)}
            class="flex-1 px-2 py-1 rounded bg-zinc-800 border border-zinc-700 text-xs text-zinc-100"
          />
        </div>
        <div class="flex items-center gap-4">
          <label class="text-xs text-zinc-400 w-32">AI 模型</label>
          <input
            value={ollamaModelInput()}
            onInput={(e) => setOllamaModelInput(e.currentTarget.value)}
            class="flex-1 px-2 py-1 rounded bg-zinc-800 border border-zinc-700 text-xs text-zinc-100"
          />
        </div>
      </div>

      {/* Clear data */}
      <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-5 space-y-4">
        <h2 class="text-sm font-semibold flex items-center gap-2">
          <Trash2 size={14} class="text-red-400" />
          数据管理
        </h2>
        <p class="text-xs text-zinc-500">清除所有活动记录和配置。此操作不可恢复。</p>
        <Show
          when={!clearConfirm()}
          fallback={
            <div class="flex items-center gap-2">
              <button
                onClick={async () => {
                  setClearing(true);
                  try { await invoke("clear_all_data"); showMessage("success", "已清除"); setClearConfirm(false); }
                  catch (e) { showMessage("error", String(e)); }
                  finally { setClearing(false); }
                }}
                disabled={clearing()}
                class="px-3 py-1.5 rounded text-xs font-medium bg-red-600 hover:bg-red-500 text-white"
              >
                {clearing() ? "清除中..." : "确认清除"}
              </button>
              <button onClick={() => setClearConfirm(false)} class="px-3 py-1.5 rounded text-xs bg-zinc-800 text-zinc-400">
                取消
              </button>
            </div>
          }
        >
          <button
            onClick={() => setClearConfirm(true)}
            class="px-3 py-1.5 rounded text-xs font-medium bg-zinc-800 hover:bg-zinc-700 text-zinc-300"
          >
            清除所有数据
          </button>
        </Show>
      </div>

      {/* Save message */}
      <Show when={saveMessage()}>
        {(msg) => (
          <div class={`fixed bottom-6 right-6 px-4 py-2 rounded-lg text-xs ${
            msg().type === "success" ? "bg-green-600 text-white" : "bg-red-600 text-white"
          }`}>
            {msg().text}
          </div>
        )}
      </Show>

      {/* About */}
      <div class="rounded-xl bg-zinc-900 border border-zinc-800 p-5">
        <h2 class="text-sm font-semibold flex items-center gap-2 mb-2">
          <Info size={14} class="text-indigo-400" />
          关于 WorkMirror
        </h2>
        <p class="text-xs text-zinc-500">版本 0.1.0</p>
        <p class="text-xs text-zinc-600 mt-1">A privacy-first, 100% local AI work tracker.</p>
      </div>

    </div>
  );
};

export default Settings;
