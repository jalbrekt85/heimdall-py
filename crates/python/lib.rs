use alloy_json_abi::{Function, EventParam, Param, StateMutability};
use heimdall_decompiler::{decompile, DecompilerArgsBuilder};
use indexmap::IndexMap;
use pyo3::exceptions::{PyRuntimeError, PyIOError, PyValueError, PyException};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use pyo3::create_exception;
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
use once_cell::sync::Lazy;

mod cache;

// Global tokio runtime - created once per process, reused for all decompilations
static TOKIO_RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)  // Limited threads to avoid overwhelming system
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
});

create_exception!(heimdall_py, DecompileError, PyException, "Base exception for expected decompilation failures");
create_exception!(heimdall_py, DecompileTimeoutError, DecompileError, "Decompilation timed out");

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

    #[pyo3(get)]
    decompile_error: Option<String>,

    #[pyo3(get)]
    storage_error: Option<String>,

    by_selector: IndexMap<[u8; 4], usize>,
    by_name: IndexMap<String, usize>,
}

#[pymethods]
impl ABIFunction {
    fn __str__(&self) -> String {
        format!("{}({})",
            self.name,
            self.inputs.iter()
                .map(|p| format!("{} {}", p.type_, p.name))
                .collect::<Vec<_>>()
                .join(", "))
    }

    fn __repr__(&self) -> String {
        format!("ABIFunction(name='{}', inputs={:?}, outputs={:?}, state_mutability='{}')",
            self.name, self.inputs.len(), self.outputs.len(), self.state_mutability)
    }

    #[getter]
    fn selector(&self, py: Python) -> PyObject {
        PyBytes::new(py, &self.selector).into()
    }

    #[getter]
    fn signature(&self) -> String {
        self.signature.clone()
    }
}

