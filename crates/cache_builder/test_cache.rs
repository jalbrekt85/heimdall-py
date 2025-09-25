// Quick test to debug cache issues
use lmdb::{Database, Environment, EnvironmentFlags, Transaction};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cache_dir = PathBuf::from(std::env::var("HOME")? + "/Library/Caches/heimdall");
    let cache_path = cache_dir.join("heimdall_abi_cache.mdb");

    println!("Opening cache at: {:?}", cache_path);

    let env = Environment::new()
        .set_flags(EnvironmentFlags::NO_SUB_DIR)
        .set_map_size(1024 * 1024 * 1024 * 1024) // 1TB
        .set_max_readers(8192)
        .set_max_dbs(1)
        .open(&cache_path)?;

    let db = env.open_db(None)?;

    // Count entries before clear
    let txn = env.begin_ro_txn()?;
    let mut cursor = txn.open_ro_cursor(db)?;
    let mut count = 0;
    for _ in cursor.iter_start() {
        count += 1;
    }
    drop(cursor);
    drop(txn);
    println!("Entries before clear: {}", count);

    // Clear the database
    println!("Clearing database...");
    let mut txn = env.begin_rw_txn()?;
    txn.clear_db(db)?;
    txn.commit()?;

    // Count entries after clear
    let txn = env.begin_ro_txn()?;
    let mut cursor = txn.open_ro_cursor(db)?;
    let mut count_after = 0;
    for _ in cursor.iter_start() {
        count_after += 1;
    }
    drop(cursor);
    drop(txn);
    println!("Entries after clear: {}", count_after);

    Ok(())
}