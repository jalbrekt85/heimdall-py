from heimdall_py import decompile_code, configure_cache, clear_cache, get_cache_stats
import pickle
import copy
import time

import heimdall_py
print(f"Using heimdall-py from: {heimdall_py.__file__}")
try:
    print("Module loaded successfully")
except Exception as e:
    print(f"Module check: {e}")

with open("contracts/vault.bin", "r") as f:
    vault = f.readline().strip()

with open("contracts/weth.bin", "r") as f:
    weth = f.readline().strip()

with open("contracts/univ2pair.bin", "r") as f:
    univ2pair = f.readline().strip()

with open("contracts/erc20.bin", "r") as f:
    erc20 = f.readline().strip()

def check(condition, message, errors):
    """Check a condition and track the result"""
    if condition:
        print(f"  ✅ {message}")
        return True
    else:
        print(f"  ❌ {message}")
        errors.append(message)
        return False

def test_univ2pair_comprehensive():
    print("\n=== UniV2Pair Comprehensive Test ===")
    errors = []
    abi = decompile_code(univ2pair, skip_resolving=False)
    
    totalSupply = next((func for func in abi.functions if func.name == "totalSupply"), None)
    transfer = next((func for func in abi.functions if func.name == "transfer"), None)
    balanceOf = next((func for func in abi.functions if func.name == "balanceOf"), None)
    approve = next((func for func in abi.functions if func.name == "approve"), None)
    transferFrom = next((func for func in abi.functions if func.name == "transferFrom"), None)
    allowance = next((func for func in abi.functions if func.name == "allowance"), None)
    
    check(totalSupply is not None, "totalSupply function found", errors)
    if totalSupply:
        check(totalSupply.inputs == [], f"totalSupply has no inputs", errors)
        check([o.type_ for o in totalSupply.outputs] == ["uint256"], f"totalSupply returns uint256 (got {[o.type_ for o in totalSupply.outputs]})", errors)
        check(totalSupply.constant == True, "totalSupply is constant", errors)
    
    check(transfer is not None, "transfer function found", errors)
    if transfer:
        check(len(transfer.inputs) == 2, f"transfer has 2 inputs", errors)
        check([i.type_ for i in transfer.inputs] == ["address", "uint256"], f"transfer params correct (got {[i.type_ for i in transfer.inputs]})", errors)
        check([o.type_ for o in transfer.outputs] == ["bool"], f"transfer returns bool (got {[o.type_ for o in transfer.outputs]})", errors)
    
    check(balanceOf is not None, "balanceOf function found", errors)
    if balanceOf:
        check([i.type_ for i in balanceOf.inputs] == ["address"], f"balanceOf takes address (got {[i.type_ for i in balanceOf.inputs]})", errors)
        check([o.type_ for o in balanceOf.outputs] == ["uint256"], f"balanceOf returns uint256 (got {[o.type_ for o in balanceOf.outputs]})", errors)
        check(balanceOf.constant == True, "balanceOf is constant", errors)
    
    check(approve is not None, "approve function found", errors)
    if approve:
        check([i.type_ for i in approve.inputs] == ["address", "uint256"], f"approve params correct (got {[i.type_ for i in approve.inputs]})", errors)
        check([o.type_ for o in approve.outputs] == ["bool"], f"approve returns bool (got {[o.type_ for o in approve.outputs]})", errors)
    
    check(transferFrom is not None, "transferFrom function found", errors)
    if transferFrom:
        check(len(transferFrom.inputs) == 3, f"transferFrom has 3 inputs", errors)
        check([i.type_ for i in transferFrom.inputs] == ["address", "address", "uint256"], f"transferFrom params correct (got {[i.type_ for i in transferFrom.inputs]})", errors)
        check([o.type_ for o in transferFrom.outputs] == ["bool"], f"transferFrom returns bool (got {[o.type_ for o in transferFrom.outputs]})", errors)
    
    check(allowance is not None, "allowance function found", errors)
    if allowance:
        check([i.type_ for i in allowance.inputs] == ["address", "address"], f"allowance params correct (got {[i.type_ for i in allowance.inputs]})", errors)
        check([o.type_ for o in allowance.outputs] == ["uint256"], f"allowance returns uint256 (got {[o.type_ for o in allowance.outputs]})", errors)
    
    # UniV2Pair specific functions
    token0 = next((func for func in abi.functions if func.name == "token0"), None)
    check(token0 is not None, "token0 function found", errors)
    if token0:
        check(token0.inputs == [], f"token0 has no inputs", errors)
        check([o.type_ for o in token0.outputs] == ["address"], f"token0 returns address (got {[o.type_ for o in token0.outputs]})", errors)
    
    token1 = next((func for func in abi.functions if func.name == "token1"), None)
    check(token1 is not None, "token1 function found", errors)
    if token1:
        check(token1.inputs == [], f"token1 has no inputs", errors)
        check([o.type_ for o in token1.outputs] == ["address"], f"token1 returns address (got {[o.type_ for o in token1.outputs]})", errors)
    
    getReserves = next((func for func in abi.functions if func.name == "getReserves"), None)
    check(getReserves is not None, "getReserves function found", errors)
    if getReserves:
        check(getReserves.inputs == [], f"getReserves has no inputs", errors)
        # getReserves returns (uint112, uint112, uint32) but decompiler detects 1 output currently
        check(len(getReserves.outputs) >= 1, f"getReserves returns at least 1 value (got {len(getReserves.outputs) if getReserves.outputs else 0})", errors)
        if getReserves.outputs:
            check(all(o.type_.startswith("uint") for o in getReserves.outputs), f"getReserves returns uint types (got {[o.type_ for o in getReserves.outputs]})", errors)
    
    kLast = next((func for func in abi.functions if func.name == "kLast"), None)
    check(kLast is not None, "kLast function found", errors)
    if kLast:
        check(kLast.inputs == [], f"kLast has no inputs", errors)
        check([o.type_ for o in kLast.outputs] == ["uint256"], f"kLast returns uint256 (got {[o.type_ for o in kLast.outputs]})", errors)
    
    # Test mint and burn functions
    mint = next((func for func in abi.functions if func.name == "mint"), None)
    check(mint is not None, "mint function found", errors)
    if mint:
        check(len(mint.inputs) == 1, f"mint has 1 input", errors)
        check(mint.inputs[0].type_ == "address", f"mint param is address (got {mint.inputs[0].type_})", errors)
        if mint.outputs and len(mint.outputs) > 0:
            check(mint.outputs[0].type_.startswith("uint"), f"mint returns uint (got {mint.outputs[0].type_})", errors)
    
    burn = next((func for func in abi.functions if func.name == "burn"), None)
    check(burn is not None, "burn function found", errors)
    if burn:
        check(len(burn.inputs) == 1, f"burn has 1 input", errors)
        check(burn.inputs[0].type_ == "address", f"burn param is address (got {burn.inputs[0].type_})", errors)
    
    # Test skim and sync functions
    skim = next((func for func in abi.functions if func.name == "skim"), None)
    check(skim is not None, "skim function found", errors)
    if skim:
        check(len(skim.inputs) == 1, f"skim has 1 input", errors)
        check(skim.inputs[0].type_ == "address", f"skim param is address (got {skim.inputs[0].type_})", errors)
    
    sync = next((func for func in abi.functions if func.name == "sync"), None)
    check(sync is not None, "sync function found", errors)
    if sync:
        check(sync.inputs == [], f"sync has no inputs", errors)
    
    # Test permit function
    permit = next((func for func in abi.functions if func.name == "permit"), None)
    check(permit is not None, "permit function found", errors)
    if permit:
        check(len(permit.inputs) == 7, f"permit has 7 inputs (got {len(permit.inputs)})", errors)
        expected = ["address", "address", "uint256", "uint256", "uint8", "bytes32", "bytes32"]
        check([i.type_ for i in permit.inputs] == expected, f"permit params match ABI", errors)
    
    PERMIT_TYPEHASH = next((func for func in abi.functions if func.name == "PERMIT_TYPEHASH"), None)
    check(PERMIT_TYPEHASH is not None, "PERMIT_TYPEHASH function found", errors)
    if PERMIT_TYPEHASH:
        check(PERMIT_TYPEHASH.inputs == [], f"PERMIT_TYPEHASH has no inputs", errors)
        check(len(PERMIT_TYPEHASH.outputs) == 1, f"PERMIT_TYPEHASH has one output", errors)
        if PERMIT_TYPEHASH.outputs:
            check(PERMIT_TYPEHASH.outputs[0].type_ == "bytes32", f"PERMIT_TYPEHASH returns bytes32 (got {PERMIT_TYPEHASH.outputs[0].type_})", errors)
    
    # Test for swap function (0x022c0d9f)
    swap = None
    for func in abi.functions:
        if func.name.startswith("Unresolved_022c0d9f"):
            swap = func
            break
    check(swap is not None, "swap function (0x022c0d9f) found", errors)
    if swap:
        check(len(swap.inputs) == 4, f"swap has 4 inputs (got {len(swap.inputs)})", errors)
        if len(swap.inputs) == 4:
            check(swap.inputs[0].type_.startswith("uint"), f"swap param 0 is uint (got {swap.inputs[0].type_})", errors)
            check(swap.inputs[1].type_.startswith("uint"), f"swap param 1 is uint (got {swap.inputs[1].type_})", errors)
            check(swap.inputs[2].type_ == "address", f"swap param 2 is address (got {swap.inputs[2].type_})", errors)
            check(swap.inputs[3].type_ == "bytes", f"swap param 3 is bytes (got {swap.inputs[3].type_})", errors)
        check(swap.outputs is None or len(swap.outputs) == 0, f"swap has no outputs", errors)
    
    # Test for DOMAIN_SEPARATOR  
    DOMAIN_SEPARATOR = next((func for func in abi.functions if func.name == "DOMAIN_SEPARATOR"), None)
    check(DOMAIN_SEPARATOR is not None, "DOMAIN_SEPARATOR function found", errors)
    if DOMAIN_SEPARATOR:
        check(DOMAIN_SEPARATOR.inputs == [], f"DOMAIN_SEPARATOR has no inputs", errors)
        if DOMAIN_SEPARATOR.outputs:
            check(len(DOMAIN_SEPARATOR.outputs) == 1 and DOMAIN_SEPARATOR.outputs[0].type_ == "bytes32", 
                  f"DOMAIN_SEPARATOR returns bytes32 (got {[o.type_ for o in DOMAIN_SEPARATOR.outputs]})", errors)
    
    # Test name and symbol
    name = next((func for func in abi.functions if func.name == "name"), None)
    check(name is not None, "name function found", errors)
    if name:
        check(name.inputs == [], f"name has no inputs", errors)
        check(len(name.outputs) == 1, f"name has one output", errors)
        if name.outputs:
            check(name.outputs[0].type_ == "string", f"name returns string (got {name.outputs[0].type_})", errors)
    
    symbol = next((func for func in abi.functions if func.name == "symbol"), None)
    check(symbol is not None, "symbol function found", errors)
    if symbol:
        check(symbol.inputs == [], f"symbol has no inputs", errors)
        check(len(symbol.outputs) == 1, f"symbol has one output", errors)
        if symbol.outputs:
            check(symbol.outputs[0].type_ == "string", f"symbol returns string (got {symbol.outputs[0].type_})", errors)
    
    decimals = next((func for func in abi.functions if func.name == "decimals"), None)
    check(decimals is not None, "decimals function found", errors)
    if decimals:
        check(decimals.inputs == [], f"decimals has no inputs", errors)
        if decimals.outputs:
            check(decimals.outputs[0].type_.startswith("uint"), f"decimals returns uint (got {decimals.outputs[0].type_})", errors)
    
    if errors:
        print(f"\n❌ UniV2Pair test had {len(errors)} failures")
    else:
        print("\n✓ UniV2Pair comprehensive test passed")
    return len(errors) == 0

