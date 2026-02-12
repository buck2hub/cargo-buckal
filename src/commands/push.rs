use clap::Parser;
use reqwest::{
    blocking::Client,
    header::{AUTHORIZATION, CONTENT_TYPE},
};
use serde_json::json;
use sha1::{Digest, Sha1};
use walkdir::WalkDir;

use crate::{
    RUST_CRATES_ROOT, buckal_error, buckal_log,
    config::Config,
    registry::{
        SessionCompleteRequest, SessionCompleteResponse, SessionFileResponse, SessionManifestFile,
        SessionManifestRequest, SessionManifestResponse, SessionStartRequest, SessionStartResponse,
    },
    utils::{UnwrapOrExit, get_buck2_root},
};

#[derive(Parser, Debug)]
pub struct PushArgs {
    /// Registry to use
    #[arg(long)]
    pub registry: Option<String>,
    /// Description of the BUCK file changes
    #[arg(long, short)]
    pub message: Option<String>,
}

pub fn execute(args: &PushArgs) {
    let mut config = Config::load();

    let registry_name = args
        .registry
        .as_deref()
        .unwrap_or_else(|| config.default_registry())
        .to_string();

    if let Some(registry) = config.registries.get_mut(&registry_name) {
        if let Some(token) = &registry.token {
            let client = Client::new();
            // Step 1: Create a new upload session
            let start_request = SessionStartRequest {
                path: "/".to_string(),
            };
            let response: SessionStartResponse = client
                .post(format!("{}/api/v1/buck/session/start", registry.api))
                .body(json!(start_request).to_string())
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .header(CONTENT_TYPE, "application/json")
                .send()
                .unwrap_or_exit_ctx("failed to start session")
                .json()
                .unwrap_or_exit();
            if !response.req_result {
                buckal_error!("failed to start session: {}", response.err_message);
                std::process::exit(1);
            }
            let cl_link = response.data.cl_link;
            buckal_log!("Push", format!("session started. Change List: {}", cl_link));
            // Step 2: Upload file manifest
            let mut manifest = SessionManifestRequest {
                commit_message: Some(
                    args.message
                        .as_deref()
                        .unwrap_or("Update third-party BUCK files")
                        .to_string(),
                ),
                files: vec![],
            };
            let buck2_root = get_buck2_root().unwrap_or_exit();
            let third_party_dir = buck2_root.join(RUST_CRATES_ROOT);
            for entry in WalkDir::new(&third_party_dir).into_iter() {
                let entry = entry.unwrap_or_exit_ctx("failed to read third-party directory");
                let entry_path = entry.path();
                if entry_path.is_file() && entry_path.file_name().unwrap() == "BUCK" {
                    let file_content = std::fs::read(entry_path).unwrap_or_exit();
                    let file_size = file_content.len() as i64;
                    let file_hash = Sha1::digest(file_content);
                    let relative_path = entry_path
                        .strip_prefix(&buck2_root)
                        .unwrap_or_exit_ctx("failed to resolve relative path")
                        .to_string_lossy()
                        .into_owned();
                    manifest.files.push(SessionManifestFile {
                        path: relative_path,
                        size: file_size,
                        hash: format!("sha1:{}", hex::encode(file_hash)),
                    });
                }
            }
            let response: SessionManifestResponse = client
                .post(format!(
                    "{}/api/v1/buck/session/{}/manifest",
                    registry.api, cl_link
                ))
                .body(json!(manifest).to_string())
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .header(CONTENT_TYPE, "application/json")
                .send()
                .unwrap_or_exit_ctx("failed to upload manifest")
                .json()
                .unwrap_or_exit();
            if !response.req_result {
                buckal_error!("failed to upload manifest: {}", response.err_message);
                std::process::exit(1);
            }
            let files_to_upload = response.data.files_to_upload;
            if files_to_upload.is_empty() {
                buckal_log!("Push", "no files need to be uploaded");
            } else {
                buckal_log!(
                    "Push",
                    format!("{} files need to be uploaded", files_to_upload.len())
                );
                // Step 3: Upload BUCK files
                for file in files_to_upload {
                    // Security check: ensure the path doesn't escape buck2_root
                    if !is_safe_subpath(&file.path) {
                        buckal_error!(
                            "path traversal detected: `{}` escapes `{}`",
                            file.path,
                            buck2_root
                        );
                        std::process::exit(1);
                    }

                    let full_path = buck2_root.join(&file.path);
                    let file_content = std::fs::read(&full_path).unwrap_or_exit();
                    let file_size = file_content.len() as i64;
                    buckal_log!("Uploading", &file.path);
                    let response: SessionFileResponse = client
                        .post(format!(
                            "{}/api/v1/buck/session/{}/file",
                            registry.api, &cl_link
                        ))
                        .body(file_content)
                        .header(AUTHORIZATION, format!("Bearer {}", token))
                        .header(CONTENT_TYPE, "application/octet-stream")
                        .header("X-File-Path", &file.path)
                        .header("X-File-Size", file_size.to_string())
                        .send()
                        .unwrap_or_exit_ctx(format!("failed to upload file {}", file.path))
                        .json()
                        .unwrap_or_exit();
                    if !response.req_result {
                        buckal_error!(
                            "failed to upload file {}: {}",
                            file.path,
                            response.err_message
                        );
                        std::process::exit(1);
                    }
                }
            }
            // Step 4: Complete the session
            let response: SessionCompleteResponse = client
                .post(format!(
                    "{}/api/v1/buck/session/{}/complete",
                    registry.api, cl_link
                ))
                .body(
                    json!(SessionCompleteRequest {
                        commit_message: None,
                    })
                    .to_string(),
                )
                .header(AUTHORIZATION, format!("Bearer {}", token))
                .header(CONTENT_TYPE, "application/json")
                .send()
                .unwrap_or_exit_ctx("failed to complete session")
                .json()
                .unwrap_or_exit();
            if !response.req_result {
                buckal_error!("failed to complete session: {}", response.err_message);
                std::process::exit(1);
            }
            buckal_log!("Push", "session completed successfully");
        } else {
            buckal_error!("no token found, please run `cargo buckal login` first");
            std::process::exit(1);
        }
    } else {
        buckal_error!("registry `{}` not found in configuration", registry_name);
        std::process::exit(1);
    }
}

