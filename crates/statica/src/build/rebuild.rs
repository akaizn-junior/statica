//! Watch-mode rebuild planning.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::discover::{self, PageFile};
use crate::error::Result;

use super::{build, build_scoped, BuildOptions, BuildReport};

/// Which page sources should be emitted by this build pass.
///
/// Full builds may also run global output passes: redirects, default 404, asset
/// copy/process, feeds, and minification. Selected page builds are watch-mode
/// rebuilds for direct page edits and only re-emit the matching routes.
#[derive(Debug, Clone)]
pub(super) enum BuildScope {
    Full,
    SelectedPages(PageSelection),
}

impl BuildScope {
    pub(super) fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub(super) fn contains_page(&self, file: &PageFile) -> bool {
        match self {
            Self::Full => true,
            Self::SelectedPages(paths) => paths.contains(file),
        }
    }

    pub(super) fn discover_detail(&self, sources: usize) -> String {
        match self {
            Self::Full => format!("{sources} sources"),
            Self::SelectedPages(paths) => format!("{sources} sources, {} selected", paths.len()),
        }
    }
}

#[derive(Debug, Clone)]
enum RebuildPlan {
    /// All watched changes were ignored, so the existing output remains valid.
    Noop,
    /// Run the complete pipeline. `clean` is true when deleted inputs may have
    /// left stale generated files behind.
    Full { clean: bool },
    /// Re-emit exactly these discovered page source files.
    SelectedPages(PageSelection),
}

/// Watch events from generated or build-output directories do not invalidate
/// source output. Everything else is meaningful until proven otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangedPathKind {
    Ignored,
    Meaningful,
}

/// Result of mapping changed source paths onto page routes.
#[derive(Debug, Clone)]
enum PageRebuildSelection {
    Selected(PageSelection),
    RequiresFullBuild,
}

/// Whether page-only rebuilds can preserve output correctness for this config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageRebuildEligibility {
    Eligible,
    RequiresFullBuild,
}

/// Explicit set of source pages selected for a scoped rebuild.
#[derive(Debug, Clone)]
pub(super) struct PageSelection {
    files: HashSet<PageFile>,
}

impl PageSelection {
    fn new(files: HashSet<PageFile>) -> Self {
        Self { files }
    }

    pub(super) fn len(&self) -> usize {
        self.files.len()
    }

    pub(super) fn contains(&self, file: &PageFile) -> bool {
        self.files.contains(file)
    }
}

pub fn rebuild_paths(opts: &BuildOptions, changed: &[PathBuf]) -> Result<BuildReport> {
    match plan_rebuild(opts, changed)? {
        RebuildPlan::Noop => Ok(BuildReport::default()),
        RebuildPlan::SelectedPages(paths) => {
            let mut incremental = opts.clone();
            incremental.clean = false;
            build_scoped(&incremental, &BuildScope::SelectedPages(paths))
        }
        RebuildPlan::Full { clean } => {
            let mut full = opts.clone();
            full.clean = clean;
            build(&full)
        }
    }
}

fn plan_rebuild(opts: &BuildOptions, changed: &[PathBuf]) -> Result<RebuildPlan> {
    let meaningful = meaningful_changed_paths(opts, changed);

    match meaningful.as_slice() {
        [] if changed.is_empty() => Ok(RebuildPlan::Full { clean: true }),
        [] => Ok(RebuildPlan::Noop),
        paths => match page_rebuild_eligibility(opts) {
            PageRebuildEligibility::Eligible => match selected_changed_pages(opts, paths)? {
                PageRebuildSelection::Selected(selected) => {
                    Ok(RebuildPlan::SelectedPages(selected))
                }
                PageRebuildSelection::RequiresFullBuild => Ok(RebuildPlan::Full {
                    clean: paths.iter().any(|path| !path.exists()),
                }),
            },
            PageRebuildEligibility::RequiresFullBuild => Ok(RebuildPlan::Full {
                clean: paths.iter().any(|path| !path.exists()),
            }),
        },
    }
}

fn meaningful_changed_paths(opts: &BuildOptions, changed: &[PathBuf]) -> Vec<PathBuf> {
    changed
        .iter()
        .filter_map(|path| {
            let path = normalize_changed_path(&opts.root, path);
            match changed_path_kind(opts, &path) {
                ChangedPathKind::Ignored => None,
                ChangedPathKind::Meaningful => Some(path),
            }
        })
        .collect()
}

fn changed_path_kind(opts: &BuildOptions, path: &Path) -> ChangedPathKind {
    if path.starts_with(&opts.out_dir) {
        return ChangedPathKind::Ignored;
    }
    if path
        .components()
        .any(|component| component.as_os_str() == std::ffi::OsStr::new("target"))
    {
        return ChangedPathKind::Ignored;
    }
    ChangedPathKind::Meaningful
}

fn page_rebuild_eligibility(opts: &BuildOptions) -> PageRebuildEligibility {
    if opts.minify.enabled || opts.process.enabled {
        return PageRebuildEligibility::RequiresFullBuild;
    }
    PageRebuildEligibility::Eligible
}

fn selected_changed_pages(
    opts: &BuildOptions,
    changed: &[PathBuf],
) -> Result<PageRebuildSelection> {
    let pages = discover::discover_pages(&opts.root, &opts.ignore_dirs)?;
    let mut selected = HashSet::new();

    for path in changed {
        if !path.exists() || path.file_name().is_none_or(|name| name != "index.html") {
            return Ok(PageRebuildSelection::RequiresFullBuild);
        }
        let Some(page) = pages
            .iter()
            .find(|page| same_path(page.path.as_path(), path))
        else {
            return Ok(PageRebuildSelection::RequiresFullBuild);
        };
        selected.insert(page.path.clone());
    }

    if selected.is_empty() {
        Ok(PageRebuildSelection::RequiresFullBuild)
    } else {
        Ok(PageRebuildSelection::Selected(PageSelection::new(selected)))
    }
}

fn normalize_changed_path(root: &Path, path: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    path.canonicalize().unwrap_or(path)
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
