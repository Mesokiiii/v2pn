import { Component, Show, createSignal, onMount, onCleanup } from "solid-js";
import { Portal } from "solid-js/web";
import { AlertTriangle, X } from "lucide-solid";

/**
 * Modal confirmation dialog. Imperative API:
 *
 *   const yes = await confirm({
 *     title: "Удалить подписку?",
 *     body: "Все её серверы исчезнут. Действие нельзя отменить.",
 *     confirmLabel: "Удалить",
 *     destructive: true,
 *   });
 *   if (yes) { … }
 *
 * No global state singletons — each call mounts a fresh `<ConfirmRoot>`
 * via `Portal`, listens for the user's choice, then cleans up.
 *
 * Behaviour:
 *  - Backdrop click closes with `false`.
 *  - `Escape` closes with `false`.
 *  - `Enter` on the focused confirm button → `true`.
 *  - First button focused on mount so keyboard users don't have to chase
 *    the cursor.
 *  - `destructive: true` switches the confirm button to a red variant —
 *    the same visual cue Apple/GitHub/Linear use for "are you sure"
 *    moments.
 */

interface ConfirmOptions {
  title: string;
  /** Optional body — short paragraph (~2 sentences). */
  body?: string;
  /** Default: "Подтвердить" / "Confirm". Caller usually overrides. */
  confirmLabel?: string;
  /** Default: "Отмена" / "Cancel". */
  cancelLabel?: string;
  /** Renders the confirm button in red. */
  destructive?: boolean;
}

export function confirm(opts: ConfirmOptions): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    // Mount a one-shot root that resolves the promise on close. We
    // create a detached <div> so multiple confirms can stack if needed
    // (each gets its own portal target).
    const host = document.createElement("div");
    document.body.appendChild(host);

    let dispose: (() => void) | undefined;

    // Lazy-import Solid's render to keep this module side-effect-free.
    import("solid-js/web").then(({ render }) => {
      dispose = render(
        () => (
          <ConfirmRoot
            opts={opts}
            onClose={(value) => {
              resolve(value);
              setTimeout(() => {
                dispose?.();
                host.remove();
              }, 200); // allow exit animation
            }}
          />
        ),
        host,
      );
    });
  });
}

const ConfirmRoot: Component<{
  opts: ConfirmOptions;
  onClose: (value: boolean) => void;
}> = (p) => {
  const [exiting, setExiting] = createSignal(false);
  let confirmBtn: HTMLButtonElement | undefined;

  const finish = (value: boolean) => {
    setExiting(true);
    // Match the CSS exit animation duration (160 ms).
    setTimeout(() => p.onClose(value), 160);
  };

  onMount(() => {
    confirmBtn?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") finish(false);
    };
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  return (
    <Portal>
      <div
        class="confirm-backdrop"
        data-exiting={exiting() ? "1" : "0"}
        onClick={(e) => {
          if (e.target === e.currentTarget) finish(false);
        }}
      >
        <div
          class="confirm-card"
          role="alertdialog"
          aria-labelledby="confirm-title"
          aria-describedby="confirm-body"
          data-destructive={p.opts.destructive ? "1" : "0"}
        >
          <button
            type="button"
            class="confirm-close"
            aria-label="Закрыть"
            onClick={() => finish(false)}
          >
            <X size={14} />
          </button>

          <div class="confirm-header">
            <div class="confirm-icon">
              <AlertTriangle size={18} strokeWidth={2.2} />
            </div>
            <div class="confirm-text">
              <h3 id="confirm-title" class="confirm-title">
                {p.opts.title}
              </h3>
              <Show when={p.opts.body}>
                <p id="confirm-body" class="confirm-body">
                  {p.opts.body}
                </p>
              </Show>
            </div>
          </div>

          <div class="confirm-divider" aria-hidden="true" />

          <div class="confirm-footer">
            <button
              type="button"
              class="confirm-btn confirm-btn--ghost"
              onClick={() => finish(false)}
            >
              {p.opts.cancelLabel ?? "Отмена"}
            </button>
            <button
              ref={confirmBtn}
              type="button"
              class="confirm-btn"
              classList={{
                "confirm-btn--danger": !!p.opts.destructive,
                "confirm-btn--primary": !p.opts.destructive,
              }}
              onClick={() => finish(true)}
              onKeyDown={(e) => {
                if (e.key === "Enter") finish(true);
              }}
            >
              {p.opts.confirmLabel ?? "Подтвердить"}
            </button>
          </div>
        </div>
      </div>
    </Portal>
  );
};
