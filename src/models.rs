use serde::{Serialize, Deserialize};

#[derive(Serialize)]
pub struct UploadResponse {
    pub full_url: String,
    pub filename: String,
    pub path: String,
}

#[derive(Deserialize)]
pub struct DirectoryQuery {
    pub directory: Option<String>,
}