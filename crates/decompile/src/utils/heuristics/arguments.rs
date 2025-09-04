use futures::future::BoxFuture;
use hashbrown::HashSet;

use alloy::primitives::U256;
use eyre::eyre;
use heimdall_common::utils::strings::find_balanced_encapsulator;
use heimdall_vm::core::{
    opcodes::{opcode_name, wrapped::WrappedInput, CALLDATALOAD, ISZERO},
    types::{byte_size_to_type, convert_bitmask},
    vm::State,
};
use tracing::{debug, trace};

use crate::{
    core::analyze::{AnalyzerState, AnalyzerType},
    interfaces::{AnalyzedFunction, CalldataFrame, TypeHeuristic},
    utils::constants::{AND_BITMASK_REGEX, AND_BITMASK_REGEX_2, STORAGE_ACCESS_REGEX},
    Error,
};

use heimdall_vm::core::opcodes::wrapped::WrappedOpcode;

fn contains_push20(operation: &WrappedOpcode, depth: u32) -> bool {
    if depth > 16 {
        return false;
    }
    
    if operation.opcode == 0x73 {
        return true;
    }
    
    // Recursively check all inputs
    for input in &operation.inputs {
        if let WrappedInput::Opcode(wrapped_op) = input {
            if contains_push20(wrapped_op, depth + 1) {
                return true;
            }
        }
    }
    
    false
}

