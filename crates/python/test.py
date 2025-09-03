from heimdall_py import decompile_code

with open("contracts/vault.bin", "r") as f:
    vault = f.readline().strip()

with open("contracts/weth.bin", "r") as f:
    weth = f.readline().strip()

with open("contracts/erc20.bin", "r") as f:
    erc20 = f.readline().strip()

def test_erc20_comprehensive():
    abi = decompile_code(erc20, skip_resolving=False)
    
    totalSupply = next((func for func in abi.functions if func.name == "totalSupply"), None)
    transfer = next((func for func in abi.functions if func.name == "transfer"), None)
    balanceOf = next((func for func in abi.functions if func.name == "balanceOf"), None)
    approve = next((func for func in abi.functions if func.name == "approve"), None)
    transferFrom = next((func for func in abi.functions if func.name == "transferFrom"), None)
    allowance = next((func for func in abi.functions if func.name == "allowance"), None)
    
    assert totalSupply is not None, "totalSupply function not found"
    assert totalSupply.inputs == [], f"totalSupply should have no inputs, got {[i.type_ for i in totalSupply.inputs]}"
    assert [o.type_ for o in totalSupply.outputs] == ["uint256"], f"totalSupply should return uint256, got {[o.type_ for o in totalSupply.outputs]}"
    assert totalSupply.constant == True, "totalSupply should be constant"
    
    assert transfer is not None, "transfer function not found"
    assert [i.type_ for i in transfer.inputs] == ["address", "uint256"], f"transfer should accept (address, uint256), got {[i.type_ for i in transfer.inputs]}"
    assert [o.type_ for o in transfer.outputs] == ["bool"], f"transfer should return bool, got {[o.type_ for o in transfer.outputs]}"
    
    assert balanceOf is not None, "balanceOf function not found"
    assert [i.type_ for i in balanceOf.inputs] == ["address"], f"balanceOf should accept address, got {[i.type_ for i in balanceOf.inputs]}"
    assert [o.type_ for o in balanceOf.outputs] == ["uint256"], f"balanceOf should return uint256, got {[o.type_ for o in balanceOf.outputs]}"
    assert balanceOf.constant == True, "balanceOf should be constant"
    
    assert approve is not None, "approve function not found"
    assert [i.type_ for i in approve.inputs] == ["address", "uint256"], f"approve should accept (address, uint256), got {[i.type_ for i in approve.inputs]}"
    assert [o.type_ for o in approve.outputs] == ["bool"], f"approve should return bool, got {[o.type_ for o in approve.outputs]}"
    
    assert transferFrom is not None, "transferFrom function not found"
    assert [i.type_ for i in transferFrom.inputs] == ["address", "address", "uint256"], f"transferFrom should accept (address, address, uint256), got {[i.type_ for i in transferFrom.inputs]}"
    assert [o.type_ for o in transferFrom.outputs] == ["bool"], f"transferFrom should return bool, got {[o.type_ for o in transferFrom.outputs]}"
    
    assert allowance is not None, "allowance function not found"
    assert [i.type_ for i in allowance.inputs] == ["address", "address"], f"allowance should accept (address, address), got {[i.type_ for i in allowance.inputs]}"
    assert [o.type_ for o in allowance.outputs] == ["uint256"], f"allowance should return uint256, got {[o.type_ for o in allowance.outputs]}"
    assert allowance.constant == True, "allowance should be constant"
    
    decimals = next((func for func in abi.functions if func.name == "decimals"), None)
    if decimals:
        assert len(decimals.outputs) == 1, f"decimals should have one output, got {len(decimals.outputs)}"
        output_type = decimals.outputs[0].type_
        assert output_type in ["uint8", "uint256"], f"decimals should return uint8 or uint256, got {output_type}"
    
    factory = next((func for func in abi.functions if func.name == "factory"), None)
    if factory:
        assert [o.type_ for o in factory.outputs] == ["address"], f"factory should return address, got {[o.type_ for o in factory.outputs]}"
    
    print("✓ ERC20 comprehensive test passed")

