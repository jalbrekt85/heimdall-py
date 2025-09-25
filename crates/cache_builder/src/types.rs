use alloy_json_abi::{EventParam, Function, Param, StateMutability};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use storage_layout_extractor as sle;
use tiny_keccak::{Hasher, Keccak};

// These structs MUST match exactly with the Python bindings in crates/python/src/lib.rs
// to ensure cache compatibility

#[derive(Clone, Serialize, Deserialize)]
pub struct ABIParam {
    pub name: String,
    pub type_: String,
    pub internal_type: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ABIFunction {
    pub name: String,
    pub inputs: Vec<ABIParam>,
    pub outputs: Vec<ABIParam>,
    pub input_types: Vec<String>,
    pub output_types: Vec<String>,
    pub state_mutability: String,
    pub constant: bool,
    pub payable: bool,
    pub selector: [u8; 4],
    pub signature: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ABIEventParam {
    pub name: String,
    pub type_: String,
    pub indexed: bool,
    pub internal_type: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ABIEvent {
    pub name: String,
    pub inputs: Vec<ABIEventParam>,
    pub anonymous: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ABIError {
    pub name: String,
    pub inputs: Vec<ABIParam>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StorageSlot {
    pub index: u64,
    pub offset: u32,
    pub typ: String,
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

#[derive(Clone, Serialize, Deserialize)]
pub struct ABI {
    pub functions: Vec<ABIFunction>,
    pub events: Vec<ABIEvent>,
    pub errors: Vec<ABIError>,
    pub constructor: Option<ABIFunction>,
    pub fallback: Option<ABIFunction>,
    pub receive: Option<ABIFunction>,
    pub storage_layout: Vec<StorageSlot>,
    pub decompile_error: Option<String>,
    pub storage_error: Option<String>,
    pub by_selector: IndexMap<[u8; 4], usize>,
    pub by_name: IndexMap<String, usize>,
}

impl ABI {
    pub fn new() -> Self {
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

    pub fn rebuild_indices(&mut self) {
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

pub fn state_mutability_to_string(state_mutability: StateMutability) -> String {
    match state_mutability {
        StateMutability::Pure => "pure".to_string(),
        StateMutability::View => "view".to_string(),
        StateMutability::NonPayable => "nonpayable".to_string(),
        StateMutability::Payable => "payable".to_string(),
    }
}

pub fn convert_param(param: &Param) -> ABIParam {
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

pub fn convert_event_param(param: &EventParam) -> ABIEventParam {
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

pub fn convert_function(func: &Function) -> ABIFunction {
    let signature = format!(
        "{}({})",
        func.name,
        func.inputs
            .iter()
            .map(|p| p.ty.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );

    // For unresolved functions, extract the actual selector from the name
    // Otherwise use the calculated selector
    let selector = if func.name.starts_with("Unresolved_") {
        let hex_str = &func.name[11..]; // Skip "Unresolved_"
        hex::decode(hex_str)
            .ok()
            .and_then(|bytes| {
                if bytes.len() == 4 {
                    let mut arr = [0u8; 4];
                    arr.copy_from_slice(&bytes);
                    Some(arr)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| *func.selector())
    } else {
        *func.selector()
    };

    let inputs: Vec<ABIParam> = func.inputs.iter().map(convert_param).collect();
    let outputs: Vec<ABIParam> = func.outputs.iter().map(convert_param).collect();
    let input_types = inputs.iter().map(|p| p.type_.clone()).collect();
    let output_types = outputs.iter().map(|p| p.type_.clone()).collect();

    ABIFunction {
        name: func.name.clone(),
        inputs,
        outputs,
        input_types,
        output_types,
        state_mutability: state_mutability_to_string(func.state_mutability),
        constant: matches!(
            func.state_mutability,
            StateMutability::Pure | StateMutability::View
        ),
        payable: matches!(func.state_mutability, StateMutability::Payable),
        selector,
        signature,
    }
}