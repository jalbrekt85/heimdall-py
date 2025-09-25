mod cache;
mod parquet_reader;
mod processor;
mod stats;
mod stream_processor;
mod types;

use cache::AbiCache;
use clap::Parser;
use colored::Colorize;
use eyre::Result;
use parquet_reader::ParquetReader;
use processor::ABANDONED_THREADS;
use stats::Stats;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

#[derive(Parser, Debug)]
#[clap(name = "heimdall-cache-builder")]
#[clap(about = "High-performance cache builder for Heimdall ABI decompilation")]
struct Args {
    /// Directory containing parquet files
    #[clap(short, long, default_value = "parquets")]
    parquet_dir: PathBuf,

    /// Cache directory (defaults to system cache)
    #[clap(short, long)]
    cache_dir: Option<PathBuf>,

    /// Number of worker threads (defaults to number of CPU cores)
    #[clap(short = 'w', long)]
    workers: Option<usize>,

    /// Decompilation timeout in seconds
    #[clap(short = 't', long, default_value = "25")]
    timeout: u64,

    /// Skip resolving function signatures
    #[clap(short = 's', long, default_value = "true")]
    skip_resolving: bool,

    /// Extract storage layout (slower but more complete)
    #[clap(short = 'e', long, default_value = "true")]
    extract_storage: bool,

    /// Update interval for progress display in milliseconds
    #[clap(short = 'u', long, default_value = "500")]
    update_interval: u64,


    /// Verbose output
    #[clap(short, long)]
    verbose: bool,

    /// Debug mode - just inspect cache without processing
    #[clap(long)]
    debug_cache: bool,

    /// Clear cache before processing
    #[clap(long)]
    clear_cache: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing
    if args.verbose || std::env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("heimdall_cache_builder=debug".parse()?)
                    .add_directive("heimdall_decompiler=info".parse()?),
            )
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter("heimdall_cache_builder=info")
            .init();
    }

    // Print banner
    println!("\n{}", "=== Heimdall Cache Builder ===".bright_cyan().bold());
    println!("Maximizing performance on your 14-core M4 Pro system\n");

    // Debug mode - just inspect cache
    if args.debug_cache {
        let cache = Arc::new(AbiCache::new(args.cache_dir.clone())?);
        println!("Debug mode: Inspecting cache...\n");

        // Load one contract to test
        let contracts = ParquetReader::read_contracts(&args.parquet_dir.join("ethereum__contracts__14869832_to_14870831.parquet"))?;

        if let Some(contract) = contracts.first() {
            println!("Testing with first contract from parquet:");
            println!("  Address: {}", &contract.address[..10]);
            println!("  Code length: {}", contract.code.len());

            // Check if it exists in cache
            let exists = cache.exists(&contract.code, args.skip_resolving);
            println!("  Exists in cache: {}", exists);

            // Generate the cache key and show it
            let clean_bytecode = contract.code.strip_prefix("0x").unwrap_or(&contract.code);
            let hash = blake3::hash(clean_bytecode.as_bytes());
            let suffix = if args.skip_resolving { "_unresolved" } else { "_resolved" };
            println!("  Blake3 hash: {}", hash.to_hex());
            println!("  Cache key suffix: {}", suffix);
        }

        println!("\n{}", cache.get_stats_summary());
        return Ok(());
    }

    // Initialize cache
    let cache = Arc::new(AbiCache::new(args.cache_dir.clone())?);
    println!(
        "{}",
        format!(
            "Cache directory: {}",
            args.cache_dir
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "System default".to_string())
        )
        .green()
    );

    // Clear cache if requested
    if args.clear_cache {
        println!("{}", "Clearing cache...".yellow());
        cache.clear()?;
        println!("{}", "Cache cleared successfully".green());
    }

    // Initialize stats
    let stats = Stats::new();

    // Determine number of workers
    let num_workers = args
        .workers
        .unwrap_or_else(|| num_cpus::get());

    println!(
        "\n{}: streaming parquet files with {} workers",
        "Processing".bright_blue().bold(),
        num_workers
    );
    println!(
        "Settings: timeout={}s, skip_resolving={}, extract_storage={}",
        args.timeout, args.skip_resolving, args.extract_storage
    );

    // Use the stream processor for all processing
    let processor = stream_processor::StreamProcessor::new(
        cache.clone(),
        stats.clone(),
        num_workers,
        args.timeout,
        args.skip_resolving,
        args.extract_storage,
    );

    let start = Instant::now();
    processor.process_all_parquets(&args.parquet_dir)?;
    let duration = start.elapsed();

    // Print final statistics
    println!("\n{}", stats.get_final_summary().bright_green());
    println!("Total time: {:.2}s", duration.as_secs_f64());

    // Print cache statistics
    println!("{}", "Cache Statistics:".bright_cyan());
    println!("{}", cache.get_stats_summary());

    // Check for abandoned threads
    let abandoned = ABANDONED_THREADS.load(Ordering::Relaxed);
    if abandoned > 0 {
        println!(
            "\n{}",
            format!(
                "WARNING: {} threads were abandoned due to timeouts",
                abandoned
            )
            .yellow()
        );
    }

    println!("\n{}", "✅ Cache building complete!".green().bold());

    Ok(())
}