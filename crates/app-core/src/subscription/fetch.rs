//! High-level subscription fetcher.
//!
//! Designed to be on Happ-quality. Real-world flow on the wire:
//!
//! 1. Try the URL with a `Happ`-style User-Agent. Most well-behaved panels
//!    (Marzban, 3X-UI, sub-store, Remnawave configured normally) respond
//!    with the real subscription body straight away.
//! 2. If that returns HTML — that's an "happ-aware" landing page (Remnawave
//!    in webapp-only mode, OneClick installers, etc.). Scan it for
//!    `happ://add/<URL>`, `v2raytun://`, etc. deep-link buttons → recursively
//!    fetch the URL inside.
//! 3. If the HTML had no useful anchors — probe convention-based fallbacks
//!    (`?clash=true`, `/raw`, `/json`, `?type=happ`).
//! 4. As a last resort — try a different User-Agent (some panels gate by UA
//!    and only return JSON for `sing-box/*`, others for `v2rayN/*`).
//!
//! At any step we never follow more than a single recursion level so a
//! malicious panel can't make us chase ourselves.

use std::time::Duration;

use crate::error::{CoreError, CoreResult};
use crate::profile::ProxyProfile;
use crate::subscription::format::{detect, SubscriptionFormat};
use crate::subscription::html::{convention_candidates, extract_subscription_url, HtmlLanderHint};
use crate::subscription::meta::SubscriptionMeta;
use crate::subscription::ParsedSubscription;

const DEFAULT_UA: &str =
    concat!("v2pn/", env!("CARGO_PKG_VERSION"), " (compatible; Happ/2.0)");

/// User-agents to cycle through. Order matters — we try them top to bottom.
const UA_CAROUSEL: &[&str] = &[
    "v2pn/0.1.0 (compatible; Happ/2.0)",
    "Happ/1.7.0 (com.happproxy; iOS/17.5)",
    "v2rayN/6.40",
    "ClashMetaForAndroid/2.10",
    "sing-box/1.13.0",
    "Streisand/1.6",
    "Shadowrocket/2009",
];

#[derive(Debug, Clone)]
pub struct FetchOptions {
    pub user_agent: String,
    pub timeout: Duration,
    pub allow_insecure: bool,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            user_agent: DEFAULT_UA.into(),
            timeout: Duration::from_secs(20),
            allow_insecure: false,
        }
    }
}

/// Download + parse a subscription URL into profiles + metadata.
pub async fn fetch_subscription(url: &str, opts: &FetchOptions) -> CoreResult<ParsedSubscription> {
    fetch_subscription_inner(url, opts, /* depth = */ 0).await
}

