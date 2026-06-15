//! Bullet-proof removal of a stale Wintun adapter.
//!
//! Background — three failure modes we have to defend against
//! ============================================================
//!
//! 1. **Hard sing-box exit** (BSOD, taskkill, OOM): the kernel holds the
//!    Wintun adapter for several hundred ms after the owning process
//!    dies. A reconnect within that window trips
//!    `Cannot create a file when that file already exists.`.
//!
//! 2. **Suspend / resume**: the OS suspends every process *before* our
//!    300–800 ms "TUN grace" sleep can run, so the adapter is left in a
//!    half-released state. On resume `WintunCreateAdapter` returns
//!    `Cannot create a file when that file already exists.` AND the
//!    fallback `WintunOpenAdapter` returns `Element not found.` —
//!    because the SwDevice host hasn't finished tearing the device node
//!    down yet. This was the dominant error in production logs.
//!
//! 3. **Network-stack instability post-resume**: even after the adapter
//!    object is gone, the IPHelper and DNS resolvers can still hold
//!    references to its routing entries for another second or two.
//!
//! Strategy
//! ========
//!
//! We run two complementary cleanup paths in a retry loop, with
//! exponential backoff up to a configurable budget:
//!
//!   - **`wintun.dll` session API** (`WintunOpenAdapter` →
//!     `WintunCloseAdapter`). This is the only mechanism that can
//!     touch the SwDevice handle from a *different* process than the
//!     one that originally created the adapter — we dynamic-load the
//!     same `wintun.dll` we ship next to our exe, so versions match.
//!
//!   - **`netsh interface delete interface`** wipes the routing /
//!     IP-config layer that sits above the kernel adapter. This is the
//!     historical fallback — it doesn't fix a wedged Wintun session by
//!     itself, but it does clear ARP / IPv4/IPv6 address bindings that
//!     would otherwise survive a fresh adapter creation.
//!
//! Both are best-effort. The function returns a [`CleanupOutcome`]
//! reporting what worked, but no callsite treats failure as fatal —
//! the eventual `WintunCreateAdapter` in sing-box is the authoritative
//! "did it work" signal.

#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::time::Duration;

/// What happened during a thorough cleanup pass. Useful for deciding
/// whether to delay the next supervisor start (e.g. on the resume
/// path the connect path waits a bit longer when we know nothing
/// matched our cleanup attempts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupOutcome {
    /// No adapter was ever observed under the configured name during
    /// any attempt — nothing to clean up. The most common case on a
    /// healthy machine.
    NotPresent,
    /// At least one attempt successfully closed/removed the adapter
    /// via the Wintun DLL or netsh.
    Removed,
    /// Adapter was present at first observation, but every cleanup
    /// attempt failed. Caller should still proceed (sing-box may
    /// succeed on its own retry path) but expect the first start to
    /// likely fail.
    StillPresent,
    /// Cleanup is not applicable on this platform (non-Windows builds).
    Unsupported,
}

/// Tuning knobs for [`cleanup_thorough_async`]. Defaults are picked
/// for the typical post-suspend case where the OS needs ~3–5 seconds
/// of real time after our resume callback fires before the SwDevice
/// host fully releases the adapter.
#[derive(Debug, Clone, Copy)]
pub struct CleanupBudget {
    /// Maximum number of cleanup attempts before giving up. Each
    /// attempt = one wintun-DLL pass + one netsh pass + a wait.
    pub max_attempts: u32,
    /// Initial wait between attempts. Doubled on each retry, capped
    /// at `max_wait_per_step`.
    pub initial_wait: Duration,
    /// Cap on per-step wait so a long retry budget doesn't grow into
    /// minute-long sleeps.
    pub max_wait_per_step: Duration,
}

impl CleanupBudget {
    /// Fast budget for the regular connect path — adapter is rarely
    /// stale here, and the user is actively waiting on the
    /// "Connecting…" spinner.
    pub const FAST: Self = Self {
        max_attempts: 3,
        initial_wait: Duration::from_millis(250),
        max_wait_per_step: Duration::from_millis(1_500),
    };

    /// Aggressive budget for the resume-from-suspend path — the
    /// adapter is almost guaranteed to be wedged here, and the user
    /// is waiting on a *blank* screen so an extra second of cleanup
    /// is invisible compared to the "VPN is broken until I click"
    /// alternative.
    pub const RESUME: Self = Self {
        max_attempts: 8,
        initial_wait: Duration::from_millis(500),
        max_wait_per_step: Duration::from_millis(3_500),
    };

