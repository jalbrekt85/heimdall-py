#!/usr/bin/env python3
"""
Type validation test for Heimdall decompiler.
Tests input/output types for contracts against their expected ABIs.
Automatically compares results against baseline and highlights differences.
"""

import json
import os
import tempfile
import time
from datetime import datetime
from pathlib import Path
import heimdall_py
from heimdall_py import decompile_code, configure_cache

# Configure temporary cache directory
TEST_CACHE_DIR = tempfile.mkdtemp(prefix="heimdall_type_test_")
configure_cache(enabled=True, directory=TEST_CACHE_DIR)

# Contract bytecodes
with open("contracts/vault.bin", "r") as f:
    VAULT_CODE = f.readline().strip()

with open("contracts/weth.bin", "r") as f:
    WETH_CODE = f.readline().strip()

with open("contracts/univ2pair.bin", "r") as f:
    UNIV2PAIR_CODE = f.readline().strip()

with open("contracts/erc20.bin", "r") as f:
    ERC20_CODE = f.readline().strip()

BASELINE_FILE = "test_type_baseline.json"

# Load expected ABIs from JSON files - FAIL if not found
EXPECTED_ABIS = {
    "weth": heimdall_py.ABI.from_json("abis/weth.json"),
    "univ2pair": heimdall_py.ABI.from_json("abis/univ2pair.json"),
    "erc20": heimdall_py.ABI.from_json("abis/erc20.json")
}


class TypeTestResult:
    """Container for test results"""
    def __init__(self, contract_name):
        self.contract_name = contract_name
        self.functions = {}
        self.timestamp = datetime.now().isoformat()

    def add_function(self, func_name, selector, actual_inputs, actual_outputs, expected_inputs, expected_outputs, matches):
        self.functions[func_name] = {
            "selector": selector,
            "actual_inputs": actual_inputs,
            "actual_outputs": actual_outputs,
            "expected_inputs": expected_inputs,
            "expected_outputs": expected_outputs,
            "matches": matches
        }

    def to_dict(self):
        return {
            "contract": self.contract_name,
            "timestamp": self.timestamp,
            "functions": self.functions
        }

    @classmethod
    def from_dict(cls, data):
        result = cls(data["contract"])
        result.timestamp = data["timestamp"]
        result.functions = data["functions"]
        return result


def format_type_info(func):
    """Extract type information from a function"""
    inputs = [i.type_ for i in func.inputs] if func.inputs else []
    outputs = [o.type_ for o in func.outputs] if func.outputs else []
    return inputs, outputs


def format_signature(inputs, outputs):
    """Format function signature"""
    input_str = ', '.join(inputs) if inputs else ''
    output_str = ', '.join(outputs) if outputs else ''
    return f"({input_str}) -> ({output_str})" if outputs else f"({input_str}) -> void"


def test_contract_types(contract_name, bytecode, expected_abi):
    """Test a contract's function types against expected ABI"""
    print(f"\n{'='*80}")
    print(f"{contract_name.upper()} CONTRACT")
    print('='*80)

    result = TypeTestResult(contract_name)

    # Decompile the contract
    decompiled_abi = decompile_code(bytecode, skip_resolving=False)

    # Print header
    print(f"{'Function':<25} {'Status':<8} {'Actual':<35} {'Expected':<35}")
    print('-'*103)

    # Track which functions we've found
    found_functions = set()

    # Check each expected function
    for expected_func in expected_abi.functions:
        func_name = expected_func.name
        selector = "0x" + bytes(expected_func.selector).hex()

        # Look for this function in decompiled ABI
        decompiled_func = decompiled_abi.get_function(selector)

        if decompiled_func:
            found_functions.add(func_name)
            actual_inputs, actual_outputs = format_type_info(decompiled_func)
            expected_inputs, expected_outputs = format_type_info(expected_func)

            matches = (actual_inputs == expected_inputs and actual_outputs == expected_outputs)

            result.add_function(
                func_name, selector, actual_inputs, actual_outputs,
                expected_inputs, expected_outputs, matches
            )

            status = "✅" if matches else "❌"
            actual_sig = format_signature(actual_inputs, actual_outputs)
            expected_sig = format_signature(expected_inputs, expected_outputs)

            print(f"{func_name:<25} {status:<8} {actual_sig:<35} {expected_sig:<35}")
        else:
            # Function not found
            expected_inputs, expected_outputs = format_type_info(expected_func)
            result.add_function(
                func_name, selector, [], [],
                expected_inputs, expected_outputs, False
            )

            expected_sig = format_signature(expected_inputs, expected_outputs)
            print(f"{func_name:<25} {'MISSING':<8} {'NOT FOUND':<35} {expected_sig:<35}")

    # Check for extra functions in decompiled that aren't in expected
    for decompiled_func in decompiled_abi.functions:
        if decompiled_func.name in found_functions:
            continue
        if decompiled_func.name.startswith("Unresolved_"):
            continue

        actual_inputs, actual_outputs = format_type_info(decompiled_func)
        selector = "0x" + bytes(decompiled_func.selector).hex()

        result.add_function(
            decompiled_func.name, selector, actual_inputs, actual_outputs,
            None, None, None
        )

        actual_sig = format_signature(actual_inputs, actual_outputs)
        print(f"{decompiled_func.name:<25} {'EXTRA':<8} {actual_sig:<35} {'---':<35}")

    return result


def save_baseline(results, filename=BASELINE_FILE):
    """Save test results as baseline"""
    baseline_data = {
        "timestamp": datetime.now().isoformat(),
        "results": [r.to_dict() for r in results]
    }
    with open(filename, 'w') as f:
        json.dump(baseline_data, f, indent=2)
    print(f"\n✅ Baseline saved to {filename}")


