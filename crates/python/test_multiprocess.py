#!/usr/bin/env python3
"""Final multiprocess test using Pool.map for simplicity"""

import multiprocessing as mp
import time
import tempfile
import glob
import os
import pyarrow.parquet as pq
from heimdall_py import decompile_code, configure_cache, clear_cache, get_cache_stats
from collections import Counter

def process_batch(args):
    """Process a batch of contracts"""
    batch_id, contracts, cache_dir = args

    try:
        print(f"  Batch {batch_id}: Starting with {len(contracts)} contracts", flush=True)

        configure_cache(enabled=True, directory=cache_dir)

        processed = 0
        errors = 0
        error_details = []
        timeouts = 0
        start_time = time.time()

        for i, (contract_address, code) in enumerate(contracts):
            try:
                # Convert bytes to hex if needed
                if isinstance(code, bytes):
                    code = code.hex()
                if isinstance(contract_address, bytes):
                    contract_address = contract_address.hex()

                # Print progress for debugging
                if processed % 50 == 0:
                    print(f"  Batch {batch_id}: Processed {processed}/{len(contracts)} contracts", flush=True)

                # Decompile with timeout tracking
                if i == 150 and batch_id in [2, 6]:
                    print(f"  Batch {batch_id}: About to decompile contract {i}: {contract_address[:10]}...", flush=True)
                if i == 250 and batch_id == 10:
                    print(f"  Batch {batch_id}: About to decompile contract {i}: {contract_address[:10]}...", flush=True)
                if i == 300 and batch_id == 3:
                    print(f"  Batch {batch_id}: About to decompile contract {i}: {contract_address[:10]}...", flush=True)

                decompile_start = time.time()
                abi = decompile_code(code, skip_resolving=True)
                decompile_time = time.time() - decompile_start

                if decompile_time > 5:  # Log slow decompilations
                    print(f"  Batch {batch_id}: SLOW decompile ({decompile_time:.1f}s) for {contract_address[:10]}...", flush=True)

                processed += 1

                if abi.decompile_error:
                    errors += 1
                    error_details.append(abi.decompile_error[:100])
                    # Check if it's a timeout error
                    if "timed out" in abi.decompile_error.lower():
                        timeouts += 1
                        print(f"  Batch {batch_id}: TIMEOUT for {contract_address[:10]}...", flush=True)
            except Exception as e:
                errors += 1
                error_details.append(str(e)[:100])
                print(f"  Batch {batch_id}: Exception for {contract_address[:10]}: {str(e)[:50]}", flush=True)

        # Get cache stats
        stats = get_cache_stats()
        elapsed = time.time() - start_time
        abandoned_threads = stats.get('abandoned_threads', 0)

        print(f"  Batch {batch_id}: COMPLETED - processed {processed}/{len(contracts)} contracts", flush=True)
        if abandoned_threads > 0:
            print(f"  Batch {batch_id}: Note - {abandoned_threads} threads abandoned so far", flush=True)

        return {
            'batch_id': batch_id,
            'processed': processed,
            'errors': errors,
            'error_details': error_details,
            'timeouts': timeouts,
            'time': elapsed,
            'cache_hits': stats['hits'],
            'cache_misses': stats['misses'],
            'abandoned_threads': abandoned_threads
        }
    except Exception as e:
        print(f"  Batch {batch_id}: FATAL ERROR - {str(e)}", flush=True)
        import traceback
        traceback.print_exc()
        # Return minimal results
        return {
            'batch_id': batch_id,
            'processed': 0,
            'errors': len(contracts),
            'error_details': [f"Fatal error: {str(e)}"],
            'timeouts': 0,
            'time': 0,
            'cache_hits': 0,
            'cache_misses': 0
        }

def main():
    cache_dir = "/tmp/heimdall_multiprocess_test_cache"
    configure_cache(enabled=True, directory=cache_dir)

    print(f"Cache directory: {cache_dir}")

    print("\nLoading contracts from parquet files...")
    parquet_files = sorted(glob.glob("parquets/*.parquet"))
    all_contracts = []

    for i, pf in enumerate(parquet_files):
        try:
            df = pq.read_table(pf).to_pandas()
            for idx in range(len(df)):
                row = df.iloc[idx]
                all_contracts.append((row['contract_address'], row['code']))
            if (i+1) % 10 == 0:
                print(f"  [{i+1}/{len(parquet_files)}] Loaded {len(all_contracts)} contracts so far...")
        except Exception as e:
            print(f"  Error reading {pf}: {e}")

    print(f"\nTotal contracts loaded: {len(all_contracts)}")

    num_workers = 14
    batch_size = len(all_contracts) // num_workers
    batches = []

    for i in range(num_workers):
        start = i * batch_size
        end = start + batch_size if i < num_workers - 1 else len(all_contracts)
        batches.append((i, all_contracts[start:end], cache_dir))

    print(f"Split into {len(batches)} batches of ~{batch_size} contracts each")

    print(f"\n=== Starting {num_workers} workers ===\n")
    start_time = time.time()

    with mp.Pool(num_workers) as pool:
        results = pool.map(process_batch, batches)

    total_time = time.time() - start_time

    total_processed = sum(r['processed'] for r in results)
    total_errors = sum(r['errors'] for r in results)
    total_timeouts = sum(r['timeouts'] for r in results)
    total_cache_hits = sum(r['cache_hits'] for r in results)
    total_cache_misses = sum(r['cache_misses'] for r in results)
    all_errors = []
    for r in results:
        all_errors.extend(r['error_details'])

    print("\n=== Results by Worker ===")
    for r in results:
        if r['processed'] > 0:
            print(f"Batch {r['batch_id']:2d}: {r['processed']:4d} contracts in {r['time']:6.2f}s ({r['processed']/r['time']:6.1f} contracts/sec)")

    print(f"\n=== Summary ===")
    print(f"Total contracts: {len(all_contracts)}")
    print(f"Total processed: {total_processed}")
    print(f"Total errors: {total_errors} ({total_errors/len(all_contracts)*100:.1f}%)")
    print(f"Total timeouts: {total_timeouts}")
    print(f"Success rate: {(total_processed-total_errors)/total_processed*100:.1f}%")
    print(f"Total time: {total_time:.2f}s")
    print(f"Overall throughput: {total_processed/total_time:.1f} contracts/sec")

    # Error distribution
    if all_errors:
        error_counts = Counter(all_errors)
        print(f"\n=== Top Error Types ===")
        for error, count in error_counts.most_common(5):
            print(f"  {count:4d}x: {error}")

    print(f"\n=== Cache Stats ===")
    print(f"Total hits: {total_cache_hits}")
    print(f"Total misses: {total_cache_misses}")
    if total_cache_hits + total_cache_misses > 0:
        print(f"Hit rate: {total_cache_hits/(total_cache_hits+total_cache_misses)*100:.1f}%")

    # Get final stats including abandoned threads
    final_stats = get_cache_stats()
    abandoned = final_stats.get('abandoned_threads', 0)
    print(f"Abandoned threads: {abandoned}")
    if abandoned > 0:
        print(f"WARNING: {abandoned} threads were abandoned due to timeouts")

    print(f"\nCache directory kept at: {cache_dir}")

if __name__ == "__main__":
    main()