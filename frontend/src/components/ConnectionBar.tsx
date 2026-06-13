import { Component, Show } from "solid-js";
import type { ConnectionState, ProxyProfile } from "~/lib/api";

interface Props {
  connection: ConnectionState;
  selected: ProxyProfile | null;
  error: string | null;
}

export const ConnectionBar: Component<Props> = (props) => {
  const dot = () => {
    switch (props.connection.state) {
      case "connected": return "connected";
      case "starting":
      case "stopping": return "connecting";
      case "failed":   return "error";
      default:         return "idle";
    }
  };

  const label = () => {
    switch (props.connection.state) {
      case "connected": return "Connected";
      case "starting":  return "Connecting…";
      case "stopping":  return "Stopping…";
      case "failed":    return "Failed";
      default:          return "Disconnected";
    }
  };

  return (
    <footer class="hairline-t flex h-7 shrink-0 items-center gap-4 bg-[var(--color-bg-0)] px-4 text-[11px] text-[var(--color-fg-2)]">
      <div class="flex items-center gap-1.5">
        <span class="dot" data-state={dot()} />
        <span>{label()}</span>
      </div>

      <Show when={props.selected && props.connection.state === "connected" ? props.selected : null}>
        {(p) => (
          <div class="flex items-center gap-1.5">
            <span class="text-[var(--color-fg-2)]">via</span>
            <span class="font-mono text-[var(--color-fg-1)]">
              {p().protocol}/{p().server}:{p().port}
            </span>
          </div>
        )}
      </Show>

      <Show when={props.connection.state === "failed"}>
        <div class="flex items-center gap-1.5 text-[var(--color-bad)]">
          <span class="max-w-[420px] truncate">
            {props.connection.state === "failed" ? props.connection.reason : ""}
          </span>
        </div>
      </Show>

      <div class="flex-1" />

      <Show when={props.error}>
        {(e) => (
          <div class="flex items-center gap-1.5 text-[var(--color-bad)]">
            <span class="dot" data-state="error" />
            <span class="max-w-[420px] truncate">{e()}</span>
          </div>
        )}
      </Show>

      <span class="font-mono text-[10.5px] tabular-nums text-[var(--color-fg-3)]">v0.1.0</span>
    </footer>
  );
};
