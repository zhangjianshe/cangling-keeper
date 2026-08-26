use serde::{Deserialize, Serialize};

/// Authentication method used to connect an SSH session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Auth {
    Password {
        password: String,
    },
    Certificate {
        #[serde(rename = "certificateId")]
        certificate_id: String,
    },
}

impl Default for Auth {
    fn default() -> Self {
        Auth::Password {
            password: String::new(),
        }
    }
}