fn parse_function_entry(entry: &serde_json::Map<String, Value>) -> PyResult<Option<ABIFunction>> {
    let name = entry.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let inputs = entry.get("inputs")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|input| {
                let param = input.as_object()?;
                Some(ABIParam {
                    name: param.get("name")?.as_str()?.to_string(),
                    type_: param.get("type")?.as_str()?.to_string(),
                    internal_type: param.get("internalType")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect())
        .unwrap_or_default();

    let outputs = entry.get("outputs")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|output| {
                let param = output.as_object()?;
                Some(ABIParam {
                    name: param.get("name")?.as_str()?.to_string(),
                    type_: param.get("type")?.as_str()?.to_string(),
                    internal_type: param.get("internalType")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect())
        .unwrap_or_default();

    let state_mutability = entry.get("stateMutability")
        .and_then(|v| v.as_str())
        .unwrap_or("nonpayable")
        .to_string();

    let signature = format!("{}({})",
        name,
        inputs.iter()
            .map(|p| &p.type_)
            .cloned()
            .collect::<Vec<_>>()
            .join(","));

    let mut hasher = Keccak::v256();
    hasher.update(signature.as_bytes());
    let mut result = [0u8; 32];
    hasher.finalize(&mut result);
    let selector: [u8; 4] = result[..4].try_into().unwrap();

    Ok(Some(ABIFunction {
        name: name.to_string(),
        inputs,
        outputs,
        state_mutability: state_mutability.clone(),
        constant: state_mutability == "view" || state_mutability == "pure",
        payable: state_mutability == "payable",
        selector,
        signature,
    }))
}

fn parse_event_entry(entry: &serde_json::Map<String, Value>) -> PyResult<Option<ABIEvent>> {
    let name = entry.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let inputs = entry.get("inputs")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|input| {
                let param = input.as_object()?;
                Some(ABIEventParam {
                    name: param.get("name")?.as_str()?.to_string(),
                    type_: param.get("type")?.as_str()?.to_string(),
                    indexed: param.get("indexed")?.as_bool()?,
                    internal_type: param.get("internalType")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect())
        .unwrap_or_default();

    let anonymous = entry.get("anonymous")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(Some(ABIEvent {
        name: name.to_string(),
        inputs,
        anonymous,
    }))
}

fn parse_error_entry(entry: &serde_json::Map<String, Value>) -> PyResult<Option<ABIError>> {
    let name = entry.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let inputs = entry.get("inputs")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|input| {
                let param = input.as_object()?;
                Some(ABIParam {
                    name: param.get("name")?.as_str()?.to_string(),
                    type_: param.get("type")?.as_str()?.to_string(),
                    internal_type: param.get("internalType")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect())
        .unwrap_or_default();

    Ok(Some(ABIError {
        name: name.to_string(),
        inputs,
    }))
}

fn parse_constructor_entry(entry: &serde_json::Map<String, Value>) -> PyResult<Option<ABIFunction>> {
    let inputs = entry.get("inputs")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|input| {
                let param = input.as_object()?;
                Some(ABIParam {
                    name: param.get("name")?.as_str()?.to_string(),
                    type_: param.get("type")?.as_str()?.to_string(),
                    internal_type: param.get("internalType")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect())
        .unwrap_or_default();

    let state_mutability = entry.get("stateMutability")
        .and_then(|v| v.as_str())
        .unwrap_or("nonpayable")
        .to_string();

    let signature = format!("constructor({})",
        inputs.iter()
            .map(|p| &p.type_)
            .cloned()
            .collect::<Vec<_>>()
            .join(","));

    Ok(Some(ABIFunction {
        name: "constructor".to_string(),
        inputs,
        outputs: Vec::new(),
        state_mutability: state_mutability.clone(),
        constant: false,
        payable: state_mutability == "payable",
        selector: [0; 4],
        signature,
    }))
}

fn parse_fallback_entry(entry: &serde_json::Map<String, Value>) -> PyResult<Option<ABIFunction>> {
    let state_mutability = entry.get("stateMutability")
        .and_then(|v| v.as_str())
        .unwrap_or("nonpayable")
        .to_string();

    Ok(Some(ABIFunction {
        name: "fallback".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        state_mutability: state_mutability.clone(),
        constant: false,
        payable: state_mutability == "payable",
        selector: [0; 4],
        signature: "fallback()".to_string(),
    }))
}

fn parse_receive_entry(_entry: &serde_json::Map<String, Value>) -> PyResult<Option<ABIFunction>> {
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
            decompile_error: None,
            storage_error: None,
            by_selector: IndexMap::new(),
            by_name: IndexMap::new(),
        }
    }

    #[staticmethod]
    fn from_json(file_path: String) -> PyResult<Self> {
        let contents = fs::read_to_string(&file_path)
            .map_err(|e| PyIOError::new_err(format!("Failed to read file {}: {}", file_path, e)))?;

        let json_value: Value = serde_json::from_str(&contents)
            .map_err(|e| PyValueError::new_err(format!("Invalid JSON: {}", e)))?;

        let abi_array = if let Some(obj) = json_value.as_object() {
            // Handle { "abi": [...] } format
            obj.get("abi")
                .and_then(|v| v.as_array())
                .ok_or_else(|| PyValueError::new_err("JSON object does not contain 'abi' field"))?
        } else if let Some(array) = json_value.as_array() {
            // Handle plain array format
            array
        } else {
            return Err(PyValueError::new_err("JSON must be either an array or an object with 'abi' field"));
        }?;

        let mut abi = ABI::new();

        for entry in abi_array {
            let entry_type = entry.get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let entry_obj = entry.as_object()
                .ok_or_else(|| PyValueError::new_err("ABI entry is not an object"))?;

            match entry_type {
                "function" => {
                    if let Some(func) = parse_function_entry(entry_obj)? {
                        abi.functions.push(func);
                    }
                },
                "event" => {
                    if let Some(event) = parse_event_entry(entry_obj)? {
                        abi.events.push(event);
                    }
                },
                "error" => {
                    if let Some(error) = parse_error_entry(entry_obj)? {
                        abi.errors.push(error);
                    }
                },
                "constructor" => {
                    abi.constructor = parse_constructor_entry(entry_obj)?;
                },
                "fallback" => {
                    abi.fallback = parse_fallback_entry(entry_obj)?;
                },
                "receive" => {
                    abi.receive = parse_receive_entry(entry_obj)?;
                },
                _ => {}
            }
        }

        abi.rebuild_indices();
        Ok(abi)
    }

    fn get_function(&self, _py: Python, key: &PyAny) -> PyResult<Option<ABIFunction>> {
        if let Ok(name) = key.extract::<String>() {
            if name.starts_with("0x") {
                // Hex selector like "0x12345678"
                if let Ok(selector_vec) = hex::decode(&name[2..]) {
                    if selector_vec.len() >= 4 {
                        let selector: [u8; 4] = selector_vec[..4].try_into().unwrap();
                        if let Some(&idx) = self.by_selector.get(&selector) {
                            return Ok(Some(self.functions[idx].clone()));
                        }
                    }
                }
            } else {
                if let Some(&idx) = self.by_name.get(&name) {
                    return Ok(Some(self.functions[idx].clone()));
                }
            }
        }

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
            &self.decompile_error,
            &self.storage_error,
            &self.by_selector,
            &self.by_name,
        );

        let bytes = bincode::serialize(&state)
            .map_err(|e| PyRuntimeError::new_err(format!("Serialization failed: {}", e)))?;

        Ok(PyBytes::new(py, &bytes).into())
    }

    fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        let bytes: &[u8] = state.extract::<&PyBytes>(py)?.as_bytes();

        type StateType = (
            Vec<ABIFunction>,
            Vec<ABIEvent>,
            Vec<ABIError>,
            Option<ABIFunction>,
            Option<ABIFunction>,
            Option<ABIFunction>,
            Vec<StorageSlot>,
            Option<String>,
            Option<String>,
            IndexMap<[u8; 4], usize>,
            IndexMap<String, usize>,
        );

        let (functions, events, errors, constructor, fallback, receive, storage_layout, _decompile_error, _storage_error, by_selector, by_name): StateType =
            bincode::deserialize(bytes)
                .map_err(|e| PyRuntimeError::new_err(format!("Deserialization failed: {}", e)))?;

        self.functions = functions;
        self.events = events;
        self.errors = errors;
        self.constructor = constructor;
        self.fallback = fallback;
        self.receive = receive;
        self.storage_layout = storage_layout;
        self.decompile_error = None;
        self.storage_error = None;
        self.by_selector = by_selector;
        self.by_name = by_name;

        Ok(())
    }

    fn rebuild_indices(&mut self) {
        self.by_selector.clear();
        self.by_name.clear();

        for (idx, func) in self.functions.iter().enumerate() {
            self.by_selector.insert(func.selector, idx);
            if !func.name.is_empty() {
                self.by_name.insert(func.name.clone(), idx);
            }
        }
    }
}

fn state_mutability_to_string(state_mutability: StateMutability) -> String {
    match state_mutability {
        StateMutability::Pure => "pure".to_string(),
        StateMutability::View => "view".to_string(),
        StateMutability::NonPayable => "nonpayable".to_string(),
        StateMutability::Payable => "payable".to_string(),
    }
}

fn convert_param(param: &Param) -> ABIParam {
    ABIParam {
        name: param.name.clone(),
        type_: param.ty.as_str().to_string(),
        internal_type: param.internal_type.as_ref().map(|it| match it {
            alloy_json_abi::InternalType::AddressPayable(_) => "address payable".to_string(),
            alloy_json_abi::InternalType::Contract(_) => "contract".to_string(),
            alloy_json_abi::InternalType::Enum { .. } => "enum".to_string(),
            alloy_json_abi::InternalType::Struct { .. } => "struct".to_string(),
            alloy_json_abi::InternalType::Other { contract: _, ty } => ty.to_string(),
        }),
    }
}

fn convert_event_param(param: &EventParam) -> ABIEventParam {
    ABIEventParam {
        name: param.name.clone(),
        type_: param.ty.as_str().to_string(),
        indexed: param.indexed,
        internal_type: param.internal_type.as_ref().map(|it| match it {
            alloy_json_abi::InternalType::AddressPayable(_) => "address payable".to_string(),
            alloy_json_abi::InternalType::Contract(_) => "contract".to_string(),
            alloy_json_abi::InternalType::Enum { .. } => "enum".to_string(),
            alloy_json_abi::InternalType::Struct { .. } => "struct".to_string(),
            alloy_json_abi::InternalType::Other { contract: _, ty } => ty.to_string(),
        }),
    }
}

fn convert_function(func: &Function) -> ABIFunction {
    let signature = format!("{}({})",
        func.name,
        func.inputs.iter()
            .map(|p| p.ty.as_str())
            .collect::<Vec<_>>()
            .join(","));

    ABIFunction {
        name: func.name.clone(),
        inputs: func.inputs.iter().map(convert_param).collect(),
        outputs: func.outputs.iter().map(convert_param).collect(),
        state_mutability: state_mutability_to_string(func.state_mutability),
        constant: matches!(func.state_mutability, StateMutability::Pure | StateMutability::View),
        payable: matches!(func.state_mutability, StateMutability::Payable),
        selector: func.selector(),
        signature,
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

    // Run decompilation with panic catching using the global runtime
    let (decompile_result, decompile_error) = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        TOKIO_RUNTIME.block_on(async move {
            decompile(args).await
        })
    })) {
        Ok(Ok(result)) => (Some(result), None),
        Ok(Err(e)) => {
            let error_msg = format!("Decompilation failed: {}", e);
            // Check if it's a timeout error from our VM checks
            if error_msg.contains("timed out") || error_msg.contains("Execution timed out") {
                (None, Some(format!("Decompilation timed out after {} seconds", timeout_ms / 1000)))
            } else {
                (None, Some(error_msg))
            }
        }
        Err(panic) => {
            // Panic occurred (e.g., arithmetic overflow)
            let panic_msg = if let Some(s) = panic.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = panic.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "Unknown panic during decompilation".to_string()
            };
            (None, Some(format!("Decompilation panicked: {}", panic_msg)))
        }
    };

    let (functions, events, errors, constructor, fallback, receive) = if let Some(result) = decompile_result {
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

        (functions, events, errors, constructor, fallback, receive)
    } else {
        (Vec::new(), Vec::new(), Vec::new(), None, None, None)
    };

    let mut by_selector = IndexMap::new();
    let mut by_name = IndexMap::new();

    for (idx, func) in functions.iter().enumerate() {
        by_selector.insert(func.selector, idx);
        if !func.name.is_empty() {
            by_name.insert(func.name.clone(), idx);
        }
    }

    let mut storage_error: Option<String> = None;
    let storage_layout = if extract_storage {
        let bytecode_str = code.strip_prefix("0x").unwrap_or(&code);
        let bytes = match hex::decode(bytecode_str) {
            Ok(b) => b,
            Err(e) => {
                storage_error = Some(format!("Failed to decode bytecode: {}", e));
                Vec::new()
            }
        };

        if !bytes.is_empty() {
            let contract = Contract::new(
                bytes,
                Chain::Ethereum {
                    version: EthereumVersion::Shanghai,
                },
            );

            let extract_timeout = timeout_secs.unwrap_or(25);

            let storage_result = py.allow_threads(move || {
                let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<StorageSlot>, String>>();
                let done = Arc::new(AtomicBool::new(false));
                let done_clone = done.clone();

                let handle = thread::spawn(move || {
                    // Wrap SLE execution in catch_unwind to handle panics
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                                Ok(slots)
                            },
                            Err(e) => {
                                let error_msg = if format!("{:?}", e).contains("StoppedByWatchdog") {
                                    format!("Storage extraction timed out after {} seconds", extract_timeout)
                                } else {
                                    format!("Storage extraction failed: {:?}", e)
                                };
                                Err(error_msg)
                            }
                        }
                    })) {
                        Ok(result) => {
                            let _ = tx.send(result);
                        }
                        Err(panic) => {
                            // Panic occurred during storage extraction
                            let panic_msg = if let Some(s) = panic.downcast_ref::<String>() {
                                s.clone()
                            } else if let Some(s) = panic.downcast_ref::<&str>() {
                                s.to_string()
                            } else {
                                "Unknown panic during storage extraction".to_string()
                            };
                            let _ = tx.send(Err(format!("Storage extraction panicked: {}", panic_msg)));
                        }
                    }
                });

                match rx.recv_timeout(Duration::from_secs(extract_timeout)) {
                    Ok(Ok(slots)) => {
                        done.store(true, Ordering::SeqCst);
                        let _ = handle.join();
                        Ok(slots)
                    },
                    Ok(Err(e)) => {
                        done.store(true, Ordering::SeqCst);
                        let _ = handle.join();
                        Err(e)
                    },
                    Err(_) => {
                        done.store(true, Ordering::SeqCst);
                        match rx.recv_timeout(Duration::from_millis(100)) {
                            Ok(Ok(slots)) => {
                                let _ = handle.join();
                                Ok(slots)
                            },
                            Ok(Err(e)) => {
                                let _ = handle.join();
                                Err(e)
                            },
                            _ => {
                                let _ = handle.join();
                                Err(format!("Storage extraction timed out after {} seconds", extract_timeout))
                            }
                        }
                    }
                }
            });

            match storage_result {
                Ok(slots) => slots,
                Err(e) => {
                    storage_error = Some(e);
                    Vec::new()
                }
            }
        } else {
            storage_error = Some("Empty bytecode after decoding".to_string());
            Vec::new()
        }
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
        decompile_error: decompile_error.clone(),
        storage_error: storage_error.clone(),
        by_selector,
        by_name,
    };

    if use_cache && cache::AbiCache::is_enabled() {
        let serialized = bincode::serialize(&abi)
            .map_err(|e| PyIOError::new_err(format!("Failed to serialize ABI for caching: {}", e)))?;

        cache::AbiCache::put(&code, skip_resolving, &serialized)
            .map_err(|e| PyIOError::new_err(format!("Failed to write to cache: {}", e)))?;
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
fn heimdall_py(_py: Python, m: &PyModule) -> PyResult<()> {
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
    m.add("DecompileError", _py.get_type::<DecompileError>())?;
    m.add("DecompileTimeoutError", _py.get_type::<DecompileTimeoutError>())?;
    Ok(())
}