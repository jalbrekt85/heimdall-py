use arrow::array::{Array, BinaryArray, StringArray};
use arrow::record_batch::RecordBatch;
use eyre::Result;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::fs::File;
use std::path::Path;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct Contract {
    pub address: String,
    pub code: String,
}

pub struct ParquetReader;

impl ParquetReader {
    pub fn read_contracts(path: &Path) -> Result<Vec<Contract>> {
        let file = File::open(path)?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file)?;

        // Use larger batch size for better performance
        let mut reader = builder.with_batch_size(8192).build()?;

        let mut contracts = Vec::new();

        while let Some(batch) = reader.next() {
            let batch = batch?;
            contracts.extend(Self::extract_contracts_from_batch(&batch)?);
        }

        debug!("Read {} contracts from {:?}", contracts.len(), path);
        Ok(contracts)
    }

    fn extract_contracts_from_batch(batch: &RecordBatch) -> Result<Vec<Contract>> {
        let mut contracts = Vec::new();

        // Find the columns we need
        let address_column = batch
            .column_by_name("contract_address")
            .ok_or_else(|| eyre::eyre!("Missing contract_address column"))?;

        let code_column = batch
            .column_by_name("code")
            .ok_or_else(|| eyre::eyre!("Missing code column"))?;

        // Try to cast as different array types
        // First, let's print debug info about the actual type
        debug!("Address column type: {:?}", address_column.data_type());

        let addresses = if let Some(string_array) = address_column.as_any().downcast_ref::<StringArray>() {
            // String addresses
            (0..string_array.len())
                .map(|i| {
                    string_array
                        .value(i)
                        .to_string()
                })
                .collect::<Vec<_>>()
        } else if let Some(binary_array) = address_column.as_any().downcast_ref::<BinaryArray>() {
            // Binary addresses (need to convert to hex)
            (0..binary_array.len())
                .map(|i| {
                    hex::encode(binary_array.value(i))
                })
                .collect::<Vec<_>>()
        } else if let Some(large_binary_array) = address_column.as_any().downcast_ref::<arrow::array::LargeBinaryArray>() {
            // Large binary addresses (need to convert to hex)
            (0..large_binary_array.len())
                .map(|i| {
                    hex::encode(large_binary_array.value(i))
                })
                .collect::<Vec<_>>()
        } else if let Some(fixed_binary_array) = address_column.as_any().downcast_ref::<arrow::array::FixedSizeBinaryArray>() {
            // Fixed size binary addresses (20 bytes for Ethereum addresses)
            (0..fixed_binary_array.len())
                .map(|i| {
                    hex::encode(fixed_binary_array.value(i))
                })
                .collect::<Vec<_>>()
        } else {
            return Err(eyre::eyre!("Unsupported address column type: {:?}", address_column.data_type()));
        };

        let codes = if let Some(string_array) = code_column.as_any().downcast_ref::<StringArray>() {
            // String codes
            (0..string_array.len())
                .map(|i| {
                    string_array
                        .value(i)
                        .to_string()
                })
                .collect::<Vec<_>>()
        } else if let Some(binary_array) = code_column.as_any().downcast_ref::<BinaryArray>() {
            // Binary codes (need to convert to hex)
            (0..binary_array.len())
                .map(|i| {
                    hex::encode(binary_array.value(i))
                })
                .collect::<Vec<_>>()
        } else if let Some(large_binary_array) = code_column.as_any().downcast_ref::<arrow::array::LargeBinaryArray>() {
            // Large binary codes (need to convert to hex)
            (0..large_binary_array.len())
                .map(|i| {
                    hex::encode(large_binary_array.value(i))
                })
                .collect::<Vec<_>>()
        } else {
            return Err(eyre::eyre!("Unsupported code column type: {:?}", code_column.data_type()));
        };

        // Combine addresses and codes
        for (address, code) in addresses.into_iter().zip(codes.into_iter()) {
            // Skip empty contracts
            if code.is_empty() || code == "0x" {
                continue;
            }

            contracts.push(Contract {
                address: if address.starts_with("0x") {
                    address
                } else {
                    format!("0x{}", address)
                },
                code: if code.starts_with("0x") {
                    code
                } else {
                    format!("0x{}", code)
                },
            });
        }

        Ok(contracts)
    }

    pub fn read_all_parquets(directory: &Path) -> Result<Vec<Contract>> {
        let mut all_contracts = Vec::new();

        // Find all parquet files
        let entries = std::fs::read_dir(directory)?;
        let mut parquet_files: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map_or(false, |ext| ext == "parquet")
            })
            .map(|entry| entry.path())
            .collect();

        // Sort for consistent ordering
        parquet_files.sort();

        info!("Found {} parquet files", parquet_files.len());

        for (idx, path) in parquet_files.iter().enumerate() {
            match Self::read_contracts(&path) {
                Ok(contracts) => {
                    info!(
                        "[{}/{}] Loaded {} contracts from {}",
                        idx + 1,
                        parquet_files.len(),
                        contracts.len(),
                        path.file_name().unwrap().to_string_lossy()
                    );
                    all_contracts.extend(contracts);
                }
                Err(e) => {
                    eprintln!("Error reading {:?}: {}", path, e);
                }
            }
        }

        info!("Total contracts loaded: {}", all_contracts.len());
        Ok(all_contracts)
    }
}