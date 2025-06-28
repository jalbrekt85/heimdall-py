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
                // calculate the argument index, with the 4byte signature padding removed
                // for example, CALLDATALOAD(4) -> (4-4)/32 = 0
                //              CALLDATALOAD(36) -> (36-4)/32 = 1
                let arg_index = (state.last_instruction.inputs[0].saturating_sub(U256::from(4)) /
                    U256::from(32))
                .try_into()
                .unwrap_or(usize::MAX);

                // insert only if this argument is not already in the hashmap
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
                // if this is a bitwise mask operation on CALLDATALOAD, we can use it to determine
                // the size (and consequently type) of the variable
                if let Some(calldataload_op) = state
                    .last_instruction
                    .input_operations
                    .iter()
                    .find(|op| op.opcode == CALLDATALOAD)
                {
                    // this is a bitwise mask, we can use it to determine the size of the variable
                    let (mask_size_bytes, _potential_types) =
                        convert_bitmask(&state.last_instruction);

                    // yulify the calldataload operation, and find the associated argument index
                    // this MUST exist, as we have already inserted it in the CALLDATALOAD heuristic
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
                // Safely convert U256 to usize
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

                // add the return statement to the function logic
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

                // if we've already determined a return type, we don't want to do it again.
                // we use bytes32 as a default return type
                if function.returns.is_some() && function.returns.as_deref() != Some("bytes32") {
                    return Ok(());
                }

                // if the any input op is ISZERO(x), this is a boolean return
                if return_memory_operations.iter().any(|x| x.operation.opcode == ISZERO) {
                    function.returns = Some(String::from("bool"));
                }
                // if the input op is any of the following, it is an address return
                // this is because these push address values onto the stack
                else if return_memory_operations
                    .iter()
                    .any(|x| [0x30, 0x32, 0x33, 0x41, 0x73].contains(&x.operation.opcode))
                {
                    function.returns = Some(String::from("address"));
                }
                // if the input op is any of the following, it is a uint256 return
                // this is because these push numeric values onto the stack
                else if return_memory_operations.iter().any(|x| {
                    [0x31, 0x34, 0x3a, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x58, 0x5a]
                        .contains(&x.operation.opcode)
                }) {
                    function.returns = Some(String::from("uint256"));
                }
                // if the size of returndata is > 32, it must be a bytes or string return.
                else if size > 32 {
                    // some hardcoded function selectors where the return type is known to be a
                    // string
                    if ["06fdde03", "95d89b41", "6a98de4c", "9d2b0822", "1a0d4bca"]
                        .contains(&function.selector.as_str())
                    {
                        function.returns = Some(String::from("string memory"));
                    } else {
                        function.returns = Some(String::from("bytes memory"));
                    }
                } else {
                    // If we have no memory operations and this is a parameterless function,
                    // it's likely returning a constant
                    if return_memory_operations.is_empty() && function.arguments.is_empty() && size == 32 {
                        debug!(
                            "Checking for constant return: no memory ops, no args, size=32"
                        );
                        
                        // Check if we have any memory operations that might contain an address
                        // For dynamic memory allocation, scan all memory slots
                        let mut found_address = false;
                        
                        debug!(
                            "Scanning {} memory slots for address patterns",
                            function.memory.len()
                        );
                        
                        // Check all memory slots for PUSH20 operations
                        for (offset, memory_frame) in &function.memory {
                            debug!(
                                "Memory at offset {}: opcode={:02x}, value={}",
                                offset,
                                memory_frame.operation.opcode,
                                memory_frame.value
                            );
                            
                            if memory_frame.operation.opcode == 0x73 {
                                // PUSH20 is definitely an address
                                debug!("Found PUSH20 at offset {} - setting return type to address", offset);
                                function.returns = Some(String::from("address"));
                                found_address = true;
                                break;
                            } else if (0x60..=0x7f).contains(&memory_frame.operation.opcode) {
                                // Check if the value looks like an address
                                let value_bytes = memory_frame.value.to_be_bytes_vec();
                                let non_zero_bytes = value_bytes.iter().filter(|&&b| b != 0).count();
                                
                                if non_zero_bytes <= 20 && non_zero_bytes > 0 {
                                    // Check if the value is in the valid address range
                                    // Most addresses start with many zeros when viewed as 32-byte values
                                    let leading_zeros = value_bytes.iter().take_while(|&&b| b == 0).count();
                                    if leading_zeros >= 12 { // 32 - 20 = 12 leading zeros for addresses
                                        debug!(
                                            "Found address-like value at offset {} (leading_zeros={}, non_zero_bytes={}) - setting return type to address",
                                            offset, leading_zeros, non_zero_bytes
                                        );
                                        function.returns = Some(String::from("address"));
                                        found_address = true;
                                        break;
                                    }
                                }
                            }
                        }
                        
                        if !found_address {
                            // No address pattern found, default to uint256
                            debug!("No address patterns found in memory - defaulting to uint256");
                            function.returns = Some(String::from("uint256"));
                        }
                    } else {
                        // Check if any memory operation contains a PUSH20 (recursively)
                        let has_push20 = return_memory_operations.iter().any(|frame| {
                            debug!("Checking memory operation for PUSH20: opcode={:02x}", frame.operation.opcode);
                            contains_push20(&frame.operation, 0)
                        });
                        
                        // Also check if the value looks like an address (20 bytes in lower part of 32-byte value)
                        let has_address_value = return_memory_operations.iter().any(|frame| {
                            let bytes = frame.value.to_be_bytes_vec();
                            let leading_zeros = bytes.iter().take_while(|&&b| b == 0).count();
                            let non_zero_bytes = bytes.iter().filter(|&&b| b != 0).count();
                            
                            debug!(
                                "Checking value pattern: leading_zeros={}, non_zero_bytes={}, value={}",
                                leading_zeros, non_zero_bytes, frame.value
                            );
                            
                            // Common pattern for addresses: 12 leading zeros (32 - 20 = 12) and up to 20 non-zero bytes
                            leading_zeros >= 12 && non_zero_bytes <= 20 && non_zero_bytes > 0
                        });
                        
                        if has_push20 || has_address_value {
                            debug!("Found address pattern in return memory operations - setting return type to address");
                            function.returns = Some(String::from("address"));
                        } else {
                            // attempt to find a return type within the return memory operations
                            let mut byte_size = 32; // default to 32 bytes
                            let mut found_mask = false;
                            
                            // Try to find bitmask in the operations
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

                        // convert the cast size to a string
                        let (_, cast_types) = byte_size_to_type(byte_size);
                        
                        // Special handling for address detection:
                        let return_type = if byte_size == 20 {
                            // For 20 bytes, this is definitely an address
                            String::from("address")
                        } else if byte_size == 32 {
                            // For 32-byte returns, we need better heuristics
                            
                            // Check for explicit address mask
                            if return_memory_operations_solidified.contains("0xffffffffffffffffffffffffffffffffffffffff") {
                                String::from("address")
                            }
                            // Check if this is a simple storage getter (common pattern for address getters)
                            else if !found_mask && 
                                    function.arguments.is_empty() && 
                                    return_memory_operations.len() == 1 &&
                                    return_memory_operations_solidified.contains("storage[") {
                                // This is likely a getter for a storage variable
                                // Many address storage variables are returned without explicit masking
                                // since addresses are stored in the lower 20 bytes of a 32-byte slot
                                String::from("address")
                            }
                            // If we found a mask pattern that suggests address masking
                            else if found_mask && byte_size == 32 &&
                                    return_memory_operations_solidified.contains("& (0x") && 
                                    return_memory_operations_solidified.contains("ff") &&
                                    return_memory_operations_solidified.matches("ff").count() == 20 {
                                String::from("address")
                            }
                            else {
                                // Default to uint256 for 32-byte values
                                cast_types[0].to_string()
                            }
                        } else {
                            // For other sizes, use the default type
                            cast_types[0].to_string()
                        };
                        
                        function.returns = Some(return_type);
                        }
                    }
                }

                // check if this is a state getter
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
                // check if this instruction is operating on a known argument.
                // if it is, add 'integer' to the list of heuristics
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
                // check if this instruction is operating on a known argument.
                // if it is, add 'bytes' to the list of heuristics
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
                // if this is a boolean check on CALLDATALOAD, we can add boolean to the potential
                // types
                if let Some(calldataload_op) = state
                    .last_instruction
                    .input_operations
                    .iter()
                    .find(|op| op.opcode == CALLDATALOAD)
                {
                    // yulify the calldataload operation, and find the associated argument index
                    // this MUST exist, as we have already inserted it in the CALLDATALOAD heuristic
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

                        // NOTE: we don't want to update mask_size here, as we are only adding
                        // potential types
                        frame.heuristics.insert(TypeHeuristic::Boolean);
                    }
                }
            }

            _ => {}
        };

        Ok(())
    })
}
