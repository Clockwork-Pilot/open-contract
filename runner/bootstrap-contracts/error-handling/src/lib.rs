use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// An actionable error passed from a Rust runner layer to the command boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractError {
    exit_code: u8,
    message: String,
    contexts: Vec<String>,
}

impl ContractError {
    pub fn new(exit_code: u8, message: impl Into<String>) -> Self {
        Self {
            // An error must never make the process exit successfully.
            exit_code: exit_code.max(1),
            message: message.into(),
            contexts: Vec::new(),
        }
    }

    /// Add an outer layer while preserving the original error message.
    pub fn context(mut self, layer: impl Into<String>) -> Self {
        self.contexts.push(layer.into());
        self
    }

    pub fn exit_code(&self) -> u8 {
        self.exit_code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Render outer-to-inner context followed by the original error.
    pub fn render(&self) -> String {
        let mut parts = self.contexts.iter().rev().cloned().collect::<Vec<_>>();
        parts.push(self.message.clone());
        parts.join(": ")
    }
}

impl From<(u8, String)> for ContractError {
    fn from((exit_code, message): (u8, String)) -> Self {
        Self::new(exit_code, message)
    }
}

impl Display for ContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render())
    }
}

impl Error for ContractError {}