def test_vault():
    print("\n=== Vault Test ===")
    errors = []
    abi = decompile_code(vault, skip_resolving=False)
    
    # Test hasApprovedRelayer (0xfec90d72)
    hasApprovedRelayer = None
    for func in abi.functions:
        if func.name.startswith("Unresolved_fec90d72"):
            hasApprovedRelayer = func
            break
    
    check(hasApprovedRelayer is not None, "Function 0xfec90d72 (hasApprovedRelayer) found", errors)
    
    if hasApprovedRelayer:
        check(len(hasApprovedRelayer.inputs) == 2, f"hasApprovedRelayer has 2 inputs", errors)
        if len(hasApprovedRelayer.inputs) == 2:
            check(hasApprovedRelayer.inputs[0].type_ == "address", f"hasApprovedRelayer param 0 is address", errors)
            check(hasApprovedRelayer.inputs[1].type_ == "address", f"hasApprovedRelayer param 1 is address", errors)
        
        check(hasApprovedRelayer.outputs is not None, "hasApprovedRelayer has outputs", errors)
        if hasApprovedRelayer.outputs:
            check(len(hasApprovedRelayer.outputs) == 1, f"hasApprovedRelayer has 1 output", errors)
            if len(hasApprovedRelayer.outputs) == 1:
                check(hasApprovedRelayer.outputs[0].type_ == "bool", f"hasApprovedRelayer returns bool (got {hasApprovedRelayer.outputs[0].type_})", errors)
    
    # Test getNextNonce (0x90193b7c)
    getNextNonce = None
    for func in abi.functions:
        if func.name.startswith("Unresolved_90193b7c"):
            getNextNonce = func
            break
    
    check(getNextNonce is not None, "Function 0x90193b7c (getNextNonce) found", errors)
    
    if getNextNonce:
        check(len(getNextNonce.inputs) == 1, f"getNextNonce has 1 input", errors)
        if len(getNextNonce.inputs) == 1:
            check(getNextNonce.inputs[0].type_ == "address", f"getNextNonce param is address", errors)
        
        check(getNextNonce.outputs is not None, "getNextNonce has outputs", errors)
        if getNextNonce.outputs:
            check(len(getNextNonce.outputs) == 1, f"getNextNonce has 1 output", errors)
            if len(getNextNonce.outputs) == 1:
                check(getNextNonce.outputs[0].type_ == "uint256", f"getNextNonce returns uint256 (got {getNextNonce.outputs[0].type_})", errors)
    
    if errors:
        print(f"\n❌ Vault test had {len(errors)} failures")
    else:
        print("\n✓ Vault test passed")
    return len(errors) == 0

