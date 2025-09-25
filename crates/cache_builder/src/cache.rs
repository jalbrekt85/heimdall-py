use blake3;
use eyre::Result;
use lmdb::{Database, Environment, EnvironmentFlags, Transaction, WriteFlags};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::{env, fs};

use crate::types::ABI;

// Statistics tracking
pub struct CacheStats {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub writes: AtomicU64,
    pub errors: AtomicU64,
}

impl CacheStats {
    pub fn new() -> Self {
        CacheStats {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            writes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }
}

#[derive(Clone)]
pub struct AbiCache {
    env: Arc<Environment>,
    db: Database,
    pub stats: Arc<CacheStats>,
}

impl AbiCache {
    pub fn new(directory: Option<PathBuf>) -> Result<Self> {
        let cache_dir = directory.unwrap_or_else(get_default_cache_dir);

        fs::create_dir_all(&cache_dir)?;

        let cache_path = cache_dir.join("heimdall_abi_cache.mdb");

        // Use same configuration as Python bindings
        let env = Environment::new()
            .set_flags(EnvironmentFlags::NO_SUB_DIR)
            .set_map_size(1024 * 1024 * 1024 * 1024) // 1TB map size - just virtual address space
            .set_max_readers(8192)
            .set_max_dbs(1)
            .open(&cache_path)?;

        let db = env.open_db(None)?;

        Ok(AbiCache {
            env: Arc::new(env),
            db,
            stats: Arc::new(CacheStats::new()),
        })
    }

    // Generate cache key exactly as Python bindings do
    fn generate_cache_key(bytecode: &str, skip_resolving: bool) -> Vec<u8> {
        let clean_bytecode = bytecode.strip_prefix("0x").unwrap_or(bytecode);
        let hash = blake3::hash(clean_bytecode.as_bytes());
        let suffix = if skip_resolving {
            "_unresolved"
        } else {
            "_resolved"
        };

        let mut key = hash.as_bytes().to_vec();
        key.extend_from_slice(suffix.as_bytes());
        key
    }

    pub fn exists(&self, bytecode: &str, skip_resolving: bool) -> bool {
        let key = Self::generate_cache_key(bytecode, skip_resolving);

        match self.env.begin_ro_txn() {
            Ok(txn) => {
                let result = txn.get(self.db, &key).is_ok();
                if result {
                    self.stats.hits.fetch_add(1, Ordering::Relaxed);
                } else {
                    self.stats.misses.fetch_add(1, Ordering::Relaxed);
                }
                result
            }
            Err(_) => {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    pub fn put(&self, bytecode: &str, skip_resolving: bool, abi: &ABI) -> Result<()> {
        let key = Self::generate_cache_key(bytecode, skip_resolving);

        // Serialize exactly as Python bindings do
        let serialized = bincode::serialize(abi)?;

        let mut txn = self.env.begin_rw_txn()?;
        txn.put(self.db, &key, &serialized, WriteFlags::empty())?;
        txn.commit()?;

        self.stats.writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    // Batch write for better performance
    pub fn put_batch(&self, items: Vec<(String, bool, ABI)>) -> Result<()> {
        let mut txn = self.env.begin_rw_txn()?;

        for (bytecode, skip_resolving, abi) in items {
            let key = Self::generate_cache_key(&bytecode, skip_resolving);
            let serialized = bincode::serialize(&abi)?;
            txn.put(self.db, &key, &serialized, WriteFlags::empty())?;
            self.stats.writes.fetch_add(1, Ordering::Relaxed);
        }

        txn.commit()?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        eprintln!("DEBUG: Clearing cache database...");

        let mut txn = self.env.begin_rw_txn()?;
        txn.clear_db(self.db)?;
        txn.commit()?;

        eprintln!("DEBUG: Cache cleared");

        // Reset stats
        self.stats.hits.store(0, Ordering::Relaxed);
        self.stats.misses.store(0, Ordering::Relaxed);
        self.stats.writes.store(0, Ordering::Relaxed);
        self.stats.errors.store(0, Ordering::Relaxed);

        Ok(())
    }

    pub fn get_stats_summary(&self) -> String {
        let hits = self.stats.hits.load(Ordering::Relaxed);
        let misses = self.stats.misses.load(Ordering::Relaxed);
        let writes = self.stats.writes.load(Ordering::Relaxed);
        let errors = self.stats.errors.load(Ordering::Relaxed);

        let total_requests = hits + misses;
        let hit_rate = if total_requests > 0 {
            (hits as f64 / total_requests as f64) * 100.0
        } else {
            0.0
        };

        format!(
            "Cache: {} hits, {} misses ({:.1}% hit rate), {} writes, {} errors",
            hits, misses, hit_rate, writes, errors
        )
    }
}

// Use exact same cache directory logic as Python bindings
fn get_default_cache_dir() -> PathBuf {
    if let Ok(dir) = env::var("HEIMDALL_CACHE_DIR") {
        return PathBuf::from(dir);
    }

    #[cfg(target_os = "macos")]
    {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join("Library/Caches/heimdall")
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(cache_home) = env::var("XDG_CACHE_HOME") {
            PathBuf::from(cache_home).join("heimdall")
        } else {
            let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".cache/heimdall")
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            PathBuf::from(local_app_data).join("heimdall\\cache")
        } else {
            let temp = env::var("TEMP").unwrap_or_else(|_| "C:\\Temp".to_string());
            PathBuf::from(temp).join("heimdall\\cache")
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        PathBuf::from("/tmp/heimdall_cache")
    }
}