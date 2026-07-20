//! Local static preview server — **axum** + **tower-http** `ServeDir`
//! (directory indexes, precompressed gzip, SPA-friendly fallback).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;

use anyhow::{bail, Context, Result};
use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

use crate::style;

/// Serve `out_dir` on `host:port` until interrupted.
///
/// Prints a **Local** URL and any **Network** (LAN) URLs so phones on the same
/// Wi‑Fi can open the site when bound to `0.0.0.0` (the default).
pub async fn serve_dir(out_dir: &Path, host: IpAddr, port: u16) -> Result<()> {
    if !out_dir.is_dir() {
        bail!(
            "output directory `{}` not found — run `statica build` first",
            out_dir.display()
        );
    }

    let index = out_dir.join("index.html");
    let app = Router::new().fallback_service(
        ServeDir::new(out_dir.to_path_buf())
            .append_index_html_on_directories(true)
            .precompressed_gzip()
            .fallback(ServeFile::new(index)),
    );

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

fn print_urls(out_dir: &Path, host: IpAddr, port: u16) {
    eprintln!(
        "{} {}",
        style::accent("serving"),
        style::dim(out_dir.display().to_string()),
    );

    let local = format!("http://127.0.0.1:{port}");
    eprintln!(
        "  {}  {}",
        style::dim("Local:  "),
        style::bold(&local),
    );

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
