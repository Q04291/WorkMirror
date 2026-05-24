import { Component, createSignal, createEffect, onCleanup, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

const REMINDER_MESSAGES = [
  "你已经连续工作 50 分钟了，起来走走喝杯水 🌿",
  "休息时间到！试试站起来伸展一下",
  "今天已经高效工作 1 小时了，干得不错，记得休息 ✨",
  "专注工作值得称赞，但现在该让眼睛和肩膀放松一下了 ☕",
  "起来走走，看看窗外，给自己 5 分钟放空时间 🌻",
];

const ReminderToast: Component = () => {
  const [text, setText] = createSignal<string | null>(null);
  const [visible, setVisible] = createSignal(false);
  const [dismissing, setDismissing] = createSignal(false);

  let fadeTimer: number | undefined;

  function dismiss() {
    if (fadeTimer) clearTimeout(fadeTimer);
    setDismissing(true);
    setTimeout(() => {
      setVisible(false);
      setText(null);
      setDismissing(false);
    }, 400);
    // Tell backend the reminder was handled.
    try {
      invoke("dismiss_reminder");
    } catch {
      /* silent */
    }
  }

  async function poll() {
    try {
      const result: unknown = await invoke("check_reminder");
      if (result && typeof result === "string") {
        setText(result);
        setVisible(true);
        setDismissing(false);

        // Auto-fade after 10 seconds.
        if (fadeTimer) clearTimeout(fadeTimer);
        fadeTimer = window.setTimeout(() => dismiss(), 10000);
      }
    } catch {
      /* silent */
    }
  }

  createEffect(() => {
    // Poll immediately, then every 30 seconds.
    poll();
    const id = setInterval(poll, 30000);
    onCleanup(() => {
      clearInterval(id);
      if (fadeTimer) clearTimeout(fadeTimer);
    });
  });

  return (
    <Show when={visible()}>
      <div
        class="fixed bottom-6 right-6 z-50 transition-all duration-400"
        classList={{
          "opacity-100 translate-y-0 scale-100": !dismissing(),
          "opacity-0 translate-y-4 scale-95": dismissing(),
        }}
      >
        <div
          class="rounded-xl px-5 py-4 shadow-2xl border max-w-sm"
          style={{
            background: "linear-gradient(135deg, #065f46, #854d0e)",
            "border-color": "rgba(251, 191, 36, 0.3)",
          }}
        >
          <div class="flex items-start gap-3">
            <span class="text-xl flex-shrink-0 mt-0.5">☕</span>
            <div class="flex-1 min-w-0">
              <p class="text-sm text-zinc-100 leading-relaxed">{text()}</p>
              <div class="flex items-center gap-2 mt-3">
                <button
                  onClick={() => dismiss()}
                  class="px-3 py-1.5 text-xs font-medium rounded-lg bg-white/15 hover:bg-white/25 text-zinc-100 transition-colors"
                >
                  ☕ 休息一下
                </button>
                <button
                  onClick={() => dismiss()}
                  class="px-3 py-1.5 text-xs rounded-lg bg-white/10 hover:bg-white/20 text-zinc-300 transition-colors"
                >
                  知道了
                </button>
              </div>
            </div>
            <button
              onClick={() => dismiss()}
              class="flex-shrink-0 text-zinc-400 hover:text-zinc-200 transition-colors text-sm leading-none"
            >
              ✕
            </button>
          </div>
        </div>
      </div>
    </Show>
  );
};

export default ReminderToast;
