use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ignore::{WalkBuilder, WalkState};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::error::Result;

/// What the callback should do next for this entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkDecision {
    /// Processed the entry; keep walking.
    Continue,
    /// Logically skip recursion under this path. With `ignore` we emulate this by
    /// pruning descendants in user-space (entries under this path won’t be emitted).
    SkipDescend,
    /// Abort the entire walk ASAP.
    Abort,
}

/// Callback result: control + optional item to emit (for API streaming).
#[derive(Debug)]
pub struct CbResult<T> {
    pub decision: WalkDecision,
    pub output: Option<T>,
}

impl<T> CbResult<T> {
    pub fn cont() -> Self {
        Self {
            decision: WalkDecision::Continue,
            output: None,
        }
    }
    pub fn skip() -> Self {
        Self {
            decision: WalkDecision::SkipDescend,
            output: None,
        }
    }
    pub fn abort() -> Self {
        Self {
            decision: WalkDecision::Abort,
            output: None,
        }
    }
    pub fn emit(item: T) -> Self {
        Self {
            decision: WalkDecision::Continue,
            output: Some(item),
        }
    }
}

/// Options to control traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkOptions {
    /// Maximum recursion depth. 0 = list only root.
    pub depth: usize,
    /// Include hidden files/folders. If false, .gitignore-style hidden entries are skipped by builder.
    pub include_hidden: bool,
    /// If true, the walker uses multiple threads; otherwise it runs single-threaded.
    pub parallelize_recursion: bool,
    /// Number of threads to use when parallelizing.
    pub max_concurrency: usize,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            depth: usize::MAX,
            include_hidden: true,
            parallelize_recursion: true,
            max_concurrency: 8,
        }
    }
}

/// Entry passed to the callback.
#[derive(Debug, Clone)]
pub struct WalkEntry {
    /// Absolute path of this entry on disk.
    pub abs_path: PathBuf,
    /// Path relative to the starting root. (Useful for UI rendering.)
    pub rel_path: PathBuf,
    /// Optional std metadata (symlink-aware if you fetch it yourself). None by default for speed.
    pub metadata: Option<std::fs::Metadata>,
}

/// Walk and invoke `cb` for every entry; discards any outputs the callback returns.
/// Prefer [`walk_dir_stream`] for API endpoints that stream results.
pub async fn walk_dir<F: Clone, Fut>(root: impl AsRef<Path>, opts: WalkOptions, cb: F) -> Result<()>
where
    F: FnMut(WalkEntry) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = CbResult<()>> + Send,
{
    let mut stream = walk_dir_stream(root, opts, move |e| {
        let mut value = cb.clone();
        async move {
            let r: CbResult<()> = value(e).await;
            r
        }
    })?;

    while let Some(_evt) = stream.next().await.transpose()? {}
    Ok(())
}

