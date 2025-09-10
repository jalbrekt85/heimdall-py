use alloy_json_abi::{Function, EventParam, Param, StateMutability};
use heimdall_decompiler::{decompile, DecompilerArgsBuilder};
use indexmap::IndexMap;
use pyo3::exceptions::{PyRuntimeError, PyTimeoutError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

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
    ABIFunction {
        name: func.name.clone(),
        inputs: func.inputs.iter().map(convert_param).collect(),
        outputs: func.outputs.iter().map(convert_param).collect(),
        state_mutability: state_mutability_to_string(func.state_mutability),
        constant: matches!(func.state_mutability, StateMutability::Pure | StateMutability::View),
        payable: matches!(func.state_mutability, StateMutability::Payable),
        selector: func.selector().into(),
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