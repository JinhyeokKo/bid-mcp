use anyhow::{Context, Result, bail};
use chrono::{Duration, Local};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// 조달청 나라장터 사전규격정보서비스 (v1.0)
const PRESPEC_BASE_URL: &str = "https://apis.data.go.kr/1230000/ao/HrcspSsstndrdInfoService";

const CHUNK_DAYS: i64 = 7;
const MAX_ROWS_PER_PAGE: usize = 100;
const MAX_PAGES_PER_CHUNK: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PrespecCategory {
    /// 용역 (SW 개발, SI, 유지보수, 컨설팅 등)
    Servc,
    /// 물품 (장비, 센서, 패키지 SW 등)
    Thng,
    /// 공사 (정보통신공사 등)
    Cnstwk,
    /// 외자
    Frgcpt,
}

impl PrespecCategory {
    fn pps_search_op(self) -> &'static str {
        match self {
            Self::Servc => "getPublicPrcureThngInfoServcPPSSrch",
            Self::Thng => "getPublicPrcureThngInfoThngPPSSrch",
            Self::Cnstwk => "getPublicPrcureThngInfoCnstwkPPSSrch",
            Self::Frgcpt => "getPublicPrcureThngInfoFrgcptPPSSrch",
        }
    }

    fn opinion_op(self) -> &'static str {
        match self {
            Self::Servc => "getPublicPrcureThngOpinionInfoServc",
            Self::Thng => "getPublicPrcureThngOpinionInfoThng",
            Self::Cnstwk => "getPublicPrcureThngOpinionInfoCnstwk",
            Self::Frgcpt => "getPublicPrcureThngOpinionInfoFrgcpt",
        }
    }

    fn supports_sw_flag(self) -> bool {
        !matches!(self, Self::Cnstwk)
    }

    fn supports_detail_product_code(self) -> bool {
        matches!(self, Self::Servc | Self::Thng)
    }
}

#[derive(Debug)]
pub struct PrespecSearchQuery {
    pub category: PrespecCategory,
    /// 단건 조회용 사전규격등록번호 (지정 시 inqryDiv=2 최우선)
    pub bf_spec_rgst_no: Option<String>,
    /// 참조번호 조회 (지정 시 inqryDiv=3)
    pub ref_no: Option<String>,
    /// 품명/사업명 검색 키워드 (prdctClsfcNoNm)
    pub keyword: Option<String>,
    /// 공고(발주)기관명 (ntceInsttNm)
    pub ntce_instt_nm: Option<String>,
    /// 수요기관명 (dminsttNm)
    pub dminstt_nm: Option<String>,
    /// SW사업 대상만 조회 (swBizObjYn=Y, 공사 제외)
    pub sw_biz_only: bool,
    /// 세부품명번호 10자리 (용역/물품 전용)
    pub dtil_prdct_no: Option<String>,
    /// 최소 배정예산액 필터 (원화, 클라이언트 측 필터)
    pub min_budget: Option<u64>,
    /// 의견등록마감일시가 아직 지나지 않은 건만 조회 (클라이언트 측 필터)
    pub only_open_opinion: bool,
    /// 아직 본 입찰공고(bidNtceNoList)로 넘어가지 않은 건만 조회 (클라이언트 측 필터)
    pub only_before_bid: bool,
    /// 최근 며칠 이내 접수된 사전규격을 조회할지 (기본 7, 최대 30)
    pub days_back: i64,
}

#[derive(Debug, Serialize)]
pub struct PrespecItem {
    pub category: PrespecCategory,
    pub bf_spec_rgst_no: String,        // 사전규격등록번호 (예: R25BD00086379)
    pub business_div: String,           // 업무구분명 (일반용역, 기술용역, 물품 등)
    pub title: String,                  // 품명/사업명 (prdctClsfcNoNm)
    pub ref_no: Option<String>,         // 참조번호
    pub order_agency: String,           // 발주기관명 (orderInsttNm)
    pub demand_agency: String,          // 실수요기관명 (rlDminsttNm)
    pub budget_amt: Option<String>,     // 배정예산금액 (asignBdgtAmt)
    pub sw_biz_yn: Option<String>,      // SW사업대상여부 (swBizObjYn)
    pub receipt_dt: Option<String>,     // 접수일시 (rcptDt)
    pub opinion_close_dt: Option<String>, // 의견등록마감일시 (opninRgstClseDt)
    pub delivery_deadline: Option<String>, // 납품기한일시 (dlvrTmlmtDt)
    pub delivery_days: Option<String>,  // 납품일수 (dlvrDaynum)
    pub officer_name: Option<String>,   // 담당자명 (ofclNm)
    pub officer_tel: Option<String>,    // 담당자전화번호 (ofclTelNo)
    pub product_details: Option<String>, // 물품상세목록 (prdctDtlList)
    pub registered_dt: String,          // 등록일시 (rgstDt)
    pub changed_dt: Option<String>,     // 변경일시 (chgDt)
    pub linked_bid_nos: Vec<String>,    // 전환된 본 입찰공고번호 목록 (bidNtceNoList)
    pub spec_doc_urls: Vec<String>,     // 규격서(RFP 초안) 다운로드 URL 1~5
}

