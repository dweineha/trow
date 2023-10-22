mod admission;
pub mod api_types;
pub mod digest;
mod image;
pub mod manifest;
mod metrics;
mod proxy_auth;
mod server;
mod temporary_file;

use std::future::Future;

use anyhow::Result;

pub use admission::ImageValidationConfig;
pub use proxy_auth::{RegistryProxiesConfig, SingleRegistryProxyConfig};
pub use server::TrowServer;

pub struct TrowServerBuilder {
    data_path: String,
    proxy_registry_config: Option<RegistryProxiesConfig>,
    image_validation_config: Option<ImageValidationConfig>,
    tls_cert: Option<Vec<u8>>,
    tls_key: Option<Vec<u8>>,
    root_key: Option<Vec<u8>>,
}

pub fn build_server(
    data_path: &str,
    proxy_registry_config: Option<RegistryProxiesConfig>,
    image_validation_config: Option<ImageValidationConfig>,
) -> TrowServerBuilder {
    TrowServerBuilder {
        data_path: data_path.to_string(),
        proxy_registry_config,
        image_validation_config,
        tls_cert: None,
        tls_key: None,
        root_key: None,
    }
}

impl TrowServerBuilder {
    pub fn get_server_future(self) -> impl Future<Output = Result<TrowServer>> {
        TrowServer::new(
            &self.data_path,
            self.proxy_registry_config,
            self.image_validation_config,
        )
    }
}