    /// Same shape as [`Self::RESUME`] but tuned for the auto-restart
    /// loop inside the supervisor: fewer attempts, since the loop
    /// itself already has its own backoff schedule.
    pub const AUTO_RESTART: Self = Self {
        max_attempts: 4,
        initial_wait: Duration::from_millis(400),
        max_wait_per_step: Duration::from_millis(2_500),
    };
}

impl Default for CleanupBudget {
    fn default() -> Self {
        Self::FAST
    }
}

/* ----- public API -------------------------------------------------------- */

/// Synchronous one-shot cleanup, kept for backward compatibility with
/// older call-sites and the network-repair UI flow. Equivalent to a
/// single attempt of [`cleanup_thorough_async`] with no retry loop.
///
/// Prefer the async variant in any new code path — the synchronous one
/// can block on `netsh` for ~200–800 ms which we don't want on the
/// Tokio worker pool.
pub fn cleanup_stale_adapter(adapter_name: &str) {
    if !validate_adapter_name(adapter_name) {
        return;
    }
    #[cfg(windows)]
    {
        let _ = wintun_dll::try_close_adapter(adapter_name);
        let _ = netsh_delete(adapter_name);
    }
    #[cfg(not(windows))]
    {
        let _ = adapter_name;
    }
}

/// Aggressive, retry-driven cleanup. Spawns a `spawn_blocking` task so
/// the wintun.dll calls and netsh shell-out don't pin a Tokio worker.
///
/// Workflow per attempt:
///   1. Probe with `WintunOpenAdapter` — if the adapter doesn't exist,
///      we're done immediately.
///   2. If it exists, close it via `WintunCloseAdapter` (this is the
///      only path that can release the kernel SwDevice handle from a
///      foreign process).
///   3. Run `netsh interface delete interface <name>` to wipe routing
///      / IP-config artefacts.
///   4. Sleep for the current backoff bucket; double the bucket up to
///      `max_wait_per_step`.
///   5. Re-probe; if gone, return `Removed`. Otherwise loop until the
///      attempt budget is exhausted.
pub async fn cleanup_thorough_async(
    adapter_name: &str,
    budget: CleanupBudget,
) -> CleanupOutcome {
    if !validate_adapter_name(adapter_name) {
        return CleanupOutcome::NotPresent;
    }

    #[cfg(not(windows))]
    {
        let _ = (adapter_name, budget);
        return CleanupOutcome::Unsupported;
    }

    #[cfg(windows)]
    {
        let name = adapter_name.to_string();
        let outcome = tokio::task::spawn_blocking(move || cleanup_blocking(&name, budget))
            .await
            .unwrap_or(CleanupOutcome::StillPresent);
        outcome
    }
}

/// Heuristic for "does this stderr tail look like the post-resume
/// wintun-half-state failure?". Used by the supervisor to decide
/// whether the next auto-restart attempt should pre-clean the
/// adapter. Matches three families of message we've seen in production:
///
///   * `start inbound/tun[...]: configure tun interface: ...`
///   * `create adapter: Cannot create a file when that file already exists.`
///   * `open existing adapter: Element not found.`
///
/// Case-insensitive, substring match — sing-box log formatting has
/// shifted across versions and we don't want to whitelist by exact
/// string.
pub fn looks_like_wintun_failure(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("configure tun interface")
        || lower.contains("create adapter")
        || lower.contains("open existing adapter")
        || lower.contains("wintun"))
        && (lower.contains("file already exists")
            || lower.contains("element not found")
            || lower.contains("not found")
            || lower.contains("access is denied")
            || lower.contains("cannot create"))
}

/* ----- impl detail (Windows) -------------------------------------------- */

#[cfg(windows)]
fn cleanup_blocking(adapter_name: &str, budget: CleanupBudget) -> CleanupOutcome {
    // Probe before any destructive call so we can shortcut the common
    // "already gone" path with the cheapest possible work.
    if !wintun_dll::adapter_exists(adapter_name) {
        return CleanupOutcome::NotPresent;
    }

    let mut wait = budget.initial_wait;
    let ever_present = true; // we just observed it
    for attempt in 0..budget.max_attempts {
        let closed = wintun_dll::try_close_adapter(adapter_name);
        let netsh_ok = netsh_delete(adapter_name);
        tracing::debug!(
            target: "wintun_cleanup",
            attempt = attempt + 1,
            wintun_closed = closed,
            netsh_ok,
            "cleanup attempt completed"
        );

        std::thread::sleep(wait);

        if !wintun_dll::adapter_exists(adapter_name) {
            tracing::info!(
                target: "wintun_cleanup",
                attempts = attempt + 1,
                "adapter '{adapter_name}' released"
            );
            return CleanupOutcome::Removed;
        }

        wait = (wait * 2).min(budget.max_wait_per_step);
    }

    if ever_present {
        tracing::warn!(
            target: "wintun_cleanup",
            "adapter '{adapter_name}' still present after {} attempts",
            budget.max_attempts
        );
        CleanupOutcome::StillPresent
    } else {
        CleanupOutcome::NotPresent
    }
}

