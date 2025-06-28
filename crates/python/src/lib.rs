use alloy_json_abi::{Function, EventParam, Param, StateMutability};
use heimdall_decompiler::{decompile, DecompilerArgsBuilder};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;

#[pyclass]
#[derive(Clone)]
struct ABIParam {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    type_: String,
    #[pyo3(get)]
    internal_type: Option<String>,
}

#[pyclass]
#[derive(Clone)]
struct ABIFunction {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    inputs: Vec<ABIParam>,
    #[pyo3(get)]
    outputs: Vec<ABIParam>,
    #[pyo3(get)]
    state_mutability: String,
    #[pyo3(get)]
    constant: bool,
    #[pyo3(get)]
    payable: bool,
}

#[pyclass]
#[derive(Clone)]
struct ABIEventParam {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    type_: String,
    #[pyo3(get)]
    indexed: bool,
    #[pyo3(get)]
    internal_type: Option<String>,
}

#[pyclass]
#[derive(Clone)]
struct ABIEvent {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    inputs: Vec<ABIEventParam>,
    #[pyo3(get)]
    anonymous: bool,
}

#[pyclass]
#[derive(Clone)]
struct ABIError {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    inputs: Vec<ABIParam>,
}

#[pyclass]
#[derive(Clone)]
struct ABI {
    #[pyo3(get)]
    functions: Vec<ABIFunction>,
    #[pyo3(get)]
    events: Vec<ABIEvent>,
    #[pyo3(get)]
    errors: Vec<ABIError>,
    #[pyo3(get)]
    constructor: Option<ABIFunction>,
    #[pyo3(get)]
    fallback: Option<ABIFunction>,
    #[pyo3(get)]
    receive: Option<ABIFunction>,
}

fn convert_param(param: &Param) -> ABIParam {
    ABIParam {
        name: param.name.clone(),
        type_: param.ty.clone(),
        internal_type: param.internal_type.as_ref().map(|t| t.to_string()),
    }
}

fn convert_event_param(param: &EventParam) -> ABIEventParam {
    ABIEventParam {
        name: param.name.clone(),
        type_: param.ty.clone(),
        indexed: param.indexed,
        internal_type: param.internal_type.as_ref().map(|t| t.to_string()),
    }
}

fn convert_function(func: &Function) -> ABIFunction {
    ABIFunction {
        name: func.name.clone(),
        inputs: func.inputs.iter().map(convert_param).collect(),
        outputs: func.outputs.iter().map(convert_param).collect(),
        state_mutability: match func.state_mutability {
            StateMutability::Pure => "pure".to_string(),
            StateMutability::View => "view".to_string(),
            StateMutability::NonPayable => "nonpayable".to_string(),
            StateMutability::Payable => "payable".to_string(),
        },
        constant: matches!(func.state_mutability, StateMutability::Pure | StateMutability::View),
        payable: matches!(func.state_mutability, StateMutability::Payable),
    }
}

#[pyfunction]
#[pyo3(signature = (code, skip_resolving=false, rpc_url=None))]
fn decompile_code(code: String, skip_resolving: bool, rpc_url: Option<String>) -> PyResult<ABI> {
    // Build decompiler args
    let args = DecompilerArgsBuilder::new()
        .target(code)
        .rpc_url(rpc_url.unwrap_or_default())
        .default(true)
        .skip_resolving(skip_resolving)
        .include_solidity(false)
        .include_yul(false)
        .output(String::new())
        .timeout(25000)
        .build()
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to build args: {}", e)))?;
    
    // Create a new tokio runtime and block on the async function
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;
    
    let result = runtime.block_on(async move {
        decompile(args)
            .await
            .map_err(|e| PyRuntimeError::new_err(format!("Decompilation failed: {}", e)))
    })?;
    
    // Convert the JsonAbi to our Python ABI structure
    let json_abi = result.abi;
    
    let functions: Vec<ABIFunction> = json_abi
        .functions()
        .map(convert_function)
        .collect();
    
    let events: Vec<ABIEvent> = json_abi
        .events()
        .map(|event| ABIEvent {
            name: event.name.clone(),
            inputs: event.inputs.iter().map(convert_event_param).collect(),
            anonymous: event.anonymous,
        })
        .collect();
    
    let errors: Vec<ABIError> = json_abi
        .errors()
        .map(|error| ABIError {
            name: error.name.clone(),
            inputs: error.inputs.iter().map(convert_param).collect(),
        })
        .collect();
    
    let constructor = json_abi.constructor.as_ref().map(|c| ABIFunction {
        name: "constructor".to_string(),
        inputs: c.inputs.iter().map(convert_param).collect(),
        outputs: Vec::new(),
        state_mutability: match c.state_mutability {
            StateMutability::Pure => "pure".to_string(),
            StateMutability::View => "view".to_string(),
            StateMutability::NonPayable => "nonpayable".to_string(),
            StateMutability::Payable => "payable".to_string(),
        },
        constant: false,
        payable: matches!(c.state_mutability, StateMutability::Payable),
    });
    
    let fallback = json_abi.fallback.as_ref().map(|f| ABIFunction {
        name: "fallback".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        state_mutability: match f.state_mutability {
            StateMutability::Pure => "pure".to_string(),
            StateMutability::View => "view".to_string(),
            StateMutability::NonPayable => "nonpayable".to_string(),
            StateMutability::Payable => "payable".to_string(),
        },
        constant: false,
        payable: matches!(f.state_mutability, StateMutability::Payable),
    });
    
    let receive = json_abi.receive.as_ref().map(|_| ABIFunction {
        name: "receive".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        state_mutability: "payable".to_string(),
        constant: false,
        payable: true,
    });
    
    Ok(ABI {
        functions,
        events,
        errors,
        constructor,
        fallback,
        receive,
    })
}

/// Python module for Heimdall decompiler
#[pymodule]
fn heimdall_py(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<ABIParam>()?;
    m.add_class::<ABIFunction>()?;
    m.add_class::<ABIEventParam>()?;
    m.add_class::<ABIEvent>()?;
    m.add_class::<ABIError>()?;
    m.add_class::<ABI>()?;
    m.add_function(wrap_pyfunction!(decompile_code, m)?)?;
    Ok(())
}