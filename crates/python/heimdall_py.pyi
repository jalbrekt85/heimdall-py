"""
Type stubs for heimdall_py - Python bindings for Heimdall EVM decompiler.

This module provides functionality to decompile EVM bytecode and extract
the contract's ABI (Application Binary Interface).
"""

from typing import List, Optional, Union

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

    @property
    def selector(self) -> List[int]:
        """Returns the 4-byte function selector as a list of integers."""
        ...

    def signature(self) -> str:
        """Returns the function signature string."""
        ...

    @property
    def input_types(self) -> List[str]:
        """Returns a list of input parameter types."""
        ...

    @property
    def output_types(self) -> List[str]:
        """Returns a list of output parameter types."""
        ...

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

class StorageSlot:
    """Represents a storage slot in the contract's storage layout."""
    index: int
    offset: int
    typ: str

    def __init__(self, index: int = 0, offset: int = 0, typ: str = "") -> None:
        """
        Create a new StorageSlot.

        Args:
            index: The storage slot index
            offset: The offset within the slot
            typ: The type of the storage variable
        """
        ...

class ABI:
    """Complete ABI representation of a smart contract."""
    functions: List[ABIFunction]
    events: List[ABIEvent]
    errors: List[ABIError]
    constructor: Optional[ABIFunction]
    fallback: Optional[ABIFunction]
    receive: Optional[ABIFunction]
    storage_layout: List[StorageSlot]

    def __init__(self) -> None:
        """Create a new empty ABI."""
        ...

    @staticmethod
    def from_json(file_path: str) -> 'ABI':
        """
        Load an ABI from a JSON file following the standard Ethereum ABI format.

        Args:
            file_path: Path to the JSON file containing the ABI

        Returns:
            ABI object with all functions, events, errors, and special functions loaded

        Raises:
            IOError: If the file cannot be read
            ValueError: If the JSON is invalid or not in the expected format

        Example:
            >>> abi = ABI.from_json("abis/erc20.json")
            >>> transfer = abi.get_function("transfer")
            >>> print(f"Transfer selector: 0x{bytes(transfer.selector).hex()}")
        """
        ...

    def get_function(self, key: Union[str, bytes]) -> Optional[ABIFunction]:
        """
        Get a function by name, hex selector string (0x...), or selector bytes.

        Args:
            key: Function name, hex selector string (e.g., "0x12345678"), or 4-byte selector

        Returns:
            The matching ABIFunction, or None if not found
        """
        ...

    def __getstate__(self) -> bytes:
        """Serialize the ABI for pickling."""
        ...

    def __setstate__(self, state: bytes) -> None:
        """Deserialize the ABI from pickle."""
        ...

    def __deepcopy__(self, memo: dict) -> 'ABI':
        """Create a deep copy of the ABI."""
        ...

    def __repr__(self) -> str:
        """String representation of the ABI."""
        ...

def decompile_code(
    code: str,
    skip_resolving: bool = False,
    rpc_url: Optional[str] = None,
    timeout_secs: Optional[int] = None
) -> ABI:
    """
    Decompile EVM bytecode and extract the contract's ABI.

    Args:
        code: Hex-encoded bytecode string (with or without 0x prefix) or contract address
        skip_resolving: If True, skip signature resolution from external databases
        rpc_url: Optional RPC URL for fetching bytecode from contract addresses
        timeout_secs: Optional timeout in seconds (default: 25 seconds)

    Returns:
        ABI object containing all functions, events, errors, and special functions

    Raises:
        RuntimeError: If decompilation fails
        TimeoutError: If decompilation exceeds the timeout

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
        >>>
        >>> # Lookup function by selector
        >>> func = abi.get_function("0x70a08231")  # balanceOf selector
        >>> if func:
        ...     print(f"Found: {func.name}")
    """
    ...