#[derive(Debug, Serialize)]
pub struct PrespecOpinion {
    pub bf_spec_rgst_no: String,
    pub ref_no: Option<String>,
    pub opinion_no: String,             // 의견번호 (opninNo)
    pub reply_no: String,               // 답변번호 (rplyNo: 0=업체 의견, 1이상=발주처 답변)
    pub kind: String,                   // "opinion" | "reply"
    pub title: String,                  // 의견/답변 제목 (opninTitl)
    pub writer_org: Option<String>,     // 작성업체명/발주기관명 (mkngCorpNm)
    pub writer_name: Option<String>,    // 작성자명 (mkrNm)
    pub input_dt: Option<String>,       // 입력일시 (inptDt)
    pub content: String,                // 의견/답변 본문 (opninCntnts)
    pub attachment_urls: Vec<String>,   // 규격서의견파일 URL 1~5 (수정 RFP 포함 가능)
}

/// 13~16번: 나라장터 검색조건에 의한 사전규격 목록 조회
pub async fn search_prespecs(
    client: &Client,
    service_key: &str,
    q: &PrespecSearchQuery,
) -> Result<Vec<PrespecItem>> {
    if service_key.trim().is_empty() {
        bail!("G2B_API_KEY 환경변수가 비어 있습니다.");
    }

    let endpoint = format!("{PRESPEC_BASE_URL}/{}", q.category.pps_search_op());

    // 단건 번호(inqryDiv=2) 또는 참조번호(inqryDiv=3) 조회는 기간 분할 없이 1회 호출
    if let Some(spec_no) = non_empty(&q.bf_spec_rgst_no) {
        let params = vec![
            ("ServiceKey", service_key.to_string()),
            ("numOfRows", "20".to_string()),
            ("pageNo", "1".to_string()),
            ("type", "json".to_string()),
            ("inqryDiv", "2".to_string()),
            ("bfSpecRgstNo", spec_no),
        ];
        let Some(body) = call_prespec_json(client, &endpoint, &params).await? else {
            return Ok(Vec::new());
        };
        return Ok(extract_items(&body)
            .iter()
            .map(|v| parse_prespec_item(q.category, v))
            .collect());
    }

    if let Some(ref_no) = non_empty(&q.ref_no) {
        let params = vec![
            ("ServiceKey", service_key.to_string()),
            ("numOfRows", "50".to_string()),
            ("pageNo", "1".to_string()),
            ("type", "json".to_string()),
            ("inqryDiv", "3".to_string()),
            ("refNo", ref_no),
        ];
        let Some(body) = call_prespec_json(client, &endpoint, &params).await? else {
            return Ok(Vec::new());
        };
        return Ok(extract_items(&body)
            .iter()
            .map(|v| parse_prespec_item(q.category, v))
            .collect());
    }

    // 기간 조회(inqryDiv=1: 접수일시 기준)
    let total_days = q.days_back.clamp(1, 30);
    let now = Local::now();
    let now_str = now.format("%Y-%m-%d %H:%M:%S").to_string();

    let mut results = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut cursor_end = now;
    let earliest = now - Duration::days(total_days);

    while cursor_end > earliest {
        let cursor_start = (cursor_end - Duration::days(CHUNK_DAYS)).max(earliest);
        let bgn_str = cursor_start.format("%Y%m%d%H%M").to_string();
        let end_str = cursor_end.format("%Y%m%d%H%M").to_string();

        for page in 1..=MAX_PAGES_PER_CHUNK {
            let mut params: Vec<(&str, String)> = vec![
                ("ServiceKey", service_key.to_string()),
                ("numOfRows", MAX_ROWS_PER_PAGE.to_string()),
                ("pageNo", page.to_string()),
                ("type", "json".to_string()),
                ("inqryDiv", "1".to_string()), // 1: 접수일시
                ("inqryBgnDt", bgn_str.clone()),
                ("inqryEndDt", end_str.clone()),
            ];

            if let Some(v) = non_empty(&q.keyword) {
                params.push(("prdctClsfcNoNm", v));
            }
            if let Some(v) = non_empty(&q.ntce_instt_nm) {
                params.push(("ntceInsttNm", v));
            }
            if let Some(v) = non_empty(&q.dminstt_nm) {
                params.push(("dminsttNm", v));
            }
            if q.sw_biz_only && q.category.supports_sw_flag() {
                params.push(("swBizObjYn", "Y".to_string()));
            }
            if q.category.supports_detail_product_code() {
                if let Some(v) = non_empty(&q.dtil_prdct_no) {
                    params.push(("dtilPrdctClsfcNo", v));
                }
            }

            let Some(body) = call_prespec_json(client, &endpoint, &params).await? else {
                break;
            };

            let total_count = val_to_usize(&body["totalCount"]).unwrap_or(0);
            let items = extract_items(&body);
            if items.is_empty() {
                break;
            }

            for raw in items {
                let item = parse_prespec_item(q.category, &raw);
                if item.bf_spec_rgst_no.is_empty() || !seen.insert(item.bf_spec_rgst_no.clone()) {
                    continue;
                }

                // 클라이언트 측 실무 필터링
                if q.only_before_bid && !item.linked_bid_nos.is_empty() {
                    continue;
                }
                if q.only_open_opinion {
                    if let Some(ref close_dt) = item.opinion_close_dt {
                        if close_dt < &now_str {
                            continue;
                        }
                    }
                }
                if let Some(min_b) = q.min_budget {
                    let amt = item
                        .budget_amt
                        .as_deref()
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    if amt < min_b {
                        continue;
                    }
                }

                results.push(item);
            }

            if page * MAX_ROWS_PER_PAGE >= total_count {
                break;
            }
        }

        cursor_end = cursor_start - Duration::minutes(1);
    }

    results.sort_by(|a, b| b.registered_dt.cmp(&a.registered_dt));
    Ok(results)
}

