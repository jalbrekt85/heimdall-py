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

fn find_calldataload_in_operation(operation: &WrappedOpcode) -> Option<&WrappedOpcode> {
    if operation.opcode == CALLDATALOAD {
        return Some(operation);
    }

    // Recursively check inputs
    for input in &operation.inputs {
        if let WrappedInput::Opcode(wrapped_op) = input {
            if let Some(found) = find_calldataload_in_operation(wrapped_op) {
                return Some(found);
            }
        }
    }

    None
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

                // Store potential argument but mark as unconfirmed
                function.arguments.entry(arg_index).or_insert_with(|| {
                    debug!(
                        "[selector: {}] discovered potential argument at index {} from CALLDATALOAD({}) with input_op: {}",
                        function.selector, arg_index, state.last_instruction.inputs[0],
                        state.last_instruction.input_operations[0].to_string()
                    );
                    CalldataFrame {
                        arg_op: state.last_instruction.input_operations[0].to_string(),
                        mask_size: 32,
                        heuristics: HashSet::new(),
                        confirmed_used: false,
                    }
                });
            }

            // CALLDATACOPY
            0x37 => {
                // For functions with dynamic bytes parameters, this indicates the last parameter
                // is likely bytes since dynamic data comes at the end

                // The source offset tells us where in calldata we're reading from
                let source_offset = state.last_instruction.inputs[1];

                if source_offset >= U256::from(4) {
                    // Find the highest index argument - this is most likely the dynamic parameter
                    let max_idx = function.arguments.keys().max().copied();
                    if let Some(max_idx) = max_idx {
                        if let Some(frame) = function.arguments.get_mut(&max_idx) {
                            frame.heuristics.insert(TypeHeuristic::Bytes);
                            debug!(
                                "[selector: {}] CALLDATACOPY detected - marking last argument {} as bytes",
                                function.selector, max_idx
                            );
                        }
                    }
                }

                trace!("CALLDATACOPY detected at offset {}", source_offset);
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
                        frame.confirmed_used = true;
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
                else {
                        debug!("Function {} analyzing return type for size={} bytes", function.selector, size);
                        debug!("  Return operations: {}", return_memory_operations_solidified);

                        // Check if this looks like a dynamic string/bytes return
                        // Dynamic types have offset pointer at 0x20
                        let has_dynamic_pattern =
                            return_memory_operations_solidified.contains("memory[0x40") ||
                            return_memory_operations_solidified.contains("abi.encode") ||
                            (size >= 32 && return_memory_operations_solidified.contains("0x40"));

                        // CRITICAL FIX: Check if the return is actually returning a pointer to dynamic data
                        // This happens when we return memory[0x40] which contains 0x20 (the offset)
                        // OR if we see common string patterns in the operations
                        let returns_dynamic_pointer = return_memory_operations.iter().any(|frame| {
                            // Check if the value is 0x20 (32), which is the standard ABI offset for dynamic data
                            frame.value == U256::from(0x20)
                        }) || (
                            // Also check if the operations show dynamic memory patterns
                            return_memory_operations_solidified.contains("memory[0x40]") ||
                            return_memory_operations_solidified.contains("0x20")
                        );

                        debug!("  has_dynamic_pattern={}, returns_dynamic_pointer={}", has_dynamic_pattern, returns_dynamic_pointer);

                        // Analyze the actual memory pattern for string vs bytes detection
                        // Strings typically have readable ASCII patterns
                        let _looks_like_string = (has_dynamic_pattern || returns_dynamic_pointer) && return_memory_operations.iter().any(|frame| {
                            let bytes = frame.value.to_be_bytes_vec();
                            // Check if the bytes contain ASCII printable characters
                            let ascii_count = bytes.iter().filter(|&&b| b >= 0x20 && b <= 0x7E).count();
                            let result = ascii_count > bytes.len() / 2;  // More than half are printable ASCII
                            debug!("    Frame value check: ascii_count={}, total_bytes={}, is_string_like={}", ascii_count, bytes.len(), result);
                            result
                        });

                        // Check for bytes32 pattern: fixed 32-byte value with high entropy
                        let is_likely_bytes32 = !has_dynamic_pattern &&
                            function.arguments.is_empty() &&
                            return_memory_operations.iter().any(|frame| {
                                let bytes = frame.value.to_be_bytes_vec();
                                let non_zero_bytes = bytes.iter().filter(|&&b| b != 0).count();
                                // bytes32 constants typically have many non-zero bytes spread throughout
                                // unlike addresses which have leading zeros
                                non_zero_bytes > 20 && {
                                    // Check for even distribution (not just at the end like addresses)
                                    let first_half_non_zero = bytes[0..16].iter().filter(|&&b| b != 0).count();
                                    let second_half_non_zero = bytes[16..32].iter().filter(|&&b| b != 0).count();
                                    first_half_non_zero > 5 && second_half_non_zero > 5
                                }
                            });

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
                        } else if returns_dynamic_pointer || has_dynamic_pattern || size > 32 {
                            // If we're returning 0x20 (the ABI offset), have dynamic patterns, or size > 32
                            // Since string and bytes are indistinguishable at bytecode level,
                            // we ALWAYS default to string (not bytes) for dynamic data
                            debug!("Dynamic data detected (pointer={}, pattern={}, size={}) - defaulting to string",
                                   returns_dynamic_pointer, has_dynamic_pattern, size);
                            function.returns = Some(String::from("string"));
                        } else if is_likely_bytes32 {
                            debug!("Memory pattern suggests bytes32");
                            function.returns = Some(String::from("bytes32"));
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

                        // Check if the return value is always less than 256 (uint8 range)
                        // Be more precise - only detect uint8 if we have explicit masking or small constant values
                        let is_likely_uint8 = function.arguments.is_empty() && (
                            // Explicit masking to 1 byte
                            byte_size == 1 ||
                            (found_mask && byte_size == 1) ||
                            return_memory_operations_solidified.contains("& 0xff") ||
                            return_memory_operations_solidified.contains("0xff &") ||
                            // Constant value in typical decimals range (0-18)
                            (return_memory_operations.len() == 1 &&
                             return_memory_operations.iter().all(|frame|
                                frame.value <= U256::from(18) && frame.value > U256::from(0)
                             ))
                        );

                        let (_, cast_types) = byte_size_to_type(byte_size);

                        let return_type = if is_likely_uint8 {
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
                            else if is_likely_bytes32 {
                                String::from("bytes32")
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
                    frame.confirmed_used = true;
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
                    frame.confirmed_used = true;
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
                        frame.confirmed_used = true;
                    }
                }
            }

            // SSTORE - storage operations definitely use their inputs
            0x55 => {
                // Check if any CALLDATALOAD value is used in SSTORE
                for input_op in &state.last_instruction.input_operations {
                    if let Some(calldataload_op) = find_calldataload_in_operation(input_op) {
                        let arg_op = calldataload_op.inputs[0].to_string();
                        if let Some((arg_index, frame)) =
                            function.arguments.iter_mut().find(|(_, frame)| frame.arg_op == arg_op)
                        {
                            debug!(
                                "[selector: {}] argument {} used in SSTORE",
                                function.selector, arg_index
                            );
                            frame.confirmed_used = true;
                        }
                    }
                }
            }

            // EQ - equality comparison uses its inputs
            0x14 => {
                // Check if any CALLDATALOAD value is used in EQ
                for input_op in &state.last_instruction.input_operations {
                    if let Some(calldataload_op) = find_calldataload_in_operation(input_op) {
                        let arg_op = calldataload_op.inputs[0].to_string();
                        if let Some((arg_index, frame)) =
                            function.arguments.iter_mut().find(|(_, frame)| frame.arg_op == arg_op)
                        {
                            debug!(
                                "[selector: {}] argument {} used in EQ comparison",
                                function.selector, arg_index
                            );
                            frame.confirmed_used = true;
                        }
                    }
                }
            }

            _ => {}
        };

        Ok(())
    })
}
