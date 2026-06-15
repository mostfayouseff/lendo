pub mod kamino;
pub mod marginfi;
pub mod solend;

use async_trait::async_trait;

use crate::{
    error::FlashLoanError,
    types::{FlashLoanParams, FlashLoanReceipt, FlashLoanRequest},
};

#[async_trait]
pub trait FlashLoanProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn get_params(&self, mint: &str, amount: u64) -> Result<FlashLoanParams, FlashLoanError>;
    async fn build_transaction(&self, req: &FlashLoanRequest, params: &FlashLoanParams) -> Result<FlashLoanReceipt, FlashLoanError>;
    async fn check_liquidity(&self, mint: &str, amount: u64) -> Result<bool, FlashLoanError>;
}

pub struct ProviderFactory;

impl ProviderFactory {
    pub fn create(name: &str, rpc_url: &str) -> Result<Box<dyn FlashLoanProvider>, FlashLoanError> {
        match name.to_lowercase().as_str() {
            "solend"   => Ok(Box::new(solend::SolendProvider::new(rpc_url))),
            "marginfi" => Ok(Box::new(marginfi::MarginFiProvider::new(rpc_url))),
            "kamino"   => Ok(Box::new(kamino::KaminoProvider::new(rpc_url))),
            other      => Err(FlashLoanError::ProviderUnavailable(other.to_string())),
        }
    }
}