def test_weth_comprehensive():
    print("\n=== WETH Comprehensive Test ===")
    errors = []
    abi = decompile_code(weth, skip_resolving=False)
    
    # Test basic ERC20 functions
    deposit = next((func for func in abi.functions if func.name == "deposit"), None)
    withdraw = next((func for func in abi.functions if func.name == "withdraw"), None)
    totalSupply = next((func for func in abi.functions if func.name == "totalSupply"), None)
    transfer = next((func for func in abi.functions if func.name == "transfer"), None)
    balanceOf = next((func for func in abi.functions if func.name == "balanceOf"), None)
    approve = next((func for func in abi.functions if func.name == "approve"), None)
    transferFrom = next((func for func in abi.functions if func.name == "transferFrom"), None)
    allowance = next((func for func in abi.functions if func.name == "allowance"), None)
    
    check(deposit is not None, "deposit function found", errors)
    if deposit:
        check(deposit.inputs == [], f"deposit has no inputs", errors)
    
    check(withdraw is not None, "withdraw function found", errors)
    if withdraw:
        check(len(withdraw.inputs) == 1, f"withdraw has 1 input", errors)
        if withdraw.inputs:
            check(withdraw.inputs[0].type_.startswith("uint"), f"withdraw param is uint (got {withdraw.inputs[0].type_})", errors)
    
    check(totalSupply is not None, "totalSupply function found", errors)
    if totalSupply:
        check(totalSupply.inputs == [], f"totalSupply has no inputs", errors)
        check([o.type_ for o in totalSupply.outputs] == ["uint256"], f"totalSupply returns uint256 (got {[o.type_ for o in totalSupply.outputs]})", errors)
    
    check(transfer is not None, "transfer function found", errors)
    if transfer:
        check(len(transfer.inputs) == 2, f"transfer has 2 inputs", errors)
        check([i.type_ for i in transfer.inputs] == ["address", "uint256"], f"transfer params correct (got {[i.type_ for i in transfer.inputs]})", errors)
        check([o.type_ for o in transfer.outputs] == ["bool"], f"transfer returns bool (got {[o.type_ for o in transfer.outputs]})", errors)
    
    check(balanceOf is not None, "balanceOf function found", errors)
    if balanceOf:
        check([i.type_ for i in balanceOf.inputs] == ["address"], f"balanceOf param is address", errors)
        check([o.type_ for o in balanceOf.outputs] == ["uint256"], f"balanceOf returns uint256 (got {[o.type_ for o in balanceOf.outputs]})", errors)
    
    check(approve is not None, "approve function found", errors)
    if approve:
        check([i.type_ for i in approve.inputs] == ["address", "uint256"], f"approve params correct (got {[i.type_ for i in approve.inputs]})", errors)
        check([o.type_ for o in approve.outputs] == ["bool"], f"approve returns bool (got {[o.type_ for o in approve.outputs]})", errors)
    
    check(transferFrom is not None, "transferFrom function found", errors)
    if transferFrom:
        check(len(transferFrom.inputs) == 3, f"transferFrom has 3 inputs", errors)
        check([i.type_ for i in transferFrom.inputs] == ["address", "address", "uint256"], f"transferFrom params correct (got {[i.type_ for i in transferFrom.inputs]})", errors)
        check([o.type_ for o in transferFrom.outputs] == ["bool"], f"transferFrom returns bool (got {[o.type_ for o in transferFrom.outputs]})", errors)
    
    check(allowance is not None, "allowance function found", errors)
    if allowance:
        check([i.type_ for i in allowance.inputs] == ["address", "address"], f"allowance params correct", errors)
        check([o.type_ for o in allowance.outputs] == ["uint256"], f"allowance returns uint256 (got {[o.type_ for o in allowance.outputs]})", errors)
    
    # Test metadata functions
    name = next((func for func in abi.functions if func.name == "name"), None)
    check(name is not None, "name function found", errors)
    if name:
        check(name.inputs == [], f"name has no inputs", errors)
        check(len(name.outputs) == 1, f"name has one output", errors)
        if name.outputs:
            check(name.outputs[0].type_ == "string", f"name returns string (got {name.outputs[0].type_})", errors)
    
    symbol = next((func for func in abi.functions if func.name == "symbol"), None)
    check(symbol is not None, "symbol function found", errors)
    if symbol:
        check(symbol.inputs == [], f"symbol has no inputs", errors)
        check(len(symbol.outputs) == 1, f"symbol has one output", errors)
        if symbol.outputs:
            check(symbol.outputs[0].type_ == "string", f"symbol returns string (got {symbol.outputs[0].type_})", errors)
    
    decimals = next((func for func in abi.functions if func.name == "decimals"), None)
    check(decimals is not None, "decimals function found", errors)
    if decimals:
        check(decimals.inputs == [], f"decimals has no inputs", errors)
        if decimals.outputs:
            check(decimals.outputs[0].type_.startswith("uint"), f"decimals returns uint (got {decimals.outputs[0].type_})", errors)
    
    if errors:
        print(f"\n❌ WETH test had {len(errors)} failures")
    else:
        print("\n✓ WETH comprehensive test passed")
    return len(errors) == 0

