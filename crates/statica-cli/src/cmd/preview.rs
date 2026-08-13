//! Local static preview server — **axum** + **tower-http** `ServeDir`
//! (directory indexes, precompressed gzip, 404 fallback).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;

use anyhow::{bail, Context, Result};
use axum::body::{to_bytes, Body};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{extract::Request, Router};
use tokio::sync::broadcast;
use tower::{service_fn, Service};
use tower_http::services::{ServeDir, ServeFile};

use crate::style;

/// Serve `out_dir` on `host:port` until interrupted.
///
/// Prints a **Local** URL and any **Network** (LAN) URLs so phones on the same
/// Wi‑Fi can open the site when bound to `0.0.0.0` (the default).
pub async fn serve_dir(out_dir: &Path, host: IpAddr, port: u16) -> Result<()> {
    serve_dir_inner(out_dir, host, port, None).await
}

pub async fn serve_dir_with_reload(
    out_dir: &Path,
    host: IpAddr,
    port: u16,
    reloads: broadcast::Sender<()>,
) -> Result<()> {
    serve_dir_inner(out_dir, host, port, Some(reloads)).await
}

async fn serve_dir_inner(
    out_dir: &Path,
    host: IpAddr,
    port: u16,
    reloads: Option<broadcast::Sender<()>>,
) -> Result<()> {
    if !out_dir.is_dir() {
        bail!(
            "output directory `{}` not found — run `statica build` first",
            out_dir.display()
        );
    }

    let not_found = not_found_file(out_dir);
    let not_found_service = service_fn(move |request| {
        let mut service = ServeFile::new(not_found.clone());
        async move {
            match service.call(request).await {
                Ok(mut response) => {
                    *response.status_mut() = StatusCode::NOT_FOUND;
                    Ok(response)
                }
                Err(_) => {
                    let mut response = Response::new(Default::default());
                    *response.status_mut() = StatusCode::NOT_FOUND;
                    Ok(response)
                }
            }
        }
    });
    let app = Router::new().fallback_service(
        ServeDir::new(out_dir)
            .append_index_html_on_directories(true)
            .precompressed_gzip()
            .fallback(not_found_service),
    );
    let app = if let Some(reloads) = reloads {
        app.route("/__statica/reload", get(move || reload(reloads.clone())))
            .layer(middleware::from_fn(watch_response))
    } else {
        app
    };

    let addr = SocketAddr::from((host, port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind http://{host}:{port}"))?;

    print_urls(out_dir, host, port);

    axum::serve(listener, app)
        .await
        .context("preview server exited with error")?;
    Ok(())
}

async fn reload(reloads: broadcast::Sender<()>) -> StatusCode {
    let mut rx = reloads.subscribe();
    match rx.recv().await {
        Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => StatusCode::NO_CONTENT,
        Err(broadcast::error::RecvError::Closed) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

const LIVE_RELOAD_SNIPPET: &str = r#"<script type="module" data-statica-live-reload>
(() => {
  const listen = async () => {
    for (;;) {
      try {
        await fetch("/__statica/reload", { cache: "no-store" });
        location.reload();
        return;
      } catch (_) {
        await new Promise((resolve) => setTimeout(resolve, 700));
      }
    }
  };
  listen();
})();
</script>"#;

async fn watch_response(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    if !should_inject_live_reload(&response) {
        return with_watch_headers(response);
    }

    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
        return with_watch_headers(Response::from_parts(parts, Body::empty()));
    };
    let Ok(html) = String::from_utf8(bytes.to_vec()) else {
        return with_watch_headers(Response::from_parts(parts, Body::from(bytes)));
    };
    let updated = with_live_reload(&html);
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    with_watch_headers(Response::from_parts(parts, Body::from(updated)))
}

fn with_watch_headers(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    response
}

fn should_inject_live_reload(response: &Response) -> bool {
    if response.status() != StatusCode::OK && response.status() != StatusCode::NOT_FOUND {
        return false;
    }
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"))
}

fn with_live_reload(html: &str) -> String {
    if html.contains("data-statica-live-reload") {
        return replace_live_reload_snippet(html);
    }
    if let Some(pos) = html.rfind("</body>") {
        let mut out = String::with_capacity(html.len() + LIVE_RELOAD_SNIPPET.len() + 1);
        out.push_str(&html[..pos]);
        out.push_str(LIVE_RELOAD_SNIPPET);
        out.push('\n');
        out.push_str(&html[pos..]);
        out
    } else {
        format!("{html}\n{LIVE_RELOAD_SNIPPET}\n")
    }
}

fn replace_live_reload_snippet(html: &str) -> String {
    let marker = "data-statica-live-reload";
    let Some(marker_pos) = html.find(marker) else {
        return html.to_owned();
    };
    let before_marker = &html[..marker_pos];
    let Some(script_start_rel) = before_marker.rfind("<script") else {
        return html.to_owned();
    };
    let after_marker = &html[marker_pos..];
    let Some(script_end_rel) = after_marker.find("</script>") else {
        return html.to_owned();
    };
    let script_end = marker_pos + script_end_rel + "</script>".len();

    let mut updated = String::with_capacity(html.len() + LIVE_RELOAD_SNIPPET.len());
    updated.push_str(&html[..script_start_rel]);
    updated.push_str(LIVE_RELOAD_SNIPPET);
    updated.push_str(&html[script_end..]);
    updated
}

fn not_found_file(out_dir: &Path) -> std::path::PathBuf {
    let flat = out_dir.join("404.html");
    if flat.is_file() {
        return flat;
    }
    let nested = out_dir.join("404").join("index.html");
    if nested.is_file() {
        return nested;
    }
    out_dir.join("index.html")
}

fn print_urls(out_dir: &Path, host: IpAddr, port: u16) {
    eprintln!(
        "{} {}",
        style::accent("serving"),
        style::dim(out_dir.display().to_string()),
    );

    let local = format!("http://127.0.0.1:{port}");
    eprintln!("  {}  {}", style::dim("Local:  "), style::bold(&local),);

    let lan = lan_urls(host, port);
    if lan.is_empty() {
        if host.is_loopback() {
            eprintln!(
                "  {}  {}",
                style::dim("Network:"),
                style::dim("use --host 0.0.0.0 to reach phones on Wi‑Fi"),
            );
        }
        return;
    }

    for (i, url) in lan.iter().enumerate() {
        let label = if i == 0 {
            style::dim("Network:")
        } else {
            style::dim("        ")
        };
        eprintln!("  {label}  {}", style::bold(url));
    }
}

/// LAN URLs reachable when listening on all interfaces or a specific non-loopback IP.
fn lan_urls(bind: IpAddr, port: u16) -> Vec<String> {
    if bind.is_loopback() {
        return Vec::new();
    }

    let mut ips: Vec<IpAddr> = Vec::new();

    if !bind.is_unspecified() {
        // Bound to one interface address — advertise that.
        ips.push(bind);
    } else if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (_, ip) in ifaces {
            if let IpAddr::V4(v4) = ip {
                if !v4.is_loopback() && !v4.is_unspecified() && !is_link_local(v4) {
                    ips.push(IpAddr::V4(v4));
                }
            }
        }
    }

    ips.sort_by_key(|ip| ip.to_string());
    ips.dedup();
    ips.into_iter()
        .map(|ip| format_http_url(ip, port))
        .collect()
}

fn is_link_local(v4: Ipv4Addr) -> bool {
    // 169.254.0.0/16
    v4.octets()[0] == 169 && v4.octets()[1] == 254
}

fn format_http_url(host: IpAddr, port: u16) -> String {
    match host {
        IpAddr::V6(v6) => format!("http://[{v6}]:{port}"),
        IpAddr::V4(v4) => format!("http://{v4}:{port}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_reload_is_injected_before_body_close() {
        let html = with_live_reload("<!doctype html><body><h1>Hi</h1></body>");

        assert_eq!(html.matches("data-statica-live-reload").count(), 1);
        assert!(html.contains("/__statica/reload"));
        assert!(html.contains("</script>\n</body>"));
    }

    #[test]
    fn live_reload_replaces_existing_snippet() {
        let html = with_live_reload(
            r#"<!doctype html><body><h1>Hi</h1><script type="module" data-statica-live-reload>
(() => {
  fetch("/__statica/reload").then(() => location.reload());
})();
</script>
</body>"#,
        );

        assert_eq!(html.matches("data-statica-live-reload").count(), 1);
        assert!(html.contains("/__statica/reload"));
        assert!(!html.contains("then(() => location.reload())"));
    }

    #[test]
    fn watch_responses_disable_browser_cache() {
        let response = with_watch_headers(Response::new(Body::empty()));

        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "no-store, max-age=0"
        );
    }
}
