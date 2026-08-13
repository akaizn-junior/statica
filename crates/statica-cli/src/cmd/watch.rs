use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use statica::BuildOptions;
use tokio::sync::broadcast;

use super::{preview, util};
use crate::cli::ConfigCli;
use crate::config::PreviewConfig;
use crate::style;

pub async fn run(dir: &Path, overrides: &ConfigCli) -> Result<()> {
    let (root, config) = util::load_project(dir, overrides)?;
    let opts = util::build_options(&config, &root, overrides, true);
    let host = config.preview.host_addr()?;
    let port = config.preview.port;

    eprintln!(
        "{} {}",
        style::accent("statica watch"),
        style::dim(root.display().to_string()),
    );

    let report = util::run_build(&opts)?;
    inject_live_reload(&opts.out_dir)?;
    util::log_build(&report, &opts.out_dir, "Built", opts.verbose);
    util::write_report_json(&report, overrides.report_json.as_deref())?;

    let (reloads, _) = broadcast::channel(16);
    start_watcher(
        root.clone(),
        opts.clone(),
        &config.preview,
        overrides.report_json.clone(),
        reloads.clone(),
    )?;
    preview::serve_dir_with_reload(&opts.out_dir, host, port, reloads).await
}

fn start_watcher(
    root: PathBuf,
    opts: BuildOptions,
    preview_cfg: &PreviewConfig,
    report_json: Option<PathBuf>,
    reloads: broadcast::Sender<()>,
) -> Result<()> {
    let ignore_dirs = opts.ignore_dirs.clone();
    let debounce = Duration::from_millis(preview_cfg.debounce_ms);
    let poll = Duration::from_secs(preview_cfg.poll_interval_secs.max(1));

    let (tx, rx) = mpsc::channel::<Vec<PathBuf>>();
    let watch_root = root.clone();
    let mut watcher = RecommendedWatcher::new(
        {
            let watch_root = watch_root.clone();
            let ignore_dirs = ignore_dirs.clone();
            move |res: notify::Result<notify::Event>| {
                let Ok(event) = res else {
                    return;
                };
                if matches!(
                    event.kind,
                    EventKind::Access(_) | EventKind::Other | EventKind::Any
                ) {
                    return;
                }
                let paths: Vec<PathBuf> = event
                    .paths
                    .into_iter()
                    .filter(|p| !should_ignore_path(&watch_root, p, &ignore_dirs))
                    .collect();
                if !paths.is_empty() {
                    let _ = tx.send(paths);
                }
            }
        },
        Config::default().with_poll_interval(poll),
    )
    .context("failed to start filesystem watcher")?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .context("failed to watch project directory")?;

    std::thread::Builder::new()
        .name("statica-watch".into())
        .spawn(move || {
            let _watcher = watcher;
            let mut pending: Vec<PathBuf> = Vec::new();
            while let Ok(paths) = rx.recv() {
                pending.extend(paths);
                let deadline = Instant::now() + debounce;
                while Instant::now() < deadline {
                    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                        Ok(paths) => pending.extend(paths),
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                pending.sort();
                pending.dedup();
                if pending.is_empty() {
                    continue;
                }
                let changed = std::mem::take(&mut pending);
                let mut rebuild_opts = opts.clone();
                rebuild_opts.clean = false;
                match util::run_rebuild(&rebuild_opts, &changed) {
                    Ok(report) => {
                        if let Err(e) = inject_live_reload(&rebuild_opts.out_dir) {
                            eprintln!("{} {e:#}", style::error("reload injection failed:"));
                        }
                        util::log_build(
                            &report,
                            &rebuild_opts.out_dir,
                            "Rebuilt",
                            rebuild_opts.verbose,
                        );
                        if let Err(e) = util::write_report_json(&report, report_json.as_deref()) {
                            eprintln!("{} {e:#}", style::error("report failed:"));
                        }
                        let _ = reloads.send(());
                    }
                    Err(e) => eprintln!("{} {e:#}", style::error("rebuild failed:")),
                }
            }
        })
        .context("failed to spawn watch thread")?;
    Ok(())
}

fn should_ignore_path(root: &Path, path: &Path, ignore_dirs: &[String]) -> bool {
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
    {
        return true;
    }
    let matches_ignore = |c: &std::ffi::OsStr| {
        ignore_dirs
            .iter()
            .any(|d| c == std::ffi::OsStr::new(d.as_str()))
    };
    if let Ok(rel) = path.strip_prefix(root) {
        return rel.components().any(|c| matches_ignore(c.as_os_str()));
    }
    path.components().any(|c| matches_ignore(c.as_os_str()))
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

fn inject_live_reload(out_dir: &Path) -> Result<()> {
    if !out_dir.is_dir() {
        return Ok(());
    }
    inject_live_reload_dir(out_dir)
}

fn inject_live_reload_dir(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            inject_live_reload_dir(&path)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("html") {
            inject_live_reload_file(&path)?;
        }
    }
    Ok(())
}

fn inject_live_reload_file(path: &Path) -> Result<()> {
    let html = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    if html.contains("data-statica-live-reload") {
        return Ok(());
    }
    let updated = if let Some(pos) = html.rfind("</body>") {
        let mut out = String::with_capacity(html.len() + LIVE_RELOAD_SNIPPET.len() + 1);
        out.push_str(&html[..pos]);
        out.push_str(LIVE_RELOAD_SNIPPET);
        out.push('\n');
        out.push_str(&html[pos..]);
        out
    } else {
        format!("{html}\n{LIVE_RELOAD_SNIPPET}\n")
    };
    fs::write(path, updated).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "statica-live-reload-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn live_reload_injection_is_idempotent() {
        let dir = temp_dir();
        fs::create_dir_all(dir.join("nested")).unwrap();
        let page = dir.join("nested/index.html");
        fs::write(&page, "<!doctype html><body><h1>Hi</h1></body>").unwrap();

        inject_live_reload(&dir).unwrap();
        inject_live_reload(&dir).unwrap();

        let html = fs::read_to_string(&page).unwrap();
        assert_eq!(html.matches("data-statica-live-reload").count(), 1);
        assert!(html.contains("/__statica/reload"));
        assert!(html.contains("</script>\n</body>"));

        let _ = fs::remove_dir_all(dir);
    }
}
