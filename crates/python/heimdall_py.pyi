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
    extract_storage: bool = True,
    use_cache: bool = True,
    rpc_url: Optional[str] = None,
    timeout_secs: Optional[int] = None
) -> ABI:
    """
    Decompile EVM bytecode and extract the contract's ABI.

    Args:
        code: Hex-encoded bytecode string (with or without 0x prefix) or contract address
        skip_resolving: If True, skip signature resolution from external databases
        extract_storage: If True, extract storage layout from bytecode
        use_cache: If True, use LMDB cache for storing/retrieving ABIs
        rpc_url: Optional RPC URL for fetching bytecode from contract addresses
        timeout_secs: Optional timeout in seconds (default: 25 seconds)

    Returns:
        ABI object containing all functions, events, errors, special functions, and optionally storage layout

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
        >>> # Extract storage layout along with ABI
        >>> abi = decompile_code(bytecode, extract_storage=True)
        >>> for slot in abi.storage_layout:
        ...     print(f"Slot {slot.index}: {slot.typ}")
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

def configure_cache(
    enabled: bool = True,
    directory: Optional[str] = None
) -> None:
    """
    Configure the ABI cache settings.

    Args:
        enabled: Whether to enable caching
        directory: Optional custom cache directory path. If not provided, uses:
                  - Linux: ~/.cache/heimdall/
                  - macOS: ~/Library/Caches/heimdall/
                  - Windows: %LOCALAPPDATA%\\heimdall\\cache\\
                  - Or $HEIMDALL_CACHE_DIR if set

    Example:
        >>> # Disable caching
        >>> configure_cache(enabled=False)
        >>>
        >>> # Use custom cache directory
        >>> configure_cache(directory="/tmp/my_cache")
    """
    ...

def clear_cache() -> None:
    """
    Clear all entries from the ABI cache.

    Example:
        >>> clear_cache()
        >>> # All cached ABIs are now removed
    """
    ...

def get_cache_stats() -> dict:
    """
    Get statistics about cache usage.

    Returns:
        Dictionary with cache statistics:
        - hits: Number of cache hits
        - misses: Number of cache misses
        - writes: Number of successful cache writes
        - errors: Number of cache errors
        - hit_rate: Ratio of hits to total requests (0.0 to 1.0)
        - enabled: Whether cache is currently enabled

    Example:
        >>> stats = get_cache_stats()
        >>> print(f"Cache hit rate: {stats['hit_rate']:.2%}")
        >>> print(f"Total hits: {stats['hits']}")
    """
    ...