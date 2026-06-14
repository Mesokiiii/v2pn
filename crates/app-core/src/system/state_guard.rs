//! Bullet-proof connection guard.
//!
//! ```text
//!   acquire() ──▶ snapshot proxy ──▶ apply proxy ──▶ write state file
//!                                                            │
//!                                                            ▼
//!                          (everything we need to clean up on a crash)
//!
//!   release() ──▶ restore proxy ──▶ delete state file
//!         OR
//!   Drop::drop ──▶ same path, runs on panic / scope exit
//! ```
//!
//! Why RAII matters here: even if a Tauri command path panics, even if the
//! supervisor task is aborted, **as long as the `Guard` is dropped**, the
//! Drop impl will restore the system proxy. The file mirror gives us a
//! second line of defense for the cases where the Drop didn't run (process
//! killed, BSOD, power loss): the next launch finds the orphan state and
//! restores it.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::sys_proxy::{ActiveSystemProxy, ProxySnapshot, SystemProxy};

/// Persisted on disk; mirrors the Guard so we can recover after a hard exit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedState {
    /// Schema version — bump on layout change so old recoveries don't crash.
    pub schema: u32,
    /// PID of the v2pn process that owned this proxy state.
    pub pid: u32,
    /// PID of the sing-box sidecar we spawned. Captured separately from
    /// `pid` so recovery can force-kill it on the next launch even if our
    /// own process died first. `None` for legacy state files written by
    /// older v2pn versions (we treat that as "no orphan to clean up").
    #[serde(default)]
    pub child_pid: Option<u32>,
    /// Unix timestamp (seconds) — useful for diagnostics, not for logic.
    pub started_at: i64,
    /// Whether we actually changed the OS proxy. (For pure TUN mode we don't,
    /// but we still write a state file so close/recover paths stay uniform.)
    pub touched_proxy: bool,
    /// Proxy address we set ("127.0.0.1:7890") — for human-readable diagnostics.
    pub applied_proxy: Option<String>,
    /// Snapshot taken *before* we touched anything — what `restore()` reverts to.
    pub saved: ProxySnapshot,
}

impl PersistedState {
    pub fn current_pid() -> u32 {
        std::process::id()
    }
}

const SCHEMA_VERSION: u32 = 1;
const STATE_FILE_NAME: &str = "active.state.json";

/// RAII guard. Owns the system proxy mutation for the duration of a
/// connection.
pub struct ConnectionGuard {
    state_dir: PathBuf,
    /// `None` after a successful `release()` — Drop becomes a no-op.
    inner: Option<GuardInner>,
}

struct GuardInner {
    sys: ActiveSystemProxy,
    persisted: PersistedState,
}

impl ConnectionGuard {
    /// Create a guard for *proxy mode*: snapshot, apply, persist.
    pub fn acquire_proxy(state_dir: &Path, addr: &str, bypass: &[&str]) -> CoreResult<Self> {
        std::fs::create_dir_all(state_dir)?;
        // If a previous run left an active state file — that's a programming
        // error here (the recovery layer should have cleaned it up first).
        let path = state_dir.join(STATE_FILE_NAME);
        if path.exists() {
            return Err(CoreError::Other(format!(
                "stale state file present at {} — run recovery first",
                path.display()
            )));
        }

        let sys = ActiveSystemProxy::new();
        let saved = sys.snapshot()?;

        // Apply BEFORE writing the state file: if apply fails, we never
        // create the file and Drop has nothing to do.
        sys.apply(addr, bypass)?;

        let persisted = PersistedState {
            schema: SCHEMA_VERSION,
            pid: PersistedState::current_pid(),
            child_pid: None,
            started_at: now_unix(),
            touched_proxy: true,
            applied_proxy: Some(addr.to_string()),
            saved,
        };

        write_state_file(state_dir, &persisted)?;

        Ok(Self {
            state_dir: state_dir.to_path_buf(),
            inner: Some(GuardInner { sys, persisted }),
        })
    }

    /// Create a guard for *TUN mode*: no proxy mutation, but we still write
    /// the state file so the lifecycle is uniform.
    pub fn acquire_tun(state_dir: &Path) -> CoreResult<Self> {
        std::fs::create_dir_all(state_dir)?;
        let path = state_dir.join(STATE_FILE_NAME);
        if path.exists() {
            return Err(CoreError::Other(format!(
                "stale state file present at {} — run recovery first",
                path.display()
            )));
        }

        let sys = ActiveSystemProxy::new();
        let saved = sys.snapshot()?;

        let persisted = PersistedState {
            schema: SCHEMA_VERSION,
            pid: PersistedState::current_pid(),
            child_pid: None,
            started_at: now_unix(),
            touched_proxy: false,
            applied_proxy: None,
            saved,
        };
        write_state_file(state_dir, &persisted)?;

        Ok(Self {
            state_dir: state_dir.to_path_buf(),
            inner: Some(GuardInner { sys, persisted }),
        })
    }