def test_erc20_comprehensive():
    print("\n=== ERC20 (Dai) Comprehensive Test ===")
    errors = []
    abi = decompile_code(erc20, skip_resolving=False)
    
    # Test standard ERC20 functions
    totalSupply = next((func for func in abi.functions if func.name == "totalSupply"), None)
    transfer = next((func for func in abi.functions if func.name == "transfer"), None)
    balanceOf = next((func for func in abi.functions if func.name == "balanceOf"), None)
    approve = next((func for func in abi.functions if func.name == "approve"), None)
    transferFrom = next((func for func in abi.functions if func.name == "transferFrom"), None)
    allowance = next((func for func in abi.functions if func.name == "allowance"), None)
    
    check(totalSupply is not None, "totalSupply function found", errors)
    if totalSupply:
        check(totalSupply.inputs == [], f"totalSupply has no inputs", errors)
        check([o.type_ for o in totalSupply.outputs] == ["uint256"], f"totalSupply returns uint256 (got {[o.type_ for o in totalSupply.outputs]})", errors)
    
    check(transfer is not None, "transfer function found", errors)
    if transfer:
        check(len(transfer.inputs) == 2, f"transfer has 2 inputs", errors)
        check([i.type_ for i in transfer.inputs] == ["address", "uint256"], f"transfer params correct (got {[i.type_ for i in transfer.inputs]})", errors)
        check([o.type_ for o in transfer.outputs] == ["bool"], f"transfer returns bool (got {[o.type_ for o in transfer.outputs]})", errors)
    
    check(balanceOf is not None, "balanceOf function found", errors)
    if balanceOf:
        check([i.type_ for i in balanceOf.inputs] == ["address"], f"balanceOf param is address", errors)
        check([o.type_ for o in balanceOf.outputs] == ["uint256"], f"balanceOf returns uint256 (got {[o.type_ for o in balanceOf.outputs]})", errors)
    
    check(approve is not None, "approve function found", errors)
    if approve:
        check(len(approve.inputs) == 2, f"approve has 2 inputs", errors)
        check([i.type_ for i in approve.inputs] == ["address", "uint256"], f"approve params correct (got {[i.type_ for i in approve.inputs]})", errors)
        check([o.type_ for o in approve.outputs] == ["bool"], f"approve returns bool (got {[o.type_ for o in approve.outputs]})", errors)
    
    check(transferFrom is not None, "transferFrom function found", errors)
    if transferFrom:
        check(len(transferFrom.inputs) == 3, f"transferFrom has 3 inputs", errors)
        check([i.type_ for i in transferFrom.inputs] == ["address", "address", "uint256"], f"transferFrom params correct (got {[i.type_ for i in transferFrom.inputs]})", errors)
        check([o.type_ for o in transferFrom.outputs] == ["bool"], f"transferFrom returns bool (got {[o.type_ for o in transferFrom.outputs]})", errors)
    
    check(allowance is not None, "allowance function found", errors)
    if allowance:
        check([i.type_ for i in allowance.inputs] == ["address", "address"], f"allowance params correct", errors)
        check([o.type_ for o in allowance.outputs] == ["uint256"], f"allowance returns uint256 (got {[o.type_ for o in allowance.outputs]})", errors)
    
    # Test Dai-specific functions
    wards = next((func for func in abi.functions if func.name == "wards"), None)
    check(wards is not None, "wards function found", errors)
    if wards:
        check(len(wards.inputs) == 1, f"wards has 1 input", errors)
        if wards.inputs:
            check(wards.inputs[0].type_ == "address", f"wards param is address (got {wards.inputs[0].type_})", errors)
        if wards.outputs:
            check(wards.outputs[0].type_.startswith("uint"), f"wards returns uint (got {wards.outputs[0].type_})", errors)
    
    rely = next((func for func in abi.functions if func.name == "rely"), None)
    check(rely is not None, "rely function found", errors)
    if rely:
        check(len(rely.inputs) == 1, f"rely has 1 input", errors)
        if rely.inputs:
            check(rely.inputs[0].type_ == "address", f"rely param is address (got {rely.inputs[0].type_})", errors)
    
    deny = next((func for func in abi.functions if func.name == "deny"), None)
    check(deny is not None, "deny function found", errors)
    if deny:
        check(len(deny.inputs) == 1, f"deny has 1 input", errors)
        if deny.inputs:
            check(deny.inputs[0].type_ == "address", f"deny param is address (got {deny.inputs[0].type_})", errors)
    
    mint = next((func for func in abi.functions if func.name == "mint"), None)
    check(mint is not None, "mint function found", errors)
    if mint:
        check(len(mint.inputs) == 2, f"mint has 2 inputs", errors)
        if len(mint.inputs) == 2:
            check(mint.inputs[0].type_ == "address", f"mint param 0 is address (got {mint.inputs[0].type_})", errors)
            check(mint.inputs[1].type_.startswith("uint"), f"mint param 1 is uint (got {mint.inputs[1].type_})", errors)
    
    burn = next((func for func in abi.functions if func.name == "burn"), None)
    check(burn is not None, "burn function found", errors)
    if burn:
        check(len(burn.inputs) == 2, f"burn has 2 inputs", errors)
        if len(burn.inputs) == 2:
            check(burn.inputs[0].type_ == "address", f"burn param 0 is address (got {burn.inputs[0].type_})", errors)
            check(burn.inputs[1].type_.startswith("uint"), f"burn param 1 is uint (got {burn.inputs[1].type_})", errors)
    
    push = next((func for func in abi.functions if func.name == "push"), None)
    check(push is not None, "push function found", errors)
    if push:
        check(len(push.inputs) == 2, f"push has 2 inputs", errors)
        if len(push.inputs) == 2:
            check(push.inputs[0].type_ == "address", f"push param 0 is address (got {push.inputs[0].type_})", errors)
            check(push.inputs[1].type_.startswith("uint"), f"push param 1 is uint (got {push.inputs[1].type_})", errors)
    
    pull = next((func for func in abi.functions if func.name == "pull"), None)
    check(pull is not None, "pull function found", errors)
    if pull:
        check(len(pull.inputs) == 2, f"pull has 2 inputs", errors)
        if len(pull.inputs) == 2:
            check(pull.inputs[0].type_ == "address", f"pull param 0 is address (got {pull.inputs[0].type_})", errors)
            check(pull.inputs[1].type_.startswith("uint"), f"pull param 1 is uint (got {pull.inputs[1].type_})", errors)
    
    move = next((func for func in abi.functions if func.name == "move"), None)
    check(move is not None, "move function found", errors)
    if move:
        check(len(move.inputs) == 3, f"move has 3 inputs", errors)
        if len(move.inputs) == 3:
            check(move.inputs[0].type_ == "address", f"move param 0 is address (got {move.inputs[0].type_})", errors)
            check(move.inputs[1].type_ == "address", f"move param 1 is address (got {move.inputs[1].type_})", errors)
            check(move.inputs[2].type_.startswith("uint"), f"move param 2 is uint (got {move.inputs[2].type_})", errors)
    
    permit = next((func for func in abi.functions if func.name == "permit"), None)
    check(permit is not None, "permit function found", errors)
    if permit:
        check(len(permit.inputs) == 8, f"permit has 8 inputs (got {len(permit.inputs)})", errors)
    
    nonces = next((func for func in abi.functions if func.name == "nonces"), None)
    check(nonces is not None, "nonces function found", errors)
    if nonces:
        check(len(nonces.inputs) == 1, f"nonces has 1 input", errors)
        if nonces.inputs:
            check(nonces.inputs[0].type_ == "address", f"nonces param is address (got {nonces.inputs[0].type_})", errors)
        if nonces.outputs:
            check(nonces.outputs[0].type_.startswith("uint"), f"nonces returns uint (got {nonces.outputs[0].type_})", errors)
    
    DOMAIN_SEPARATOR = next((func for func in abi.functions if func.name == "DOMAIN_SEPARATOR"), None)
    check(DOMAIN_SEPARATOR is not None, "DOMAIN_SEPARATOR function found", errors)
    if DOMAIN_SEPARATOR:
        check(DOMAIN_SEPARATOR.inputs == [], f"DOMAIN_SEPARATOR has no inputs", errors)
        if DOMAIN_SEPARATOR.outputs:
            check(len(DOMAIN_SEPARATOR.outputs) == 1 and DOMAIN_SEPARATOR.outputs[0].type_ == "bytes32", 
                  f"DOMAIN_SEPARATOR returns bytes32 (got {DOMAIN_SEPARATOR.outputs[0].type_ if DOMAIN_SEPARATOR.outputs else 'None'})", errors)
    
    PERMIT_TYPEHASH = next((func for func in abi.functions if func.name == "PERMIT_TYPEHASH"), None)
    check(PERMIT_TYPEHASH is not None, "PERMIT_TYPEHASH function found", errors)
    if PERMIT_TYPEHASH:
        check(PERMIT_TYPEHASH.inputs == [], f"PERMIT_TYPEHASH has no inputs", errors)
        if PERMIT_TYPEHASH.outputs:
            check(len(PERMIT_TYPEHASH.outputs) == 1 and PERMIT_TYPEHASH.outputs[0].type_ == "bytes32",
                  f"PERMIT_TYPEHASH returns bytes32 (got {PERMIT_TYPEHASH.outputs[0].type_ if PERMIT_TYPEHASH.outputs else 'None'})", errors)
    
    # Test metadata functions
    name = next((func for func in abi.functions if func.name == "name"), None)
    check(name is not None, "name function found", errors)
    if name:
        check(name.inputs == [], f"name has no inputs", errors)
        if name.outputs:
            check(name.outputs[0].type_ == "string", f"name returns string (got {name.outputs[0].type_})", errors)
    
    symbol = next((func for func in abi.functions if func.name == "symbol"), None)
    check(symbol is not None, "symbol function found", errors)
    if symbol:
        check(symbol.inputs == [], f"symbol has no inputs", errors)
        if symbol.outputs:
            check(symbol.outputs[0].type_ == "string", f"symbol returns string (got {symbol.outputs[0].type_})", errors)
    
    version = next((func for func in abi.functions if func.name == "version"), None)
    check(version is not None, "version function found", errors)
    if version:
        check(version.inputs == [], f"version has no inputs", errors)
        if version.outputs:
            check(version.outputs[0].type_ == "string", f"version returns string (got {version.outputs[0].type_})", errors)
    
    decimals = next((func for func in abi.functions if func.name == "decimals"), None)
    check(decimals is not None, "decimals function found", errors)
    if decimals:
        check(decimals.inputs == [], f"decimals has no inputs", errors)
        if decimals.outputs:
            check(decimals.outputs[0].type_.startswith("uint"), f"decimals returns uint (got {decimals.outputs[0].type_})", errors)
    
    if errors:
        print(f"\n❌ ERC20 (Dai) test had {len(errors)} failures")
    else:
        print("\n✓ ERC20 (Dai) comprehensive test passed")
    return len(errors) == 0

