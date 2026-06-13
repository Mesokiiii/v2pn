import { Component, createSignal, onMount, Show } from "solid-js";
import { X, Link2, FileText, ClipboardPaste } from "lucide-solid";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { t } from "~/lib/i18n";

interface Props {
  open: boolean;
  onClose: () => void;
  onSubmitUrl: (url: string) => void;
  onSubmitText: (text: string) => void;
}

export const ImportDialog: Component<Props> = (props) => {
  return (
    <Show when={props.open}>
      <ImportDialogInner {...props} />
    </Show>
  );
};

const ImportDialogInner: Component<Props> = (props) => {
  const [tab, setTab] = createSignal<"url" | "text">("url");
  const [url, setUrl] = createSignal("");
  const [text, setText] = createSignal("");
  const [mounted, setMounted] = createSignal(false);

  onMount(() => {
    requestAnimationFrame(() => setMounted(true));
  });

  async function pasteFromClipboard() {
    try {
      const t = await readText();
      if (!t) return;
      if (/^https?:\/\//i.test(t.trim())) {
        setTab("url");
        setUrl(t.trim());
      } else {
        setTab("text");
        setText(t);
      }
    } catch {
      /* empty */
    }
  }

  function submit() {
    if (tab() === "url" && url().trim()) props.onSubmitUrl(url().trim());
    else if (tab() === "text" && text().trim()) props.onSubmitText(text());
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") props.onClose();
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") submit();
  }

  return (
    <div
      class="modal-overlay fixed inset-0 z-50 grid place-items-center"
      data-open={mounted()}
      onClick={props.onClose}
      onKeyDown={onKey}
    >
      <div
        class="modal-card hairline relative w-[560px] max-w-[92vw] overflow-hidden rounded-[10px] bg-[var(--color-bg-1)]"
        data-open={mounted()}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="import-title"
      >
        <span class="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-white/15 to-transparent" />

        <header class="flex items-start justify-between gap-3 border-b border-[var(--color-line)] px-5 pb-4 pt-4">
          <div class="min-w-0">
            <h3
              id="import-title"
              class="text-[14px] font-semibold tracking-tight text-[var(--color-fg-0)]"
            >
              {t("importDialog.title")}
            </h3>
            <p class="mt-0.5 text-[12px] leading-snug text-[var(--color-fg-2)]">
              {t("importDialog.subtitle")}
            </p>
          </div>
          <button
            type="button"
            onClick={props.onClose}
            aria-label={t("importDialog.cancel")}
            class="tactile-row -mr-1 -mt-1 grid h-7 w-7 shrink-0 place-items-center rounded-md text-[var(--color-fg-2)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
          >
            <X size={14} />
          </button>
        </header>

        <div class="border-b border-[var(--color-line)] px-5 pt-3">
          <div class="relative flex gap-5">
            <Tab
              active={tab() === "url"}
              onClick={() => setTab("url")}
              icon={<Link2 size={12} />}
              label={t("importDialog.tabUrl")}
            />
            <Tab
              active={tab() === "text"}
              onClick={() => setTab("text")}
              icon={<FileText size={12} />}
              label={t("importDialog.tabText")}
            />
            <span
              class="absolute bottom-0 h-px bg-[var(--color-fg-0)] transition-[transform,width] duration-300"
              style={{
                width: "32px",
                transform: `translateX(${tab() === "url" ? 0 : 60}px)`,
              }}
            />
          </div>
        </div>

        <div class="px-5 py-4">
          <Show when={tab() === "url"}>
            <FieldLabel>{t("importDialog.urlLabel")}</FieldLabel>
            <div class="input-wrap">
              <Link2
                size={13}
                class="input-icon text-[var(--color-fg-3)]"
                aria-hidden="true"
              />
              <input
                type="url"
                autofocus
                spellcheck={false}
                placeholder={t("importDialog.urlPlaceholder")}
                value={url()}
                onInput={(e) => setUrl(e.currentTarget.value)}
                class="input-with-icon"
              />
            </div>
            <Hint>{t("importDialog.urlHint")}</Hint>
          </Show>

          <Show when={tab() === "text"}>
            <FieldLabel>{t("importDialog.textLabel")}</FieldLabel>
            <textarea
              autofocus
              spellcheck={false}
              placeholder={t("importDialog.textPlaceholder")}
              value={text()}
              onInput={(e) => setText(e.currentTarget.value)}
              rows={6}
              class="hairline w-full resize-y rounded-md bg-[var(--color-bg-0)] px-3 py-2 font-mono text-[12px] leading-relaxed outline-none transition-colors duration-150 placeholder:text-[var(--color-fg-3)] focus:border-[color-mix(in_srgb,var(--color-accent)_55%,transparent)]"
            />
            <Hint>{t("importDialog.textHint", { hash: "#" })}</Hint>
          </Show>
        </div>

        <footer class="flex items-center justify-between border-t border-[var(--color-line)] bg-[var(--color-bg-0)] px-5 py-3">
          <button
            type="button"
            onClick={pasteFromClipboard}
            class="tactile-row -ml-1.5 flex items-center gap-1.5 rounded-md px-2 py-1 text-[11.5px] text-[var(--color-fg-2)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
          >
            <ClipboardPaste size={12} />
            {t("importDialog.pasteFromClipboard")}
          </button>

          <div class="flex items-center gap-2">
            <span class="kbd hidden sm:inline-flex">{t("importDialog.submitHotkey")}</span>
            <button
              type="button"
              onClick={props.onClose}
              class="tactile hairline h-7 rounded-md px-3 text-[12px] text-[var(--color-fg-1)] hover:bg-[var(--color-tint-2)] hover:text-[var(--color-fg-0)]"
            >
              {t("importDialog.cancel")}
            </button>
            <button
              type="button"
              onClick={submit}
              class="tactile h-7 rounded-md bg-[var(--color-accent)] px-3.5 text-[12px] font-medium text-white shadow-[0_1px_0_0_rgba(255,255,255,0.12)_inset,0_1px_2px_rgba(0,0,0,0.18)] hover:brightness-110"
            >
              {t("importDialog.import")}
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
};

const Tab: Component<{
  active: boolean;
  onClick: () => void;
  icon: any;
  label: string;
}> = (p) => (
  <button
    type="button"
    onClick={p.onClick}
    class="tactile-row flex items-center gap-1.5 pb-2.5 text-[12px]"
    classList={{
      "text-[var(--color-fg-0)]": p.active,
      "text-[var(--color-fg-2)] hover:text-[var(--color-fg-1)]": !p.active,
    }}
  >
    <span
      class="text-[var(--color-fg-3)] transition-colors duration-150"
      classList={{ "text-[var(--color-fg-1)]": p.active }}
    >
      {p.icon}
    </span>
    {p.label}
  </button>
);

const FieldLabel: Component<{ children: any }> = (p) => (
  <div class="mb-1.5 text-[11.5px] font-medium text-[var(--color-fg-1)]">
    {p.children}
  </div>
);

const Hint: Component<{ children: any }> = (p) => (
  <p class="mt-2 text-[11px] leading-relaxed text-[var(--color-fg-2)]">
    {p.children}
  </p>
);
