"""
Type stubs for heimdall_py - Python bindings for Heimdall EVM decompiler.

This module provides functionality to decompile EVM bytecode and extract
the contract's ABI (Application Binary Interface).
"""

from typing import List, Optional

class ABIParam:
    """Represents a parameter in a function, event, or error."""
    name: str
    type_: str
    internal_type: Optional[str]

class ABIFunction:
    """Represents a function in the contract ABI."""
    name: str
    inputs: List[ABIParam]
    outputs: List[ABIParam]
    state_mutability: str  # "pure", "view", "nonpayable", or "payable"
    constant: bool
    payable: bool

class ABIEventParam:
    """Represents a parameter in an event."""
    name: str
    type_: str
    indexed: bool
    internal_type: Optional[str]

class ABIEvent:
    """Represents an event in the contract ABI."""
    name: str
    inputs: List[ABIEventParam]
    anonymous: bool

class ABIError:
    """Represents a custom error in the contract ABI."""
    name: str
    inputs: List[ABIParam]

class ABI:
    """Complete ABI representation of a smart contract."""
    functions: List[ABIFunction]
    events: List[ABIEvent]
    errors: List[ABIError]
    constructor: Optional[ABIFunction]
    fallback: Optional[ABIFunction]
    receive: Optional[ABIFunction]

def decompile_code(code: str, skip_resolving: bool = False, rpc_url: Optional[str] = None) -> ABI:
    """
    Decompile EVM bytecode and extract the contract's ABI.
    
    Args:
        code: Hex-encoded bytecode string (with or without 0x prefix) or contract address
        skip_resolving: If True, skip signature resolution from external databases
        rpc_url: Optional RPC URL for fetching bytecode from contract addresses
        
    Returns:
        ABI object containing all functions, events, errors, and special functions
        
    Raises:
        RuntimeError: If decompilation fails
        
    Example:
        >>> # Decompile bytecode directly
        >>> bytecode = "0x60806040..."
        >>> abi = decompile_code(bytecode)
        >>> for func in abi.functions:
        ...     print(f"{func.name}({', '.join(p.type_ for p in func.inputs)})")
        >>> 
        >>> # Skip signature resolution for faster decompilation
        >>> abi = decompile_code(bytecode, skip_resolving=True)
        >>> 
        >>> # Decompile from contract address (requires RPC URL)
        >>> abi = decompile_code("0x123...", rpc_url="https://localhost:8545")
    """
    ...