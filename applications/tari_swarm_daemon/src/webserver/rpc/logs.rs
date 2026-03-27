//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fs::DirEntry,
    io,
    path::{Path, PathBuf},
};

use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
use serde::Deserialize;
use tokio::fs;

use crate::{config::InstanceType, webserver::context::HandlerContext};

#[derive(Debug, Clone, Deserialize)]
pub struct ListLogFilesRequest {
    pub instance_type: InstanceType,
    #[serde(default)]
    pub index: Option<usize>,
}

/// (full path, name, path without extension)
pub type ListValidatorNodesResponse = Vec<(String, String, String)>;

pub async fn list_log_files(
    context: &HandlerContext,
    req: ListLogFilesRequest,
) -> Result<ListValidatorNodesResponse, anyhow::Error> {
    let instances = context
        .process_manager()
        .list_instances(Some(req.instance_type))
        .await?;

    let mut log_files = Vec::new();
    if let Some(index) = req.index {
        let instance = instances.get(index).ok_or_else(|| {
            JsonRpcError::new(
                JsonRpcErrorReason::InvalidParams,
                format!("Invalid index {index}"),
                serde_json::Value::Null,
            )
        })?;
        visit_dirs(&instance.base_path.join("log"), &mut |dir| {
            if dir.path().extension() == Some("log".as_ref()) {
                let path = dir.path();
                let path_without_ext = path.with_extension("");
                log_files.push((
                    path.to_string_lossy().to_string(),
                    instance.name.clone(),
                    path_without_ext.to_string_lossy().to_string(),
                ));
            }
        })?;
    } else {
        for instance in instances {
            visit_dirs(&instance.base_path.join("log"), &mut |dir| {
                if dir.path().extension() == Some("log".as_ref()) {
                    let path = dir.path();
                    let path_without_ext = path.with_extension("");
                    log_files.push((
                        path.to_string_lossy().to_string(),
                        instance.name.clone(),
                        path_without_ext.to_string_lossy().to_string(),
                    ));
                }
            })?;
        }
    }

    Ok(log_files)
}

fn visit_dirs<F: FnMut(&DirEntry)>(dir: &Path, cb: &mut F) -> io::Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb)?;
            } else {
                cb(&entry);
            }
        }
    }
    Ok(())
}

pub type ListStdoutLogsRequest = ListLogFilesRequest;

/// (full path, name)
pub type ListStdoutLogsResponse = Vec<(String, &'static str)>;
pub async fn list_stdout_files(
    context: &HandlerContext,
    req: ListStdoutLogsRequest,
) -> Result<ListStdoutLogsResponse, anyhow::Error> {
    let instances = context
        .process_manager()
        .list_instances(Some(req.instance_type))
        .await?;

    let mut log_files = Vec::new();
    if let Some(index) = req.index {
        let instance = instances.get(index).ok_or_else(|| {
            JsonRpcError::new(
                JsonRpcErrorReason::InvalidParams,
                format!("Invalid index {index}"),
                serde_json::Value::Null,
            )
        })?;
        if instance.base_path.join("stdout.log").exists() {
            log_files.push((
                instance.base_path.join("stdout.log").to_string_lossy().to_string(),
                "stdout",
            ));
        }
        if instance.base_path.join("stderr.log").exists() {
            log_files.push((
                instance.base_path.join("stderr.log").to_string_lossy().to_string(),
                "stdout",
            ));
        }
    } else {
        for instance in instances {
            if instance.base_path.join("stdout.log").exists() {
                log_files.push((
                    instance.base_path.join("stdout.log").to_string_lossy().to_string(),
                    "stdout",
                ));
            }
            if instance.base_path.join("stderr.log").exists() {
                log_files.push((
                    instance.base_path.join("stderr.log").to_string_lossy().to_string(),
                    "stdout",
                ));
            }
        }
    }

    Ok(log_files)
}

// TODO: this is to preserve the existing API, but it should be changed to a struct
pub type GetLogFileRequest = PathBuf;
pub type GetLogFileResponse = String;
pub async fn get_log_file(
    context: &HandlerContext,
    req: GetLogFileRequest,
) -> Result<GetLogFileResponse, anyhow::Error> {
    let file_path = &req;

    if !file_path.starts_with(&context.config().base_dir) ||
        file_path.extension() != Some("log".as_ref()) ||
        !file_path.exists()
    {
        return Err(JsonRpcError::new(
            JsonRpcErrorReason::InvalidParams,
            "Invalid file path".to_string(),
            serde_json::Value::Null,
        )
        .into());
    }

    let contents = fs::read_to_string(file_path).await?;

    Ok(contents)
}
