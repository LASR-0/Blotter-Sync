//! The read-only status page at `/`: what this server is, which devices
//! sync with it, and its latest backup. No actions; pairing and revoking
//! stay on the CLI (the Home Assistant add-on brings a page for them).

use blotter_core::snapshots;

use crate::api::{ApiError, Versions};
use crate::auth;
use crate::changes;
use crate::server::Server;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// `2026-10-22T03:03:05Z` as `2026-10-22 03:03 UTC`.
fn when(t: Option<&str>) -> String {
    match t {
        Some(t) if t.len() >= 16 => format!("{} {} UTC", &t[..10], &t[11..16]),
        Some(t) => esc(t),
        None => "never".to_owned(),
    }
}

pub fn render(server: &Server) -> Result<String, ApiError> {
    let meta = server.store.meta().map_err(ApiError::internal)?;
    let seq = changes::current_seq(&server.store)?;
    let devices = auth::devices(&server.store)?;
    let backup = snapshots::list(server.planner.db().conn())
        .map_err(ApiError::internal)?
        .into_iter()
        .next()
        .map(|s| blotter_core::time::fmt_instant(s.created_at));
    let pairing = auth::pairing_open(&server.store, chrono::Utc::now())?;
    let v = Versions::SERVER;

    let mut rows = String::new();
    for d in &devices {
        let status = match (&d.revoked_at, d.clock_ahead_ms) {
            (Some(_), _) => "<span class=\"off\">disconnected</span>".to_owned(),
            (None, Some(ms)) => format!(
                "<span class=\"warn\">clock {} min ahead of the server</span>",
                ms / 60_000
            ),
            (None, None) => "<span class=\"ok\">syncing</span>".to_owned(),
        };
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{status}</td></tr>",
            esc(&d.name),
            when(Some(&d.paired_at)),
            when(d.last_seen_at.as_deref()),
        ));
    }
    if devices.is_empty() {
        rows.push_str(
            "<tr><td colspan=\"4\" class=\"none\">No devices yet. Run \
             <code>blotter-sync pair</code> on the server, then enter the code in \
             Blotter under Settings → Sync.</td></tr>",
        );
    }

    Ok(format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta http-equiv="refresh" content="60">
<title>Blotter Sync</title>
<style>
:root {{ --bg: #fbf8f1; --surface: #ffffff; --text: #22201c; --muted: #6f6a60; --border: #e6e0d4;
        --accent: #fe5858; --ok: #2f8f63; --warn: #b26a00; --off: #8a847a; color-scheme: light; }}
@media (prefers-color-scheme: dark) {{
  :root {{ --bg: #121317; --surface: #1b1d22; --text: #ece9e2; --muted: #9a958b; --border: #2c2f36;
          --accent: #fe5858; --ok: #56c49b; --warn: #e3a64a; --off: #7d786f; color-scheme: dark; }}
}}
* {{ box-sizing: border-box; }}
body {{ margin: 0; background: var(--bg); color: var(--text);
        font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; }}
main {{ max-width: 760px; margin: 0 auto; padding: 32px 16px 48px; }}
h1 {{ font-size: 22px; margin: 0 0 4px; }}
h1 span {{ color: var(--accent); }}
p.lede {{ color: var(--muted); margin: 0 0 24px; }}
section {{ background: var(--surface); border: 1px solid var(--border); border-radius: 10px;
           padding: 16px; margin-bottom: 16px; overflow-x: auto; }}
h2 {{ font-size: 13px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); margin: 0 0 10px; }}
dl {{ display: grid; grid-template-columns: max-content 1fr; gap: 6px 16px; margin: 0; }}
dt {{ color: var(--muted); }} dd {{ margin: 0; font-family: ui-monospace, Consolas, monospace; font-size: 13px; word-break: break-all; }}
table {{ width: 100%; border-collapse: collapse; font-size: 14px; }}
th, td {{ text-align: left; padding: 6px 8px; border-top: 1px solid var(--border); white-space: nowrap; }}
th {{ color: var(--muted); font-weight: 500; border-top: 0; }}
td.none {{ white-space: normal; color: var(--muted); }}
.ok {{ color: var(--ok); }} .warn {{ color: var(--warn); }} .off {{ color: var(--off); }}
@media (max-width: 520px) {{ dl {{ grid-template-columns: 1fr; gap: 0; }} dd {{ margin-bottom: 8px; }} }}
code {{ font-family: ui-monospace, Consolas, monospace; font-size: 13px; }}
</style>
</head>
<body>
<main>
<h1>Blotter <span>Sync</span></h1>
<p class="lede">Keeps every Blotter desktop's planner in sync. This page refreshes every minute.</p>
<section>
<h2>Server</h2>
<dl>
<dt>Version</dt><dd>{version} (Blotter core {core})</dd>
<dt>Sync protocol · schema</dt><dd>{protocol} · {schema}</dd>
<dt>Server id</dt><dd>{server_id}</dd>
<dt>Epoch</dt><dd>{epoch}</dd>
<dt>Changes logged</dt><dd>{seq}</dd>
<dt>Latest backup</dt><dd>{backup}</dd>
<dt>Pairing code</dt><dd>{pairing}</dd>
</dl>
</section>
<section>
<h2>Devices</h2>
<table>
<thead><tr><th>Name</th><th>Paired</th><th>Last seen</th><th>Status</th></tr></thead>
<tbody>{rows}</tbody>
</table>
</section>
</main>
</body>
</html>
"#,
        version = crate::VERSION,
        core = blotter_core::VERSION,
        protocol = v.protocol,
        schema = v.schema,
        server_id = esc(&meta.server_id),
        epoch = esc(&meta.epoch),
        backup = when(backup.as_deref()),
        pairing = if pairing { "open" } else { "none open" },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{PairRequest, open_pairing, pair};

    #[test]
    fn the_page_lists_devices_safely_and_never_their_tokens() {
        let server = Server::open_in_memory().unwrap();
        let empty = render(&server).unwrap();
        assert!(empty.contains("No devices yet"));
        assert!(empty.contains(crate::VERSION));

        let now = chrono::Utc::now();
        let code = open_pairing(&server.store, now).unwrap().code;
        let paired = pair(
            &server.store,
            &PairRequest {
                code,
                device_id: "01M5GHBBDWMQRX2Q9VC2GR82XE".into(),
                name: "<b>Luke's</b> laptop".into(),
            },
            now,
        )
        .unwrap();
        let page = render(&server).unwrap();
        assert!(page.contains("&lt;b&gt;Luke&#39;s&lt;/b&gt; laptop"));
        assert!(!page.contains("<b>Luke"));
        assert!(!page.contains(&paired.token));
        assert!(page.contains("syncing"));
    }

    #[test]
    fn times_read_plainly() {
        assert_eq!(when(Some("2026-10-22T03:03:05Z")), "2026-10-22 03:03 UTC");
        assert_eq!(when(None), "never");
    }
}
