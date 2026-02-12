use serde::{Deserialize, Serialize};

/// Request body for session start
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartRequest {
    /// Path to current project or workspace
    /// Should be set to "/", otherwise the server will return "Internal Server Error"
    pub path: String,
}

/// Session data containing upload configuration and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartData {
    /// Content Link URL
    pub cl_link: String,
    /// Session expiration time
    pub expires_at: String,
    /// Maximum concurrent uploads allowed
    pub max_concurrent_uploads: i64,
    /// Maximum file size in bytes
    pub max_file_size: i64,
    /// Maximum number of files allowed
    pub max_files: i64,
}

/// Response from session start request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartResponse {
    /// Session data containing upload configuration
    pub data: SessionStartData,
    /// Error message if any
    pub err_message: String,
    /// Request result status
    pub req_result: bool,
}

/// Request body for session complete
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCompleteRequest {
    /// Commit message for the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
}

/// Completion data containing upload results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCompleteData {
    /// Content Link ID
    pub cl_id: i64,
    /// Content Link URL
    pub cl_link: String,
    /// Commit ID
    pub commit_id: String,
    /// Creation timestamp
    pub created_at: String,
    /// Number of files uploaded
    pub files_count: i64,
}

/// Response from session complete request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCompleteResponse {
    /// Completion data containing upload results
    pub data: SessionCompleteData,
    /// Error message if any
    pub err_message: String,
    /// Request result status
    pub req_result: bool,
}

/// File upload data containing verification status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFileData {
    /// File path
    pub file_path: String,
    /// Uploaded size in bytes
    pub uploaded_size: i64,
    /// Whether the file upload was verified
    pub verified: bool,
}

/// Response from file upload request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFileResponse {
    /// File upload data
    pub data: SessionFileData,
    /// Error message if any
    pub err_message: String,
    /// Request result status
    pub req_result: bool,
}

/// Manifest file entry for session request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifestFile {
    /// File hash
    pub hash: String,
    /// File path
    pub path: String,
    /// File size in bytes
    pub size: i64,
}

/// Request body for session manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifestRequest {
    /// Commit message for the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
    /// Files included in the manifest
    pub files: Vec<SessionManifestFile>,
}

/// File entry that needs upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifestUploadFile {
    /// File path
    pub path: String,
    /// Upload reason
    pub reason: String,
}

/// Manifest response data with upload summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifestData {
    /// Files that need to be uploaded
    pub files_to_upload: Vec<SessionManifestUploadFile>,
    /// Count of unchanged files
    pub files_unchanged: i64,
    /// Total file count in manifest
    pub total_files: i64,
    /// Total size of files in bytes
    pub total_size: i64,
    /// Upload size in bytes
    pub upload_size: i64,
}

/// Response from session manifest request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifestResponse {
    /// Manifest response data
    pub data: SessionManifestData,
    /// Error message if any
    pub err_message: String,
    /// Request result status
    pub req_result: bool,
}