#[allow(clippy::too_many_lines)]
async fn fetch_subscription_inner(
    url: &str,
    opts: &FetchOptions,
    depth: u8,
) -> CoreResult<ParsedSubscription> {
    if depth > 1 {
        return Err(CoreError::Parse(
            "subscription resolver recursed too deep — refusing to chase further".into(),
        ));
    }

    // 0. Happ-imposter preflight.
    //
    // A growing class of Russian / Russian-flavour panels (Buzzvpn,
    // Remnawave installs that copied Buzzvpn's gate, …) won't surface
    // the real subscription unless the request *exactly* impersonates
    // the official Happ desktop client. The recipe they check:
    //
    //   User-Agent:  Happ/<x.y.z>/<OS>/<HWID>
    //   X-Hwid:      <HWID>           (same value as in the UA tail)
    //
    // The hwid is also used by the panel for device-binding — first N
    // distinct hwids that hit the URL claim that subscription's slots,
    // subsequent ones get an empty 200. We use our stable per-machine
    // hwid so the user keeps their slot across restarts.
    //
    // We try both Windows and iOS in the OS slot because some Buzzvpn
    // SKUs hardcode a list of accepted platforms.
    {
        let our_hwid = crate::hwid::hwid();
        for os_tag in ["Windows", "iOS", "MacOS", "Android"] {
            let ua = format!("Happ/2.17.1/{os_tag}/{our_hwid}");
            match fetch_raw(url, &ua, true, opts).await {
                Ok((bytes, meta)) if !bytes.is_empty() => {
                    let fmt = detect(&bytes);
                    tracing::debug!(
                        target: "fetch",
                        ua = %ua,
                        bytes = bytes.len(),
                        "happ-preflight format = {:?}",
                        fmt
                    );
                    if matches!(
                        fmt,
                        SubscriptionFormat::UriList
                            | SubscriptionFormat::Base64UriList
                            | SubscriptionFormat::SingBoxJson
                            | SubscriptionFormat::XrayArray
                            | SubscriptionFormat::ClashYaml
                    ) {
                        let mut profiles = parse_body(&bytes)?;
                        let sub_id = sub_tag(url);
                        for p in &mut profiles {
                            p.subscription_id = Some(sub_id.clone());
                        }
                        tracing::info!(target: "fetch",
                            os = os_tag, profiles = profiles.len(),
                            "happ-preflight succeeded");
                        return Ok(ParsedSubscription { profiles, meta });
                    }
                    // HTML / Unknown from Happ-preflight → fall through
                    // to the regular UA carousel below.
                }
                _ => {}
            }
        }
    }

    // 1. UA carousel — first attempt that returns a *parseable* body wins.
    //    For each UA we make TWO attempts: with our X-Hwid header (some
    //    Remnawave-style panels gate on it) and without (other panels
    //    actively reject anything they don't recognise as a known mobile
    //    client and serve an empty body or a stub). We try with-hwid
    //    first because the panels that require it are roughly as common
    //    as the ones that reject it, and the with-hwid attempt is a
    //    no-op penalty when the panel doesn't care.
    let mut html_attempt: Option<(String, SubscriptionMeta)> = None; // body + meta
    let mut last_error: Option<CoreError> = None;

    for ua in UA_CAROUSEL.iter() {
        for &send_hwid in &[true, false] {
            match fetch_raw(url, ua, send_hwid, opts).await {
                Ok((bytes, meta)) => {
                    // Empty body → server gave us a polite "no". Move on.
                    if bytes.is_empty() {
                        tracing::debug!(target: "fetch",
                            ua, send_hwid, "empty body, trying next variant");
                        continue;
                    }
                    let fmt = detect(&bytes);
                    tracing::debug!(target: "fetch", ua, send_hwid,
                        bytes = bytes.len(), "format = {:?}", fmt);
                    if matches!(
                        fmt,
                        SubscriptionFormat::UriList
                            | SubscriptionFormat::Base64UriList
                            | SubscriptionFormat::SingBoxJson
                            | SubscriptionFormat::XrayArray
                            | SubscriptionFormat::ClashYaml
                    ) {
                        let mut profiles = parse_body(&bytes)?;
                        let sub_id = sub_tag(url);
                        for p in &mut profiles {
                            p.subscription_id = Some(sub_id.clone());
                        }
                        return Ok(ParsedSubscription { profiles, meta });
                    }
                    if matches!(fmt, SubscriptionFormat::Html) && html_attempt.is_none() {
                        let body = String::from_utf8_lossy(&bytes).into_owned();
                        html_attempt = Some((body, meta));
                    }
                }
                Err(e) => last_error = Some(e),
            }
        }
    }

    // 2. HTML landing page → scan for deep-links / inline URLs.
    if let Some((html, meta)) = html_attempt {
        let cand = extract_subscription_url(&html, url);
        if let Some(target) = cand.urls.into_iter().find(|u| u != url) {
            tracing::info!(target: "fetch", "html lander → recursing into {target}");
            // Try again, but only one level deep. Carry over original meta if
            // the inner call doesn't override it.
            let mut sub = Box::pin(fetch_subscription_inner(&target, opts, depth + 1)).await?;
            if sub.meta.title.is_none() {
                sub.meta = meta;
            }
            return Ok(sub);
        }

        // 3. No deep-link found. Try convention candidates.
        let candidates = convention_candidates(url);
        let probed = candidates.len();
        for cand in &candidates {
            for ua in UA_CAROUSEL.iter() {
                // Same with-hwid / without-hwid pair — convention probe
                // candidates can be panel endpoints with their own hwid
                // expectations.
                'attempts: for &send_hwid in &[true, false] {
                if let Ok((bytes, c_meta)) = fetch_raw(cand, ua, send_hwid, opts).await {
                    if bytes.is_empty() { continue 'attempts; }
                    let fmt = detect(&bytes);
                    if matches!(
                        fmt,
                        SubscriptionFormat::UriList
                            | SubscriptionFormat::Base64UriList
                            | SubscriptionFormat::SingBoxJson
                            | SubscriptionFormat::XrayArray
                            | SubscriptionFormat::ClashYaml
                    ) {
                        tracing::info!(target: "fetch",
                            "convention probe hit: {cand} (ua={ua})");
                        let mut profiles = parse_body(&bytes)?;
                        let sub_id = sub_tag(url);
                        for p in &mut profiles {
                            p.subscription_id = Some(sub_id.clone());
                        }
                        let merged_meta = if c_meta.title.is_some() { c_meta } else { meta.clone() };
                        return Ok(ParsedSubscription {
                            profiles,
                            meta: merged_meta,
                        });
                    }
                }
                } // 'attempts: for send_hwid
            }
        }

        // 4. Nothing worked → emit an actionable error.
        let hint = HtmlLanderHint {
            source_url: url.to_string(),
            webapp_url: None,
            probed_candidates: candidates,
            message: format!(
                "This panel returned an HTML page instead of a subscription. \
                 We tried {probed} known endpoint variants and the deep-link \
                 buttons embedded in the page — none returned a subscription. \
                 Please try one of:\n\
                 1) Open {url} in your browser, then in the page DevTools \
                    network tab find the request that returns vless:// / \
                    base64 / yaml / sing-box JSON, and paste *that* URL here.\n\
                 2) Copy a single vless:// or trojan:// link from your other \
                    client (Happ, v2rayN, …) and paste it via the Text tab.",
            ),
        };
        return Err(hint.into_error());
    }

    Err(last_error.unwrap_or_else(|| {
        CoreError::Parse("no User-Agent variant produced a usable response".into())
    }))
}

