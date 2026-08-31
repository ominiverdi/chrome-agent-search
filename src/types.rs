use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(crate) struct TimedText {
    pub(crate) ok: bool,
    pub(crate) url: String,
    pub(crate) page: String,
    pub(crate) goto_seconds: f64,
    pub(crate) read_seconds: f64,
    pub(crate) text_chars: usize,
    pub(crate) text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct SearchResult {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) snippet: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) sources: Vec<String>,
}

impl SearchResult {
    pub(crate) fn new(title: String, url: String, snippet: String) -> Self {
        Self {
            title,
            url,
            snippet,
            sources: Vec::new(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub(crate) struct SearchOutput {
    pub(crate) ok: bool,
    pub(crate) query: String,
    pub(crate) engine: String,
    pub(crate) page: String,
    pub(crate) seconds: f64,
    pub(crate) count: usize,
    pub(crate) results: Vec<SearchResult>,
}

#[derive(Serialize)]
pub(crate) struct FullResult {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) snippet: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) sources: Vec<String>,
    pub(crate) fetch_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fetch_method: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FullOutput {
    pub(crate) ok: bool,
    pub(crate) query: String,
    pub(crate) engine: String,
    pub(crate) count: usize,
    pub(crate) results: Vec<FullResult>,
}

#[derive(Serialize)]
pub(crate) struct CleanupRecord {
    pub(crate) browser: String,
    pub(crate) ok: bool,
    pub(crate) output: String,
    pub(crate) error: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CleanupOutput {
    pub(crate) ok: bool,
    pub(crate) records: Vec<CleanupRecord>,
}

#[derive(Serialize)]
pub(crate) struct DiagnoseOutput {
    pub(crate) ok: bool,
    pub(crate) requested_url: String,
    pub(crate) final_url: Option<String>,
    pub(crate) redirected: Option<bool>,
    pub(crate) http_status: Option<u16>,
    pub(crate) title: Option<String>,
    pub(crate) snapshot_excerpt: Option<String>,
    pub(crate) hint: String,
}