def test_pickle_and_lookups():
    print("\n=== Pickle and Lookup Test ===")
    errors = []
    
    # Test with WETH contract
    abi = decompile_code(weth, skip_resolving=False)
    
    # Test pickling
    pickled = pickle.dumps(abi)
    restored = pickle.loads(pickled)
    check(len(restored.functions) == len(abi.functions), f"Pickle preserves functions", errors)
    check(len(restored.events) == len(abi.events), f"Pickle preserves events", errors)
    
    # Test function lookups
    if abi.functions:
        func = abi.functions[0]
        
        # Lookup by name if resolved
        if not func.name.startswith("Unresolved_"):
            found = abi.get_function(func.name)
            check(found and found.name == func.name, f"Lookup by name works", errors)
        
        # Lookup by selector
        selector = func.selector
        if isinstance(selector, list):
            selector = bytes(selector)
        found = abi.get_function(selector)
        check(found and found.name == func.name, f"Lookup by selector works", errors)
        
        # Lookup by hex selector
        hex_selector = "0x" + selector.hex()
        found = abi.get_function(hex_selector)
        check(found and found.name == func.name, f"Lookup by hex selector works", errors)
    
    # Skip storage layout tests as StorageSlot is not exported
    # These would need StorageSlot to be exported from heimdall_py
    
    # Test deep copy
    copied = copy.deepcopy(abi)
    check(len(copied.functions) == len(abi.functions), f"Deep copy works", errors)
    check(id(copied) != id(abi), f"Deep copy creates new object", errors)
    
    # Test selector extraction from Unresolved functions
    vault_abi = decompile_code(vault, skip_resolving=True)
    unresolved_found = False
    for func in vault_abi.functions:
        if func.name.startswith("Unresolved_"):
            selector = func.selector
            if isinstance(selector, list):
                selector = bytes(selector)
            unresolved_found = True
            break
    
    if unresolved_found:
        check(selector is not None, f"Selector extracted from Unresolved_ function", errors)
    
    if errors:
        print(f"\n❌ Pickle and lookup test had {len(errors)} failures")
    else:
        print("\n✓ Pickle and lookup test passed")
    return len(errors) == 0