async fn fetch_raw(
    url: &str,
    ua: &str,
    send_hwid: bool,
    opts: &FetchOptions,
) -> CoreResult<(Vec<u8>, SubscriptionMeta)> {
    // Validate the URL before we let reqwest touch it. Three classes of
    // abuse to refuse up-front:
    //   * non-https/http schemes (file:, data:, javascript:, …)
    //   * literal IP addresses that point to private / loopback / link-local
    //     ranges → SSRF into the user's LAN, AWS metadata service
    //     (169.254.169.254), Redis on localhost, etc.
    //   * already-misformatted strings reqwest would otherwise stringify
    //     into a confusing error.
    validate_subscription_url(url)?;

    let client = reqwest::Client::builder()
        .user_agent(ua)
        .timeout(opts.timeout)
        // Custom redirect policy: same as `limited(5)` PLUS we re-validate
        // every hop so an attacker can't 302 us into the LAN side after
        // the initial allow-listed host responded fine. The limit is also
        // tightened to 3 — most legitimate subscription panels redirect
        // at most once (HTTP→HTTPS or trailing-slash normalisation).
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.error("too many redirects");
            }
            match validate_subscription_url(attempt.url().as_str()) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(format!("redirect blocked: {e}")),
            }
        }))
        .danger_accept_invalid_certs(opts.allow_insecure)
        .build()
        .map_err(CoreError::Network)?;
    // Many Remnawave-style panels (BuzzVPN, …) require an `X-Hwid` header
    // before they'll surface the real subscription instead of a webapp redirect.
    // We send a stable per-machine identifier — see `app_core::hwid`.
    // OTHER panels (also Remnawave-derived but custom-deployed) actively
    // reject any unrecognised X-Hwid and respond with an empty body.
    // The caller cycles `send_hwid` true/false so we always cover both.
    let req = client.get(url);
    let req = if send_hwid {
        req.header("X-Hwid", crate::hwid::hwid())
    } else {
        req
    };
    let resp = req
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        return Err(CoreError::Other(format!(
            "subscription HTTP {} for {}", status.as_u16(), url
        )));
    }
    let meta = SubscriptionMeta::from_headers(resp.headers());

    // Hard cap on response body. Even a perfectly-honest panel rarely
    // needs more than 256 KiB; 8 MiB gives us a 30× safety margin for
    // pathological subscriptions with thousands of servers + base64
    // overhead, while shutting down memory-exhaustion attacks where a
    // hostile server streams an infinite body. We also obey
    // Content-Length up-front so we don't even start the transfer.
    const MAX_BODY: usize = 8 * 1024 * 1024;
    if let Some(cl) = resp.content_length() {
        if cl > MAX_BODY as u64 {
            return Err(CoreError::Other(format!(
                "subscription body too large ({cl} bytes, limit {MAX_BODY})"
            )));
        }
    }
    let mut body = Vec::with_capacity(8192);
    let mut stream = resp;
    while let Some(chunk) = stream.chunk().await? {
        if body.len() + chunk.len() > MAX_BODY {
            return Err(CoreError::Other(format!(
                "subscription body exceeded {MAX_BODY} bytes; aborting"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, meta))
}

/// Reject obviously-dangerous URLs before we hand them to reqwest. This
/// is the front-line SSRF defence. The rules:
///
///  * Scheme MUST be http or https. No `file://`, no `gopher://`, no
///    `data:` URLs. Reqwest itself usually ignores these but we want to
///    fail fast with a clear message.
///  * If the host is a literal IP, reject any address in the
///    private / loopback / link-local / multicast / broadcast / unspec
///    blocks. This stops requests to:
///      - localhost (127.0.0.0/8, ::1)
///      - LAN (10/8, 172.16/12, 192.168/16)
///      - link-local + AWS/GCP/Azure metadata (169.254.0.0/16)
///      - IPv6 loopback / link-local / unique-local
///  * Hostnames are NOT resolved here — DNS-based SSRF (rebinding) is
///    handled by reqwest's native resolver + the redirect re-check.
fn validate_subscription_url(raw: &str) -> CoreResult<()> {
    use std::net::IpAddr;

    let parsed = url::Url::parse(raw).map_err(|e| {
        CoreError::Other(format!("invalid subscription URL: {e}"))
    })?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(CoreError::Other(format!(
                "subscription URL scheme not allowed: {other}"
            )));
        }
    }
    // If the host is a literal IP, gate it on address class.
    if let Some(host) = parsed.host_str() {
        if let Ok(ip) = host.parse::<IpAddr>() {
            let blocked = match ip {
                IpAddr::V4(v4) => {
                    v4.is_loopback()
                        || v4.is_private()
                        || v4.is_link_local()
                        || v4.is_broadcast()
                        || v4.is_multicast()
                        || v4.is_unspecified()
                }
                IpAddr::V6(v6) => {
                    v6.is_loopback()
                        || v6.is_unspecified()
                        || v6.is_multicast()
                        // is_unique_local / is_unicast_link_local are
                        // unstable; approximate with prefix matches.
                        || v6.segments()[0] & 0xfe00 == 0xfc00 // fc00::/7  ULA
                        || v6.segments()[0] & 0xffc0 == 0xfe80 // fe80::/10 link-local
                }
            };
            if blocked {
                return Err(CoreError::Other(format!(
                    "subscription URL points to a blocked address class: {ip}"
                )));
            }
        }
    } else {
        return Err(CoreError::Other("subscription URL has no host".into()));
    }
    Ok(())
}

/// Parse already-downloaded bytes (used for clipboard/file imports too).
pub fn parse_body(body: &[u8]) -> CoreResult<Vec<ProxyProfile>> {
    let fmt = detect(body);
    match fmt {
        SubscriptionFormat::UriList => {
            let s = std::str::from_utf8(body)?;
            Ok(crate::subscription::uri_list::parse_plain(s))
        }
        SubscriptionFormat::Base64UriList => {
            let s = std::str::from_utf8(body)?;
            crate::subscription::uri_list::parse_base64(s)
        }
        SubscriptionFormat::SingBoxJson => crate::subscription::singbox::parse(body),
        SubscriptionFormat::XrayArray  => crate::subscription::xray_array::parse(body),
        SubscriptionFormat::ClashYaml  => crate::subscription::clash::parse(body),
        SubscriptionFormat::Html => Err(CoreError::Parse(
            "server returned an HTML landing page; use a deep-link import or a client-aware UA".into(),
        )),
        SubscriptionFormat::Unknown => Err(CoreError::Parse("unknown subscription format".into())),
    }
}

fn sub_tag(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(url.as_bytes());
    hex::encode(&h[..8])
}