def load_baseline(filename=BASELINE_FILE):
    """Load baseline results"""
    if not os.path.exists(filename):
        return None
    with open(filename, 'r') as f:
        data = json.load(f)
    return {
        "timestamp": data["timestamp"],
        "results": {r["contract"]: TypeTestResult.from_dict(r) for r in data["results"]}
    }


def compare_with_baseline(current_results, baseline):
    """Compare current results with baseline"""
    if baseline is None:
        return

    print("\n" + "="*80)
    print("BASELINE COMPARISON")
    print("="*80)
    print(f"Baseline from: {baseline['timestamp']}")
    print(f"Current test:  {datetime.now().isoformat()}")

    baseline_results = baseline["results"]
    current_dict = {r.contract_name: r for r in current_results}

    for result in current_results:
        contract_name = result.contract_name
        if contract_name not in baseline_results:
            print(f"\n{contract_name}: NEW CONTRACT (not in baseline)")
            continue

        current = current_dict[contract_name]
        baseline_contract = baseline_results[contract_name]

        # Find differences
        differences = []
        for func_name in current.functions:
            if func_name not in baseline_contract.functions:
                differences.append((func_name, "NEW", None, current.functions[func_name]))
            else:
                curr_func = current.functions[func_name]
                base_func = baseline_contract.functions[func_name]
                if (curr_func["actual_inputs"] != base_func.get("actual_inputs") or
                    curr_func["actual_outputs"] != base_func.get("actual_outputs")):
                    differences.append((func_name, "CHANGED", base_func, curr_func))

        for func_name in baseline_contract.functions:
            if func_name not in current.functions:
                differences.append((func_name, "REMOVED", baseline_contract.functions[func_name], None))

        if differences:
            print(f"\n{contract_name} Changes:")
            print(f"{'Function':<25} {'Change':<10} {'Baseline':<35} {'Current':<35}")
            print('-'*105)
            for func_name, change_type, base, curr in differences:
                if change_type == "NEW":
                    curr_sig = format_signature(curr["actual_inputs"], curr["actual_outputs"])
                    print(f"{func_name:<25} {change_type:<10} {'---':<35} {curr_sig:<35}")
                elif change_type == "REMOVED":
                    base_sig = format_signature(base["actual_inputs"], base["actual_outputs"])
                    print(f"{func_name:<25} {change_type:<10} {base_sig:<35} {'---':<35}")
                elif change_type == "CHANGED":
                    base_sig = format_signature(base["actual_inputs"], base["actual_outputs"])
                    curr_sig = format_signature(curr["actual_inputs"], curr["actual_outputs"])
                    print(f"{func_name:<25} {change_type:<10} {base_sig:<35} {curr_sig:<35}")
        else:
            print(f"\n{contract_name}: No changes from baseline")


def print_summary(results):
    """Print summary of validation results"""
    print("\n" + "="*80)
    print("SUMMARY")
    print("="*80)

    print(f"{'Contract':<20} {'Total':<10} {'Matching':<15} {'Missing':<10} {'Extra':<10}")
    print('-'*65)

    total_all = 0
    matching_all = 0
    missing_all = 0
    extra_all = 0

    for result in results:
        total = len(result.functions)
        matching = sum(1 for f in result.functions.values() if f.get("matches") is True)
        missing = sum(1 for f in result.functions.values()
                     if not f["actual_inputs"] and not f["actual_outputs"])
        extra = sum(1 for f in result.functions.values()
                   if f.get("expected_inputs") is None)
        expected = total - extra

        total_all += expected
        matching_all += matching
        missing_all += missing
        extra_all += extra

        print(f"{result.contract_name:<20} {expected:<10} {matching:<15} {missing:<10} {extra:<10}")

    print('-'*65)
    print(f"{'TOTAL':<20} {total_all:<10} {matching_all:<15} {missing_all:<10} {extra_all:<10}")

    if total_all > 0:
        pct = (matching_all / total_all) * 100
        print(f"\nOverall match rate: {matching_all}/{total_all} ({pct:.1f}%)")


def run_tests(save_as_baseline=False):
    """Run all type validation tests"""
    print("HEIMDALL TYPE VALIDATION TEST")
    print(f"Time: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")

    results = []
    start = time.time()

    # Test each contract
    results.append(test_contract_types("WETH", WETH_CODE, EXPECTED_ABIS["weth"]))
    results.append(test_contract_types("UniV2Pair", UNIV2PAIR_CODE, EXPECTED_ABIS["univ2pair"]))
    results.append(test_contract_types("ERC20 (Dai)", ERC20_CODE, EXPECTED_ABIS["erc20"]))

    elapsed = time.time() - start

    # Print summary
    print_summary(results)
    print(f"\nTest completed in {elapsed:.2f} seconds")

    # Compare with baseline
    baseline = load_baseline()
    if baseline and not save_as_baseline:
        compare_with_baseline(results, baseline)

    # Save baseline if requested
    if save_as_baseline:
        save_baseline(results)
    elif baseline is None:
        save_baseline(results)
        print("\n📝 No baseline existed. Current results saved as new baseline.")

    # Cleanup
    import shutil
    if os.path.exists(TEST_CACHE_DIR):
        shutil.rmtree(TEST_CACHE_DIR)

    return results


if __name__ == "__main__":
    import sys
    should_save_baseline = "--save-baseline" in sys.argv
    run_tests(save_as_baseline=should_save_baseline)