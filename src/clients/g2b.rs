use anyhow::{Context, Result, bail};
use chrono::{Duration, Local};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// 조달청 나라장터 입찰공고정보서비스 v1.3 (2026.11.10 개정) Base URL
const BASE_URL: &str = "https://apis.data.go.kr/1230000/ad/BidPublicInfoService02";

/// 2026.11.10 개정: 전체 오퍼레이션 최대 조회기간 7일 제한 -> 경계값 오류 방지를 위해 6일 단위 분할
const CHUNK_DAYS: i64 = 6;
const MAX_ROWS_PER_PAGE: usize = 100;
const MAX_PAGES_PER_CHUNK: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BidCategory {
    /// 용역 (SW 개발, SI, SM, 연구용역, 컨설팅 등)
    Servc,
    /// 물품 (장비, 센서, PLC, 패키지 SW 구매 등)
    Thng,
    /// 기타공고 (대학, 협동조합 등 나라장터 기타 등록 공고)
    Etc,
}

impl BidCategory {
    /// 29번(AtchFileInfo02) 업무구분코드: 1=물품, 2=외자, 3=공사, 5=용역
    fn bsns_div_cd(self) -> Option<&'static str> {
        match self {
            Self::Thng => Some("1"),
            Self::Servc => Some("5"),
            Self::Etc => None,
        }
    }

    fn pps_search_op(self) -> &'static str {
        match self {
            Self::Servc => "getBidPblancListInfoServcPPSSrch02",
            Self::Thng => "getBidPblancListInfoThngPPSSrch02",
            Self::Etc => "getBidPblancListInfoEtcPPSSrch02",
        }
    }

    fn detail_op(self) -> &'static str {
        match self {
            Self::Servc => "getBidPblancListInfoServc02",
            Self::Thng => "getBidPblancListInfoThng02",
            Self::Etc => "getBidPblancListInfoEtc02",
        }
    }

    fn basis_amount_op(self) -> Option<&'static str> {
        match self {
            Self::Servc => Some("getBidPblancListInfoServcBsisAmount02"),
            Self::Thng => Some("getBidPblancListInfoThngBsisAmount02"),
            Self::Etc => None,
        }
    }

    fn change_history_op(self) -> Option<&'static str> {
        match self {
            Self::Servc => Some("getBidPblancListInfoChgHstryServc02"),
            Self::Thng => Some("getBidPblancListInfoChgHstryThng02"),
            Self::Etc => None,
        }
    }
}

