use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use statica_core::BuildOptions;

use crate::cli::ConfigCli;
use crate::config::PreviewConfig;
use crate::style;
use super::{preview, util};

pub async fn run(dir: &Path, overrides: &ConfigCli) -> Result<()> {
    let (root, config) = util::load_project(dir, overrides)?;
    let opts = config.to_build_options(&root);
    let host = config.preview.host_addr()?;
    let port = config.preview.port;

    eprintln!(
        "{} {}",
        style::accent("statica watch"),
        style::dim(root.display().to_string()),
    );

    let report = util::run_build(&opts)?;
    util::log_build(&report, &opts.out_dir, "built");

    start_watcher(root.clone(), opts.clone(), &config.preview)?;
    preview::serve_dir(&opts.out_dir, host, port).await
}

fn start_watcher(root: PathBuf, opts: BuildOptions, preview_cfg: &PreviewConfig) -> Result<()> {
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
            loop {
                match rx.recv() {
                    Ok(paths) => pending.extend(paths),
                    Err(_) => break,
                }
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
                    Ok(report) => util::log_build(&report, &rebuild_opts.out_dir, "rebuilt"),
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
