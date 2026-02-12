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
        SessionCompleteRequest, SessionManifestFile, SessionManifestRequest,
        SessionManifestResponse, SessionStartRequest, SessionStartResponse,
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
        if registry.token.is_none() {
            buckal_error!("no token found, please run `cargo buckal login` first");
            std::process::exit(1);
        } else {
            let client = Client::new();
            // Step 1: Create a new upload session
            let start_request = SessionStartRequest {
                path: "/".to_string(),
            };
            let response: SessionStartResponse = client
                .post(format!("{}/api/v1/buck/session/start", registry.api))
                .body(json!(start_request).to_string())
                .header(
                    AUTHORIZATION,
                    format!("Bearer {}", registry.token.as_ref().unwrap()),
                )
                .header(CONTENT_TYPE, "application/json")
                .send()
                .and_then(|r| r.error_for_status())
                .unwrap_or_exit_ctx("failed to start session")
                .json()
                .unwrap_or_exit();
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
                let entry_path = entry.as_ref().unwrap().path();
                if entry_path.is_file() && entry_path.file_name().unwrap() == "BUCK" {
                    let file_content = std::fs::read(entry_path).unwrap_or_exit();
                    let file_size = file_content.len() as i64;
                    let file_hash = Sha1::digest(file_content);
                    let relative_path = entry_path
                        .strip_prefix(&buck2_root)
                        .ok()
                        .unwrap()
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
                .header(
                    AUTHORIZATION,
                    format!("Bearer {}", registry.token.as_ref().unwrap()),
                )
                .header(CONTENT_TYPE, "application/json")
                .send()
                .and_then(|r| r.error_for_status())
                .unwrap_or_exit_ctx("failed to upload manifest")
                .json()
                .unwrap_or_exit();
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
                    let full_path = buck2_root.join(&file.path);
                    let file_content = std::fs::read(&full_path).unwrap_or_exit();
                    let file_size = file_content.len() as i64;
                    buckal_log!("Uploading", &file.path);
                    client
                        .post(format!(
                            "{}/api/v1/buck/session/{}/file",
                            registry.api, &cl_link
                        ))
                        .body(file_content)
                        .header(
                            AUTHORIZATION,
                            format!("Bearer {}", registry.token.as_ref().unwrap()),
                        )
                        .header(CONTENT_TYPE, "application/octet-stream")
                        .header("X-File-Path", &file.path)
                        .header("X-File-Size", file_size.to_string())
                        .send()
                        .and_then(|r| r.error_for_status())
                        .unwrap_or_exit_ctx(format!("failed to upload file {}", file.path));
                }
            }
            // Step 4: Complete the session
            client
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
                .header(
                    AUTHORIZATION,
                    format!("Bearer {}", registry.token.as_ref().unwrap()),
                )
                .header(CONTENT_TYPE, "application/json")
                .send()
                .and_then(|r| r.error_for_status())
                .unwrap_or_exit_ctx("failed to complete session");
            buckal_log!("Push", "session completed successfully");
        }
    } else {
        buckal_error!("registry `{}` not found in configuration", registry_name);
        std::process::exit(1);
    }
}
