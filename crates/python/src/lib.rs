use alloy_json_abi::{Function, EventParam, Param, StateMutability};
use heimdall_decompiler::{decompile, DecompilerArgsBuilder};
use indexmap::IndexMap;
use pyo3::exceptions::{PyRuntimeError, PyTimeoutError, PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tiny_keccak::{Hasher, Keccak};

#[pyclass(module = "heimdall_py")]
#[derive(Clone, Serialize, Deserialize)]
struct ABIParam {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    type_: String,
    #[pyo3(get)]
    internal_type: Option<String>,
}

#[pyclass(module = "heimdall_py")]
#[derive(Clone, Serialize, Deserialize)]
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
    
    selector: [u8; 4],
    signature: String,
}

#[pyclass(module = "heimdall_py")]
#[derive(Clone, Serialize, Deserialize)]
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

#[pyclass(module = "heimdall_py")]
#[derive(Clone, Serialize, Deserialize)]
struct ABIEvent {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    inputs: Vec<ABIEventParam>,
    #[pyo3(get)]
    anonymous: bool,
}

#[pyclass(module = "heimdall_py")]
#[derive(Clone, Serialize, Deserialize)]
struct ABIError {
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    inputs: Vec<ABIParam>,
}

#[pyclass(module = "heimdall_py")]
#[derive(Clone, Serialize, Deserialize)]
struct StorageSlot {
    #[pyo3(get, set)]
    index: u64,
    #[pyo3(get, set)]
    offset: u32,
    #[pyo3(get, set)]
    typ: String,
}

#[pymethods]
impl StorageSlot {
    #[new]
    #[pyo3(signature = (index=0, offset=0, typ=String::new()))]
    fn new(index: u64, offset: u32, typ: String) -> Self {
        StorageSlot { index, offset, typ }
    }
}

#[pyclass(module = "heimdall_py")]
#[derive(Clone, Serialize, Deserialize)]
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
    
    #[pyo3(get, set)]
    storage_layout: Vec<StorageSlot>,
    
    by_selector: IndexMap<[u8; 4], usize>,
    by_name: IndexMap<String, usize>,
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

fn state_mutability_to_string(sm: StateMutability) -> String {
    match sm {
        StateMutability::Pure => "pure",
        StateMutability::View => "view",
        StateMutability::NonPayable => "nonpayable",
        StateMutability::Payable => "payable",
    }.to_string()
}

// Helper function to collapse tuple types
fn collapse_if_tuple(component: &Value) -> PyResult<String> {
    let typ = component.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if !typ.starts_with("tuple") {
        return Ok(typ.to_string());
    }

    let components = component.get("components")
        .and_then(|v| v.as_array());

    let components = match components {
        Some(comps) => comps,
        None => return Ok(typ.to_string()),
    };

    if components.is_empty() {
        return Ok(typ.to_string());
    }

    let mut collapsed_components = Vec::new();
    for comp in components {
        collapsed_components.push(collapse_if_tuple(comp)?);
    }

    let delimited = collapsed_components.join(",");
    let array_dim = &typ[5..]; // Everything after "tuple"
    Ok(format!("({}){}", delimited, array_dim))
}

fn parse_param(param: &Value) -> PyResult<ABIParam> {
    let name = param.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let type_ = collapse_if_tuple(param)?;

    let internal_type = param.get("internalType")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(ABIParam {
        name,
        type_,
        internal_type,
    })
}

fn parse_event_param(param: &Value) -> PyResult<ABIEventParam> {
    let name = param.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let type_ = collapse_if_tuple(param)?;

    let indexed = param.get("indexed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let internal_type = param.get("internalType")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(ABIEventParam {
        name,
        type_,
        indexed,
        internal_type,
    })
}

fn compute_selector(name: &str, input_types: &[String]) -> [u8; 4] {
    let signature = format!("{}({})", name, input_types.join(","));
    let mut hasher = Keccak::v256();
    hasher.update(signature.as_bytes());
    let mut output = [0u8; 32];
    hasher.finalize(&mut output);
    let mut selector = [0u8; 4];
    selector.copy_from_slice(&output[..4]);
    selector
}

