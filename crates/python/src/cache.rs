use blake3;
use lmdb::{Database, Environment, EnvironmentFlags, Transaction, WriteFlags};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

lazy_static::lazy_static! {
    static ref CACHE: RwLock<Option<Arc<AbiCache>>> = RwLock::new(None);
    static ref CACHE_STATS: Mutex<CacheStats> = Mutex::new(CacheStats::default());
}

#[derive(Default, Debug, Clone)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub writes: u64,
    pub errors: u64,
}

#[derive(Clone)]
pub struct AbiCache {
    env: Arc<Environment>,
    db: Database,
    enabled: bool,
}

impl AbiCache {
    pub fn init(directory: Option<PathBuf>, enabled: bool) -> Result<(), String> {
        {
            let cache = CACHE.read().unwrap();
            if cache.is_some() {
                return Ok(());
            }
        }

        if !enabled {
            let mut cache = CACHE.write().unwrap();
            *cache = None;
            return Ok(());
        }

        let cache_dir = directory.unwrap_or_else(get_default_cache_dir);

        fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;

        let cache_path = cache_dir.join("heimdall_abi_cache.mdb");

        let env = Environment::new()
            .set_flags(EnvironmentFlags::NO_SUB_DIR)
            .set_map_size(1024 * 1024 * 1024 * 1024)  // 1TB map size - just virtual address space
            .set_max_readers(8192)
            .set_max_dbs(1)
            .open(&cache_path)
            .map_err(|e| format!("Failed to open LMDB environment: {}", e))?;

        let db = env.open_db(None)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        let cache_instance = AbiCache {
            env: Arc::new(env),
            db,
            enabled
        };

        let mut cache = CACHE.write().unwrap();
        *cache = Some(Arc::new(cache_instance));

        Ok(())
    }

    fn generate_cache_key(bytecode: &str, skip_resolving: bool) -> Vec<u8> {
        let clean_bytecode = bytecode.strip_prefix("0x").unwrap_or(bytecode);
        let hash = blake3::hash(clean_bytecode.as_bytes());
        let suffix = if skip_resolving { "_unresolved" } else { "_resolved" };

        let mut key = hash.as_bytes().to_vec();
        key.extend_from_slice(suffix.as_bytes());
        key
    }

    pub fn get(bytecode: &str, skip_resolving: bool) -> Option<Vec<u8>> {
        let cache_handle = {
            let cache_guard = CACHE.read().unwrap();
            cache_guard.as_ref()?.clone()
        };

        if !cache_handle.enabled {
            return None;
        }

        let key = Self::generate_cache_key(bytecode, skip_resolving);

        let txn = cache_handle.env.begin_ro_txn().ok()?;
        let result = txn.get(cache_handle.db, &key).ok().map(|data| data.to_vec());

        let mut stats = CACHE_STATS.lock().unwrap();
        if result.is_some() {
            stats.hits += 1;
        } else {
            stats.misses += 1;
        }

        result
    }

    pub fn put(bytecode: &str, skip_resolving: bool, abi_data: &[u8]) -> Result<(), String> {
        let cache_handle = {
            let cache_guard = CACHE.read().unwrap();
            cache_guard.as_ref()
                .ok_or_else(|| "Cache not initialized".to_string())?
                .clone()
        };

        if !cache_handle.enabled {
            return Ok(());
        }

        let key = Self::generate_cache_key(bytecode, skip_resolving);

        let mut txn = cache_handle.env.begin_rw_txn()
            .map_err(|e| format!("Failed to begin write transaction: {}", e))?;

        txn.put(cache_handle.db, &key, &abi_data, WriteFlags::empty())
            .map_err(|e| format!("Failed to write to cache: {}", e))?;

        txn.commit()
            .map_err(|e| format!("Failed to commit transaction: {}", e))?;

        let mut stats = CACHE_STATS.lock().unwrap();
        stats.writes += 1;

        Ok(())
    }

    pub fn clear() -> Result<(), String> {
        let cache_handle = {
            let cache_guard = CACHE.read().unwrap();
            cache_guard.as_ref()
                .ok_or_else(|| "Cache not initialized".to_string())?
                .clone()
        };

        let mut txn = cache_handle.env.begin_rw_txn()
            .map_err(|e| format!("Failed to begin write transaction: {}", e))?;

        txn.clear_db(cache_handle.db)
            .map_err(|e| format!("Failed to clear cache: {}", e))?;

        txn.commit()
            .map_err(|e| format!("Failed to commit transaction: {}", e))?;

        let mut stats = CACHE_STATS.lock().unwrap();
        *stats = CacheStats::default();

        Ok(())
    }

    pub fn get_stats() -> CacheStats {
        CACHE_STATS.lock().unwrap().clone()
    }

    pub fn is_enabled() -> bool {
        let cache_guard = CACHE.read().unwrap();
        cache_guard.as_ref().map(|c| c.enabled).unwrap_or(false)
    }
}

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