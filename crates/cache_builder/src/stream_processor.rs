use crate::cache::AbiCache;
use crate::parquet_reader::Contract;
use crate::processor::{ContractProcessor, ABANDONED_THREADS};
use crate::stats::Stats;
use crossbeam_channel::{bounded, Receiver, Sender};
use eyre::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tracing::{debug, error, info, warn};

const BATCH_SIZE: usize = 1000; // Process files in batches
const CHANNEL_BUFFER: usize = 10000; // Buffer for work queue

pub struct StreamProcessor {
    cache: Arc<AbiCache>,
    stats: Arc<Stats>,
    workers: usize,
    timeout_secs: u64,
    skip_resolving: bool,
    extract_storage: bool,
}

impl StreamProcessor {
    pub fn new(
        cache: Arc<AbiCache>,
        stats: Arc<Stats>,
        workers: usize,
        timeout_secs: u64,
        skip_resolving: bool,
        extract_storage: bool,
    ) -> Self {
        StreamProcessor {
            cache,
            stats,
            workers,
            timeout_secs,
            skip_resolving,
            extract_storage,
        }
    }

    pub fn process_all_parquets(&self, parquet_dir: &Path) -> Result<()> {
        // Find all parquet files
        let parquet_files = self.find_parquet_files(parquet_dir)?;
        let total_files = parquet_files.len();

        if total_files == 0 {
            warn!("No parquet files found in {:?}", parquet_dir);
            return Ok(());
        }

        info!("Found {} parquet files to process", total_files);

        // Set up progress bars
        let multi_progress = MultiProgress::new();
        let file_progress = multi_progress.add(ProgressBar::new(total_files as u64));
        file_progress.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] Files: {bar:40.cyan/blue} {pos}/{len} ({per_sec}) {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );

