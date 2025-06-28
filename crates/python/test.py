from heimdall_py import decompile_code

with open("contracts/vault.bin", "r") as f:
    vault = f.readline().strip()

with open("contracts/weth.bin", "r") as f:
    weth = f.readline().strip()

for bytecode in [vault, weth]:
    abi = decompile_code(bytecode, skip_resolving=True)

    # Display functions
    print("\nFunctions:")
    for func in abi.functions:
        inputs = ", ".join([f"{p.name}: {p.type_}" if p.name else p.type_ for p in func.inputs])
        outputs = ", ".join([f"{p.name}: {p.type_}" if p.name else p.type_ for p in func.outputs])
        print(f"  {func.name}({inputs}) -> ({outputs})")
        print(f"    State Mutability: {func.state_mutability}")
        print(f"    Payable: {func.payable}")

    # Display events
    if abi.events:
        print("\nEvents:")
        for event in abi.events:
            params = ", ".join([f"{p.name}: {p.type_}" + (" indexed" if p.indexed else "") for p in event.inputs])
            print(f"  {event.name}({params})")
            if event.anonymous:
                print("    Anonymous: true")

    # Display errors
    if abi.errors:
        print("\nErrors:")
        for error in abi.errors:
            params = ", ".join([f"{p.name}: {p.type_}" for p in error.inputs])
            print(f"  {error.name}({params})")

    # Display special functions
    if abi.constructor:
        print("\nConstructor:")
        inputs = ", ".join([f"{p.name}: {p.type_}" if p.name else p.type_ for p in abi.constructor.inputs])
        print(f"  constructor({inputs})")
        print(f"    Payable: {abi.constructor.payable}")

    if abi.fallback:
        print("\nFallback:")
        print(f"  Payable: {abi.fallback.payable}")

    if abi.receive:
        print("\nReceive:")
        print("  Payable: true")

    print("\nRaw function data:")
    print([{
        "name": x.name, 
        "inputs": [{"name": p.name, "type": p.type_} for p in x.inputs], 
        "outputs": [{"name": p.name, "type": p.type_} for p in x.outputs]
    } for x in abi.functions])