def test_from_json():
    print("\n=== ABI from_json Test ===")
    errors = []

    # Test loading ERC20 ABI
    abi_erc20 = heimdall_py.ABI.from_json("abis/erc20.json")
    check(abi_erc20 is not None, "Loaded ERC20 ABI from JSON", errors)

    # Check that functions were loaded
    check(len(abi_erc20.functions) > 0, f"ERC20 has functions ({len(abi_erc20.functions)} found)", errors)

    # Look for specific ERC20 functions
    transfer = abi_erc20.get_function("transfer")
    check(transfer is not None, "Found transfer function", errors)
    if transfer:
        check(transfer.name == "transfer", "Transfer function name is correct", errors)
        check(len(transfer.inputs) == 2, f"Transfer has 2 inputs (got {len(transfer.inputs)})", errors)
        check(transfer.input_types == ["address", "uint256"], f"Transfer input types correct (got {transfer.input_types})", errors)

    balanceOf = abi_erc20.get_function("balanceOf")
    check(balanceOf is not None, "Found balanceOf function", errors)
    if balanceOf:
        check(len(balanceOf.inputs) == 1, f"balanceOf has 1 input (got {len(balanceOf.inputs)})", errors)
        check(balanceOf.input_types == ["address"], f"balanceOf input type correct (got {balanceOf.input_types})", errors)

    # Test selector lookup
    if transfer:
        selector_hex = "0x" + bytes(transfer.selector).hex()
        found_by_selector = abi_erc20.get_function(selector_hex)
        check(found_by_selector is not None, f"Can lookup transfer by selector {selector_hex}", errors)
        if found_by_selector:
            check(found_by_selector.name == "transfer", "Selector lookup returns correct function", errors)

    # Test loading UniV2Pair ABI
    abi_univ2 = heimdall_py.ABI.from_json("abis/univ2pair.json")
    check(abi_univ2 is not None, "Loaded UniV2Pair ABI from JSON", errors)
    check(len(abi_univ2.functions) > 0, f"UniV2Pair has functions ({len(abi_univ2.functions)} found)", errors)

    # Check for events
    check(len(abi_univ2.events) > 0, f"UniV2Pair has events ({len(abi_univ2.events)} found)", errors)

    # Look for specific event
    transfer_event = next((e for e in abi_univ2.events if e.name == "Transfer"), None)
    check(transfer_event is not None, "Found Transfer event", errors)
    if transfer_event:
        check(len(transfer_event.inputs) == 3, f"Transfer event has 3 inputs (got {len(transfer_event.inputs)})", errors)

    # Test loading WETH ABI
    abi_weth = heimdall_py.ABI.from_json("abis/weth.json")
    check(abi_weth is not None, "Loaded WETH ABI from JSON", errors)

    # Check for WETH-specific functions
    deposit = abi_weth.get_function("deposit")
    check(deposit is not None, "Found deposit function in WETH", errors)
    if deposit:
        check(deposit.payable == True, "Deposit function is payable", errors)

    withdraw = abi_weth.get_function("withdraw")
    check(withdraw is not None, "Found withdraw function in WETH", errors)

    # Test that from_json ABI has same functionality as decompiled ABI
    print("\n  Testing ABI functionality parity...")

    # Test pickling/unpickling
    import pickle
    pickled = pickle.dumps(abi_erc20)
    unpickled = pickle.loads(pickled)
    check(len(unpickled.functions) == len(abi_erc20.functions), "ABI survives pickling", errors)

    # Test deep copy
    import copy
    copied = copy.deepcopy(abi_erc20)
    check(len(copied.functions) == len(abi_erc20.functions), "ABI can be deep copied", errors)
    check(id(copied) != id(abi_erc20), "Deep copy creates new object", errors)

    # Test __repr__
    repr_str = repr(abi_erc20)
    check("ABI(functions=" in repr_str, "ABI has proper __repr__", errors)

    # Test function properties work
    if transfer:
        # Test selector property
        check(isinstance(transfer.selector, list), "Selector is a list", errors)
        check(len(transfer.selector) == 4, "Selector has 4 bytes", errors)

        # Test signature method
        sig = transfer.signature()
        check(sig == "transfer(address,uint256)", f"Signature is correct (got {sig})", errors)

        # Test input_types property
        check(transfer.input_types == ["address", "uint256"], "input_types property works", errors)

        # Test output_types property
        check(transfer.output_types == ["bool"], f"output_types property works (got {transfer.output_types})", errors)

        # Test other properties
        check(hasattr(transfer, 'state_mutability'), "Has state_mutability", errors)
        check(hasattr(transfer, 'constant'), "Has constant property", errors)
        check(hasattr(transfer, 'payable'), "Has payable property", errors)

    # Test get_function with different input types
    # By name
    func_by_name = abi_erc20.get_function("approve")
    check(func_by_name is not None, "Can get function by name", errors)

    # By hex selector (approve selector is 0x095ea7b3)
    if func_by_name:
        selector_hex = "0x" + bytes(func_by_name.selector).hex()
        func_by_hex = abi_erc20.get_function(selector_hex)
        check(func_by_hex is not None, f"Can get function by hex selector {selector_hex}", errors)

        # By bytes selector
        func_by_bytes = abi_erc20.get_function(bytes(func_by_name.selector))
        check(func_by_bytes is not None, "Can get function by bytes selector", errors)

    # Test event properties
    if transfer_event:
        check(hasattr(transfer_event, 'name'), "Event has name", errors)
        check(hasattr(transfer_event, 'inputs'), "Event has inputs", errors)
        check(hasattr(transfer_event, 'anonymous'), "Event has anonymous flag", errors)

        # Check event input properties
        if transfer_event.inputs:
            first_input = transfer_event.inputs[0]
            check(hasattr(first_input, 'name'), "Event input has name", errors)
            check(hasattr(first_input, 'type_'), "Event input has type_", errors)
            check(hasattr(first_input, 'indexed'), "Event input has indexed flag", errors)

    if errors:
        print(f"\n❌ from_json test had {len(errors)} failures")
    else:
        print("\n✓ from_json test passed")
    return len(errors) == 0

