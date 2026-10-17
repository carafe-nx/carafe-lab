//! A command error in the form the window receives it.

use serde::Serialize;
use ts_rs::TS;

/// A command error: a text the window shows as it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandError {
    /// What went wrong.
    pub message: String,
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }
}
