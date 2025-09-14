use alloy_json_abi::{Function, EventParam, Param, StateMutability};
use heimdall_decompiler::{decompile, DecompilerArgsBuilder};
use indexmap::IndexMap;
use pyo3::exceptions::{PyRuntimeError, PyTimeoutError, PyIOError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tiny_keccak::{Hasher, Keccak};
use storage_layout_extractor::{self as sle, extractor::{chain::{version::EthereumVersion, Chain}, contract::Contract}};

mod cache;

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

    fn __repr__(&self) -> String {
        format!("StorageSlot(index={}, offset={}, typ='{}')",
                self.index, self.offset, self.typ)
    }
}

impl From<sle::layout::StorageSlot> for StorageSlot {
    fn from(slot: sle::layout::StorageSlot) -> Self {
        let index_str = format!("{:?}", slot.index);
        let index = index_str.parse::<u64>().unwrap_or(0);

        StorageSlot {
            index,
            offset: slot.offset as u32,
            typ: slot.typ.to_solidity_type(),
        }
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
#[pyo3(signature = (code, skip_resolving=false, extract_storage=true, use_cache=true, rpc_url=None, timeout_secs=None))]
fn decompile_code(py: Python<'_>, code: String, skip_resolving: bool, extract_storage: bool, use_cache: bool, rpc_url: Option<String>, timeout_secs: Option<u64>) -> PyResult<ABI> {
    if use_cache && cache::AbiCache::is_enabled() {
        if let Some(cached_data) = cache::AbiCache::get(&code, skip_resolving) {
            let abi: ABI = bincode::deserialize(&cached_data)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to deserialize cached ABI: {}", e)))?;
            return Ok(abi);
        }
    }

    let timeout_ms = timeout_secs.unwrap_or(25).saturating_mul(1000);
    let timeout_duration = Duration::from_millis(timeout_ms);
    let args = DecompilerArgsBuilder::new()
        .target(code.clone())
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
    
    let storage_layout = if extract_storage {
        let bytecode_str = code.strip_prefix("0x").unwrap_or(&code);

        let bytes = hex::decode(bytecode_str)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to decode bytecode for storage extraction: {}", e)))?;

        let contract = Contract::new(
            bytes,
            Chain::Ethereum {
                version: EthereumVersion::Shanghai,
            },
        );

        let extract_timeout = timeout_secs.unwrap_or(25);

        py.allow_threads(move || {
            let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<StorageSlot>, String>>();
            let done = Arc::new(AtomicBool::new(false));
            let done_clone = done.clone();

            let handle = thread::spawn(move || {
                let watchdog = sle::watchdog::FlagWatchdog::new(done_clone).in_rc();

                let result = sle::new(
                    contract,
                    sle::vm::Config::default(),
                    sle::tc::Config::default(),
                    watchdog,
                )
                .analyze();

                match result {
                    Ok(layout) => {
                        let slots: Vec<StorageSlot> = layout
                            .slots()
                            .iter()
                            .filter(|slot| {
                                let typ = slot.typ.to_solidity_type();
                                typ != "unknown"
                            })
                            .map(|slot| slot.clone().into())
                            .collect();
                        let _ = tx.send(Ok(slots));
                    },
                    Err(e) => {
                        if format!("{:?}", e).contains("StoppedByWatchdog") {
                            eprintln!("Storage extraction stopped by timeout after {} seconds", extract_timeout);
                            let _ = tx.send(Ok(Vec::new()));
                        } else {
                            // Log error but don't fail - storage extraction is optional
                            eprintln!("Storage extraction failed: {:?}", e);
                            let _ = tx.send(Ok(Vec::new()));
                        }
                    }
                }
            });

            match rx.recv_timeout(Duration::from_secs(extract_timeout)) {
                Ok(Ok(slots)) => {
                    done.store(true, Ordering::SeqCst);
                    let _ = handle.join();
                    Ok(slots)
                },
                Ok(Err(_)) => {
                    done.store(true, Ordering::SeqCst);
                    let _ = handle.join();
                    Ok(Vec::new())
                },
                Err(_) => {
                    done.store(true, Ordering::SeqCst);
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(Ok(slots)) => {
                            let _ = handle.join();
                            Ok(slots)
                        },
                        _ => {
                            eprintln!("Storage extraction timed out after {} seconds", extract_timeout);
                            let _ = handle.join();
                            Ok(Vec::new())
                        }
                    }
                }
            }
        }).unwrap_or_else(|_: PyErr| Vec::new())
    } else {
        Vec::new()
    };

    let abi = ABI {
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

    if use_cache && cache::AbiCache::is_enabled() {
        let serialized = bincode::serialize(&abi)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to serialize ABI for caching: {}", e)))?;

        if let Err(e) = cache::AbiCache::put(&code, skip_resolving, &serialized) {
            eprintln!("Warning: Failed to cache ABI: {}", e);
        }
    }

    Ok(abi)
}

#[pyfunction]
#[pyo3(signature = (enabled=true, directory=None))]
fn configure_cache(_py: Python<'_>, enabled: bool, directory: Option<String>) -> PyResult<()> {
    let dir_path = directory.map(PathBuf::from);

    cache::AbiCache::init(dir_path, enabled)
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to configure cache: {}", e)))?;

    if enabled && !cache::AbiCache::is_enabled() {
        cache::AbiCache::init(None, true)
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to initialize cache: {}", e)))?;
    }

    Ok(())
}

#[pyfunction]
fn clear_cache(_py: Python<'_>) -> PyResult<()> {
    cache::AbiCache::clear()
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to clear cache: {}", e)))?;
    Ok(())
}

#[pyfunction]
fn get_cache_stats(py: Python<'_>) -> PyResult<PyObject> {
    let stats = cache::AbiCache::get_stats();

    let dict = PyDict::new(py);
    dict.set_item("hits", stats.hits)?;
    dict.set_item("misses", stats.misses)?;
    dict.set_item("writes", stats.writes)?;
    dict.set_item("errors", stats.errors)?;

    let total_requests = stats.hits + stats.misses;
    let hit_rate = if total_requests > 0 {
        stats.hits as f64 / total_requests as f64
    } else {
        0.0
    };
    dict.set_item("hit_rate", hit_rate)?;
    dict.set_item("enabled", cache::AbiCache::is_enabled())?;

    Ok(dict.into())
}

#[pymodule]
fn heimdall_py(py: Python, m: &PyModule) -> PyResult<()> {
    if let Err(e) = cache::AbiCache::init(None, true) {
        eprintln!("Warning: Failed to initialize cache: {}", e);
    }
    m.add_class::<ABIParam>()?;
    m.add_class::<ABIFunction>()?;
    m.add_class::<ABIEventParam>()?;
    m.add_class::<ABIEvent>()?;
    m.add_class::<ABIError>()?;
    m.add_class::<StorageSlot>()?;
    m.add_class::<ABI>()?;
    m.add_function(wrap_pyfunction!(decompile_code, m)?)?;
    m.add_function(wrap_pyfunction!(configure_cache, m)?)?;
    m.add_function(wrap_pyfunction!(clear_cache, m)?)?;
    m.add_function(wrap_pyfunction!(get_cache_stats, m)?)?;
    Ok(())
}