    /// Happy path: explicit release. Restores the proxy and deletes the
    /// state file. Idempotent — calling Drop after `release()` is a no-op.
    pub fn release(mut self) -> CoreResult<()> {
        if let Some(inner) = self.inner.take() {
            if inner.persisted.touched_proxy {
                inner.sys.restore(&inner.persisted.saved)?;
            }
            delete_state_file(&self.state_dir)?;
        }
        Ok(())
    }

    /// Read-only accessor for diagnostics.
    pub fn snapshot(&self) -> Option<&ProxySnapshot> {
        self.inner.as_ref().map(|i| &i.persisted.saved)
    }

    /// Record the spawned sing-box PID into the on-disk state file. Called
    /// by the supervisor wiring right after a successful `start`. Errors
    /// here are non-fatal — we still have the in-memory guard, and the
    /// orphan scanner on the next launch can find sing-box by walking the
    /// process table even without this hint.
    pub fn update_child_pid(&mut self, pid: u32) -> CoreResult<()> {
        let Some(inner) = self.inner.as_mut() else {
            return Err(CoreError::Other(
                "ConnectionGuard already released".into(),
            ));
        };
        inner.persisted.child_pid = Some(pid);
        write_state_file(&self.state_dir, &inner.persisted)
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else { return };
        // We're already on a panic path or normal scope exit. Errors here go
        // to the log; we cannot reasonably surface them.
        if inner.persisted.touched_proxy {
            if let Err(e) = inner.sys.restore(&inner.persisted.saved) {
                tracing::error!(target: "state_guard", "Drop::restore failed: {e}");
            } else {
                tracing::info!(target: "state_guard", "Drop: proxy restored");
            }
        }
        if let Err(e) = delete_state_file(&self.state_dir) {
            tracing::error!(target: "state_guard", "Drop::delete_state failed: {e}");
        }
    }
}

/* ---------- bootstrap recovery ----------------------------------------- */

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryOutcome {
    /// No state file present — nothing to do.
    NothingToDo,
    /// State file was found and the previous owner is still alive (likely a
    /// double-launch race where single-instance was bypassed). We do *not*
    /// touch anything in that case.
    OwnedByLiveProcess { pid: u32 },
    /// Orphan state — previous v2pn died while connected. We restored the
    /// saved proxy and deleted the file.
    Recovered { saved: ProxySnapshot, applied: Option<String> },
}

/// Inspect the state directory at startup and tidy up after a crashed previous run.
pub fn recover_orphaned_state(state_dir: &Path) -> CoreResult<RecoveryOutcome> {
    let path = state_dir.join(STATE_FILE_NAME);
    if !path.exists() {
        return Ok(RecoveryOutcome::NothingToDo);
    }

    let bytes = fs::read(&path)?;
    let persisted: PersistedState = serde_json::from_slice(&bytes).map_err(|e| {
        CoreError::Other(format!("corrupt state file {}: {e}", path.display()))
    })?;

    if persisted.schema != SCHEMA_VERSION {
        // Unknown schema — treat as orphan to be safe.
        tracing::warn!(target: "recovery", "schema mismatch (got {}, want {}); cleaning up",
            persisted.schema, SCHEMA_VERSION);
    } else if pid_is_alive(persisted.pid) && persisted.pid != PersistedState::current_pid() {
        return Ok(RecoveryOutcome::OwnedByLiveProcess { pid: persisted.pid });
    }

    // Force-kill any orphan sing-box child the previous run left behind.
    // The Job Object guarantee (kill-on-close) handles every normal crash
    // path *if it was active*. This branch covers the edge cases:
    //   * pre-Job-Object versions (older v2pn installs)
    //   * Job Object creation failed at supervisor::new
    //   * Some Windows configurations that disallow job assignment for
    //     elevated children of unelevated parents (or vice versa)
    // Using the exact PID we recorded keeps us from killing some other
    // user's sing-box that happens to be running on the box.
    if let Some(child_pid) = persisted.child_pid {
        if child_pid != 0 && pid_is_alive(child_pid) {
            tracing::warn!(
                target: "recovery",
                "orphan sing-box detected (PID {child_pid}) — terminating"
            );
            if let Err(e) = crate::process_guard::taskkill_force(child_pid) {
                tracing::error!(
                    target: "recovery",
                    "taskkill_force({child_pid}) failed: {e}; proceeding anyway"
                );
            } else {
                tracing::info!(target: "recovery", "PID {child_pid} terminated");
            }
        }
    }

    // Restore proxy regardless — it's idempotent and safer than leaving stale.
    if persisted.touched_proxy {
        let sys = ActiveSystemProxy::new();
        if let Err(e) = sys.restore(&persisted.saved) {
            tracing::error!(target: "recovery", "restore failed: {e}");
        }
    }

    delete_state_file(state_dir)?;

    tracing::info!(
        target: "recovery",
        "orphan state from PID {} cleaned up (was using {:?})",
        persisted.pid, persisted.applied_proxy
    );

    Ok(RecoveryOutcome::Recovered {
        saved: persisted.saved,
        applied: persisted.applied_proxy,
    })
}