/// 17~20번: 특정 사전규격의 규격서 의견 및 발주처 답변 목록 조회
pub async fn get_prespec_opinions(
    client: &Client,
    service_key: &str,
    category: PrespecCategory,
    bf_spec_rgst_no: &str,
) -> Result<Vec<PrespecOpinion>> {
    if service_key.trim().is_empty() {
        bail!("G2B_API_KEY 환경변수가 비어 있습니다.");
    }

    let spec_no = bf_spec_rgst_no.trim();
    if spec_no.is_empty() {
        bail!("사전규격등록번호(bf_spec_rgst_no)가 비어 있습니다.");
    }

    let endpoint = format!("{PRESPEC_BASE_URL}/{}", category.opinion_op());
    let params = vec![
        ("ServiceKey", service_key.to_string()),
        ("numOfRows", "100".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "2".to_string()), // 2: 사전규격등록번호
        ("bfSpecRgstNo", spec_no.to_string()),
    ];

    let Some(body) = call_prespec_json(client, &endpoint, &params).await? else {
        return Ok(Vec::new());
    };

    let mut opinions: Vec<PrespecOpinion> = extract_items(&body)
        .iter()
        .map(parse_prespec_opinion)
        .collect();

    // 의견번호 -> 답변번호 순으로 정렬 (질의 바로 아래 발주처 답변이 오도록)
    opinions.sort_by_key(|o| {
        (
            o.opinion_no.parse::<u64>().unwrap_or(0),
            o.reply_no.parse::<u64>().unwrap_or(0),
        )
    });

    Ok(opinions)
}

