use anyhow::{Context, Result, bail};
use reqwest::{Client, header::CONTENT_DISPOSITION};
use serde::Serialize;
use std::path::{Path, PathBuf};

const ALLOWED_PREFIXES: &[&str] = &[
    "https://www.g2b.go.kr/",
    "http://www.g2b.go.kr/",
    "https://www.bizinfo.go.kr/",
    "http://www.bizinfo.go.kr/",
];

#[derive(Debug, Serialize)]
pub struct DownloadedFile {
    pub saved_path: String,
    pub file_name: String,
    pub bytes: usize,
    pub extension: Option<String>,
    pub hint: String,
}

/// 나라장터(g2b.go.kr) 및 기업마당(bizinfo.go.kr) 첨부파일을 로컬 디렉토리에 저장
pub async fn download_file(
    client: &Client,
    file_url: &str,
    suggested_name: Option<&str>,
    dest_dir: &Path,
) -> Result<DownloadedFile> {
    let url = file_url.trim();
    if !ALLOWED_PREFIXES.iter().any(|p| url.starts_with(p)) {
        bail!("허용되지 않은 다운로드 도메인입니다 (g2b.go.kr / bizinfo.go.kr만 허용): {url}");
    }

    tokio::fs::create_dir_all(dest_dir)
        .await
        .with_context(|| format!("다운로드 디렉토리 생성 실패: {}", dest_dir.display()))?;

    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("첨부파일 다운로드 요청 실패: {url}"))?;

    if !resp.status().is_success() {
        bail!("첨부파일 다운로드 HTTP 오류: {}", resp.status());
    }

    let header_filename = resp
        .headers()
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(extract_filename_from_disposition);

    let raw_name = suggested_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or(header_filename)
        .unwrap_or_else(|| "downloaded_attachment.bin".to_string());

    let safe_name = sanitize_filename(&raw_name);
    let out_path: PathBuf = dest_dir.join(&safe_name);

    let bytes = resp.bytes().await.context("첨부파일 본문 수신 실패")?;
    tokio::fs::write(&out_path, &bytes)
        .await
        .with_context(|| format!("파일 저장 실패: {}", out_path.display()))?;

    let extension = out_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase);

    let hint = match extension.as_deref() {
        Some("pdf") => "PDF 파일입니다. 에이전트 내장 파일 읽기 도구로 직접 분석할 수 있습니다.",
        Some("hwp") => "구형 HWP 바이너리 파일입니다. 로컬 변환 도구(예: hwp5txt)로 텍스트 추출 후 읽으세요.",
        Some("hwpx") => "HWPX(ZIP+XML) 파일입니다. 로컬 변환 도구 또는 python-hwpx/unzip으로 텍스트 추출 후 읽으세요.",
        Some("zip") => "ZIP 압축 파일입니다. 압축 해제 후 내부 제안요청서(PDF/HWP/HWPX)를 확인하세요.",
        _ => "로컬에 저장되었습니다. 확장자에 맞는 도구로 내용을 확인하세요.",
    }
    .to_string();

    Ok(DownloadedFile {
        saved_path: out_path.display().to_string(),
        file_name: safe_name,
        bytes: bytes.len(),
        extension,
        hint,
    })
}

fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("attachment.bin");
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.');
    if trimmed.is_empty() {
        "attachment.bin".to_string()
    } else {
        trimmed.to_string()
    }
}

fn extract_filename_from_disposition(header: &str) -> Option<String> {
    for part in header.split(';') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("filename=") {
            let unquoted = rest.trim().trim_matches('"');
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}