use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Level {
    /// "High-level" | "Detailed" | "Implementation"
    pub level: String,
    pub title: String,
    /// Markdown.
    pub content: String,
}
