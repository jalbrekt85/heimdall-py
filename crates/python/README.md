# Heimdall Python Bindings

Python bindings for the Heimdall EVM decompiler, providing access to extract ABI (Application Binary Interface) from EVM bytecode.

## Installation

Build and install the Python module:

```bash
# From the heimdall-rs root directory
cd crates/python
maturin develop
```

## Usage

```python
import heimdall_py

# Decompile bytecode to extract ABI
bytecode = "0x60806040..."
abi = heimdall_py.decompile_code(bytecode)

# Access functions
for func in abi.functions:
    print(f"Function: {func.name}")
    print(f"  Inputs: {[(p.name, p.type_) for p in func.inputs]}")
    print(f"  Outputs: {[(p.name, p.type_) for p in func.outputs]}")
    print(f"  State Mutability: {func.state_mutability}")
    print(f"  Payable: {func.payable}")

# Access events
for event in abi.events:
    print(f"Event: {event.name}")
    for param in event.inputs:
        print(f"  {param.name}: {param.type_} (indexed: {param.indexed})")

# Access errors
for error in abi.errors:
    print(f"Error: {error.name}")
    for param in error.inputs:
        print(f"  {param.name}: {param.type_}")

# Special functions
if abi.constructor:
    print(f"Constructor inputs: {[(p.name, p.type_) for p in abi.constructor.inputs]}")

if abi.fallback:
    print(f"Has fallback function (payable: {abi.fallback.payable})")

if abi.receive:
    print("Has receive function")
```

## ABI Structure

The decompiler returns an `ABI` object with the following structure:

- `functions`: List of regular functions
- `events`: List of events
- `errors`: List of custom errors
- `constructor`: Optional constructor function
- `fallback`: Optional fallback function
- `receive`: Optional receive function

Each function contains:
- `name`: Function name
- `inputs`: List of input parameters
- `outputs`: List of output parameters
- `state_mutability`: One of "pure", "view", "nonpayable", or "payable"
- `constant`: Boolean indicating if function is constant (view/pure)
- `payable`: Boolean indicating if function accepts Ether

## Development

To build the Python bindings:

```bash
# Install maturin
pip install maturin

# Build in development mode
maturin develop

# Build release wheel
maturin build --release
```

## Requirements

- Python 3.10+
- Rust toolchain
- maturin for building