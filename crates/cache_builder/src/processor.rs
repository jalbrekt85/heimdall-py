use crate::cache::AbiCache;
use crate::types::{
    convert_event_param, convert_function, convert_param, state_mutability_to_string, ABI,
    ABIError, ABIEvent, ABIFunction, ABIParam, StorageSlot,
};
use alloy_json_abi::StateMutability;
use eyre::Result;
use heimdall_decompiler::{decompile, DecompilerArgsBuilder};
use indexmap::IndexMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use storage_layout_extractor::{self as sle, extractor::{chain::{version::EthereumVersion, Chain}, contract::Contract}};
use tokio::runtime::Runtime;
use tracing::{debug, warn};

// Track abandoned threads globally
pub static ABANDONED_THREADS: AtomicUsize = AtomicUsize::new(0);

pub struct ContractProcessor {
    cache: Arc<AbiCache>,
    timeout_secs: u64,
    skip_resolving: bool,
    extract_storage: bool,
}

impl ContractProcessor {
    pub fn new(
        cache: Arc<AbiCache>,
        timeout_secs: u64,
        skip_resolving: bool,
        extract_storage: bool,
    ) -> Self {
        ContractProcessor {
            cache,
            timeout_secs,
            skip_resolving,
            extract_storage,
        }
    }

    pub async fn process_contract(
        &self,
        contract_address: String,
        code: String,
    ) -> Result<ProcessResult> {
        let start_time = Instant::now();

        // Check cache first
        if self.cache.exists(&code, self.skip_resolving) {
            return Ok(ProcessResult {
                address: contract_address,
                cached: true,
                success: true,
                error: None,
                duration: start_time.elapsed(),
            });
        }

        // Decompile the contract
        let (abi, decompile_error) = match self.decompile_with_timeout(&code).await {
            Ok(abi) => (abi, None),
            Err(e) => {
                let error_msg = format!("{:?}", e);
                if error_msg.contains("timed out") || error_msg.contains("Execution timed out") {
                    // Create minimal ABI with error
                    let mut abi = ABI::new();
                    abi.decompile_error = Some(format!("Decompilation timed out after {} seconds", self.timeout_secs));
                    (abi, Some(error_msg))
                } else {
                    // Other error - still save to cache with error
                    let mut abi = ABI::new();
                    abi.decompile_error = Some(error_msg.clone());
                    (abi, Some(error_msg))
                }
            }
        };

        // Write to cache
        if let Err(e) = self.cache.put(&code, self.skip_resolving, &abi) {
            warn!("Failed to write to cache: {}", e);
        }

        Ok(ProcessResult {
            address: contract_address,
            cached: false,
            success: decompile_error.is_none(),
            error: decompile_error,
            duration: start_time.elapsed(),
        })
    }

    async fn decompile_with_timeout(&self, code: &str) -> Result<ABI> {
        let timeout_ms = self.timeout_secs.saturating_mul(1000);

        let args = DecompilerArgsBuilder::new()
            .target(code.to_string())
            .rpc_url(String::new())
            .default(true)
            .skip_resolving(self.skip_resolving)
            .include_solidity(false)
            .include_yul(false)
            .output(String::new())
            .timeout(timeout_ms)
            .build()?;

        // Run decompilation with timeout
        let decompile_result = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            decompile(args),
        )
        .await??;

        // Convert to our ABI format
        let json_abi = decompile_result.abi;

        let functions: Vec<ABIFunction> = json_abi.functions().map(convert_function).collect();

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
            let signature = format!(
                "constructor({})",
                c.inputs
                    .iter()
                    .map(|p| p.ty.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let inputs: Vec<ABIParam> = c.inputs.iter().map(convert_param).collect();
            let input_types = inputs.iter().map(|p| p.type_.clone()).collect();

            ABIFunction {
                name: "constructor".to_string(),
                inputs,
                outputs: Vec::new(),
                input_types,
                output_types: Vec::new(),
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
            input_types: Vec::new(),
            output_types: Vec::new(),
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
            input_types: Vec::new(),
            output_types: Vec::new(),
            state_mutability: "payable".to_string(),
            constant: false,
            payable: true,
            selector: [0; 4],
            signature: "receive()".to_string(),
        });

        // Build indices
        let mut by_selector = IndexMap::new();
        let mut by_name = IndexMap::new();

        for (idx, func) in functions.iter().enumerate() {
            by_selector.insert(func.selector, idx);
            if !func.name.is_empty() {
                by_name.insert(func.name.clone(), idx);
            }
        }

        // Extract storage if requested
        let (storage_layout, storage_error) = if self.extract_storage {
            self.extract_storage_with_timeout(code)
        } else {
            (Vec::new(), None)
        };

        Ok(ABI {
            functions,
            events,
            errors,
            constructor,
            fallback,
            receive,
            storage_layout,
            decompile_error: None,
            storage_error,
            by_selector,
            by_name,
        })
    }

    fn extract_storage_with_timeout(&self, code: &str) -> (Vec<StorageSlot>, Option<String>) {
        let bytecode_str = code.strip_prefix("0x").unwrap_or(code);
        let bytes = match hex::decode(bytecode_str) {
            Ok(b) => b,
            Err(e) => {
                return (Vec::new(), Some(format!("Failed to decode bytecode: {}", e)));
            }
        };

        if bytes.is_empty() {
            return (Vec::new(), Some("Empty bytecode after decoding".to_string()));
        }

        let contract = Contract::new(
            bytes,
            Chain::Ethereum {
                version: EthereumVersion::Shanghai,
            },
        );

        let (tx, rx) = std::sync::mpsc::channel::<Result<Vec<StorageSlot>, String>>();
        let done = Arc::new(AtomicBool::new(false));
        let done_clone = done.clone();
        let timeout_secs = self.timeout_secs;

        let handle = thread::spawn(move || {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let watchdog = sle::watchdog::FlagWatchdog::new(done_clone)
                    .polling_every(100)
                    .in_rc();

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
                    }
                    Err(e) => {
                        let error_msg = if format!("{:?}", e).contains("StoppedByWatchdog") {
                            format!("Storage extraction timed out after {} seconds", timeout_secs)
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

        match rx.recv_timeout(Duration::from_secs(self.timeout_secs)) {
            Ok(Ok(slots)) => {
                done.store(true, Ordering::SeqCst);
                let _ = handle.join();
                (slots, None)
            }
            Ok(Err(e)) => {
                done.store(true, Ordering::SeqCst);
                let _ = handle.join();
                (Vec::new(), Some(e))
            }
            Err(_) => {
                // Timeout occurred
                done.store(true, Ordering::SeqCst);

                // Give thread grace period to finish
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(Ok(slots)) => {
                        let _ = handle.join();
                        (slots, None)
                    }
                    Ok(Err(e)) => {
                        let _ = handle.join();
                        (Vec::new(), Some(e))
                    }
                    _ => {
                        // Thread unresponsive - abandon it
                        std::mem::drop(handle);
                        ABANDONED_THREADS.fetch_add(1, Ordering::Relaxed);
                        (
                            Vec::new(),
                            Some(format!("Storage extraction timed out after {} seconds", self.timeout_secs)),
                        )
                    }
                }
            }
        }
    }
}

pub struct ProcessResult {
    pub address: String,
    pub cached: bool,
    pub success: bool,
    pub error: Option<String>,
    pub duration: Duration,
}