pub(crate) fn argument_heuristic<'a>(
    function: &'a mut AnalyzedFunction,
    state: &'a State,
    analyzer_state: &'a mut AnalyzerState,
) -> BoxFuture<'a, Result<(), Error>> {
    Box::pin(async move {
        match state.last_instruction.opcode {
            // CALLDATALOAD
            0x35 => {
                let arg_index = (state.last_instruction.inputs[0].saturating_sub(U256::from(4)) /
                    U256::from(32))
                .try_into()
                .unwrap_or(usize::MAX);

                function.arguments.entry(arg_index).or_insert_with(|| {
                    debug!(
                        "discovered new argument at index {} from CALLDATALOAD({})",
                        arg_index, state.last_instruction.inputs[0]
                    );
                    CalldataFrame {
                        arg_op: state.last_instruction.input_operations[0].to_string(),
                        mask_size: 32, // init to 32 because all CALLDATALOADs are 32 bytes
                        heuristics: HashSet::new(),
                    }
                });
            }

            // CALLDATACOPY
            0x37 => {
                // TODO: implement CALLDATACOPY support
                trace!("CALLDATACOPY detected; not implemented");
            }

            // AND | OR
            0x16 | 0x17 => {
                if let Some(calldataload_op) = state
                    .last_instruction
                    .input_operations
                    .iter()
                    .find(|op| op.opcode == CALLDATALOAD)
                {
                    let (mask_size_bytes, _potential_types) =
                        convert_bitmask(&state.last_instruction);

                    let arg_op = calldataload_op.inputs[0].to_string();
                    if let Some((arg_index, frame)) =
                        function.arguments.iter_mut().find(|(_, frame)| frame.arg_op == arg_op)
                    {
                        debug!(
                            "instruction {} ({}) indicates argument {} is masked to {} bytes",
                            state.last_instruction.instruction,
                            opcode_name(state.last_instruction.opcode),
                            arg_index,
                            mask_size_bytes
                        );

                        frame.mask_size = mask_size_bytes;
                    }
                }
            }

            // RETURN
            0xf3 => {
                if !function.logic.contains(&"__HAS_RETURN__".to_string()) {
                    function.logic.push("__HAS_RETURN__".to_string());
                }
                
                let size: usize = state.last_instruction.inputs[1].try_into().unwrap_or(0);
                

                let return_memory_operations = function.get_memory_range(
                    state.last_instruction.inputs[0],
                    state.last_instruction.inputs[1],
                );
                let return_memory_operations_solidified = return_memory_operations
                    .iter()
                    .map(|x| x.operation.solidify())
                    .collect::<Vec<String>>()
                    .join(", ");

                if analyzer_state.analyzer_type == AnalyzerType::Solidity {
                    if return_memory_operations.len() <= 1 {
                        function
                            .logic
                            .push(format!("return {return_memory_operations_solidified};"));
                    } else {
                        function.logic.push(format!(
                            "return abi.encodePacked({return_memory_operations_solidified});"
                        ));
                    }
                } else if analyzer_state.analyzer_type == AnalyzerType::Yul {
                    function.logic.push(format!(
                        "return({}, {})",
                        state.last_instruction.input_operations[0].yulify(),
                        state.last_instruction.input_operations[1].yulify()
                    ));
                }

                if function.returns.is_some() && function.returns.as_deref() != Some("bytes32") {
                    return Ok(());
                }

                let last_ops_have_iszero = return_memory_operations
                    .last()
                    .map(|x| x.operation.opcode == ISZERO)
                    .unwrap_or(false);
                
                if last_ops_have_iszero && !function.arguments.is_empty() {
                    if function.returns.is_none() || function.returns.as_deref() == Some("bool") {
                        function.returns = Some(String::from("bool"));
                    }
                }
                else if return_memory_operations
                    .iter()
                    .any(|x| [0x30, 0x32, 0x33, 0x41, 0x73].contains(&x.operation.opcode))
                {
                    function.returns = Some(String::from("address"));
                }
                else if return_memory_operations.iter().any(|x| {
                    [0x31, 0x34, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x58, 0x5a]
                        .contains(&x.operation.opcode)
                }) {
                    function.returns = Some(String::from("uint256"));
                }
                else if size > 32 {
                    function.returns = Some(String::from("bytes memory"));
                } else {
                        let has_push20 = return_memory_operations.iter().any(|frame| {
                            debug!("Checking memory operation for PUSH20: opcode={:02x}", frame.operation.opcode);
                            contains_push20(&frame.operation, 0)
                        });
                        
                        let has_address_value = if function.arguments.is_empty() {
                            return_memory_operations.iter().any(|frame| {
                                let bytes = frame.value.to_be_bytes_vec();
                                let leading_zeros = bytes.iter().take_while(|&&b| b == 0).count();
                                let non_zero_bytes = bytes.iter().filter(|&&b| b != 0).count();
                                
                                leading_zeros == 12 && non_zero_bytes >= 10 && non_zero_bytes <= 20
                            })
                        } else {
                            return_memory_operations.iter().any(|frame| {
                                let bytes = frame.value.to_be_bytes_vec();
                                let leading_zeros = bytes.iter().take_while(|&&b| b == 0).count();
                                let non_zero_bytes = bytes.iter().filter(|&&b| b != 0).count();
                                leading_zeros >= 12 && non_zero_bytes <= 20 && non_zero_bytes > 0
                            })
                        };
                        
                        
                        if has_push20 || has_address_value {
                            debug!("Found address pattern in return memory operations - setting return type to address");
                            function.returns = Some(String::from("address"));
                        } else {
                            let mut byte_size = 32;
                            let mut found_mask = false;
                            
                            if let Some(bitmask) = AND_BITMASK_REGEX
                            .find(&return_memory_operations_solidified)
                            .ok()
                            .flatten()
                        {
                            let cast = bitmask.as_str();
                            byte_size = cast.matches("ff").count();
                            found_mask = true;
                        } else if let Some(bitmask) = AND_BITMASK_REGEX_2
                            .find(&return_memory_operations_solidified)
                            .ok()
                            .flatten()
                        {
                            let cast = bitmask.as_str();
                            byte_size = cast.matches("ff").count();
                            found_mask = true;
                        }

                        let (_, cast_types) = byte_size_to_type(byte_size);
                        
                        let return_type = if function.arguments.is_empty() && byte_size == 1 {
                            String::from("uint8")
                        } else if byte_size == 20 {
                            String::from("address")
                        } else if byte_size == 32 {
                            if return_memory_operations_solidified.contains("0xffffffffffffffffffffffffffffffffffffffff") {
                                String::from("address")
                            }
                            else if found_mask && byte_size == 32 &&
                                    return_memory_operations_solidified.contains("& (0x") && 
                                    return_memory_operations_solidified.contains("ff") &&
                                    return_memory_operations_solidified.matches("ff").count() == 20 {
                                String::from("address")
                            }
                            else {
                                cast_types[0].to_string()
                            }
                        } else {
                            cast_types[0].to_string()
                        };
                        
                        function.returns = Some(return_type);
                    }
                }

                if function.arguments.is_empty() {
                    if let Some(storage_access) = STORAGE_ACCESS_REGEX
                        .find(&return_memory_operations_solidified)
                        .unwrap_or(None)
                    {
                        let storage_access = storage_access.as_str();
                        let access_range =
                            find_balanced_encapsulator(storage_access, ('[', ']'))
                                .map_err(|e| eyre!("failed to find access range: {e}"))?;

                        function.maybe_getter_for =
                            Some(format!("storage[{}]", &storage_access[access_range]));
                    }
                }

                debug!(
                    "return type determined to be '{:?}' from ops '{}'",
                    function.returns, return_memory_operations_solidified
                );
            }

            // integer type heuristics
            0x02 | 0x04 | 0x05 | 0x06 | 0x07 | 0x08 | 0x09 | 0x0b | 0x10 | 0x11 | 0x12 | 0x13 => {
                if let Some((arg_index, frame)) =
                    function.arguments.iter_mut().find(|(_, frame)| {
                        state
                            .last_instruction
                            .output_operations
                            .iter()
                            .any(|operation| operation.to_string().contains(frame.arg_op.as_str()))
                    })
                {
                    debug!(
                        "instruction {} ({}) indicates argument {} may be a numeric type",
                        state.last_instruction.instruction,
                        opcode_name(state.last_instruction.opcode),
                        arg_index
                    );

                    frame.heuristics.insert(TypeHeuristic::Numeric);
                }
            }

            // bytes type heuristics
            0x18 | 0x1a | 0x1b | 0x1c | 0x1d | 0x20 => {
                if let Some((arg_index, frame)) =
                    function.arguments.iter_mut().find(|(_, frame)| {
                        state
                            .last_instruction
                            .output_operations
                            .iter()
                            .any(|operation| operation.to_string().contains(frame.arg_op.as_str()))
                    })
                {
                    debug!(
                        "instruction {} ({}) indicates argument {} may be a bytes type",
                        state.last_instruction.instruction,
                        opcode_name(state.last_instruction.opcode),
                        arg_index
                    );

                    frame.heuristics.insert(TypeHeuristic::Bytes);
                }
            }

            // boolean type heuristics
            0x15 => {
                if !function.logic.contains(&"__USES_ISZERO__".to_string()) {
                    function.logic.push("__USES_ISZERO__".to_string());
                }
                
                if let Some(calldataload_op) = state
                    .last_instruction
                    .input_operations
                    .iter()
                    .find(|op| op.opcode == CALLDATALOAD)
                {
                    let arg_op = calldataload_op.inputs[0].to_string();
                    if let Some((arg_index, frame)) =
                        function.arguments.iter_mut().find(|(_, frame)| frame.arg_op == arg_op)
                    {
                        debug!(
                            "instruction {} ({}) indicates argument {} may be a boolean",
                            state.last_instruction.instruction,
                            opcode_name(state.last_instruction.opcode),
                            arg_index
                        );

                        frame.heuristics.insert(TypeHeuristic::Boolean);
                    }
                }
            }

            _ => {}
        };

        Ok(())
    })
}
