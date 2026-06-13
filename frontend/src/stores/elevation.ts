/* Elevation store — exposes whether v2pn is currently running with admin
 * rights. Used to decide whether the TUN switch should prompt for a
 * UAC restart or just go ahead.
 */
import { createSignal, onMount } from "solid-js";
import { api, type ElevationStatus } from "~/lib/api";

const [status, setStatus] = createSignal<ElevationStatus>({
  elevated: true,
  supported: true,
});

let initialised = false;

export { status as elevationStatus };

export function initElevation() {
  if (initialised) return;
  initialised = true;
  onMount(async () => {
    try {
      const s = await api.elevationStatus();
      setStatus(s);
    } catch (e) {
      console.warn("elevation_status failed", e);
    }
  });
}

export function isElevated(): boolean {
  return status().elevated;
}

export async function restartAsAdmin(): Promise<void> {
  await api.restartAsAdmin();
}