def test_weth_comprehensive():
    abi = decompile_code(weth, skip_resolving=False)
    
    deposit = next((func for func in abi.functions if func.name == "deposit"), None)
    withdraw = next((func for func in abi.functions if func.name == "withdraw"), None)
    
    balanceOf = next((func for func in abi.functions if func.name == "balanceOf"), None)
    transfer = next((func for func in abi.functions if func.name == "transfer"), None)
    approve = next((func for func in abi.functions if func.name == "approve"), None)
    transferFrom = next((func for func in abi.functions if func.name == "transferFrom"), None)
    totalSupply = next((func for func in abi.functions if func.name == "totalSupply"), None)
    allowance = next((func for func in abi.functions if func.name == "allowance"), None)
    
    assert deposit is not None, "deposit function not found"
    assert deposit.inputs == [], f"deposit should have no parameters, got {[i.type_ for i in deposit.inputs]}"
    assert deposit.payable == True, "deposit should be payable"
    
    assert withdraw is not None, "withdraw function not found"
    assert [i.type_ for i in withdraw.inputs] == ["uint256"], f"withdraw should accept uint256, got {[i.type_ for i in withdraw.inputs]}"
    
    assert balanceOf is not None, "balanceOf function not found"
    assert [i.type_ for i in balanceOf.inputs] == ["address"], f"balanceOf should accept address, got {[i.type_ for i in balanceOf.inputs]}"
    assert [o.type_ for o in balanceOf.outputs] == ["uint256"], f"balanceOf should return uint256, got {[o.type_ for o in balanceOf.outputs]}"
    
    assert transfer is not None, "transfer function not found"
    assert [i.type_ for i in transfer.inputs] == ["address", "uint256"], f"transfer should accept (address, uint256), got {[i.type_ for i in transfer.inputs]}"
    assert [o.type_ for o in transfer.outputs] == ["bool"], f"transfer should return bool, got {[o.type_ for o in transfer.outputs]}"
    
    assert approve is not None, "approve function not found"
    assert [i.type_ for i in approve.inputs] == ["address", "uint256"], f"approve should accept (address, uint256), got {[i.type_ for i in approve.inputs]}"
    assert [o.type_ for o in approve.outputs] == ["bool"], f"approve should return bool, got {[o.type_ for o in approve.outputs]}"
    
    assert transferFrom is not None, "transferFrom function not found"
    assert [i.type_ for i in transferFrom.inputs] == ["address", "address", "uint256"], f"transferFrom should accept (address, address, uint256), got {[i.type_ for i in transferFrom.inputs]}"
    assert [o.type_ for o in transferFrom.outputs] == ["bool"], f"transferFrom should return bool, got {[o.type_ for o in transferFrom.outputs]}"
    
    assert totalSupply is not None, "totalSupply function not found"
    assert totalSupply.inputs == [], f"totalSupply should have no inputs, got {[i.type_ for i in totalSupply.inputs]}"
    assert [o.type_ for o in totalSupply.outputs] == ["uint256"], f"totalSupply should return uint256, got {[o.type_ for o in totalSupply.outputs]}"
    
    assert allowance is not None, "allowance function not found"
    assert [i.type_ for i in allowance.inputs] == ["address", "address"], f"allowance should accept (address, address), got {[i.type_ for i in allowance.inputs]}"
    assert [o.type_ for o in allowance.outputs] == ["uint256"], f"allowance should return uint256, got {[o.type_ for o in allowance.outputs]}"
    
    decimals = next((func for func in abi.functions if func.name == "decimals"), None)
    if decimals:
        assert len(decimals.outputs) == 1, f"decimals should have one output, got {len(decimals.outputs)}"
        output_type = decimals.outputs[0].type_
        assert output_type in ["uint8", "uint256"], f"decimals should return uint8 or uint256, got {output_type}"
    
    name = next((func for func in abi.functions if func.name == "name"), None)
    if name:
        assert len(name.outputs) == 1, f"name should have one output, got {len(name.outputs)}"
        assert name.outputs[0].type_ == "string", f"name should return string, got {name.outputs[0].type_}"
    
    symbol = next((func for func in abi.functions if func.name == "symbol"), None)
    if symbol:
        assert len(symbol.outputs) == 1, f"symbol should have one output, got {len(symbol.outputs)}"
        assert symbol.outputs[0].type_ == "string", f"symbol should return string, got {symbol.outputs[0].type_}"
    
    print("✓ WETH comprehensive test passed")

def test_vault():
    """Test vault contract (kept from original tests)"""
    abi = decompile_code(vault, skip_resolving=False)

    weth = next((func for func in abi.functions if func.name == "WETH"), None)
    assert weth is not None, "WETH function not found"
    assert [o.type_ for o in weth.outputs] == ["address"], f"WETH function should return address, got {[o.type_ for o in weth.outputs]}"

    getNextNonce = next((func for func in abi.functions if func.name == "getNextNonce"), None)
    assert getNextNonce is not None, "getNextNonce function not found"
    assert [i.type_ for i in getNextNonce.inputs] == ["address"], f"getNextNonce function should accept address, got {[i.type_ for i in getNextNonce.inputs]}"
    assert [o.type_ for o in getNextNonce.outputs] == ["uint256"], f"getNextNonce function should return uint256, got {[o.type_ for o in getNextNonce.outputs]}"
    
    print("✓ Vault test passed")

if __name__ == "__main__":
    print("Running comprehensive contract tests...\n")
    test_vault()
    test_weth_comprehensive()
    test_erc20_comprehensive()
    print("\n✅ All comprehensive tests passed!")