//! The [`Index`]: an opened workspace, and the options to open one
//! (`SPEC.md` §4.7).

use crate::config::Config;
use crate::error::{Error, Result};
use crate::lock;
use crate::service::SearchService;
use graph_search_core::config::WalkPolicy;
use graph_search_core::ports::GraphStore;
use graph_search_core::reconcile::Projector;
use graph_search_core::walk::resolve_search_root;
use graph_search_langs::all_extractors;
use graph_search_types::result::SyncReport;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// How eager a query is about freshness (`SPEC.md` §4.7, §6.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reconcile {
    /// Never reconcile in a query; results carry the stale flag.
    Never,
    /// Reconcile before answering, when the cheap scan sees drift. The
    /// default: forgetting `sync` is safe, not silently wrong.
    #[default]
    BeforeQuery,
    /// Only `sync`/`reindex` reconcile (queries never check? they do, but
    /// never mutate).
    Explicit,
}

/// How query freshness checks compare the live tree with indexed sources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Verification {
    /// Check inclusion, size, modification time, and parser versions.
    #[default]
    Metadata,
    /// Hash file contents too, detecting same-size edits with restored mtimes.
    Content,
}

/// How to open an index.
#[derive(Clone, Debug)]
pub struct OpenOptions {
    /// The workspace root.
    pub root: PathBuf,
    /// The store directory; the default is `<root>/.graph-search/index`.
    pub store: Option<PathBuf>,
    /// Extra excludes, appended to the configuration's.
    pub excludes: Vec<String>,
    /// Languages enabled for extraction.
    pub languages: Option<Vec<graph_search_types::Language>>,
    /// How queries treat staleness.
    pub reconcile: Reconcile,
    /// Strength of filesystem freshness verification.
    pub verification: Verification,
    /// Refuse to build or mutate.
    pub read_only: bool,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            store: None,
            excludes: Vec::new(),
            languages: None,
            reconcile: Reconcile::default(),
            verification: Verification::default(),
            read_only: false,
        }
    }
}

/// The process-wide language registry, seen through core's port.
struct RegistryList {
    extractors: &'static [Box<dyn graph_search_core::ports::LanguageExtractor>],
}

impl graph_search_core::ports::LanguageRegistry for RegistryList {
    fn typescript_config(
        &self,
        file: &graph_search_core::ports::SourceFile<'_>,
    ) -> Option<graph_search_types::typescript::TypeScriptConfig> {
        crate::typescript_config::extract(file)
    }

    fn package_manifest(
        &self,
        file: &graph_search_core::ports::SourceFile<'_>,
    ) -> Option<graph_search_types::package::PackageManifest> {
        crate::package_manifests::extract(file)
    }

    fn extractor_for(
        &self,
        path: &Path,
    ) -> Option<&dyn graph_search_core::ports::LanguageExtractor> {
        self.extractors
            .iter()
            .map(std::convert::AsRef::as_ref)
            .find(|extractor| extractor.supports(path))
    }
}

/// An opened workspace index, held for as long as it is queried.
///
/// Store callbacks hold a shared or exclusive lock for the whole operation.
/// Snapshots borrow the guarded store; queries do not release the guard before
/// materializing results. `GraphStore: Send` does not require `Sync`, so this
/// handle does not promise concurrent shared access across threads (`SPEC.md` §6.6).
///
/// ```compile_fail
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<graph_search::Index>();
/// ```
pub struct Index {
    root: PathBuf,
    store_dir: PathBuf,
    policy: WalkPolicy,
    reconcile: Reconcile,
    verification: Verification,
    read_only: bool,
    store: RwLock<Box<dyn GraphStore>>,
    /// Whether the index has been verified fresh *in this process* — the
    /// resident fast path for `files` (`SPEC.md` §4.6).
    fresh: AtomicBool,
}

impl Index {
    /// Opens (creating, unless `read_only`) the workspace index.
    ///
    /// # Errors
    /// When the root is missing, the configuration is bad, or the store
    /// cannot be opened.
    pub fn open(options: OpenOptions) -> Result<Self> {
        let root = options
            .root
            .canonicalize()
            .map_err(|source| Error::Core(graph_search_core::Error::io(&options.root, source)))?;
        if !root.is_dir() {
            return Err(Error::Core(graph_search_core::Error::RootMissing { root }));
        }
        let mut config = Config::load(&root)?;
        config.policy.excludes.extend(options.excludes);
        if let Some(languages) = options.languages {
            config.policy.languages = languages;
        }
        let store_dir =
            resolve_store_path(&options.store.unwrap_or_else(|| root.join(&config.store)))?;
        if root.starts_with(&store_dir) {
            return Err(Error::Config(String::from(
                "the store directory must not equal or contain the source root",
            )));
        }
        config.policy.excluded_paths.push(store_dir.clone());
        if !options.read_only {
            std::fs::create_dir_all(&store_dir)
                .map_err(|source| Error::Core(graph_search_core::Error::io(&store_dir, source)))?;
        }
        let store = graph_search_engine::GrafeoStore::open(
            &store_dir,
            &graph_search_engine::StoreOptions {
                read_only: options.read_only,
                ..graph_search_engine::StoreOptions::default()
            },
        )
        .map_err(Error::Core)?;
        Ok(Self {
            root,
            store_dir,
            policy: config.policy,
            reconcile: options.reconcile,
            verification: options.verification,
            read_only: options.read_only,
            store: RwLock::new(Box::new(store)),
            fresh: AtomicBool::new(false),
        })
    }

