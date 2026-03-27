pub mod fingerprint;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use self::fingerprint::{compute_hash, metadata_matches};
use crate::model::FileIndex;

const CACHE_VERSION: u32 = 3;
const CACHE_FILENAME: &str = "cache.bin";

#[derive(Serialize, Deserialize)]
struct CacheStore {
    version: u32,
    entries: HashMap<PathBuf, CacheEntry>,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    mtime: u64,
    size: u64,
    content_hash: u64,
    file_index: FileIndex,
}

pub struct Cache {
    store: CacheStore,
    cache_dir: PathBuf,
    dirty: bool,
}

impl Cache {
    /// Load cache from disk, or create empty if not found / incompatible.
    pub fn load(cache_dir: &Path) -> Self {
        let cache_path = cache_dir.join(CACHE_FILENAME);
        let store = if cache_path.exists() {
            match fs::read(&cache_path) {
                Ok(data) => match bincode::deserialize::<CacheStore>(&data) {
                    Ok(store) if store.version == CACHE_VERSION => store,
                    _ => Self::empty_store(),
                },
                Err(_) => Self::empty_store(),
            }
        } else {
            Self::empty_store()
        };

        Cache {
            store,
            cache_dir: cache_dir.to_path_buf(),
            dirty: false,
        }
    }

    /// Create a no-op cache that never hits and never saves.
    pub fn disabled() -> Self {
        Cache {
            store: Self::empty_store(),
            cache_dir: PathBuf::new(),
            dirty: false,
        }
    }

    fn empty_store() -> CacheStore {
        CacheStore {
            version: CACHE_VERSION,
            entries: HashMap::new(),
        }
    }

    /// Try to get a cached FileIndex for a file. Returns Some if the
    /// file hasn't changed (based on mtime + size).
    pub fn get(&self, relative_path: &Path, size: u64, mtime: u64) -> Option<FileIndex> {
        let entry = self.store.entries.get(relative_path)?;
        if metadata_matches(entry.mtime, entry.size, mtime, size) {
            Some(entry.file_index.clone())
        } else {
            None
        }
    }

    /// Insert or update a cache entry for a file.
    pub fn insert(
        &mut self,
        relative_path: &Path,
        size: u64,
        mtime: u64,
        content: &[u8],
        file_index: FileIndex,
    ) {
        let content_hash = compute_hash(content);
        self.store.entries.insert(
            relative_path.to_path_buf(),
            CacheEntry {
                mtime,
                size,
                content_hash,
                file_index,
            },
        );
        self.dirty = true;
    }

    /// Remove entries for files that no longer exist.
    pub fn prune(&mut self, existing_paths: &[PathBuf]) {
        let existing: std::collections::HashSet<&PathBuf> = existing_paths.iter().collect();
        let before = self.store.entries.len();
        self.store.entries.retain(|path, _| existing.contains(path));
        if self.store.entries.len() != before {
            self.dirty = true;
        }
    }

    /// Save cache to disk if it has been modified.
    pub fn save(&self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if self.cache_dir.as_os_str().is_empty() {
            return Ok(()); // disabled cache
        }
        fs::create_dir_all(&self.cache_dir)?;
        let data = bincode::serialize(&self.store)?;
        fs::write(self.cache_dir.join(CACHE_FILENAME), data)?;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.store.entries.len()
    }
}