#[derive(Debug)]
pub struct BidSearchQuery {
    pub category: BidCategory,
    pub keyword: Option<String>,        // bidNtceNm (일부 일치)
    pub dminstt_nm: Option<String>,     // dminsttNm (수요기관명 일부)
    pub indstryty_nm: Option<String>,   // indstrytyNm (용역/물품)
    pub indstryty_cd: Option<String>,   // indstrytyCd (용역/물품, 4자리)
    pub region_cd: Option<String>,      // prtcptLmtRgnCd (용역/물품)
    pub min_presmpt_prce: Option<u64>,  // presmptPrceBgn (용역/물품)
    pub max_presmpt_prce: Option<u64>,  // presmptPrceEnd (용역/물품)
    pub dtil_prdct_no: Option<String>,  // dtilPrdctClsfcNo (물품/용역 10자리)
    pub exclude_closed: bool,           // bidClseExcpYn (Y/N)
    pub days_back: i64,                 // 조회 일수 (6일 단위 자동 분할)
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentInfo {
    /// 출처: "atch_file"(29번) | "eorder_rfp"(20번) | "ppi_rfp"(23번) | "inline"(공고 본문)
    pub source: String,
    pub seq: String,
    pub doc_div: Option<String>,
    pub file_name: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct BidSummary {
    pub category: BidCategory,
    pub bid_no: String,
    pub bid_ord: String,
    pub kind: String,
    pub title: String,
    pub notice_agency: String,
    pub demand_agency: String,
    pub bid_method: String,
    pub contract_method: String,
    pub award_method: String,
    pub award_standard: Option<String>,
    pub tech_eval_rate: Option<String>,
    pub price_eval_rate: Option<String>,
    pub budget_amt: Option<String>,
    pub estimated_price: Option<String>,
    pub vat: Option<String>,
    pub notice_dt: String,
    pub bid_begin_dt: Option<String>,
    pub bid_close_dt: Option<String>,
    pub open_dt: Option<String>,
    pub service_div: Option<String>,
    pub info_biz_yn: Option<String>,
    pub industry_limit_yn: Option<String>,
    pub joint_contract: Option<String>,
    pub procurement_category: Option<String>,
    pub product_name: Option<String>,
    pub prespec_no: Option<String>,
    pub prev_bid_no: Option<String>,
    pub detail_url: String,
    pub std_doc_url: Option<String>,
    pub inline_attachments: Vec<AttachmentInfo>,
}

#[derive(Debug, Serialize)]
pub struct BasisAmountInfo {
    pub basis_amt: Option<String>,
    pub open_dt: Option<String>,
    pub range_begin_rate: Option<String>,
    pub range_end_rate: Option<String>,
    pub remark1: Option<String>,
    pub remark2: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LicenseLimitInfo {
    pub group_no: Option<String>,
    pub seq: Option<String>,
    pub license_name: String,
    pub allowed_industries: Option<String>,
    pub main_fields: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChangeHistoryItem {
    pub changed_at: String,
    pub data_div: Option<String>,
    pub item_name: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BidDetail {
    pub bid: BidSummary,
    pub officer_name: Option<String>,
    pub officer_tel: Option<String>,
    pub qual_reg_close_dt: Option<String>,
    pub qual_contents: Option<String>, // 기타공고 bidQlfctRgstCntnts
    pub remarks: Option<String>,       // 기타공고 rmrkCntnts
    pub order_plan_no: Option<String>,
    pub basis_amount: Option<BasisAmountInfo>,
    pub license_limits: Vec<LicenseLimitInfo>,
    pub allowed_regions: Vec<String>,
    pub attachments: Vec<AttachmentInfo>,
    pub change_history: Vec<ChangeHistoryItem>,
}

/// 12번(용역) / 14번(물품) / 22번(기타) 나라장터 검색조건 조회
pub async fn search_bids(
    client: &Client,
    service_key: &str,
    q: &BidSearchQuery,
) -> Result<Vec<BidSummary>> {
    if service_key.trim().is_empty() {
        bail!("G2B_API_KEY 환경변수가 비어 있습니다.");
    }

    let endpoint = format!("{BASE_URL}/{}", q.category.pps_search_op());
    let total_days = q.days_back.clamp(1, 30);
    let now = Local::now();

    let mut results = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    let mut cursor_end = now;
    let earliest = now - Duration::days(total_days);

    while cursor_end > earliest {
        let cursor_start = (cursor_end - Duration::days(CHUNK_DAYS)).max(earliest);
        let bgn_str = cursor_start.format("%Y%m%d%H%M").to_string();
        let end_str = cursor_end.format("%Y%m%d%H%M").to_string();

        for page in 1..=MAX_PAGES_PER_CHUNK {
            let mut params: Vec<(&str, String)> = vec![
                ("serviceKey", service_key.to_string()),
                ("numOfRows", MAX_ROWS_PER_PAGE.to_string()),
                ("pageNo", page.to_string()),
                ("type", "json".to_string()),
                ("inqryDiv", "1".to_string()), // 1: 공고게시일시
                ("inqryBgnDt", bgn_str.clone()),
                ("inqryEndDt", end_str.clone()),
                (
                    "bidClseExcpYn",
                    if q.exclude_closed { "Y" } else { "N" }.to_string(),
                ),
            ];

            if let Some(v) = non_empty(&q.keyword) {
                params.push(("bidNtceNm", v));
            }
            if let Some(v) = non_empty(&q.dminstt_nm) {
                params.push(("dminsttNm", v));
            }

            // 기타공고(22번)는 업종/지역/추정가격/세부품명 파라미터 미지원
            if q.category != BidCategory::Etc {
                if let Some(v) = non_empty(&q.indstryty_nm) {
                    params.push(("indstrytyNm", v));
                }
                if let Some(v) = non_empty(&q.indstryty_cd) {
                    params.push(("indstrytyCd", v));
                }
                if let Some(v) = non_empty(&q.region_cd) {
                    params.push(("prtcptLmtRgnCd", v));
                }
                if let Some(v) = q.min_presmpt_prce {
                    params.push(("presmptPrceBgn", v.to_string()));
                }
                if let Some(v) = q.max_presmpt_prce {
                    params.push(("presmptPrceEnd", v.to_string()));
                }
                if let Some(v) = non_empty(&q.dtil_prdct_no) {
                    params.push(("dtilPrdctClsfcNo", v));
                }
            }

            let Some(body) = call_g2b_json(client, &endpoint, &params).await? else {
                break; // resultCode=03 (No Data)
            };

            let total_count = val_to_usize(&body["totalCount"]).unwrap_or(0);
            let items = extract_items(&body);
            if items.is_empty() {
                break;
            }

            for item in items {
                let summary = parse_bid_summary(q.category, &item);
                let key = (summary.bid_no.clone(), summary.bid_ord.clone());
                if !key.0.is_empty() && seen.insert(key) {
                    results.push(summary);
                }
            }

            if page * MAX_ROWS_PER_PAGE >= total_count {
                break;
            }
        }

        cursor_end = cursor_start - Duration::minutes(1);
    }

    results.sort_by(|a, b| b.notice_dt.cmp(&a.notice_dt));
    Ok(results)
}

/// 공고 상세 + 기초금액 + 면허제한(15) + 참가가능지역(16) + 첨부파일(29/20/23) + 변경이력(10/8) 통합 조회
pub async fn get_bid_detail(
    client: &Client,
    service_key: &str,
    category: BidCategory,
    bid_no: &str,
) -> Result<BidDetail> {
    if service_key.trim().is_empty() {
        bail!("G2B_API_KEY 환경변수가 비어 있습니다.");
    }

    let bid_no = bid_no.trim();
    let detail_endpoint = format!("{BASE_URL}/{}", category.detail_op());
    let base_params = vec![
        ("serviceKey", service_key.to_string()),
        ("numOfRows", "20".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "2".to_string()), // 2: 입찰공고번호
        ("bidNtceNo", bid_no.to_string()),
    ];

    let Some(detail_body) = call_g2b_json(client, &detail_endpoint, &base_params).await? else {
        bail!("입찰공고번호 '{bid_no}'({category:?})에 해당하는 공고를 찾지 못했습니다.");
    };

    let mut items = extract_items(&detail_body);
    if items.is_empty() {
        bail!("입찰공고번호 '{bid_no}'({category:?})에 해당하는 공고를 찾지 못했습니다.");
    }

    // 재공고 차수가 여러 건일 경우 가장 최신 차수(bidNtceOrd 최대값) 선택
    items.sort_by(|a, b| get_str(b, "bidNtceOrd").cmp(&get_str(a, "bidNtceOrd")));
    let latest = &items[0];
    let bid_ord = get_str(latest, "bidNtceOrd");
    let ord_for_query = if bid_ord.is_empty() {
        "000".to_string()
    } else {
        bid_ord.clone()
    };

    let summary = parse_bid_summary(category, latest);

    let (basis_res, license_res, region_res, atch_res, eorder_res, ppi_res, chg_res) = tokio::join!(
        fetch_basis_amount(client, service_key, category, bid_no),
        fetch_license_limits(client, service_key, bid_no, &ord_for_query),
        fetch_allowed_regions(client, service_key, bid_no, &ord_for_query),
        fetch_atch_files_29(client, service_key, category, bid_no, &ord_for_query),
        fetch_eorder_files_20(client, service_key, bid_no, &ord_for_query),
        fetch_ppi_rfp_files_23(client, service_key, bid_no),
        fetch_change_history(client, service_key, category, bid_no),
    );

    // 첨부파일 병합: 29번(일반/변환본) -> 20번(e발주 RFP) -> 23번(혁신장터 RFP) -> 본문 inline fallback
    let mut attachments = Vec::new();
    let mut seen_urls = HashSet::new();
    for group in [
        atch_res.unwrap_or_default(),
        eorder_res.unwrap_or_default(),
        ppi_res.unwrap_or_default(),
        summary.inline_attachments.clone(),
    ] {
        for file in group {
            if seen_urls.insert(file.url.clone()) {
                attachments.push(file);
            }
        }
    }

    Ok(BidDetail {
        officer_name: get_opt_str(latest, "ntceInsttOfclNm"),
        officer_tel: get_opt_str(latest, "ntceInsttOfclTelNo"),
        qual_reg_close_dt: get_opt_str(latest, "bidQlfctRgstDt"),
        qual_contents: get_opt_str(latest, "bidQlfctRgstCntnts"),
        remarks: get_opt_str(latest, "rmrkCntnts"),
        order_plan_no: get_opt_str(latest, "orderPlanUntyNo"),
        bid: summary,
        basis_amount: basis_res.ok().flatten(),
        license_limits: license_res.unwrap_or_default(),
        allowed_regions: region_res.unwrap_or_default(),
        attachments,
        change_history: chg_res.unwrap_or_default(),
    })
}

/// 5번(물품) / 7번(용역) 기초금액조회
async fn fetch_basis_amount(
    client: &Client,
    service_key: &str,
    category: BidCategory,
    bid_no: &str,
) -> Result<Option<BasisAmountInfo>> {
    let Some(op) = category.basis_amount_op() else {
        return Ok(None);
    };
    let endpoint = format!("{BASE_URL}/{op}");
    let params = vec![
        ("serviceKey", service_key.to_string()),
        ("numOfRows", "10".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "2".to_string()),
        ("bidNtceNo", bid_no.to_string()),
    ];

    let Some(body) = call_g2b_json(client, &endpoint, &params).await? else {
        return Ok(None);
    };
    let Some(item) = extract_items(&body).into_iter().next() else {
        return Ok(None);
    };

    Ok(Some(BasisAmountInfo {
        basis_amt: get_opt_str(&item, "bssamt"),
        open_dt: get_opt_str(&item, "bssamtOpenDt"),
        range_begin_rate: get_opt_str(&item, "rsrvtnPrceRngBgnRate"),
        range_end_rate: get_opt_str(&item, "rsrvtnPrceRngEndRate"),
        remark1: get_opt_str(&item, "rmrk1"),
        remark2: get_opt_str(&item, "rmrk2"),
    }))
}

/// 15번: 면허제한정보조회 (inqryDiv=2 시 bidNtceNo + bidNtceOrd 필수)
async fn fetch_license_limits(
    client: &Client,
    service_key: &str,
    bid_no: &str,
    bid_ord: &str,
) -> Result<Vec<LicenseLimitInfo>> {
    let endpoint = format!("{BASE_URL}/getBidPblancListInfoLicenseLimit02");
    let params = vec![
        ("serviceKey", service_key.to_string()),
        ("numOfRows", "100".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "2".to_string()),
        ("bidNtceNo", bid_no.to_string()),
        ("bidNtceOrd", bid_ord.to_string()),
    ];

    let Some(body) = call_g2b_json(client, &endpoint, &params).await? else {
        return Ok(Vec::new());
    };

    Ok(extract_items(&body)
        .into_iter()
        .filter_map(|item| {
            let name = get_opt_str(&item, "lcnsLmtNm")?;
            Some(LicenseLimitInfo {
                group_no: get_opt_str(&item, "lmtGrpNo"),
                seq: get_opt_str(&item, "lmtSno"),
                license_name: name,
                allowed_industries: get_opt_str(&item, "permsnIndstrytyList"),
                main_fields: get_opt_str(&item, "indstrytyMfrcFldList"),
            })
        })
        .collect())
}

/// 16번: 참가가능지역정보조회 (inqryDiv=2 시 bidNtceNo + bidNtceOrd 필수)
async fn fetch_allowed_regions(
    client: &Client,
    service_key: &str,
    bid_no: &str,
    bid_ord: &str,
) -> Result<Vec<String>> {
    let endpoint = format!("{BASE_URL}/getBidPblancListInfoPrtcptPsblRgn02");
    let params = vec![
        ("serviceKey", service_key.to_string()),
        ("numOfRows", "100".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "2".to_string()),
        ("bidNtceNo", bid_no.to_string()),
        ("bidNtceOrd", bid_ord.to_string()),
    ];

    let Some(body) = call_g2b_json(client, &endpoint, &params).await? else {
        return Ok(Vec::new());
    };

    Ok(extract_items(&body)
        .into_iter()
        .filter_map(|item| get_opt_str(&item, "prtcptPsblRgnNm"))
        .collect())
}

/// 29번: 분리된 일반 첨부파일정보조회 (공고서, 규격서, 현장설명서, PDF 변환본 포함)
async fn fetch_atch_files_29(
    client: &Client,
    service_key: &str,
    category: BidCategory,
    bid_no: &str,
    bid_ord: &str,
) -> Result<Vec<AttachmentInfo>> {
    let Some(bsns_cd) = category.bsns_div_cd() else {
        return Ok(Vec::new());
    };
    let endpoint = format!("{BASE_URL}/getBidPblancListInfoAtchFileInfo02");
    let params = vec![
        ("serviceKey", service_key.to_string()),
        ("numOfRows", "100".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "2".to_string()),
        ("bidNtceNo", bid_no.to_string()),
        ("bidNtceOrd", bid_ord.to_string()),
        ("bsnsDivCd", bsns_cd.to_string()), // 1:물품, 5:용역
    ];

    let Some(body) = call_g2b_json(client, &endpoint, &params).await? else {
        return Ok(Vec::new());
    };

    Ok(extract_items(&body)
        .into_iter()
        .filter_map(|item| {
            let url = get_opt_str(&item, "atchFileUrl")?;
            let seq = get_str(&item, "atchSno");
            let file_name =
                get_opt_str(&item, "atchFileNm").unwrap_or_else(|| format!("atch_{seq}.bin"));
            // 표 명세(docDivNm)와 XML 예제(atchDocDivNm) 불일치 모두 대응
            let doc_div = get_opt_str_any(&item, &["atchDocDivNm", "docDivNm"]);
            Some(AttachmentInfo {
                source: "atch_file".to_string(),
                seq,
                doc_div,
                file_name,
                url,
            })
        })
        .collect())
}

/// 20번: e발주 첨부파일정보조회 (SW·SI 제안요청서 RFP 핵심)
async fn fetch_eorder_files_20(
    client: &Client,
    service_key: &str,
    bid_no: &str,
    bid_ord: &str,
) -> Result<Vec<AttachmentInfo>> {
    let endpoint = format!("{BASE_URL}/getBidPblancListInfoEorderAtchFileInfo02");
    let params = vec![
        ("serviceKey", service_key.to_string()),
        ("numOfRows", "100".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "2".to_string()),
        ("bidNtceNo", bid_no.to_string()),
        ("bidNtceOrd", bid_ord.to_string()),
    ];

    let Some(body) = call_g2b_json(client, &endpoint, &params).await? else {
        return Ok(Vec::new());
    };

    Ok(extract_items(&body)
        .into_iter()
        .filter_map(|item| {
            let url = get_opt_str(&item, "eorderAtchFileUrl")?;
            let seq = get_str(&item, "atchSno");
            let file_name = get_opt_str(&item, "eorderAtchFileNm")
                .unwrap_or_else(|| format!("eorder_rfp_{seq}.bin"));
            Some(AttachmentInfo {
                source: "eorder_rfp".to_string(),
                seq,
                doc_div: get_opt_str(&item, "eorderDocDivNm"),
                file_name,
                url,
            })
        })
        .collect())
}

/// 23번: 혁신장터 최종제안요청서 교부 첨부파일정보조회 (주의: 입찰공고번호는 inqryDiv=3)
async fn fetch_ppi_rfp_files_23(
    client: &Client,
    service_key: &str,
    bid_no: &str,
) -> Result<Vec<AttachmentInfo>> {
    let endpoint = format!("{BASE_URL}/getBidPblancListPPIFnlRfpIssAtchFileInfo02");
    let params = vec![
        ("serviceKey", service_key.to_string()),
        ("numOfRows", "100".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "3".to_string()), // 3: 입찰공고번호
        ("bidNtceNo", bid_no.to_string()),
    ];

    let Some(body) = call_g2b_json(client, &endpoint, &params).await? else {
        return Ok(Vec::new());
    };

    Ok(extract_items(&body)
        .into_iter()
        .filter_map(|item| {
            let url = get_opt_str(&item, "atchFileUrl")?;
            let seq = get_str(&item, "atchSno");
            let file_name = get_opt_str(&item, "atchFileNm")
                .unwrap_or_else(|| format!("ppi_rfp_{seq}.bin"));
            Some(AttachmentInfo {
                source: "ppi_rfp".to_string(),
                seq,
                doc_div: get_opt_str(&item, "atchDocDivNm"),
                file_name,
                url,
            })
        })
        .collect())
}

/// 8번(물품) / 10번(용역) 변경이력조회
async fn fetch_change_history(
    client: &Client,
    service_key: &str,
    category: BidCategory,
    bid_no: &str,
) -> Result<Vec<ChangeHistoryItem>> {
    let Some(op) = category.change_history_op() else {
        return Ok(Vec::new());
    };
    let endpoint = format!("{BASE_URL}/{op}");
    let params = vec![
        ("serviceKey", service_key.to_string()),
        ("numOfRows", "50".to_string()),
        ("pageNo", "1".to_string()),
        ("type", "json".to_string()),
        ("inqryDiv", "2".to_string()),
        ("bidNtceNo", bid_no.to_string()),
    ];

    let Some(body) = call_g2b_json(client, &endpoint, &params).await? else {
        return Ok(Vec::new());
    };

    Ok(extract_items(&body)
        .into_iter()
        .map(|item| ChangeHistoryItem {
            changed_at: get_str(&item, "chgDt"),
            data_div: get_opt_str(&item, "chgDataDivNm"),
            item_name: get_str(&item, "chgItemNm"),
            before: get_opt_str(&item, "bfchgVal"),
            after: get_opt_str(&item, "afchgVal"),
        })
        .collect())
}

/// 공통 JSON 호출: resultCode=03(No Data)은 Ok(None), XML 게이트웨이 오류 등은 상세 에러 반환
async fn call_g2b_json(
    client: &Client,
    endpoint: &str,
    params: &[(&str, String)],
) -> Result<Option<Value>> {
    let resp = client
        .get(endpoint)
        .query(params)
        .send()
        .await
        .with_context(|| format!("API 요청 실패: {endpoint}"))?;

    let status = resp.status();
    let text = resp.text().await.context("응답 본문 수신 실패")?;

    if !status.is_success() {
        bail!("HTTP {status} 오류: {}", truncate(&text, 300));
    }

    if text.trim_start().starts_with('<') {
        bail!("공공데이터포털 XML 오류 응답: {}", truncate(&text, 400));
    }

    let json: Value = serde_json::from_str(&text)
        .with_context(|| format!("JSON 파싱 실패: {}", truncate(&text, 300)))?;

    let header = &json["response"]["header"];
    let result_code = get_str(header, "resultCode");
    match result_code.as_str() {
        "" | "00" => Ok(Some(json["response"]["body"].clone())),
        "03" => Ok(None), // 03: No Data (정상 빈 결과)
        _ => {
            let msg = get_str(header, "resultMsg");
            bail!("나라장터 API 오류 (code={result_code}): {msg}");
        }
    }
}

/// 공공데이터포털 JSON의 items 변형(배열 / {item: 배열} / {item: 단일객체} / 빈 문자열) 모두 대응
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

fn parse_bid_summary(category: BidCategory, item: &Value) -> BidSummary {
    // 표 명세(pubPrcrmntLrgclsfcNm)와 XML 예제(pubPrcrmntLrgClsfcNm) 대소문자 차이 방어
    let lrg = get_opt_str_any(item, &["pubPrcrmntLrgClsfcNm", "pubPrcrmntLrgclsfcNm"]);
    let mid = get_opt_str_any(item, &["pubPrcrmntMidClsfcNm", "pubPrcrmntMidclsfcNm"]);
    let cls = get_opt_str(item, "pubPrcrmntClsfcNm");
    let cat_parts: Vec<String> = [lrg, mid, cls].into_iter().flatten().collect();
    let procurement_category = if cat_parts.is_empty() {
        None
    } else {
        Some(cat_parts.join(" > "))
    };

    let max_inline = if category == BidCategory::Etc { 5 } else { 10 };
    let mut inline_attachments = Vec::new();
    for seq in 1..=max_inline {
        let url_key = format!("ntceSpecDocUrl{seq}");
        let name_key = format!("ntceSpecFileNm{seq}");
        if let Some(url) = get_opt_str(item, &url_key) {
            let file_name =
                get_opt_str(item, &name_key).unwrap_or_else(|| format!("spec_doc_{seq}"));
            inline_attachments.push(AttachmentInfo {
                source: "inline".to_string(),
                seq: seq.to_string(),
                doc_div: Some("공고규격서".to_string()),
                file_name,
                url,
            });
        }
    }

    let joint_contract = get_opt_str(item, "cmmnSpldmdMethdNm")
        .or_else(|| get_opt_str(item, "cmmnSpldmdYn"));

    BidSummary {
        category,
        bid_no: get_str(item, "bidNtceNo"),
        bid_ord: get_str(item, "bidNtceOrd"),
        kind: get_str(item, "ntceKindNm"),
        title: get_str(item, "bidNtceNm"),
        notice_agency: get_str(item, "ntceInsttNm"),
        demand_agency: get_str(item, "dminsttNm"),
        bid_method: get_str(item, "bidMethdNm"),
        contract_method: get_str(item, "cntrctCnclsMthdNm"),
        award_method: get_str(item, "sucsfbidMthdNm"),
        award_standard: get_opt_str(item, "sucsfbidMthdAppStd"),
        tech_eval_rate: get_opt_str(item, "techAbltEvlRt"),
        price_eval_rate: get_opt_str(item, "bidPrceEvlRt"),
        budget_amt: get_opt_str(item, "asignBdgtAmt"),
        estimated_price: get_opt_str(item, "presmptPrce"),
        vat: get_opt_str(item, "VAT"),
        notice_dt: get_str(item, "bidNtceDt"),
        bid_begin_dt: get_opt_str(item, "bidBeginDt"),
        bid_close_dt: get_opt_str(item, "bidClseDt"),
        open_dt: get_opt_str(item, "opengDt"),
        service_div: get_opt_str(item, "srvceDivNm"),
        info_biz_yn: get_opt_str(item, "infoBizYn"),
        industry_limit_yn: get_opt_str(item, "indstrytyLmtYn"),
        joint_contract,
        procurement_category,
        product_name: get_opt_str(item, "dtilPrdctClsfcNoNm"),
        prespec_no: get_opt_str(item, "bfSpecRgstNo"),
        prev_bid_no: get_opt_str(item, "befBidBbancNo"), // v1.2 공식 필드명 오타 반영
        detail_url: get_str(item, "bidNtceDtlUrl"),
        std_doc_url: get_opt_str(item, "stdNtceDocUrl"),
        inline_attachments,
    }
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

fn get_opt_str_any(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| get_opt_str(v, k))
}

fn val_to_usize(v: &Value) -> Option<usize> {
    v.as_u64()
        .map(|n| n as usize)
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}