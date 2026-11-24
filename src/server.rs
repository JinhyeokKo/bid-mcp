use crate::clients::{
    bizinfo::{self, BizinfoSearchQuery},
    download,
    g2b::{self, BidCategory, BidSearchQuery},
    g2b_prespec::{self, PrespecCategory, PrespecSearchQuery},
};
use reqwest::Client;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchPrespecParams {
    /// 업무구분: "servc"(용역, 기본값), "thng"(물품), "cnstwk"(공사), "frgcpt"(외자)
    pub category: Option<PrespecCategory>,
    /// 사전규격등록번호 단건 조회 (예: "R26BD00189302", 지정 시 최우선)
    pub bf_spec_rgst_no: Option<String>,
    /// 참조번호 조회
    pub ref_no: Option<String>,
    /// 품명/사업명 검색 키워드 (예: "AI", "스마트공장", "유지보수", "플랫폼")
    pub keyword: Option<String>,
    /// 공고(발주)기관명 일부
    pub ntce_instt_nm: Option<String>,
    /// 수요기관명 일부
    pub dminstt_nm: Option<String>,
    /// SW사업 대상(swBizObjYn=Y)만 필터링 여부 (기본값: false, 공사 제외)
    pub sw_biz_only: Option<bool>,
    /// 세부품명번호 10자리 (용역/물품 전용)
    pub dtil_prdct_no: Option<String>,
    /// 최소 배정예산액 (원화, 예: 100000000 = 1억원 이상만 필터)
    pub min_budget: Option<u64>,
    /// 의견등록마감일시가 남아있는 건만 조회 (기본값: false)
    pub only_open_opinion: Option<bool>,
    /// 아직 본 입찰공고로 넘어가지 않은 건만 조회 (기본값: false)
    pub only_before_bid: Option<bool>,
    /// 최근 며칠 이내 접수된 사전규격 조회 (기본값: 7, 최대 30)
    pub days_back: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetPrespecOpinionsParams {
    /// 사전규격등록번호 (예: "R26BD00189302")
    pub bf_spec_rgst_no: String,
    /// 업무구분: "servc"(용역, 기본값), "thng"(물품), "cnstwk"(공사), "frgcpt"(외자)
    pub category: Option<PrespecCategory>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchG2bParams {
    /// 공고 분류: "servc"(용역, 기본값), "thng"(물품/장비/패키지SW), "etc"(대학·협동조합 등 기타공고)
    pub category: Option<BidCategory>,
    /// 공고명 검색 키워드 (일부 일치 지원, 예: "스마트공장", "MES", "AI", "플랫폼")
    pub keyword: Option<String>,
    /// 수요기관명 일부 (예: "한국생산기술연구원", "부산테크노파크")
    pub dminstt_nm: Option<String>,
    /// 업종명 일부 (용역/물품 전용, 예: "소프트웨어사업자")
    pub indstryty_nm: Option<String>,
    /// 업종코드 4자리 (용역/물품 전용)
    pub indstryty_cd: Option<String>,
    /// 참가제한지역코드 (00:전국, 11:서울, 26:부산, 27:대구, 28:인천, 41:경기, 12:전남광주통합 등)
    pub region_cd: Option<String>,
    /// 최소 추정가격 (원화, 부가세 제외 — 용역/물품 전용)
    pub min_presmpt_prce: Option<u64>,
    /// 최대 추정가격 (원화, 부가세 제외 — 용역/물품 전용)
    pub max_presmpt_prce: Option<u64>,
    /// 세부품명번호 10자리 (물품/용역 전용)
    pub dtil_prdct_no: Option<String>,
    /// 마감된 공고 제외 여부 (기본값: true)
    pub exclude_closed: Option<bool>,
    /// 최근 며칠 이내 게시 공고를 조회할지 (기본값: 7, 최대: 30 — 7일 초과 시 6일 단위 자동 분할 조회)
    pub days_back: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetG2bDetailParams {
    /// 차세대 나라장터 입찰공고번호 (예: "R26BK00933736")
    pub bid_no: String,
    /// 공고 분류: "servc"(기본값), "thng", "etc"
    pub category: Option<BidCategory>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchBizinfoParams {
    /// 공고명·사업개요·해시태그·기관명 필터 키워드 (예: "스마트공장", "바우처", "AI", "SW")
    pub keyword: Option<String>,
    /// 분야코드 (01:금융, 02:기술, 03:인력, 04:수출, 05:내수, 06:창업, 07:경영, 09:기타)
    pub field_cd: Option<String>,
    /// 해시태그 쉼표 다중입력 (분야: 금융/기술/인력/수출/내수/창업/경영/기타, 지역: 서울/부산/경기 등)
    pub hashtags: Option<String>,
    /// 마감된 지원사업 제외 여부 (기본값: true)
    pub active_only: Option<bool>,
    /// 스캔할 최신 페이지 수 (페이지당 100건, 기본값: 2, 최대: 5)
    pub max_pages: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DownloadAttachmentParams {
    /// 나라장터(www.g2b.go.kr) 또는 기업마당(www.bizinfo.go.kr) 첨부파일 다운로드 URL
    pub file_url: String,
    /// 저장할 파일명 (예: "제안요청서.hwpx", "공고문.pdf")
    pub file_name: Option<String>,
    /// 저장할 로컬 디렉토리 경로 (미지정 시 BID_DOWNLOAD_DIR 환경변수 또는 ./downloads 사용)
    pub dest_dir: Option<String>,
}

#[derive(Clone)]
pub struct BidMcpServer {
    http: Client,
    g2b_key: String,
    bizinfo_key: String,
    default_download_dir: PathBuf,
}

#[tool_router]
impl BidMcpServer {
    pub fn new(
        http: Client,
        g2b_key: String,
        bizinfo_key: String,
        default_download_dir: PathBuf,
    ) -> Self {
        Self {
            http,
            g2b_key,
            bizinfo_key,
            default_download_dir,
        }
    }

    #[tool(description = "나라장터 사전규격(본 공고 전 공개되는 RFP 초안)을 용역/물품/공사/외자별로 검색합니다. SW사업 필터, 최소 배정예산, 본공고 미전환 필터를 지원하며 규격서(RFP 초안) 다운로드 URL을 반환합니다.")]
    async fn search_g2b_prespecs(
        &self,
        Parameters(p): Parameters<SearchPrespecParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = PrespecSearchQuery {
            category: p.category.unwrap_or(PrespecCategory::Servc),
            bf_spec_rgst_no: p.bf_spec_rgst_no,
            ref_no: p.ref_no,
            keyword: p.keyword,
            ntce_instt_nm: p.ntce_instt_nm,
            dminstt_nm: p.dminstt_nm,
            sw_biz_only: p.sw_biz_only.unwrap_or(false),
            dtil_prdct_no: p.dtil_prdct_no,
            min_budget: p.min_budget,
            only_open_opinion: p.only_open_opinion.unwrap_or(false),
            only_before_bid: p.only_before_bid.unwrap_or(false),
            days_back: p.days_back.unwrap_or(7),
        };

        match g2b_prespec::search_prespecs(&self.http, &self.g2b_key, &query).await {
            Ok(list) => {
                let payload = serde_json::json!({
                    "count": list.len(),
                    "items": list,
                });
                let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "나라장터 사전규격 검색 실패: {e:#}"
            ))])),
        }
    }

    #[tool(description = "특정 나라장터 사전규격(bf_spec_rgst_no)에 등록된 타 업체의 규격서 의견·이의제기와 발주처 답변, 첨부된 수정 RFP 파일 URL을 조회합니다.")]
    async fn get_g2b_prespec_opinions(
        &self,
        Parameters(p): Parameters<GetPrespecOpinionsParams>,
    ) -> Result<CallToolResult, McpError> {
        let category = p.category.unwrap_or(PrespecCategory::Servc);
        match g2b_prespec::get_prespec_opinions(&self.http, &self.g2b_key, category, &p.bf_spec_rgst_no).await {
            Ok(list) => {
                let payload = serde_json::json!({
                    "count": list.len(),
                    "items": list,
                });
                let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "사전규격 의견 조회 실패: {e:#}"
            ))])),
        }
    }

    #[tool(description = "나라장터(G2B) 용역/물품/기타 입찰공고를 키워드, 수요기관, 업종, 지역, 추정가격 조건으로 검색합니다.")]
    async fn search_g2b_bids(
        &self,
        Parameters(p): Parameters<SearchG2bParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = BidSearchQuery {
            category: p.category.unwrap_or(BidCategory::Servc),
            keyword: p.keyword,
            dminstt_nm: p.dminstt_nm,
            indstryty_nm: p.indstryty_nm,
            indstryty_cd: p.indstryty_cd,
            region_cd: p.region_cd,
            min_presmpt_prce: p.min_presmpt_prce,
            max_presmpt_prce: p.max_presmpt_prce,
            dtil_prdct_no: p.dtil_prdct_no,
            exclude_closed: p.exclude_closed.unwrap_or(true),
            days_back: p.days_back.unwrap_or(7),
        };

        match g2b::search_bids(&self.http, &self.g2b_key, &query).await {
            Ok(list) => {
                let payload = serde_json::json!({
                    "count": list.len(),
                    "items": list,
                });
                let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "나라장터 공고 검색 실패: {e:#}"
            ))])),
        }
    }

    #[tool(description = "입찰공고번호로 나라장터 공고의 상세정보, 기초금액, 면허제한, 참가가능지역, 일반/e발주/혁신장터 첨부파일(RFP), 변경이력을 한 번에 조회합니다.")]
    async fn get_g2b_bid_detail(
        &self,
        Parameters(p): Parameters<GetG2bDetailParams>,
    ) -> Result<CallToolResult, McpError> {
        let category = p.category.unwrap_or(BidCategory::Servc);
        match g2b::get_bid_detail(&self.http, &self.g2b_key, category, &p.bid_no).await {
            Ok(detail) => {
                let text = serde_json::to_string_pretty(&detail).unwrap_or_default();
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "나라장터 공고 상세 조회 실패: {e:#}"
            ))])),
        }
    }

    #[tool(description = "기업마당(Bizinfo)에서 스마트팩토리·AI·SW·바우처 등 정부 및 지자체·테크노파크 지원사업 공고를 검색합니다.")]
    async fn search_bizinfo_programs(
        &self,
        Parameters(p): Parameters<SearchBizinfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = BizinfoSearchQuery {
            keyword: p.keyword,
            field_cd: p.field_cd,
            hashtags: p.hashtags,
            active_only: p.active_only.unwrap_or(true),
            page_unit: 100,
            max_pages: p.max_pages.unwrap_or(2),
        };

        match bizinfo::search_programs(&self.http, &self.bizinfo_key, &query).await {
            Ok(list) => {
                let payload = serde_json::json!({
                    "count": list.len(),
                    "items": list,
                });
                let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "기업마당 지원사업 검색 실패: {e:#}"
            ))])),
        }
    }

    #[tool(description = "나라장터 및 기업마당 공고의 첨부파일(공고문, 제안요청서, 과업지시서, PDF 변환본 등)을 로컬 폴더에 다운로드합니다.")]
    async fn download_attachment(
        &self,
        Parameters(p): Parameters<DownloadAttachmentParams>,
    ) -> Result<CallToolResult, McpError> {
        let dir = p
            .dest_dir
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.default_download_dir.clone());

        match download::download_file(&self.http, &p.file_url, p.file_name.as_deref(), &dir).await {
            Ok(info) => {
                let text = serde_json::to_string_pretty(&info).unwrap_or_default();
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "첨부파일 다운로드 실패: {e:#}"
            ))])),
        }
    }
}

#[tool_handler]
impl ServerHandler for BidMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "한국 공공 SW/SI 입찰(나라장터 G2B)과 스마트팩토리·바우처 정부지원사업(기업마당 Bizinfo) 공고를 통합 검색·조회하는 MCP 서버입니다. \
                공공 입찰은 search_g2b_bids -> get_g2b_bid_detail 순으로 조회하고, 정부지원사업은 search_bizinfo_programs를 사용하세요. \
                첨부파일을 다운로드할 때는 PDF(또는 공고서 변환본)가 있으면 우선 다운로드해 읽고, .hwp/.hwpx 파일은 로컬 변환 도구로 텍스트를 추출해 분석하세요.",
            )
    }
}