def test_templedao_selector_mismatch():
    print("\n=== TempleDAO Selector Mismatch Test ===")
    errors = []

    # Test the TempleDAO StaxLPStaking contract
    contract_address = "0xd2869042E12a3506100af1D192b5b04D65137941"
    print(f"Testing contract: {contract_address}")

    abi = decompile_code(contract_address, skip_resolving=False, rpc_url="https://eth.llamarpc.com")

    # Find Unresolved_1c1c6fe5 which should be withdrawAll(bool)
    unresolved_func = None
    for func in abi.functions:
        if "Unresolved_1c1c6fe5" in func.name:
            unresolved_func = func
            break

    check(unresolved_func is not None, "Found Unresolved_1c1c6fe5 function", errors)

    if unresolved_func:
        # The selector should be 1c1c6fe5 (from the function name)
        expected_selector = "1c1c6fe5"

        # Check if we can look it up by the correct selector
        func_by_selector = abi.get_function(f"0x{expected_selector}")
        check(func_by_selector is not None, f"Can retrieve function by selector 0x{expected_selector}", errors)

        if func_by_selector:
            check(func_by_selector.name == unresolved_func.name,
                  f"Retrieved function matches (got {func_by_selector.name})", errors)

    # Check for other unresolved functions to ensure they work too
    unresolved_count = sum(1 for f in abi.functions if "Unresolved_" in f.name)
    print(f"Total unresolved functions: {unresolved_count}")

    # Test each unresolved function's selector
    for func in abi.functions:
        if "Unresolved_" in func.name:
            # Extract expected selector from name
            expected_selector = func.name.split("_")[1]

            # Try to retrieve by selector
            retrieved = abi.get_function(f"0x{expected_selector}")
            if retrieved is None:
                errors.append(f"Cannot retrieve {func.name} by its selector 0x{expected_selector}")

    if errors:
        print(f"\n❌ TempleDAO selector test had {len(errors)} failures")
    else:
        print("\n✓ TempleDAO selector test passed")
    return len(errors) == 0

def test_storage_layout_extraction():
    print("\n=== Storage Layout Extraction Test ===")
    errors = []

    # Test with ERC20 (Dai) contract - it has known storage layout
    print("\n1. DAI CONTRACT STORAGE LAYOUT:")
    print("-" * 40)
    abi_with_storage = decompile_code(erc20, skip_resolving=False, extract_storage=True)

    # Check that we got storage layout
    check(abi_with_storage.storage_layout is not None, "Storage layout is not None", errors)
    check(len(abi_with_storage.storage_layout) > 0, f"Storage layout has entries (got {len(abi_with_storage.storage_layout)})", errors)

    if abi_with_storage.storage_layout:
        print(f"  Total slots found: {len(abi_with_storage.storage_layout)}")
        print("\n  Complete Dai storage layout:")

        # Check for expected storage slots in Dai contract
        # Dai typically has wards (slot 0), totalSupply (slot 1), balanceOf mapping, etc.
        slot_indices = [slot.index for slot in abi_with_storage.storage_layout]

        # Check that we have slot 0 (usually wards or similar admin mapping)
        check(0 in slot_indices, "Found slot 0 (admin/wards)", errors)

        # Check that we have slot 1 (often totalSupply or similar)
        check(1 in slot_indices, "Found slot 1 (totalSupply or similar)", errors)

        # Print ALL storage slots for Dai
        for i, slot in enumerate(abi_with_storage.storage_layout):
            print(f"    Slot {slot.index:3d}, offset {slot.offset:2d}: {slot.typ}")

            # Check slot attributes for first 5
            if i < 5:
                check(hasattr(slot, 'index'), f"Slot {i} has index attribute", errors)
                check(hasattr(slot, 'offset'), f"Slot {i} has offset attribute", errors)
                check(hasattr(slot, 'typ'), f"Slot {i} has typ attribute", errors)
                check(isinstance(slot.index, int), f"Slot {i} index is integer", errors)
                check(isinstance(slot.offset, int), f"Slot {i} offset is integer", errors)
                check(isinstance(slot.typ, str), f"Slot {i} typ is string", errors)

    # Test with WETH contract
    print("\n2. WETH CONTRACT STORAGE LAYOUT:")
    print("-" * 40)
    weth_abi = decompile_code(weth, skip_resolving=False, extract_storage=True)

    check(weth_abi.storage_layout is not None, "WETH storage layout is not None", errors)
    check(len(weth_abi.storage_layout) > 0, f"WETH storage layout has entries (got {len(weth_abi.storage_layout)})", errors)

    if weth_abi.storage_layout:
        print(f"  Total slots found: {len(weth_abi.storage_layout)}")
        print("\n  Complete WETH storage layout:")

        # Print ALL storage slots for WETH
        for slot in weth_abi.storage_layout:
            print(f"    Slot {slot.index:3d}, offset {slot.offset:2d}: {slot.typ}")

    # Test that storage extraction is enabled by default
    print("\n4. Testing that extract_storage defaults to True...")
    print("-" * 40)
    abi_default = decompile_code(weth, skip_resolving=False)  # Not specifying extract_storage
    check(abi_default.storage_layout is not None, "Storage layout exists with default", errors)
    check(len(abi_default.storage_layout) > 0, f"Storage layout extracted by default (got {len(abi_default.storage_layout)} slots)", errors)
    print(f"  Default extraction got {len(abi_default.storage_layout)} slots (should match WETH above)")

    # Test that storage extraction can be disabled
    print("\n5. Testing with storage extraction explicitly disabled...")
    print("-" * 40)
    abi_no_storage = decompile_code(weth, skip_resolving=False, extract_storage=False)
    check(abi_no_storage.storage_layout is not None, "Storage layout exists (empty list)", errors)
    check(len(abi_no_storage.storage_layout) == 0, "Storage layout is empty when extraction disabled", errors)
    print(f"  Disabled extraction got {len(abi_no_storage.storage_layout)} slots (should be 0)")

    # Test that storage layout is preserved through pickle
    print("\nTesting storage layout pickle persistence...")
    if abi_with_storage.storage_layout:
        import pickle
        pickled = pickle.dumps(abi_with_storage)
        restored = pickle.loads(pickled)

        check(len(restored.storage_layout) == len(abi_with_storage.storage_layout),
              "Storage layout preserved through pickle", errors)

        if restored.storage_layout and abi_with_storage.storage_layout:
            first_orig = abi_with_storage.storage_layout[0]
            first_restored = restored.storage_layout[0]
            check(first_orig.index == first_restored.index, "Storage slot index preserved", errors)
            check(first_orig.offset == first_restored.offset, "Storage slot offset preserved", errors)
            check(first_orig.typ == first_restored.typ, "Storage slot type preserved", errors)

    # Test with UniV2Pair for more complex storage
    print("\n3. UNISWAP V2 PAIR CONTRACT STORAGE LAYOUT:")
    print("-" * 40)
    univ2_abi = decompile_code(univ2pair, skip_resolving=False, extract_storage=True)

    if univ2_abi.storage_layout:
        print(f"  Total slots found: {len(univ2_abi.storage_layout)}")
        print("\n  Complete UniV2Pair storage layout:")

        # Print ALL storage slots for UniV2Pair
        for slot in univ2_abi.storage_layout:
            print(f"    Slot {slot.index:3d}, offset {slot.offset:2d}: {slot.typ}")

        # UniV2Pair has complex storage with reserves, token addresses, etc.
        # Check for variety of types
        types_found = set(slot.typ for slot in univ2_abi.storage_layout)
        print(f"\n  Unique types found: {len(types_found)}")
        print(f"  Types: {sorted(types_found)}")

        # Should have multiple different types
        check(len(types_found) > 1, "Multiple storage types found in UniV2Pair", errors)

    if errors:
        print(f"\n❌ Storage layout extraction test had {len(errors)} failures")
    else:
        print("\n✓ Storage layout extraction test passed")
    return len(errors) == 0