/* ---------- internals --------------------------------------------------- */

fn write_state_file(dir: &Path, state: &PersistedState) -> CoreResult<()> {
    let path = dir.join(STATE_FILE_NAME);
    let tmp = dir.join(format!("{STATE_FILE_NAME}.tmp"));

    let mut f = fs::File::create(&tmp)?;
    let bytes = serde_json::to_vec_pretty(state)?;
    f.write_all(&bytes)?;
    // Best-effort fsync — on some FS this matters for crash safety.
    let _ = f.sync_all();
    drop(f);

    // Atomic rename (POSIX rename, NTFS MoveFileEx replace).
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn delete_state_file(dir: &Path) -> CoreResult<()> {
    let path = dir.join(STATE_FILE_NAME);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(windows)]
fn pid_is_alive(pid: u32) -> bool {
    use ::windows::Win32::Foundation::CloseHandle;
    use ::windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return false;
    }
    // SAFETY: the four Win32 calls below all take/return raw pointers we
    // never dereference outside the call boundary. Each handle is closed
    // before the function returns; on `OpenProcess` failure we bail out
    // without further FFI. We pass `&mut code` (a stack u32) which `windows`
    // crate types as pointer-to-DWORD — the kernel writes 4 bytes there on
    // success, well within the variable's lifetime.
    unsafe {
        let h = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return false,
        };
        let mut code: u32 = 0;
        let alive = GetExitCodeProcess(h, &mut code).is_ok() && code == 259; // STILL_ACTIVE
        let _ = CloseHandle(h);
        alive
    }
}

#[cfg(not(windows))]
fn pid_is_alive(_pid: u32) -> bool {
    // We don't ship to non-Windows yet; treat as dead so recovery is safe.
    false
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/* ---------- tests ------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn no_state_file_recovery_is_noop() {
        let d = tmpdir();
        let r = recover_orphaned_state(d.path()).unwrap();
        assert_eq!(r, RecoveryOutcome::NothingToDo);
    }

    #[test]
    fn release_deletes_state_file() {
        let d = tmpdir();
        // Use TUN flavour to avoid touching real registry on test boxes.
        let g = ConnectionGuard::acquire_tun(d.path()).unwrap();
        assert!(d.path().join(STATE_FILE_NAME).exists());
        g.release().unwrap();
        assert!(!d.path().join(STATE_FILE_NAME).exists());
    }

    #[test]
    fn drop_deletes_state_file() {
        let d = tmpdir();
        {
            let _g = ConnectionGuard::acquire_tun(d.path()).unwrap();
            assert!(d.path().join(STATE_FILE_NAME).exists());
        } // drop here
        assert!(!d.path().join(STATE_FILE_NAME).exists());
    }

    #[test]
    fn recovery_finds_orphan_from_dead_pid() {
        let d = tmpdir();
        let fake_pid = 999_999_999u32; // surely dead
        let persisted = PersistedState {
            schema: SCHEMA_VERSION,
            pid: fake_pid,
            child_pid: None,
            started_at: 1,
            touched_proxy: false,
            applied_proxy: Some("127.0.0.1:7890".into()),
            saved: ProxySnapshot::default(),
        };
        write_state_file(d.path(), &persisted).unwrap();
        let r = recover_orphaned_state(d.path()).unwrap();
        match r {
            RecoveryOutcome::Recovered { applied, .. } => {
                assert_eq!(applied.as_deref(), Some("127.0.0.1:7890"));
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(!d.path().join(STATE_FILE_NAME).exists());
    }
}