        let contract_progress = multi_progress.add(ProgressBar::new(0));
        contract_progress.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] Contracts: {bar:40.green/blue} {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );

        // Channels for streaming contracts to workers
        let (sender, receiver): (Sender<Contract>, Receiver<Contract>) = bounded(CHANNEL_BUFFER);

        // Shared state for deduplication
        let seen_bytecodes = Arc::new(Mutex::new(HashSet::new()));
        let unique_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let duplicate_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // Spawn reader thread that streams contracts from parquet files
        let reader_handle = {
            let sender = sender.clone();
            let seen_bytecodes = seen_bytecodes.clone();
            let file_progress = file_progress.clone();
            let contract_progress = contract_progress.clone();
            let cache = self.cache.clone();
            let unique_count = unique_count.clone();
            let duplicate_count = duplicate_count.clone();
            let skip_resolving = self.skip_resolving;

            thread::spawn(move || {
                let mut total_contracts = 0usize;
                let mut processed_files = 0usize;

                for (batch_idx, file_batch) in parquet_files.chunks(BATCH_SIZE).enumerate() {
                    debug!("Processing batch {} ({} files)", batch_idx, file_batch.len());

                    for file_path in file_batch {
                        // Try to read the parquet file
                        match crate::parquet_reader::ParquetReader::read_contracts(file_path) {
                            Ok(contracts) => {
                                let file_contract_count = contracts.len();
                                total_contracts += file_contract_count;

                                // Stream each contract through deduplication
                                for contract in contracts {
                                    // Check if we've seen this bytecode before
                                    let is_duplicate = {
                                        let mut seen = seen_bytecodes.lock().unwrap();
                                        !seen.insert(contract.code.clone())
                                    };

                                    if is_duplicate {
                                        duplicate_count.fetch_add(1, Ordering::Relaxed);
                                        continue;
                                    }

                                    // Check if it's already in cache
                                    if cache.exists(&contract.code, skip_resolving) {
                                        duplicate_count.fetch_add(1, Ordering::Relaxed);
                                        continue;
                                    }

                                    // Send unique contract to workers
                                    unique_count.fetch_add(1, Ordering::Relaxed);
                                    if sender.send(contract).is_err() {
                                        warn!("Worker channels closed, stopping reader");
                                        return;
                                    }
                                }

                                processed_files += 1;
                                file_progress.set_position(processed_files as u64);
                                file_progress.set_message(format!(
                                    "{} unique, {} duplicates",
                                    unique_count.load(Ordering::Relaxed),
                                    duplicate_count.load(Ordering::Relaxed)
                                ));

                                contract_progress.set_length(total_contracts as u64);
                                contract_progress.set_position(
                                    (unique_count.load(Ordering::Relaxed) +
                                     duplicate_count.load(Ordering::Relaxed)) as u64
                                );
                            }
                            Err(e) => {
                                warn!("Failed to read {:?}: {}", file_path, e);
                                processed_files += 1;
                                file_progress.set_position(processed_files as u64);
                            }
                        }

                        // Periodically clear seen_bytecodes to prevent unbounded growth
                        // We rely on cache to prevent reprocessing
                        if processed_files % 1000 == 0 {
                            let mut seen = seen_bytecodes.lock().unwrap();
                            if seen.len() > 1_000_000 {
                                debug!("Clearing seen bytecodes set (had {} entries)", seen.len());
                                seen.clear();
                            }
                        }
                    }
                }

                info!(
                    "Reader finished: {} files, {} contracts ({} unique, {} duplicates)",
                    processed_files,
                    total_contracts,
                    unique_count.load(Ordering::Relaxed),
                    duplicate_count.load(Ordering::Relaxed)
                );
            })
        };

        // Drop the original sender so workers know when to stop
        drop(sender);

        // Process contracts in parallel using Rayon
        let cache = self.cache.clone();
        let stats = self.stats.clone();
        let timeout = self.timeout_secs;
        let skip_resolving = self.skip_resolving;
        let extract_storage = self.extract_storage;

        // Set up Rayon thread pool
        rayon::ThreadPoolBuilder::new()
            .num_threads(self.workers)
            .thread_name(|i| format!("worker-{}", i))
            .build()
            .unwrap()
            .install(|| {
                // Process contracts from the channel
                receiver.into_iter().par_bridge().for_each(|contract| {
                    // Create thread-local Tokio runtime
                    thread_local! {
                        static RUNTIME: Runtime = Runtime::new().expect("Failed to create runtime");
                    }

                    RUNTIME.with(|rt| {
                        let processor = ContractProcessor::new(
                            cache.clone(),
                            timeout,
                            skip_resolving,
                            extract_storage,
                        );

                        let result = rt.block_on(async {
                            processor.process_contract(contract.address, contract.code).await
                        });

                        match result {
                            Ok(process_result) => {
                                let is_timeout = process_result
                                    .error
                                    .as_ref()
                                    .map(|e| e.contains("timed out"))
                                    .unwrap_or(false);

                                stats.record_result(
                                    process_result.cached,
                                    process_result.success,
                                    is_timeout,
                                    process_result.duration,
                                );

                                contract_progress.inc(1);

                                if let Some(error) = process_result.error {
                                    debug!("Contract {} failed: {}",
                                           &process_result.address[..10.min(process_result.address.len())],
                                           error);
                                }
                            }
                            Err(e) => {
                                error!("Failed to process contract: {}", e);
                                stats.record_result(false, false, false, Duration::ZERO);
                                contract_progress.inc(1);
                            }
                        }
                    });
                });
            });

        // Wait for reader thread
        reader_handle.join().expect("Reader thread panicked");

        // Clear progress bars
        file_progress.finish_with_message("Complete");
        contract_progress.finish_with_message("Complete");

        Ok(())
    }

    fn find_parquet_files(&self, directory: &Path) -> Result<Vec<PathBuf>> {
        let mut parquet_files = Vec::new();

        if !directory.exists() {
            return Err(eyre::eyre!("Directory does not exist: {:?}", directory));
        }

        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("parquet") {
                parquet_files.push(path);
            }
        }

        // Sort for consistent ordering
        parquet_files.sort();

        Ok(parquet_files)
    }
}