fn parse_function_entry(entry: &Value) -> PyResult<Option<ABIFunction>> {
    let name = entry.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if name.is_empty() {
        return Ok(None);
    }

    let inputs_json = entry.get("inputs")
        .and_then(|v| v.as_array());

    let mut inputs = Vec::new();
    let mut input_types = Vec::new();

    if let Some(inputs_json) = inputs_json {
        for input in inputs_json {
            let param = parse_param(input)?;
            input_types.push(param.type_.clone());
            inputs.push(param);
        }
    }

    let outputs_json = entry.get("outputs")
        .and_then(|v| v.as_array());

    let mut outputs = Vec::new();
    if let Some(outputs_json) = outputs_json {
        for output in outputs_json {
            outputs.push(parse_param(output)?);
        }
    }

    let state_mutability = entry.get("stateMutability")
        .and_then(|v| v.as_str())
        .unwrap_or("nonpayable")
        .to_string();

    let constant = state_mutability == "view" || state_mutability == "pure";
    let payable = state_mutability == "payable";

    let selector = compute_selector(&name, &input_types);
    let signature = format!("{}({})", name, input_types.join(","));

    Ok(Some(ABIFunction {
        name,
        inputs,
        outputs,
        state_mutability,
        constant,
        payable,
        selector,
        signature,
    }))
}

fn parse_event_entry(entry: &Value) -> PyResult<Option<ABIEvent>> {
    let name = entry.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if name.is_empty() {
        return Ok(None);
    }

    let inputs_json = entry.get("inputs")
        .and_then(|v| v.as_array());

    let mut inputs = Vec::new();
    if let Some(inputs_json) = inputs_json {
        for input in inputs_json {
            inputs.push(parse_event_param(input)?);
        }
    }

    let anonymous = entry.get("anonymous")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(Some(ABIEvent {
        name,
        inputs,
        anonymous,
    }))
}

fn parse_error_entry(entry: &Value) -> PyResult<Option<ABIError>> {
    let name = entry.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if name.is_empty() {
        return Ok(None);
    }

    let inputs_json = entry.get("inputs")
        .and_then(|v| v.as_array());

    let mut inputs = Vec::new();
    if let Some(inputs_json) = inputs_json {
        for input in inputs_json {
            inputs.push(parse_param(input)?);
        }
    }

    Ok(Some(ABIError {
        name,
        inputs,
    }))
}

fn parse_constructor_entry(entry: &Value) -> PyResult<Option<ABIFunction>> {
    let inputs_json = entry.get("inputs")
        .and_then(|v| v.as_array());

    let mut inputs = Vec::new();
    let mut input_types = Vec::new();

    if let Some(inputs_json) = inputs_json {
        for input in inputs_json {
            let param = parse_param(input)?;
            input_types.push(param.type_.clone());
            inputs.push(param);
        }
    }

    let state_mutability = entry.get("stateMutability")
        .and_then(|v| v.as_str())
        .unwrap_or("nonpayable")
        .to_string();

    let payable = state_mutability == "payable";
    let signature = format!("constructor({})", input_types.join(","));

    Ok(Some(ABIFunction {
        name: "constructor".to_string(),
        inputs,
        outputs: Vec::new(),
        state_mutability,
        constant: false,
        payable,
        selector: [0; 4],
        signature,
    }))
}

fn parse_fallback_entry(entry: &Value) -> PyResult<Option<ABIFunction>> {
    let state_mutability = entry.get("stateMutability")
        .and_then(|v| v.as_str())
        .unwrap_or("nonpayable")
        .to_string();

    let payable = state_mutability == "payable";

    Ok(Some(ABIFunction {
        name: "fallback".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        state_mutability,
        constant: false,
        payable,
        selector: [0; 4],
        signature: "fallback()".to_string(),
    }))
}

fn parse_receive_entry(_entry: &Value) -> PyResult<Option<ABIFunction>> {
    Ok(Some(ABIFunction {
        name: "receive".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        state_mutability: "payable".to_string(),
        constant: false,
        payable: true,
        selector: [0; 4],
        signature: "receive()".to_string(),
    }))
}

#[pymethods]
impl ABIFunction {
    #[getter]
    fn selector(&self) -> Vec<u8> {
        self.selector.to_vec()
    }
    
    fn signature(&self) -> String {
        self.signature.clone()
    }
    
    #[getter]
    fn input_types(&self) -> Vec<String> {
        self.inputs.iter().map(|p| p.type_.clone()).collect()
    }
    
    #[getter]
    fn output_types(&self) -> Vec<String> {
        self.outputs.iter().map(|p| p.type_.clone()).collect()
    }
}


#[pymethods]
impl ABI {
    #[new]
    fn new() -> Self {
        ABI {
            functions: Vec::new(),
            events: Vec::new(),
            errors: Vec::new(),
            constructor: None,
            fallback: None,
            receive: None,
            storage_layout: Vec::new(),
            by_selector: IndexMap::new(),
            by_name: IndexMap::new(),
        }
    }

