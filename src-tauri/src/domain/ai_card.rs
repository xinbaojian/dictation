//! domain/ai_card.rs（Task 1 时的临时骨架，Task 3 补全校验与 prompt）
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfusableHint {
    pub char: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardContent {
    #[serde(default)]
    pub decomposition: Vec<String>,
    pub mnemonic: String,
    #[serde(default)]
    pub confusables: Vec<ConfusableHint>,
    pub sentences: Vec<String>,
}