/// Run `netsh interface delete interface <name>` plus the IPv4/IPv6
/// address strip with output suppressed. Returns `true` if the
/// interface-delete step exited 0 (i.e. an interface really was
/// removed); `false` otherwise (which usually just means there was
/// nothing to delete).
#[cfg(windows)]
fn netsh_delete(adapter_name: &str) -> bool {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    for family in &["ipv4", "ipv6"] {
        let _ = std::process::Command::new("netsh")
            .args([
                "interface",
                family,
                "delete",
                "address",
                adapter_name,
                "all",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
    }

    matches!(
        std::process::Command::new("netsh")
            .args(["interface", "delete", "interface", adapter_name])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output(),
        Ok(out) if out.status.success()
    )
}

/// Refuse adapter names that aren't a tight subset of what our
/// supervisor would ever generate. This guards both `netsh` shell-out
/// (no argument injection) and the wintun.dll path (the API itself
/// limits to MAX_ADAPTER_NAME, but we gate earlier).
fn validate_adapter_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 127 {
        return false;
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        tracing::warn!(
            target: "wintun_cleanup",
            "refusing to operate on non-trivial adapter name {name:?}"
        );
    }
    ok
}

/* ----- wintun.dll dynamic loader (Windows only) ------------------------- */

#[cfg(windows)]
mod wintun_dll {
    //! Dynamic loader for the bundled `wintun.dll`.
    //!
    //! We deliberately don't link the DLL — a stale wintun.dll at
    //! `LoadLibrary` time would block the entire app from starting.
    //! Instead we resolve at first use, fall back gracefully if the
    //! DLL isn't where we expect, and never fail the connect path on
    //! a load error.

    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    use ::windows::core::{s, PCSTR, PCWSTR};
    use ::windows::Win32::Foundation::{FreeLibrary, HMODULE};
    use ::windows::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_FLAGS, LOAD_WITH_ALTERED_SEARCH_PATH,
    };

    /// Opaque pointer to a Wintun adapter struct. We never dereference
    /// it from Rust — only round-trip it back into the DLL.
    type AdapterHandle = *mut c_void;

    type WintunOpenAdapterFn = unsafe extern "system" fn(name: PCWSTR) -> AdapterHandle;
    type WintunCloseAdapterFn = unsafe extern "system" fn(adapter: AdapterHandle);

    struct Api {
        // Kept alive for the lifetime of the process. We never call
        // FreeLibrary on it because subsequent cleanup passes need
        // the API to still be there. The leak is bounded — one HMODULE.
        _hmod: HMODULE,
        open: WintunOpenAdapterFn,
        close: WintunCloseAdapterFn,
    }

    // SAFETY: HMODULE is just a numeric handle; the function pointers
    // come from a DLL we ourselves shipped, and the wintun ABI is
    // documented as thread-safe for these two calls.
    unsafe impl Send for Api {}
    unsafe impl Sync for Api {}

    static API: OnceLock<Option<Api>> = OnceLock::new();

    fn api() -> Option<&'static Api> {
        API.get_or_init(load).as_ref()
    }

    fn load() -> Option<Api> {
        for path in candidate_paths() {
            if !path.exists() {
                continue;
            }
            if let Some(api) = load_from(&path) {
                tracing::debug!(
                    target: "wintun_cleanup",
                    "loaded wintun.dll from {}",
                    path.display()
                );
                return Some(api);
            }
        }
        // Last-ditch: rely on the OS search path. This works in unit
        // tests on a dev machine that has wintun.dll installed
        // system-wide; in production our shipped DLL is found via
        // the candidates above.
        load_from(Path::new("wintun.dll"))
    }

    fn candidate_paths() -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                out.push(dir.join("wintun.dll"));
                // Tauri's externalBin layout drops resources alongside
                // the binary, sometimes one level up.
                if let Some(parent) = dir.parent() {
                    out.push(parent.join("wintun.dll"));
                }
                // Dev fallback — walk up to the workspace root.
                if let Some(root) = exe.ancestors().nth(3) {
                    out.push(
                        root.join("crates")
                            .join("tauri-app")
                            .join("binaries")
                            .join("wintun.dll"),
                    );
                }
            }
        }
        out
    }

    fn load_from(path: &Path) -> Option<Api> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: we pass a valid null-terminated UTF-16 path; we own
        // the buffer for the duration of the call. LoadLibraryExW
        // reads it synchronously and copies internally.
        let hmod = unsafe {
            LoadLibraryExW(
                PCWSTR(wide.as_ptr()),
                None,
                LOAD_LIBRARY_FLAGS(LOAD_WITH_ALTERED_SEARCH_PATH.0),
            )
        }
        .ok()?;

        // SAFETY: GetProcAddress returns a function pointer whose
        // ABI matches the Wintun documented C signature; we transmute
        // through `*const c_void` which is the standard pattern for
        // dynamic FFI loading.
        let (open, close) = unsafe {
            let open = GetProcAddress(hmod, s!("WintunOpenAdapter"));
            let close = GetProcAddress(hmod, s!("WintunCloseAdapter"));
            match (open, close) {
                (Some(o), Some(c)) => (
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, WintunOpenAdapterFn>(o),
                    std::mem::transmute::<unsafe extern "system" fn() -> isize, WintunCloseAdapterFn>(c),
                ),
                _ => {
                    let _ = FreeLibrary(hmod);
                    return None;
                }
            }
        };

        Some(Api { _hmod: hmod, open, close })
    }

    fn to_wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// Cheap probe: try `WintunOpenAdapter`. If it returns non-null,
    /// the adapter exists; we close the handle immediately so we
    /// don't leak our own reference. Returns `false` if the wintun
    /// API isn't loadable (we treat that as "not present" because
    /// without the DLL we can't do anything anyway, and the netsh
    /// path will pick up the slack).
    pub fn adapter_exists(name: &str) -> bool {
        let Some(api) = api() else {
            return false;
        };
        let wide = to_wide(name);
        // SAFETY: `wide` is null-terminated and lives for the call;
        // the returned handle is closed before we return.
        unsafe {
            let h = (api.open)(PCWSTR(wide.as_ptr()));
            if h.is_null() {
                false
            } else {
                (api.close)(h);
                true
            }
        }
    }

    /// Forcibly release any open Wintun adapter under this name. The
    /// `WintunCloseAdapter` call decrements the refcount on the
    /// SwDevice node; if that brings it to zero the OS removes the
    /// device. Returns `true` when we actually closed a handle.
    pub fn try_close_adapter(name: &str) -> bool {
        let Some(api) = api() else {
            return false;
        };
        let wide = to_wide(name);
        // SAFETY: same as `adapter_exists`. We're allowed to call
        // WintunCloseAdapter only on a handle we got from Open or
        // Create — so we open first, then close exactly that handle.
        unsafe {
            let h = (api.open)(PCWSTR(wide.as_ptr()));
            if h.is_null() {
                return false;
            }
            (api.close)(h);
            true
        }
    }

    /// Suppress unused-import warning for `PCSTR` on builds that
    /// happen to elide the `s!` macro expansion.
    #[allow(dead_code)]
    const _PCSTR_USE: Option<PCSTR> = None;
}