    #[staticmethod]
    fn from_json(file_path: String) -> PyResult<Self> {
        // Read the JSON file
        let contents = fs::read_to_string(&file_path)
            .map_err(|e| PyIOError::new_err(format!("Failed to read file {}: {}", file_path, e)))?;

        // Parse the JSON
        let json_value: Value = serde_json::from_str(&contents)
            .map_err(|e| PyValueError::new_err(format!("Invalid JSON: {}", e)))?;

        // Parse the ABI array
        let abi_array = if let Some(obj) = json_value.as_object() {
            // Handle { "abi": [...] } format
            obj.get("abi")
                .and_then(|v| v.as_array())
                .ok_or_else(|| PyValueError::new_err("Expected 'abi' field with array"))?
        } else if let Some(arr) = json_value.as_array() {
            // Handle direct array format
            arr
        } else {
            return Err(PyValueError::new_err("JSON must be an array or object with 'abi' field"));
        };

        let mut abi = ABI::new();

        // Process each entry in the ABI
        for entry in abi_array {
            let entry_type = entry.get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match entry_type {
                "function" => {
                    if let Some(func) = parse_function_entry(entry)? {
                        let idx = abi.functions.len();
                        abi.by_selector.insert(func.selector, idx);
                        if !func.name.is_empty() {
                            abi.by_name.insert(func.name.clone(), idx);
                        }
                        abi.functions.push(func);
                    }
                },
                "event" => {
                    if let Some(event) = parse_event_entry(entry)? {
                        abi.events.push(event);
                    }
                },
                "error" => {
                    if let Some(error) = parse_error_entry(entry)? {
                        abi.errors.push(error);
                    }
                },
                "constructor" => {
                    abi.constructor = parse_constructor_entry(entry)?;
                },
                "fallback" => {
                    abi.fallback = parse_fallback_entry(entry)?;
                },
                "receive" => {
                    abi.receive = parse_receive_entry(entry)?;
                },
                _ => {
                    // Skip unknown types
                }
            }
        }

        Ok(abi)
    }

    fn get_function(&self, _py: Python, key: &PyAny) -> PyResult<Option<ABIFunction>> {
        // Try as string first
        if let Ok(name) = key.extract::<String>() {
            if name.starts_with("0x") {
                // Hex selector like "0x12345678"
                if let Ok(selector_bytes) = hex::decode(&name[2..]) {
                    if selector_bytes.len() >= 4 {
                        let selector: [u8; 4] = selector_bytes[..4].try_into().unwrap();
                        if let Some(&idx) = self.by_selector.get(&selector) {
                            return Ok(Some(self.functions[idx].clone()));
                        }
                    }
                }
            } else {
                // Function name lookup
                if let Some(&idx) = self.by_name.get(&name) {
                    return Ok(Some(self.functions[idx].clone()));
                }
            }
        }
        
        // Try as bytes
        if let Ok(selector_vec) = key.extract::<Vec<u8>>() {
            if selector_vec.len() >= 4 {
                let selector: [u8; 4] = selector_vec[..4].try_into().unwrap();
                if let Some(&idx) = self.by_selector.get(&selector) {
                    return Ok(Some(self.functions[idx].clone()));
                }
            }
        }
        
        Ok(None)
    }
    
    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        let state = (
            &self.functions,
            &self.events,
            &self.errors,
            &self.constructor,
            &self.fallback,
            &self.receive,
            &self.storage_layout,
            &self.by_selector,
            &self.by_name,
        );
        
        let bytes = bincode::serialize(&state)
            .map_err(|e| PyRuntimeError::new_err(format!("Serialization failed: {}", e)))?;
        Ok(PyBytes::new(py, &bytes).into())
    }
    
    fn __setstate__(&mut self, state: &PyBytes) -> PyResult<()> {
        let bytes = state.as_bytes();
        
        type StateType = (
            Vec<ABIFunction>,
            Vec<ABIEvent>,
            Vec<ABIError>,
            Option<ABIFunction>,
            Option<ABIFunction>,
            Option<ABIFunction>,
            Vec<StorageSlot>,
            IndexMap<[u8; 4], usize>,
            IndexMap<String, usize>,
        );
        
        let (functions, events, errors, constructor, fallback, receive, storage_layout, by_selector, by_name): StateType = 
            bincode::deserialize(bytes)
                .map_err(|e| PyRuntimeError::new_err(format!("Deserialization failed: {}", e)))?;
        
        *self = ABI {
            functions,
            events,
            errors,
            constructor,
            fallback,
            receive,
            storage_layout,
            by_selector,
            by_name,
        };
        
        Ok(())
    }
    
    fn __deepcopy__(&self, _memo: &PyAny) -> Self {
        self.clone()
    }
    
    fn __repr__(&self) -> String {
        format!(
            "ABI(functions={}, events={}, errors={}, storage_slots={})",
            self.functions.len(),
            self.events.len(),
            self.errors.len(),
            self.storage_layout.len()
        )
    }
}

