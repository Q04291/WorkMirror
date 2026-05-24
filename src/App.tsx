import { Component, createSignal, createEffect, onCleanup, Show, For } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import {
  LayoutDashboard,
  List,
  FileText,
  Settings,
  Activity,
  Sparkles,
} from "lucide-solid";
import Dashboard from "./pages/Dashboard";
import Growth from "./pages/Growth";
import Details from "./pages/Details";
import Report from "./pages/Report";
import SettingsPage from "./pages/Settings";
import ReminderToast from "./pages/ReminderToast";

type TabId = "dashboard" | "growth" | "details" | "report" | "settings";

const TABS: TabId[] = ["dashboard", "growth", "details", "report", "settings"];

const App: Component = () => {
  const [activeTab, setActiveTab] = createSignal<TabId>("dashboard");
  const [tracking, setTracking] = createSignal(false);

  createEffect(() => {
    document.documentElement.classList.add("dark");
  });

  // Poll tracking status (silently fail if no Tauri backend)
  async function pollTracking() {
    try {
      const status = await invoke("get_tracking_status");
      setTracking(Boolean(status));
    } catch {}
  }

  createEffect(() => {
    pollTracking();
    const id = setInterval(pollTracking, 5000);
    onCleanup(() => clearInterval(id));
  });

  return (
    <div class="flex h-screen w-screen bg-zinc-950 text-zinc-100 overflow-hidden">
      {/* Sidebar */}
      <aside class="flex flex-col w-56 border-r border-zinc-800 bg-zinc-950 flex-shrink-0">
        <div class="flex items-center gap-2.5 px-5 h-14 border-b border-zinc-800">
          <div class="w-7 h-7 rounded-lg bg-indigo-600 flex items-center justify-center">
            <Activity size={16} class="text-white" />
          </div>
          <span class="text-sm font-semibold tracking-tight">WorkMirror</span>
        </div>

        <div class="px-5 py-3 border-b border-zinc-800">
          <div class="flex items-center gap-2 text-xs">
            <span class="w-2 h-2 rounded-full" classList={{
              "bg-green-400 shadow-[0_0_6px_rgba(74,222,128,0.5)]": tracking(),
              "bg-zinc-600": !tracking(),
            }} />
            <span classList={{"text-green-400": tracking(), "text-zinc-500": !tracking()}}>
              {tracking() ? "追踪中" : "已停止"}
            </span>
          </div>
        </div>

        <nav class="flex-1 px-3 py-4 space-y-1">
          <For each={TABS}>
            {(tab) => {
              const icons: Record<TabId, any> = {
                dashboard: LayoutDashboard,
                growth: Sparkles,
                details: List,
                report: FileText,
                settings: Settings,
              };
              const labels: Record<TabId, string> = {
                dashboard: "仪表盘",
                growth: "成长",
                details: "详细数据",
                report: "报告",
                settings: "设置",
              };
              const Icon = icons[tab];
              const isActive = () => activeTab() === tab;
              return (
                <button
                  onClick={() => setActiveTab(tab)}
                  class={`w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-colors duration-150 ${
                    isActive()
                      ? "bg-zinc-800 text-zinc-100 font-medium"
                      : "text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/50"
                  }`}
                >
                  <Icon size={16} class={isActive() ? "text-indigo-400" : ""} />
                  <span>{labels[tab]}</span>
                </button>
              );
            }}
          </For>
        </nav>
      </aside>

      {/* Main */}
      <main class="flex-1 flex flex-col overflow-hidden">
        <header class="flex items-center justify-between h-14 px-6 border-b border-zinc-800 flex-shrink-0">
          <h1 class="text-sm font-medium">WorkMirror</h1>
        </header>
        <div class="flex-1 overflow-y-auto p-6">
          {activeTab() === "dashboard" && <Dashboard />}
          {activeTab() === "growth" && <Growth />}
          {activeTab() === "details" && <Details />}
          {activeTab() === "report" && <Report />}
          {activeTab() === "settings" && <SettingsPage />}
        </div>
      </main>
      <ReminderToast />
    </div>
  );
};

export default App;