/* ----- tests ------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_validation_rejects_garbage() {
        assert!(!validate_adapter_name(""));
        assert!(!validate_adapter_name("hello world"));
        assert!(!validate_adapter_name("name; del C:"));
        assert!(!validate_adapter_name(&"x".repeat(200)));
        assert!(validate_adapter_name("v2pn-tun"));
        assert!(validate_adapter_name("v2pn_tun"));
        assert!(validate_adapter_name("Wintun01"));
    }

    #[test]
    fn looks_like_wintun_failure_matches_logged_messages() {
        let real = "FATAL[0015] start service: start inbound/tun[tun-in]: \
            configure tun interface: (create adapter: Cannot create a file \
            when that file already exists. | open existing adapter: \
            Element not found.)";
        assert!(looks_like_wintun_failure(real));

        let unrelated = "ERROR connection: dial tcp 1.2.3.4:443: i/o timeout";
        assert!(!looks_like_wintun_failure(unrelated));

        let access = "create adapter: Access is denied.";
        assert!(looks_like_wintun_failure(access));
    }

    #[tokio::test]
    async fn nonexistent_adapter_returns_not_present_or_unsupported() {
        // On Windows we expect NotPresent (no adapter named
        // "definitely-not-real" exists); on POSIX the build returns
        // Unsupported. Both are "no work to do" outcomes from the
        // caller's POV.
        let outcome =
            cleanup_thorough_async("definitely-not-real", CleanupBudget::FAST).await;
        assert!(matches!(
            outcome,
            CleanupOutcome::NotPresent | CleanupOutcome::Unsupported
        ));
    }
}