    /// The workspace root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The store directory.
    #[must_use]
    pub fn store_dir(&self) -> &Path {
        &self.store_dir
    }

    /// The walk policy in force.
    #[must_use]
    pub const fn policy(&self) -> &WalkPolicy {
        &self.policy
    }

    /// The staleness posture.
    #[must_use]
    pub const fn reconcile(&self) -> Reconcile {
        self.reconcile
    }

    /// The strength of filesystem freshness checks.
    #[must_use]
    pub const fn verification(&self) -> Verification {
        self.verification
    }

    /// Whether the index was opened read-only.
    #[must_use]
    pub const fn read_only(&self) -> bool {
        self.read_only
    }

    /// The query handle.
    #[must_use]
    pub fn search(&self) -> SearchService<'_> {
        SearchService::new(self)
    }

    /// Reads through the store under the shared lock.
    pub(crate) fn store_read<R>(
        &self,
        read: impl FnOnce(&dyn GraphStore) -> Result<R>,
    ) -> Result<R> {
        match self.store.read() {
            Ok(guard) => read(guard.as_ref()),
            Err(_) => Err(Error::poisoned()),
        }
    }

    /// Mutates the store under the exclusive lock.
    pub(crate) fn store_write<R>(
        &self,
        write: impl FnOnce(&mut dyn GraphStore) -> Result<R>,
    ) -> Result<R> {
        match self.store.write() {
            Ok(mut guard) => write(guard.as_mut()),
            Err(_) => Err(Error::poisoned()),
        }
    }

    /// Whether the index has been verified fresh in this process.
    #[must_use]
    pub fn is_fresh(&self) -> bool {
        self.fresh.load(Ordering::Relaxed)
    }

    fn mark_fresh(&self) {
        self.fresh.store(true, Ordering::Relaxed);
    }

    /// A projector over this index's registry and policy. The registry is
    /// process-wide and built once; the policy is this index's.
    pub(crate) fn projector(&self) -> Projector<'_> {
        static REGISTRY: std::sync::OnceLock<
            Vec<Box<dyn graph_search_core::ports::LanguageExtractor>>,
        > = std::sync::OnceLock::new();
        static LIST: std::sync::OnceLock<RegistryList> = std::sync::OnceLock::new();
        let list = LIST.get_or_init(|| RegistryList {
            extractors: REGISTRY.get_or_init(all_extractors).as_slice(),
        });
        Projector::new(list, &self.policy)
    }

    /// Full build: parse everything, build from scratch (`SPEC.md` §6.5.1).
    ///
    /// # Errors
    /// When the index is read-only, the lock is held, or the reconcile fails.
    pub fn reindex(&self) -> Result<SyncReport> {
        if self.read_only {
            return Err(Error::ReadOnly);
        }
        let _lock = lock::try_lock(&self.store_dir)?;
        let search_root = resolve_search_root(&self.root, None).map_err(Error::Core)?;
        let report = self.store_write(|store| {
            self.projector()
                .reindex(&search_root, store)
                .map_err(Error::Core)
        })?;
        self.mark_fresh();
        Ok(report)
    }

    pub(crate) fn maintain_with_work(
        &self,
        reindex: bool,
        work: &mut graph_search_core::work::WorkBudget,
    ) -> Result<SyncReport> {
        work.check().map_err(Error::Core)?;
        if self.read_only {
            return Err(Error::ReadOnly);
        }
        let _lock = lock::try_lock(&self.store_dir)?;
        let search_root = resolve_search_root(&self.root, None).map_err(Error::Core)?;
        let report = self.store_write(|store| {
            let projector = self.projector().with_work_budget(work);
            if reindex {
                projector.reindex(&search_root, store)
            } else {
                projector.sync(&search_root, store)
            }
            .map_err(Error::Core)
        })?;
        self.mark_fresh();
        Ok(report)
    }

    /// Incremental reconcile; the normal way to keep current
    /// (`SPEC.md` §6.5.2).
    ///
    /// # Errors
    /// When the index is read-only, the lock is held, or the reconcile fails.
    pub fn sync(&self) -> Result<SyncReport> {
        if self.read_only {
            return Err(Error::ReadOnly);
        }
        let _lock = lock::try_lock(&self.store_dir)?;
        let search_root = resolve_search_root(&self.root, None).map_err(Error::Core)?;
        let report = self.store_write(|store| {
            self.projector()
                .sync(&search_root, store)
                .map_err(Error::Core)
        })?;
        self.mark_fresh();
        Ok(report)
    }
}

/// Resolve existing symlinks while allowing a not-yet-created store. Explicit
/// relative store options retain their process-working-directory semantics.
fn resolve_store_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .map_err(|source| Error::Core(graph_search_core::Error::io(path, source)))?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        if component == std::path::Component::ParentDir {
            resolved.pop();
        } else if component != std::path::Component::CurDir {
            resolved.push(component);
            match resolved.canonicalize() {
                Ok(canonical) => resolved = canonical,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(graph_search_core::Error::io(&resolved, error).into()),
            }
        }
    }
    Ok(resolved)
}
