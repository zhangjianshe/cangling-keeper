use serde::{Deserialize, Serialize};

/// An SSH key pair (certificate) managed as a first-class object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Certificate {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub private_key_path: String,
    pub public_key: String,
}