/// Validates that the target path is safely contained within the base path.
/// This prevents path traversal attacks where `..` components could escape the base directory.
fn is_safe_subpath(target: &str) -> bool {
    // Reject absolute paths
    if target.starts_with('/') || target.starts_with('\\') {
        return false;
    }

    // Reject paths with Windows drive letters or UNC paths
    if target.contains(':') {
        return false;
    }

    // Process path by normalizing .. and . components
    let mut depth: i32 = 0;
    for part in target.split(['/', '\\']) {
        match part {
            "" | "." => {
                // Skip empty parts and current dir references
            }
            ".." => {
                // For each .., decrease depth
                depth -= 1;
                // If depth goes negative, we're trying to escape the base
                if depth < 0 {
                    return false;
                }
            }
            _ => {
                // Normal path component increases depth
                depth += 1;
            }
        }
    }

    // If we reach here, the path is safe
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_relative_paths() {
        assert!(is_safe_subpath("file.txt"));
        assert!(is_safe_subpath("subdir/file.txt"));
        assert!(is_safe_subpath("nested/deep/file.txt"));
        assert!(is_safe_subpath("a/b/c/d/e.txt"));
    }

    #[test]
    fn test_path_with_dot_components() {
        // Current directory references should be normalized away
        assert!(is_safe_subpath("./file.txt"));
        assert!(is_safe_subpath("subdir/./file.txt"));
        assert!(is_safe_subpath("./subdir/./file.txt"));
    }

    #[test]
    fn test_single_dot_dot_escape() {
        // Single .. that tries to escape should be rejected
        assert!(!is_safe_subpath("../file.txt"));
        assert!(!is_safe_subpath("subdir/../../file.txt"));
    }

    #[test]
    fn test_multiple_dot_dot_escape() {
        // Multiple .. levels
        assert!(!is_safe_subpath("../../file.txt"));
        assert!(!is_safe_subpath("../../../etc/passwd"));
        assert!(!is_safe_subpath("a/b/c/../../../../file.txt"));
    }

    #[test]
    fn test_absolute_paths_unix() {
        // Unix absolute paths should be rejected
        assert!(!is_safe_subpath("/etc/passwd"));
        assert!(!is_safe_subpath("/tmp/file.txt"));
        assert!(!is_safe_subpath("/"));
    }

    #[test]
    fn test_absolute_paths_windows() {
        // Windows absolute paths should be rejected
        assert!(!is_safe_subpath("\\Windows\\System32"));
        assert!(!is_safe_subpath("\\file.txt"));
    }

    #[test]
    fn test_windows_drive_letters() {
        // Windows drive letters should be rejected
        assert!(!is_safe_subpath("C:\\Windows\\System32"));
        assert!(!is_safe_subpath("D:\\file.txt"));
        assert!(!is_safe_subpath("file:txt"));
    }

    #[test]
    fn test_unc_paths() {
        // UNC paths with colons should be rejected
        assert!(!is_safe_subpath("\\\\server\\share\\file.txt"));
        assert!(!is_safe_subpath("./\\\\server:\\share"));
    }

    #[test]
    fn test_safe_subdir_traversal() {
        // Safe traversal within subdirectories
        assert!(is_safe_subpath("subdir/file.txt"));
        assert!(is_safe_subpath("a/b/c/file.txt"));
        assert!(is_safe_subpath("deep/nested/path/file.txt"));
    }

    #[test]
    fn test_safe_with_mixed_separators() {
        // Mixed separators but still safe
        assert!(is_safe_subpath("subdir/file.txt"));
        assert!(is_safe_subpath("a\\b\\c\\file.txt"));
        assert!(is_safe_subpath("mixed/path\\file.txt"));
    }

    #[test]
    fn test_empty_and_dot_components() {
        // Empty path components and dots
        assert!(is_safe_subpath("."));
        assert!(is_safe_subpath("a/./b"));
        assert!(is_safe_subpath("a//b"));
    }

    #[test]
    fn test_complex_escape_attempts() {
        // Complex attempts to escape
        assert!(!is_safe_subpath("file/../../../../../../../etc/passwd"));
        assert!(!is_safe_subpath("./a/../b/../../file.txt"));
        assert!(!is_safe_subpath("legitimate/../../.."));
    }

    #[test]
    fn test_dot_dot_at_boundary() {
        // .. exactly at the boundary
        assert!(!is_safe_subpath(".."));
        assert!(!is_safe_subpath("a/b/c/../../../../"));
    }

    #[test]
    fn test_hidden_files() {
        // Hidden files (starting with .) should be safe if no escape
        assert!(is_safe_subpath(".hidden"));
        assert!(is_safe_subpath("dir/.hidden"));
        assert!(is_safe_subpath(".config/file.txt"));
    }
}
