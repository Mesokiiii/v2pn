import { Component, Show } from "solid-js";

interface Props {
  /** ISO 3166-1 alpha-2 (e.g. "US", "RU"). Case-insensitive. */
  code?: string | null;
  /** Pixel size of the flag's longer side (width). Default 18. */
  size?: number;
  /** Render as a circle ("fis"). Default true. */
  circle?: boolean;
  class?: string;
}

/** Country flag rendered via the `flag-icons` CSS package — sharp on every
 * platform, no reliance on Windows emoji fonts (which don't ship flag glyphs). */
export const Flag: Component<Props> = (props) => {
  const cc = () => props.code?.toLowerCase().trim();
  const px = () => props.size ?? 18;
  const circle = () => props.circle !== false;

  return (
    <Show
      when={cc() && /^[a-z]{2}$/.test(cc()!)}
      fallback={<FallbackDot size={px()} />}
    >
      <span
        class={[
          "fi",
          `fi-${cc()}`,
          circle() ? "fis" : "",
          "shrink-0 rounded-[3px]",
          props.class ?? "",
        ].join(" ").trim()}
        style={{
          width: circle() ? `${px()}px` : `${px()}px`,
          height: circle() ? `${px()}px` : `${Math.round((px() * 2) / 3)}px`,
          "background-size": "cover",
          "background-position": "center",
        }}
        aria-label={cc()?.toUpperCase()}
        role="img"
      />
    </Show>
  );
};

/** Subtle dot when we couldn't resolve a country. Better than "??". */
const FallbackDot: Component<{ size: number }> = (p) => (
  <span
    class="grid shrink-0 place-items-center rounded-full border border-[var(--color-line-strong)] bg-[var(--color-bg-2)]"
    style={{ width: `${p.size}px`, height: `${p.size}px` }}
    aria-hidden="true"
  >
    <svg width={Math.round(p.size * 0.55)} height={Math.round(p.size * 0.55)} viewBox="0 0 16 16" fill="none">
      <circle cx="8" cy="8" r="6" stroke="currentColor" stroke-opacity="0.4" stroke-width="1.2" />
      <path d="M2 8h12" stroke="currentColor" stroke-opacity="0.4" stroke-width="1.2" />
      <ellipse cx="8" cy="8" rx="3" ry="6" stroke="currentColor" stroke-opacity="0.4" stroke-width="1.2" />
    </svg>
  </span>
);