/// Start walking and **stream** items emitted by the callback.
/// The callback decides control flow (continue/skip/abort) and may optionally return a value to emit.
///
/// Uses `ignore::WalkBuilder` (ripgrep engine): fast, cross-platform, .gitignore-aware.
///
/// Note on `SkipDescend`: we can’t change filters dynamically in the underlying walker,
/// so we keep a shared prefix set. Any path under a pruned prefix is **not** sent to the callback
/// nor emitted. This avoids extra work in your API path, even if the walker may still visit them internally.
pub fn walk_dir_stream<T, F, Fut>(
    root: impl AsRef<Path>,
    mut opts: WalkOptions,
    mut cb: F,
) -> Result<ReceiverStream<Result<T>>>
where
    T: Send + 'static,
    F: FnMut(WalkEntry) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = CbResult<T>> + Send + 'static,
{
    if opts.max_concurrency == 0 {
        opts.max_concurrency = 1;
    }

    let root_abs = root
        .as_ref()
        .canonicalize()
        .unwrap_or_else(|_| root.as_ref().to_path_buf());
    let max_depth_opt = if opts.depth == usize::MAX {
        None
    } else {
        Some(opts.depth)
    };
    let threads = if opts.parallelize_recursion {
        opts.max_concurrency
    } else {
        1
    };

    let (out_tx, out_rx) = mpsc::channel::<Result<T>>(opts.max_concurrency * 4);
    let (in_tx, mut in_rx) = mpsc::channel::<PathBuf>(opts.max_concurrency * 16);

    // Abort & pruned-prefixes shared across producer/consumer.
    let abort = Arc::new(AtomicBool::new(false));
    let pruned: Arc<parking_lot::RwLock<HashSet<PathBuf>>> =
        Arc::new(parking_lot::RwLock::new(HashSet::new()));

    // -----------------
    // Producer (blocking): walk filesystem and push paths into `in_tx`.
    // -----------------
    {
        let abort_p = abort.clone();
        let pruned_p = pruned.clone();
        let root_p = root_abs.clone();
        let in_tx_p = in_tx.clone();
        tokio::task::spawn_blocking(move || {
            let mut b = WalkBuilder::new(&root_p);
            // Respect .gitignore and .ignore files by default.
            b.git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .parents(true)
                .follow_links(true)
                .hidden(!opts.include_hidden)
                .standard_filters(true)
                .max_depth(max_depth_opt)
                .threads(threads);

            let walker = b.build_parallel();

            walker.run(|| {
                let in_tx = in_tx_p.clone();
                let abort = abort_p.clone();
                let pruned = pruned_p.clone();
                Box::new({
                    let value = root_p.clone();
                    move |res| {
                        if abort.load(Ordering::Relaxed) {
                            return WalkState::Quit;
                        }
                        let entry = match res {
                            Ok(e) => e,
                            Err(_) => return WalkState::Continue,
                        };
                        let path = entry.path().to_path_buf();

                        // Filter pruned descendants early (reduce channel chatter)
                        {
                            let guard = pruned.read();
                            if guard.iter().any(|p| path.starts_with(p)) {
                                return WalkState::Continue;
                            }
                        }

                        // Skip the synthetic root entry itself; we usually want its children.
                        // (Keep it if you prefer emitting root as an entry.)
                        if path == value {
                            return WalkState::Continue;
                        }

                        // Send to async consumer; stop if receiver gone.
                        if in_tx.blocking_send(path).is_err() {
                            return WalkState::Quit;
                        }
                        WalkState::Continue
                    }
                })
            });
        });
    }

    // -----------------
    // Consumer (async): apply pruning, run callback, stream outputs.
    // -----------------
    {
        let root_c = root_abs.clone();
        tokio::spawn(async move {
            while let Some(abs) = in_rx.recv().await {
                if abort.load(Ordering::Relaxed) {
                    break;
                }

                // Ignore anything under pruned prefixes (race-safe with producer).
                {
                    let guard = pruned.read();
                    if guard.iter().any(|p| abs.starts_with(p)) {
                        continue;
                    }
                }

                let rel = abs.strip_prefix(&root_c).unwrap_or(&abs).to_path_buf();
                let entry_for_cb = WalkEntry {
                    abs_path: abs.clone(),
                    rel_path: rel.clone(),
                    metadata: None,
                };

                let CbResult { decision, output } = cb(entry_for_cb).await;

                match decision {
                    WalkDecision::Continue => {}
                    WalkDecision::SkipDescend => {
                        // Prune future descendants if this is a directory.
                        if let Ok(m) = tokio::fs::symlink_metadata(&abs).await {
                            if m.is_dir() {
                                pruned.write().insert(abs.clone());
                            }
                        }
                    }
                    WalkDecision::Abort => {
                        abort.store(true, Ordering::Relaxed);
                        break;
                    }
                }

                if let Some(item) = output {
                    if out_tx.send(Ok(item)).await.is_err() {
                        break;
                    }
                }
            }
            // Drop sender to close the stream.
        });
    }

    Ok(ReceiverStream::new(out_rx))
}
