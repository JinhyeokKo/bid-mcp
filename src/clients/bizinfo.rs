use anyhow::{Context, Result, bail};
use chrono::Local;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

const BIZINFO_URL: &str = "https://www.bizinfo.go.kr/uss/rss/bizinfoApi.do";
const BIZINFO_HOST: &str = "https://www.bizinfo.go.kr";

#[derive(Debug)]
pub struct BizinfoSearchQuery {
    /// 클라이언트 측 키워드 필터 (공고명 + 사업개요 + 해시태그 + 지원대상 + 기관명 대상)
    pub keyword: Option<String>,
    /// 분야코드 (01:금융, 02:기술, 03:인력, 04:수출, 05:내수, 06:창업, 07:경영, 09:기타)
    pub field_cd: Option<String>,
    /// 해시태그 쉼표 다중입력 (예: "기술,부산", "창업,서울")
    pub hashtags: Option<String>,
    /// 마감된 공고 제외 여부 (reqstBeginEndDe 종료일 기준)
    pub active_only: bool,
    /// 페이지당 조회 건수 (기본 100)
    pub page_unit: usize,
    /// 스캔할 최신 페이지 수 (기본 2, 최대 5)
    pub max_pages: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BizinfoAttachment {
    /// "print_doc" (본문출력파일, 주로 PDF) | "attachment" (첨부파일)
    pub kind: String,
    pub file_name: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct BizinfoProgram {
    pub id: String,                     // pblancId
    pub title: String,                  // pblancNm
    pub field: Option<String>,          // pldirSportRealmLclasCodeNm
    pub authority: Option<String>,      // jrsdInsttNm (소관기관명)
    pub agency: Option<String>,         // excInsttNm (수행기관명)
    pub target: Option<String>,         // trgetNm (지원대상)
    pub period: Option<String>,         // reqstBeginEndDe (신청기간)
    pub registered_at: Option<String>,  // creatPnttm (등록일자)
    pub hashtags: Option<String>,       // hashTags
    pub summary: Option<String>,        // bsnsSumryCn (사업개요, HTML 태그 제거)
    pub detail_url: String,             // pblancUrl
    pub attachments: Vec<BizinfoAttachment>,
}

pub async fn search_programs(
    client: &Client,
    api_key: &str,
    q: &BizinfoSearchQuery,
) -> Result<Vec<BizinfoProgram>> {
    if api_key.trim().is_empty() {
        bail!("BIZINFO_API_KEY 환경변수가 비어 있습니다.");
    }

    let page_unit = q.page_unit.clamp(1, 200);
    let max_pages = q.max_pages.clamp(1, 5);
    let kw_lower = q
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);
    let today_yyyymmdd = Local::now().format("%Y%m%d").to_string();

    let mut results = Vec::new();

    for page in 1..=max_pages {
        let mut params: Vec<(&str, String)> = vec![
            ("crtfcKey", api_key.to_string()),
            ("dataType", "json".to_string()),
            ("pageUnit", page_unit.to_string()),
            ("pageIndex", page.to_string()),
        ];

        if let Some(code) = non_empty(&q.field_cd) {
            params.push(("searchLclasId", code));
        }
        if let Some(tags) = non_empty(&q.hashtags) {
            params.push(("hashtags", tags));
        }

        let resp = client
            .get(BIZINFO_URL)
            .query(&params)
            .send()
            .await
            .context("기업마당 API 요청 실패")?;

        let status = resp.status();
        let text = resp.text().await.context("기업마당 응답 본문 수신 실패")?;
        if !status.is_success() {
            bail!("기업마당 HTTP {status} 오류: {}", truncate(&text, 300));
        }

        let json: Value = serde_json::from_str(&text)
            .with_context(|| format!("기업마당 JSON 파싱 실패: {}", truncate(&text, 300)))?;

        let items = json["jsonArray"]
            .as_array()
            .or_else(|| json.as_array())
            .cloned()
            .unwrap_or_default();

        if items.is_empty() {
            break;
        }

        let fetched_len = items.len();
        for item in items {
            let program = parse_program(&item);

            if q.active_only && is_expired(program.period.as_deref(), &today_yyyymmdd) {
                continue;
            }

            if let Some(ref kw) = kw_lower {
                let haystack = format!(
                    "{} {} {} {} {} {}",
                    program.title,
                    program.summary.as_deref().unwrap_or_default(),
                    program.hashtags.as_deref().unwrap_or_default(),
                    program.target.as_deref().unwrap_or_default(),
                    program.authority.as_deref().unwrap_or_default(),
                    program.agency.as_deref().unwrap_or_default(),
                )
                .to_lowercase();
                if !haystack.contains(kw) {
                    continue;
                }
            }

            results.push(program);
        }

        if fetched_len < page_unit {
            break;
        }
    }

    Ok(results)
}

fn parse_program(item: &Value) -> BizinfoProgram {
    let raw_url = get_str(item, "pblancUrl");
    let detail_url = normalize_url(&raw_url);

    let mut attachments = Vec::new();
    // 본문 출력물(주로 PDF)을 먼저 배치하여 에이전트가 우선 활용하도록 유도
    push_attachments(
        &mut attachments,
        "print_doc",
        get_opt_str(item, "printFlpthNm"),
        get_opt_str(item, "printFileNm"),
    );
    push_attachments(
        &mut attachments,
        "attachment",
        get_opt_str(item, "flpthNm"),
        get_opt_str(item, "fileNm"),
    );

    BizinfoProgram {
        id: get_str(item, "pblancId"),
        title: get_str(item, "pblancNm"),
        field: get_opt_str(item, "pldirSportRealmLclasCodeNm"),
        authority: get_opt_str(item, "jrsdInsttNm"),
        agency: get_opt_str(item, "excInsttNm"),
        target: get_opt_str(item, "trgetNm"),
        period: get_opt_str(item, "reqstBeginEndDe"),
        registered_at: get_opt_str(item, "creatPnttm"),
        hashtags: get_opt_str(item, "hashTags"),
        summary: get_opt_str(item, "bsnsSumryCn").map(|s| strip_html(&s)),
        detail_url,
        attachments,
    }
}

/// 기업마당 첨부파일이 단일 또는 '@' 구분 복수 건으로 올 때 모두 대응
fn push_attachments(
    out: &mut Vec<BizinfoAttachment>,
    kind: &str,
    urls_opt: Option<String>,
    names_opt: Option<String>,
) {
    let Some(urls_raw) = urls_opt else {
        return;
    };
    let urls: Vec<&str> = urls_raw
        .split('@')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let names: Vec<&str> = names_opt
        .as_deref()
        .unwrap_or_default()
        .split('@')
        .map(str::trim)
        .collect();

    for (idx, u) in urls.into_iter().enumerate() {
        let name = names
            .get(idx)
            .copied()
            .filter(|s| !s.is_empty())
            .unwrap_or("bizinfo_doc.pdf")
            .to_string();
        out.push(BizinfoAttachment {
            kind: kind.to_string(),
            file_name: name,
            url: normalize_url(u),
        });
    }
}

/// "20261101 ~ 20261130" 형태에서 종료일(8자리 숫자)만 추출해 마감 여부 판별
/// "예산 소진시까지" 등 비정형 문구는 제외하지 않고 유지
fn is_expired(period: Option<&str>, today_yyyymmdd: &str) -> bool {
    let Some(p) = period else {
        return false;
    };
    let Some((_, end_part)) = p.split_once('~') else {
        return false;
    };
    let digits: String = end_part.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 8 {
        return &digits[..8] < today_yyyymmdd;
    }
    false
}

fn normalize_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else if trimmed.starts_with('/') {
        format!("{BIZINFO_HOST}{trimmed}")
    } else {
        format!("{BIZINFO_HOST}/{trimmed}")
    }
}

fn strip_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn non_empty(opt: &Option<String>) -> Option<String> {
    opt.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn get_str(v: &Value, key: &str) -> String {
    get_opt_str(v, key).unwrap_or_default()
}

fn get_opt_str(v: &Value, key: &str) -> Option<String> {
    match &v[key] {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}