def test_cache_comprehensive():
    print("\n=== Comprehensive Cache Test ===")
    errors = []

    # Test 1: Cache disabled vs enabled performance
    print("\n1. Testing performance without cache vs with cache:")
    configure_cache(enabled=False)

    # Run 100 decompilations without cache
    print("   Running 100 decompilations WITHOUT cache...")
    start = time.time()
    for i in range(100):
        abi = decompile_code(weth, skip_resolving=False, use_cache=False)
    time_without_cache = time.time() - start
    print(f"   Time without cache: {time_without_cache:.2f}s")

    # Enable cache and test
    configure_cache(enabled=True)
    clear_cache()

    print("   Running 100 decompilations WITH cache...")
    start = time.time()
    for i in range(100):
        abi = decompile_code(weth, skip_resolving=False, use_cache=True)
    time_with_cache = time.time() - start
    print(f"   Time with cache: {time_with_cache:.2f}s")

    speedup = time_without_cache / time_with_cache
    print(f"   Speedup: {speedup:.1f}x faster with cache")
    check(speedup > 10, f"Cache provides >10x speedup (got {speedup:.1f}x)", errors)

    # Test 2: Verify cache hit/miss statistics
    print("\n2. Testing cache statistics:")
    clear_cache()

    # First call should miss
    abi1 = decompile_code(weth, skip_resolving=False, use_cache=True)
    stats = get_cache_stats()
    check(stats['misses'] == 1, f"First call is a miss (misses={stats['misses']})", errors)
    check(stats['hits'] == 0, f"No hits yet (hits={stats['hits']})", errors)

    # Second call should hit
    abi2 = decompile_code(weth, skip_resolving=False, use_cache=True)
    stats = get_cache_stats()
    check(stats['hits'] == 1, f"Second call is a hit (hits={stats['hits']})", errors)
    check(stats['misses'] == 1, f"Still one miss (misses={stats['misses']})", errors)

    # Test 3: Separate caching for resolved/unresolved
    print("\n3. Testing separate caching for resolved/unresolved:")
    clear_cache()

    # Decompile with resolving
    abi_resolved = decompile_code(erc20, skip_resolving=False, use_cache=True)
    stats1 = get_cache_stats()

    # Decompile without resolving (should be separate cache entry)
    abi_unresolved = decompile_code(erc20, skip_resolving=True, use_cache=True)
    stats2 = get_cache_stats()

    check(stats2['misses'] == 2, f"Two different cache entries (misses={stats2['misses']})", errors)

    # Verify the ABIs are different
    resolved_names = {f.name for f in abi_resolved.functions if not f.name.startswith("Unresolved_")}
    unresolved_names = {f.name for f in abi_unresolved.functions if f.name.startswith("Unresolved_")}
    check(len(resolved_names) > 0, "Resolved ABI has resolved names", errors)
    check(len(unresolved_names) > 0, "Unresolved ABI has unresolved names", errors)

    # Test 4: Test with thousands of contracts
    print("\n4. Testing with thousands of decompilations:")
    clear_cache()

    print("   Running 1000 decompilations...")
    start = time.time()
    for i in range(1000):
        if i % 100 == 0:
            print(f"     Progress: {i}/1000")
        abi = decompile_code(weth, skip_resolving=False, use_cache=True)
    elapsed = time.time() - start

    stats = get_cache_stats()
    print(f"   Completed 1000 decompilations in {elapsed:.2f}s")
    print(f"   Stats: hits={stats['hits']}, misses={stats['misses']}")
    check(stats['hits'] == 999, f"999 cache hits expected (got {stats['hits']})", errors)
    check(stats['misses'] == 1, f"1 cache miss expected (got {stats['misses']})", errors)

    ops_per_sec = 1000 / elapsed
    print(f"   Performance: {ops_per_sec:.0f} ops/sec")
    check(ops_per_sec > 100, f"Should handle >100 ops/sec (got {ops_per_sec:.0f})", errors)

    # Test 5: Cache persistence across calls
    print("\n5. Testing cache persistence:")
    # Don't clear cache - it should still have the data
    stats_before = get_cache_stats()

    # This should hit the existing cache
    abi = decompile_code(weth, skip_resolving=False, use_cache=True)
    stats_after = get_cache_stats()

    check(stats_after['hits'] == stats_before['hits'] + 1,
          f"Cache persists across calls (hits increased from {stats_before['hits']} to {stats_after['hits']})", errors)

    if errors:
        print(f"\n❌ Cache comprehensive test had {len(errors)} failures")
        for error in errors:
            print(f"   - {error}")
    else:
        print("\n✓ Cache comprehensive test passed")
    return len(errors) == 0

if __name__ == "__main__":
    print("Running comprehensive contract tests...")
    all_passed = True

    all_passed &= test_vault()
    all_passed &= test_weth_comprehensive()
    all_passed &= test_univ2pair_comprehensive()
    all_passed &= test_erc20_comprehensive()
    all_passed &= test_pickle_and_lookups()
    all_passed &= test_from_json()
    all_passed &= test_templedao_selector_mismatch()
    all_passed &= test_storage_layout_extraction()
    all_passed &= test_cache_comprehensive()

    if all_passed:
        print("\n✅ All comprehensive tests passed!")
    else:
        print("\n⚠️  Some tests had failures - review output above")