fn convert_function(func: &Function) -> ABIFunction {
    let selector = if let Some(hex_part) = func.name.strip_prefix("Unresolved_") {
        hex::decode(&hex_part[..8.min(hex_part.len())])
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .unwrap_or_else(|| func.selector().into())
    } else {
        func.selector().into()
    };

    ABIFunction {
        name: func.name.clone(),
        inputs: func.inputs.iter().map(convert_param).collect(),
        outputs: func.outputs.iter().map(convert_param).collect(),
        state_mutability: state_mutability_to_string(func.state_mutability),
        constant: matches!(func.state_mutability, StateMutability::Pure | StateMutability::View),
        payable: matches!(func.state_mutability, StateMutability::Payable),
        selector,
        signature: func.signature(),
    }
}

#[pyfunction]
#[pyo3(signature = (code, skip_resolving=false, rpc_url=None, timeout_secs=None))]
fn decompile_code(_py: Python<'_>, code: String, skip_resolving: bool, rpc_url: Option<String>, timeout_secs: Option<u64>) -> PyResult<ABI> {
    let timeout_ms = timeout_secs.unwrap_or(25).saturating_mul(1000);
    let timeout_duration = Duration::from_millis(timeout_ms);
    let args = DecompilerArgsBuilder::new()
        .target(code)
        .rpc_url(rpc_url.unwrap_or_default())
        .default(true)
        .skip_resolving(skip_resolving)
        .include_solidity(false)
        .include_yul(false)
        .output(String::new())
        .timeout(timeout_ms)
        .build()
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to build args: {}", e)))?;
    
    let (tx, rx) = std::sync::mpsc::channel();
    let done = Arc::new(AtomicBool::new(false));
    let done_clone = done.clone();
    
    let handle = thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(Err(format!("Failed to create runtime: {}", e)));
                return;
            }
        };
        
        let result = runtime.block_on(async move {
            decompile(args).await
        });
        
        done_clone.store(true, Ordering::SeqCst);
        let _ = tx.send(result.map_err(|e| format!("Decompilation failed: {}", e)));
    });
    
    let result = match rx.recv_timeout(timeout_duration) {
        Ok(Ok(result)) => {
            done.store(true, Ordering::SeqCst);
            let _ = handle.join();
            Ok(result)
        },
        Ok(Err(e)) => {
            done.store(true, Ordering::SeqCst);
            let _ = handle.join();
            Err(PyRuntimeError::new_err(e))
        },
        Err(_) => {
            Err(PyTimeoutError::new_err(format!(
                "Decompilation timed out after {} seconds", 
                timeout_ms / 1000
            )))
        }
    }?;
    
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
    
    let constructor = json_abi.constructor.as_ref().map(|c| {
        let signature = format!("constructor({})", 
            c.inputs.iter()
                .map(|p| p.ty.as_str())
                .collect::<Vec<_>>()
                .join(","));
        ABIFunction {
            name: "constructor".to_string(),
            inputs: c.inputs.iter().map(convert_param).collect(),
            outputs: Vec::new(),
            state_mutability: state_mutability_to_string(c.state_mutability),
            constant: false,
            payable: matches!(c.state_mutability, StateMutability::Payable),
            selector: [0; 4],
            signature,
        }
    });
    
    let fallback = json_abi.fallback.as_ref().map(|f| ABIFunction {
        name: "fallback".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        state_mutability: state_mutability_to_string(f.state_mutability),
        constant: false,
        payable: matches!(f.state_mutability, StateMutability::Payable),
        selector: [0; 4],
        signature: "fallback()".to_string(),
    });
    
    let receive = json_abi.receive.as_ref().map(|_| ABIFunction {
        name: "receive".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        state_mutability: "payable".to_string(),
        constant: false,
        payable: true,
        selector: [0; 4],
        signature: "receive()".to_string(),
    });
    
    let mut by_selector = IndexMap::new();
    let mut by_name = IndexMap::new();
    
    for (idx, func) in functions.iter().enumerate() {
        by_selector.insert(func.selector, idx);
        if !func.name.is_empty() {
            by_name.insert(func.name.clone(), idx);
        }
    }
    
    let abi = ABI {
        functions,
        events,
        errors,
        constructor,
        fallback,
        receive,
        storage_layout: Vec::new(),
        by_selector,
        by_name,
    };
    
    Ok(abi)
}

#[pymodule]
fn heimdall_py(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<ABIParam>()?;
    m.add_class::<ABIFunction>()?;
    m.add_class::<ABIEventParam>()?;
    m.add_class::<ABIEvent>()?;
    m.add_class::<ABIError>()?;
    m.add_class::<StorageSlot>()?;
    m.add_class::<ABI>()?;
    m.add_function(wrap_pyfunction!(decompile_code, m)?)?;
    Ok(())
}