fn parse_prespec_item(category: PrespecCategory, item: &Value) -> PrespecItem {
    let mut spec_doc_urls = Vec::new();
    for i in 1..=5 {
        let key = format!("specDocFileUrl{i}");
        if let Some(url) = get_opt_str(item, &key) {
            spec_doc_urls.push(unescape_url(&url));
        }
    }

    let linked_bid_nos = get_opt_str(item, "bidNtceNoList")
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();

    PrespecItem {
        category,
        bf_spec_rgst_no: get_str(item, "bfSpecRgstNo"),
        business_div: get_str(item, "bsnsDivNm"),
        title: get_str(item, "prdctClsfcNoNm"),
        ref_no: get_opt_str(item, "refNo"),
        order_agency: get_str(item, "orderInsttNm"),
        demand_agency: get_str(item, "rlDminsttNm"),
        budget_amt: get_opt_str(item, "asignBdgtAmt"),
        sw_biz_yn: get_opt_str(item, "swBizObjYn"),
        receipt_dt: get_opt_str(item, "rcptDt"),
        opinion_close_dt: get_opt_str(item, "opninRgstClseDt"),
        delivery_deadline: get_opt_str(item, "dlvrTmlmtDt"),
        delivery_days: get_opt_str(item, "dlvrDaynum"),
        officer_name: get_opt_str(item, "ofclNm"),
        officer_tel: get_opt_str(item, "ofclTelNo"),
        product_details: get_opt_str(item, "prdctDtlList"),
        registered_dt: get_str(item, "rgstDt"),
        changed_dt: get_opt_str(item, "chgDt"),
        linked_bid_nos,
        spec_doc_urls,
    }
}

fn parse_prespec_opinion(item: &Value) -> PrespecOpinion {
    let mut attachment_urls = Vec::new();
    for i in 1..=5 {
        let key = format!("specDocOpninFileUrl{i}");
        if let Some(url) = get_opt_str(item, &key) {
            attachment_urls.push(unescape_url(&url));
        }
    }

    let reply_no = get_str(item, "rplyNo");
    let kind = if reply_no.is_empty() || reply_no == "0" {
        "opinion"
    } else {
        "reply"
    }
    .to_string();

    PrespecOpinion {
        bf_spec_rgst_no: get_str(item, "bfSpecRgstNo"),
        ref_no: get_opt_str(item, "refNo"),
        opinion_no: get_str(item, "opninNo"),
        reply_no,
        kind,
        title: get_str(item, "opninTitl"),
        writer_org: get_opt_str(item, "mkngCorpNm"),
        writer_name: get_opt_str(item, "mkrNm"),
        input_dt: get_opt_str(item, "inptDt"),
        content: get_str(item, "opninCntnts"),
        attachment_urls,
    }
}

async fn call_prespec_json(
    client: &Client,
    endpoint: &str,
    params: &[(&str, String)],
) -> Result<Option<Value>> {
    let resp = client
        .get(endpoint)
        .query(params)
        .send()
        .await
        .with_context(|| format!("사전규격 API 요청 실패: {endpoint}"))?;

    let status = resp.status();
    let text = resp.text().await.context("사전규격 응답 본문 수신 실패")?;

    if !status.is_success() {
        bail!("HTTP {status} 오류: {}", truncate(&text, 300));
    }

    if text.trim_start().starts_with('<') {
        bail!("공공데이터포털 XML 오류 응답: {}", truncate(&text, 400));
    }

    let json: Value = serde_json::from_str(&text)
        .with_context(|| format!("사전규격 JSON 파싱 실패: {}", truncate(&text, 300)))?;

    let header = &json["response"]["header"];
    let result_code = get_str(header, "resultCode");
    match result_code.as_str() {
        "" | "00" => Ok(Some(json["response"]["body"].clone())),
        "03" => Ok(None), // 03: No Data
        _ => {
            let msg = get_str(header, "resultMsg");
            bail!("나라장터 사전규격 API 오류 (code={result_code}): {msg}");
        }
    }
}

fn extract_items(body: &Value) -> Vec<Value> {
    let items = &body["items"];
    if let Some(arr) = items.as_array() {
        return arr.clone();
    }
    if let Some(obj) = items.as_object() {
        if let Some(item) = obj.get("item") {
            if let Some(arr) = item.as_array() {
                return arr.clone();
            }
            if item.is_object() {
                return vec![item.clone()];
            }
        }
    }
    Vec::new()
}

fn unescape_url(s: &str) -> String {
    s.replace("&amp;", "&")
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

fn val_to_usize(v: &Value) -> Option<usize> {
    v.as_u64()
        .map(|n| n as usize)
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}