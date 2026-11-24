# bid-mcp

대한민국 공공 SW·SI 입찰(**조달청 나라장터 G2B**)과 스마트팩토리·AI·SW·바우처 정부지원사업(**기업마당 Bizinfo**) 공고를 AI 에이전트가 직접 검색·상세조회·첨부파일 다운로드할 수 있게 해주는 **Rust 기반 초경량 로컬 MCP(Model Context Protocol) 서버**입니다.

Google Antigravity(CLI · IDE · 2.0 App), Gemini CLI, Claude Code, Codex CLI 등 `stdio` 기반 MCP를 지원하는 프론티어 AI 에이전트에 단일 바이너리로 연결해 사용할 수 있습니다.

---

## 목차

1. [설계 철학 및 아키텍처](#1-설계-철학-및-아키텍처)
2. [프로젝트 구조](#2-프로젝트-구조)
3. [제공하는 MCP Tool (4개)](#3-제공하는-mcp-tool-4개)
4. [연동된 외부 API 상세 명세](#4-연동된-외부-api-상세-명세)
5. [사전 준비: API 키 발급](#5-사전-준비-api-키-발급)
6. [빌드 및 실행](#6-빌드-및-실행)
7. [AI 에이전트 연동 설정 (Antigravity / Gemini / Claude / Codex)](#7-ai-에이전트-연동-설정)
8. [HWP/HWPX 제안요청서 처리 및 에이전트 지침 설정](#8-hwphwpx-제안요청서-처리-및-에이전트-지침-설정)
9. [실전 사용 프롬프트 예시](#9-실전-사용-프롬프트-예시)
10. [트러블슈팅 및 운영 주의사항](#10-트러블슈팅-및-운영-주의사항)
11. [향후 확장 로드맵](#11-향후-확장-로드맵)

---

## 1. 설계 철학 및 아키텍처

### 왜 경량 MCP + 에이전트 CLI 조합인가?
* **단일 바이너리 (`stdio`):** Python 가상환경(`venv`)이나 의존성 충돌 없이 컴파일된 바이너리 파일 1개만 에이전트 설정에 지정하면 바로 동작합니다.
* **역할 분담 최적화:**
  * **`bid-mcp` (MCP 서버):** OpenAPI 호출, 1차 조건 필터링, 조회기간 분할, 중복 제거, 첨부파일 로컬 다운로드까지만 가볍게 담당합니다.
  * **프론티어 에이전트 CLI:** PDF 제안요청서(RFP)는 내장 문서 읽기 도구로 직접 열람하고, 한국 공공기관 특유의 `.hwp` / `.hwpx` 파일은 로컬 변환 도구(`hwp5txt` 등)를 터미널에서 실행해 핵심 과업·참가자격만 요약합니다.
* **나라장터 + 기업마당 통합 이유:**
  * 일반 공공 SW·SI 발주는 **나라장터**, 스마트팩토리(공급기업 매칭)·AI/클라우드/데이터 바우처·지역 테크노파크 사업은 **기업마당** 비중이 높습니다.
  * 두 포털을 합쳐도 MCP Tool이 **4개뿐**이라 에이전트 컨텍스트 부담이 적고, 첨부파일 다운로더와 HTTP 클라이언트를 공유할 수 있습니다.
  * 두 API 키 중 **하나만 설정되어 있어도** 나머지 포털의 Tool은 정상 동작합니다.

```text
[AI 에이전트: Antigravity / Gemini CLI / Claude Code / Codex CLI]
        │ (MCP over stdio / JSON-RPC)
        ▼
   [ bid-mcp ]  ──▶ 나라장터 OpenAPI (입찰공고정보서비스 v1.3, 13개 오퍼레이션)
        │       ──▶ 기업마당 OpenAPI (지원사업정보 API)
        │       ──▶ 첨부파일 다운로드 (www.g2b.go.kr / www.bizinfo.go.kr)
        ▼
  ./downloads/  ──▶ PDF: 에이전트 내장 리더로 직접 분석
                ──▶ HWP/HWPX: 로컬 변환 도구(hwp5txt 등)로 텍스트 추출 후 분석
```

---

## 2. 프로젝트 구조

```text
bid-mcp/
├── Cargo.toml                  # 패키지 정의 (name = "bid-mcp", edition = "2024")
├── README.md                   # 프로젝트 문서
└── src/
    ├── main.rs                 # stdio MCP 서버 진입점, 환경변수 로드, HTTP 클라이언트 초기화
    ├── server.rs               # rmcp 기반 MCP Tool 4개 정의 및 서버 지침(instructions)
    └── clients/
        ├── mod.rs              # 클라이언트 모듈 선언
        ├── g2b.rs              # 나라장터 입찰공고정보서비스 v1.3 (13개 오퍼레이션)
        ├── bizinfo.rs          # 기업마당 지원사업정보 API 클라이언트
        └── download.rs         # 나라장터·기업마당 공통 첨부파일 다운로더
```

---

## 3. 제공하는 MCP Tool (4개)

에이전트가 툴 선택에 혼란을 겪지 않도록 내부 14개 API 엔드포인트를 목적 기반의 **4개 Tool**로 추상화했습니다.

| Tool 이름 | 대상 포털 | 역할 |
| :--- | :---: | :--- |
| **`search_g2b_bids`** | 나라장터 | 용역(`servc`) · 물품(`thng`) · 기타(`etc`) 공고를 키워드·수요기관·업종·지역·추정가격으로 검색 |
| **`get_g2b_bid_detail`** | 나라장터 | 공고번호 1개로 최신 차수 상세 + 기초금액 + 면허제한 + 참가가능지역 + **일반/e발주/혁신장터 첨부파일** + 변경이력을 병렬 일괄 조회 |
| **`search_bizinfo_programs`** | 기업마당 | 스마트팩토리·바우처·R&D 등 정부/지자체/테크노파크 지원사업 검색 + 사업개요 + 첨부/본문 PDF URL 반환 |
| **`download_attachment`** | 공통 | 나라장터(`g2b.go.kr`) 및 기업마당(`bizinfo.go.kr`) 첨부파일을 로컬 폴더에 저장하고 확장자별 처리 힌트 반환 |

### 주요 파라미터 요약

#### 1) `search_g2b_bids`
* `category`: `"servc"`(용역, 기본값) | `"thng"`(물품/장비/패키지SW) | `"etc"`(대학·협동조합 등 기타공고)
* `keyword`: 공고명 일부 일치 검색 (`bidNtceNm`)
* `dminstt_nm`: 수요기관명 일부 일치 검색 (`dminsttNm`)
* `indstryty_nm` / `indstryty_cd`: 업종명 일부 / 4자리 업종코드 *(용역·물품 전용)*
* `region_cd`: 참가제한지역코드 *(용역·물품 전용, 아래 코드표 참고)*
* `min_presmpt_prce` / `max_presmpt_prce`: 추정가격 범위(원, 부가세 제외) *(용역·물품 전용)*
* `dtil_prdct_no`: 세부품명번호 10자리 *(물품·용역 전용)*
* `exclude_closed`: 마감 공고 제외 여부 (기본값 `true`)
* `days_back`: 최근 N일 조회 (기본값 `7`, 최대 `30` — **7일 초과 시 6일 단위 자동 분할 호출**)

#### 2) `get_g2b_bid_detail`
* `bid_no` *(필수)*: 차세대 나라장터 입찰공고번호 (예: `R26BK00933736`)
* `category`: `"servc"`(기본값) | `"thng"` | `"etc"`

#### 3) `search_bizinfo_programs`
* `keyword`: 공고명·사업개요·해시태그·지원대상·기관명 클라이언트 필터 키워드
* `field_cd`: 분야코드 (`01` 금융, `02` 기술, `03` 인력, `04` 수출, `05` 내수, `06` 창업, `07` 경영, `09` 기타 — *`08` 없음*)
* `hashtags`: 쉼표 구분 다중 해시태그 (예: `"기술,부산"`, `"창업,서울"`)
* `active_only`: 마감된 공고 제외 여부 (기본값 `true`, 비정형 기간 문구는 유지)
* `max_pages`: 최신 스캔 페이지 수 (페이지당 100건, 기본값 `2`, 최대 `5`)

#### 4) `download_attachment`
* `file_url` *(필수)*: `www.g2b.go.kr` 또는 `www.bizinfo.go.kr` 다운로드 URL (허용 목록 외 도메인 차단)
* `file_name`: 저장 파일명 (미지정 시 HTTP `Content-Disposition` 헤더에서 추출 시도)
* `dest_dir`: 저장 경로 (미지정 시 `BID_DOWNLOAD_DIR` 환경변수 또는 `./downloads`)

---

## 4. 연동된 외부 API 상세 명세

### A. 조달청 나라장터 입찰공고정보서비스 (`v1.3`, 2026.11.10 개정 기준)

* **Base URL:** `https://apis.data.go.kr/1230000/ad/BidPublicInfoService02`
* **참고:** 공공데이터포털 문서 전체 29개(포털 표기 30개) 오퍼레이션 중 **SW·스마트팩토리 SI 워크플로우에 직접 필요한 13개 오퍼레이션**만 선별 구현했습니다. (토목·건축 공사 전용 및 외자 전용 API 제외)

| 문서 번호 | 오퍼레이션명 | 국문명 | 연결 Tool | 주요 역할 및 구현 디테일 |
| :---: | :--- | :--- | :--- | :--- |
| **12** | `getBidPblancListInfoServcPPSSrch02` | 나라장터검색조건에 의한 입찰공고용역조회 | `search_g2b_bids` | SW·SI 핵심. 서버 측 키워드·업종·지역·추정가격·마감제외 필터링 |
| **14** | `getBidPblancListInfoThngPPSSrch02` | 나라장터검색조건에 의한 입찰공고물품조회 | `search_g2b_bids` | 스마트팩토리 장비·센서·PLC·패키지 SW 물품 검색 |
| **22** | `getBidPblancListInfoEtcPPSSrch02` | 나라장터검색조건에 의한 입찰공고 기타조회 | `search_g2b_bids` | 대학·협동조합 등 기타 공고 검색 (참가자격 원문 포함) |
| **2** | `getBidPblancListInfoServc02` | 입찰공고목록 정보에 대한 용역조회 | `get_g2b_bid_detail` | `inqryDiv=2` 공고번호 단건 상세 + 최신 차수(`bidNtceOrd`) 자동 선택 |
| **4** | `getBidPblancListInfoThng02` | 입찰공고목록 정보에 대한 물품조회 | `get_g2b_bid_detail` | `inqryDiv=2` 물품 공고 단건 상세 |
| **21** | `getBidPblancListInfoEtc02` | 입찰공고목록 정보에 대한 기타공고조회 | `get_g2b_bid_detail` | `inqryDiv=2` 기타 공고 단건 상세 (`bidQlfctRgstCntnts`, `rmrkCntnts`) |
| **7** | `getBidPblancListInfoServcBsisAmount02` | 입찰공고목록 정보에 대한 용역기초금액조회 | `get_g2b_bid_detail` | 용역 기초금액(`bssamt`), 예비가격 범위(`-3% ~ +3%` 등) |
| **5** | `getBidPblancListInfoThngBsisAmount02` | 입찰공고목록 정보에 대한 물품기초금액조회 | `get_g2b_bid_detail` | 물품 기초금액 조회 |
| **15** | `getBidPblancListInfoLicenseLimit02` | 입찰공고목록 정보에 대한 면허제한정보조회 | `get_g2b_bid_detail` | `bidNtceNo` + `bidNtceOrd` 필수. 제한면허·허용업종 목록 조회 |
| **16** | `getBidPblancListInfoPrtcptPsblRgn02` | 입찰공고목록 정보에 대한 참가가능지역정보조회 | `get_g2b_bid_detail` | `bidNtceNo` + `bidNtceOrd` 필수. 지역제한 투찰 가능 지역 조회 |
| **29** | `getBidPblancListInfoAtchFileInfo02` | 입찰공고목록 정보에 대한 첨부파일정보조회 | `get_g2b_bid_detail` | **v1.3 분리 API.** `bsnsDivCd`(1:물품, 5:용역) 필수. 공고문·**PDF 변환본** 포함 |
| **20** | `getBidPblancListInfoEorderAtchFileInfo02` | 입찰공고목록 정보에 대한 e발주 첨부파일정보조회 | `get_g2b_bid_detail` | **SW·SI 핵심.** e발주 시스템에 등록된 제안요청서(RFP) URL 조회 |
| **23** | `getBidPblancListPPIFnlRfpIssAtchFileInfo02` | 혁신장터 최종제안요청서 교부 첨부파일정보조회 | `get_g2b_bid_detail` | 경쟁적 대화 방식 최종 RFP 조회 (**주의: 공고번호 조회는 `inqryDiv=3`**) |
| **10 / 8** | `getBidPblancListInfoChgHstryServc02` / `Thng02` | 용역 / 물품 변경이력조회 | `get_g2b_bid_detail` | 마감일시·예산·규격 변경 전/후 이력 추적 |

> **참고:** 위 표의 10번·8번까지 포함하면 총 14개 오퍼레이션 경로(`Servc` 계열 + `Thng` 계열 + `Etc` 계열 + 공통 부가정보)가 내부적으로 연동됩니다.

#### 나라장터 v1.3 구현 시 반영된 중요 제약·방어 로직
1. **조회기간 최대 7일 제한 (2026.11.10 개정):** 기존 1개월에서 7일로 축소되었습니다. `days_back > 6` 요청 시 클라이언트 내부에서 **6일 단위 청크로 자동 분할 호출**한 뒤 `(bidNtceNo, bidNtceOrd)` 기준으로 중복을 제거합니다.
2. **첨부파일 4중 병합:** 2026.10.22 개정으로 일반 첨부가 29번으로 분리되었으며, SW 사업의 실제 RFP는 20번(e발주)에 있는 경우가 많습니다. `get_g2b_bid_detail`은 `29번(atch_file)` → `20번(eorder_rfp)` → `23번(ppi_rfp)` → `공고 본문(inline)` 순으로 URL 중복 제거 후 통합 반환합니다.
3. **공식 문서 스키마 불일치 및 오타 방어:**
   * `resultCode=03` (No Data)은 에러가 아니라 정상 빈 결과(`Ok(None)`)로 처리합니다.
   * 표 명세와 XML 예제 간 대소문자·키 이름 차이(`pubPrcrmntLrgClsfcNm`/`pubPrcrmntLrgclsfcNm`, `atchDocDivNm`/`docDivNm`)를 모두 fallback 파싱합니다.
   * v1.2 공식 필드명 오타(`befBidBbancNo`)와 XML 응답 내 `&amp;` URL 이스케이프를 그대로 반영·정규화했습니다.
   * `type=json` 요청에도 게이트웨이/키 오류 시 XML이 반환되는 공공데이터포털 특성을 감지해 명확한 에러 메시지를 출력합니다.

#### 주요 참가제한지역코드 (`region_cd`)
| 코드 | 지역 | 코드 | 지역 | 코드 | 지역 |
| :---: | :--- | :---: | :--- | :---: | :--- |
| `00` | 전국 (지역제한 없음) | `28` | 인천광역시 | `44` | 충청남도 |
| `11` | 서울특별시 | `29` | 광주광역시 | `46` | 전라남도 |
| `12` | 전남광주통합특별시 | `30` | 대전광역시 | `47` | 경상북도 |
| `26` | 부산광역시 | `31` | 울산광역시 | `48` | 경상남도 |
| `27` | 대구광역시 | `36` | 세종특별자치시 | `50` | 제주도 |
| `41` | 경기도 | `43` | 충청북도 | `51` / `52` | 강원 / 전북특별자치도 |

---

### B. 기업마당 지원사업정보 API

* **Endpoint:** `GET https://www.bizinfo.go.kr/uss/rss/bizinfoApi.do`
* **구조:** 단일 엔드포인트 호출만으로 공고 메타데이터, 사업개요 본문(`bsnsSumryCn`), 첨부파일(`flpthNm`/`fileNm`), 본문 PDF 출력물(`printFlpthNm`/`printFileNm`)을 한 번에 제공합니다.
* **구현 특징:**
  * API 자체에 키워드 검색 파라미터가 없으므로, `searchLclasId`(분야)와 `hashtags`(분야/지역)로 서버 측 1차 범위를 좁히고 `pageUnit=100`으로 최신 페이지를 스캔해 Rust에서 키워드(`title`, `summary`, `hashtags`, `target`, `authority`, `agency`)와 마감일(`reqstBeginEndDe`)을 2차 필터링합니다.
  * `searchCnt`와 페이징 파라미터를 모두 비우면 전체 수천 건이 한 번에 내려오므로 항상 `pageUnit`/`pageIndex`를 명시합니다.
  * 사업개요의 HTML 태그를 제거해 에이전트 토큰 소모를 줄였으며, `@` 구분자로 여러 개가 묶여 오는 첨부파일 URL도 자동 분리합니다.

---

## 5. 사전 준비: API 키 발급

### 1) 나라장터 API 키 (`G2B_API_KEY`)
1. [공공데이터포털(data.go.kr)](https://www.data.go.kr) 로그인
2. **`조달청_나라장터 입찰공고정보서비스`** 검색 후 **활용신청** (자동 승인)
3. 마이페이지 → 인증키 발급현황에서 **일반 인증키 (Decoding)** 복사

> **주의:** 반드시 **Decoding(일반) 키**를 사용해야 합니다. Encoding 키를 넣으면 `reqwest`가 쿼리 파라미터를 다시 URL 인코딩하면서 이중 인코딩이 발생해 `SERVICE_KEY_IS_NOT_REGISTERED_ERROR (code=30)` 오류가 납니다.

### 2) 기업마당 API 키 (`BIZINFO_API_KEY`)
1. [기업마당(bizinfo.go.kr)](https://www.bizinfo.go.kr) 로그인
2. Open API → **지원사업정보 API** 페이지에서 **사용신청** 후 발급된 인증키(`crtfcKey`) 복사

---

## 6. 빌드 및 실행

### 요구 사항
* **Rust 1.85 이상** (`edition = "2024"` 지원)
  ```bash
  rustup update
  rustc --version
  ```

### 릴리즈 빌드
```bash
cargo build --release
```

빌드가 완료되면 아래 경로에 단일 바이너리가 생성됩니다.
* **macOS / Linux:** `<프로젝트 절대경로>/target/release/bid-mcp`
* **Windows:** `<프로젝트 절대경로>\target\release\bid-mcp.exe`

### 환경변수 목록
| 환경변수 | 필수 여부 | 설명 | 기본값 |
| :--- | :---: | :--- | :--- |
| `G2B_API_KEY` | 나라장터 사용 시 필수 | 공공데이터포털 일반 인증키 (Decoding) | *(없음)* |
| `BIZINFO_API_KEY` | 기업마당 사용 시 필수 | 기업마당 Open API 인증키 (`crtfcKey`) | *(없음)* |
| `BID_DOWNLOAD_DIR` | 선택 | 첨부파일 기본 저장 폴더 경로 (`G2B_DOWNLOAD_DIR`도 호환) | `./downloads` |

---

## 7. AI 에이전트 연동 설정

### A. Google Antigravity (CLI · IDE · 2.0 App 공통)

Antigravity CLI, Antigravity IDE, Antigravity 2.0 App은 동일한 `mcp_config.json` 포맷과 공유 설정 경로를 사용합니다.

* **전역 공유 설정 (모든 프로젝트 & CLI/IDE/2.0 공통 적용):** `~/.gemini/config/mcp_config.json`
* **워크스페이스 설정 (특정 프로젝트 전용):** `<프로젝트 루트>/.agents/mcp_config.json`

> **GUI에서 여는 법:** Antigravity App/IDE → 사이드바 `Customizations` → `Installed` → `MCP Servers` → `View raw config` (또는 에이전트 패널 `...` → `MCP Servers` → `Manage MCP Servers` → `View raw config`)

```json
{
  "mcpServers": {
    "bid-mcp": {
      "command": "/절대경로/bid-mcp/target/release/bid-mcp",
      "args": [],
      "env": {
        "G2B_API_KEY": "공공데이터포털_일반인증키_Decoding",
        "BIZINFO_API_KEY": "기업마당_인증키",
        "BID_DOWNLOAD_DIR": "/절대경로/bid-workspace/downloads"
      }
    }
  }
}
```

* **연결 확인 (Antigravity CLI):** `agy` 실행 후 프롬프트에서 `/mcp` 또는 `/mcp list`를 입력해 `bid-mcp` 서버와 4개 Tool이 활성 상태인지 확인합니다.

---

### B. Gemini CLI (오픈소스 Gemini CLI)

기존 Gemini CLI를 사용하는 경우 `settings.json`의 `mcpServers` 항목에 추가합니다.

* **전역 설정:** `~/.gemini/settings.json`
* **프로젝트 설정:** `<프로젝트 루트>/.gemini/settings.json`

```json
{
  "mcpServers": {
    "bid-mcp": {
      "command": "/절대경로/bid-mcp/target/release/bid-mcp",
      "env": {
        "G2B_API_KEY": "$G2B_API_KEY",
        "BIZINFO_API_KEY": "$BIZINFO_API_KEY",
        "BID_DOWNLOAD_DIR": "./downloads"
      }
    }
  }
}
```
> Gemini CLI는 `env` 블록 내에서 `$VARIABLE_NAME` 형식의 쉘 환경변수 확장을 지원하므로, 키를 설정 파일에 직접 적지 않고 쉘 프로파일(`~/.zshrc` 등)에서 주입할 수 있습니다.

---

### C. Claude Code

* **프로젝트 설정:** `<프로젝트 루트>/.mcp.json` (또는 전역 `~/.claude.json`)

```json
{
  "mcpServers": {
    "bid-mcp": {
      "command": "/절대경로/bid-mcp/target/release/bid-mcp",
      "env": {
        "G2B_API_KEY": "공공데이터포털_일반인증키_Decoding",
        "BIZINFO_API_KEY": "기업마당_인증키",
        "BID_DOWNLOAD_DIR": "/절대경로/bid-workspace/downloads"
      }
    }
  }
}
```

또는 CLI 명령으로 바로 등록할 수 있습니다.
```bash
claude mcp add bid-mcp \
  -e G2B_API_KEY="공공데이터포털_일반인증키_Decoding" \
  -e BIZINFO_API_KEY="기업마당_인증키" \
  -e BID_DOWNLOAD_DIR="./downloads" \
  -- /절대경로/bid-mcp/target/release/bid-mcp
```

---

### D. OpenAI Codex CLI

* **전역 설정:** `~/.codex/config.toml`

```toml
[mcp_servers.bid-mcp]
command = "/절대경로/bid-mcp/target/release/bid-mcp"
args = []

[mcp_servers.bid-mcp.env]
G2B_API_KEY = "공공데이터포털_일반인증키_Decoding"
BIZINFO_API_KEY = "기업마당_인증키"
BID_DOWNLOAD_DIR = "/절대경로/bid-workspace/downloads"
```

---

## 8. HWP/HWPX 제안요청서 처리 및 에이전트 지침 설정

프론티어 에이전트 CLI는 **PDF는 내장 도구로 바로 읽지만**, 한국 공공기관의 **`.hwp` / `.hwpx` 파일은 직접 읽지 못합니다.** 아래 로컬 변환 도구를 함께 설치해 두면 에이전트가 터미널로 텍스트를 추출해 끝까지 자동 분석합니다.

### 로컬 문서 변환 도구 설치 (권장)
```bash
# 1) 구형 .hwp 텍스트 추출 (hwp5txt 명령 제공)
pip install pyhwp

# 2) 신형 .hwpx 텍스트 추출 (선택 — python-hwpx 또는 기본 unzip + XML 파싱)
pip install python-hwpx
```

### 에이전트 지침 파일 예시 (`AGENTS.md` / `GEMINI.md` / `CLAUDE.md`)
작업 폴더 루트에 아래 내용을 `AGENTS.md`(Antigravity/Codex), `GEMINI.md`(Gemini CLI/Antigravity), `CLAUDE.md`(Claude Code)로 저장해 두면 에이전트가 매번 일관된 품질로 공고를 정리합니다.

```markdown
# 공공사업·지원사업 탐색 에이전트 지침

## 1. 검색 원칙
- 공공 SW/SI 용역은 `search_g2b_bids(category="servc")`, 스마트팩토리 장비·패키지 SW는 `category="thng"`, 대학·기관 발주는 `category="etc"`를 조회한다.
- 스마트공장 지원사업·AI/클라우드/데이터 바우처·지자체/테크노파크 사업은 `search_bizinfo_programs`로 조회한다.
- 포괄적인 요청("이번 주 스마트팩토리 관련 사업 찾아줘")이면 나라장터와 기업마당을 병렬로 조회한다.

## 2. 상세 확인 및 RFP 분석
- 유망한 나라장터 공고는 반드시 `get_g2b_bid_detail`을 호출해 면허제한, 참가가능지역, 기초금액, e발주 RFP(attachments)를 확인한다.
- 첨부파일 다운로드 시 **PDF(공고서 변환본, print_doc)를 최우선**으로 받아 내장 도구로 읽는다.
- `.hwp` 파일만 있으면 `download_attachment`로 받은 뒤 `hwp5txt <파일경로>`로 텍스트를 추출해 읽는다.
- `.hwpx` 파일은 `python3` 또는 `unzip`으로 `Contents/section*.xml` 텍스트를 추출해 읽는다.
- 제안요청서 전체를 그대로 복붙하지 말고, **사업목적 / 핵심 과업 범위 / 기술 스택 / 참가자격(면허·실적·지역) / 예산 / 마감일 / 특이사항** 위주로 구조화해 요약한다.
```

---

## 9. 실전 사용 프롬프트 예시

* *"최근 14일간 나라장터에서 추정가격 1억 이상인 스마트공장·MES·AI 관련 용역 공고를 찾고, 소프트웨어사업자가 참여 가능한 건만 표로 정리해 줘."*
* *"기업마당에서 현재 접수 중인 기술(02) 분야 스마트팩토리 및 AI 바우처 공고를 찾아서 지원대상과 신청기간을 요약해 줘."*
* *"공고번호 `R26BK00933736` 상세 조회해서 면허제한이랑 e발주 제안요청서 받아온 뒤, 우리가 수행해야 할 핵심 개발 범위랑 필수 자격요건만 정리해 줘."*
* *"부산·경남 지역제한이거나 전국 대상인 최근 일주일 공공 SW·스마트팩토리 공고를 나라장터와 기업마당에서 모두 찾아 마감이 임박한 순으로 브리핑해 줘."*

---

## 10. 트러블슈팅 및 운영 주의사항

### 개발 시 절대 금지 사항
* **`println!` / `stdout` 출력 금지:** `stdio` 기반 MCP 서버는 표준 출력(`stdout`)으로 JSON-RPC 프로토콜 메시지를 주고받습니다. 디버깅 로그가 필요하면 반드시 `eprintln!` 또는 `stderr` 기반 로거(`tracing`)를 사용해야 합니다.

### 첫 실행 전 빠른 API 키 검증 (`curl`)
MCP 연결 전 터미널에서 키가 정상 작동하는지 먼저 확인하면 디버깅이 훨씬 빠릅니다.

```bash
# 나라장터 테스트 (Decoding 키 사용 시 --data-urlencode 권장)
curl -G "https://apis.data.go.kr/1230000/ad/BidPublicInfoService02/getBidPblancListInfoServcPPSSrch02" \
  --data-urlencode "serviceKey=${G2B_API_KEY}" \
  -d "numOfRows=1" -d "pageNo=1" -d "type=json" \
  -d "inqryDiv=1" -d "inqryBgnDt=202611200000" -d "inqryEndDt=202611252359"

# 기업마당 테스트
curl -G "https://www.bizinfo.go.kr/uss/rss/bizinfoApi.do" \
  --data-urlencode "crtfcKey=${BIZINFO_API_KEY}" \
  -d "dataType=json" -d "pageUnit=1" -d "pageIndex=1"
```

### 자주 만나는 나라장터 에러코드 (`v1.3` 공식 문서 기준)
| 코드 | 의미 | 조치 방법 |
| :---: | :--- | :--- |
| `03` | No Data (데이터 없음) | 정상 케이스 (`bid-mcp`가 자동으로 빈 배열 처리) |
| `06` | 날짜 Format 에러 | `YYYYMMDDHHMM` 12자리 형식 확인 |
| `07` | 입력범위값 초과 | 7일 조회기간 초과 등 파라미터 범위 확인 |
| `20` | 서비스 접근 거부 | 공공데이터포털 해당 오퍼레이션 활용승인 여부 확인 |
| `22` | 일일 트래픽 초과 | 공공데이터포털 일일 호출 한도 확인 (운영계정 증설 신청) |
| `30` | 등록되지 않은 서비스키 | Encoding 키 대신 **Decoding 키**를 넣었는지 확인 |
| `31` | 기한 만료된 서비스키 | 공공데이터포털 마이페이지에서 활용기간 연장 신청 |

### 실전 테스트 시 체크할 포인트
* 나라장터 공식 문서 예제는 XML 기반이므로, 실제 `type=json` 호출 시 간혹 `items` 구조나 특정 신규 필드의 대소문자가 다를 수 있습니다. (`g2b.rs`에 다중 방어 로직이 들어 있으나, 특이 응답이 보이면 실제 JSON 샘플 기준으로 조정하세요.)
* 일부 기관의 첨부파일 다운로드 URL이 세션/리다이렉트를 요구하거나 파일명이 EUC-KR로 인코딩되어 내려오는 경우 `src/clients/download.rs`에 헤더/인코딩 보완을 추가할 수 있습니다.

---

## 11. 향후 확장 로드맵

* **나라장터 사전규격정보서비스 추가:** 본 공고 전 사전규격 단계에서 제안서(RFP) 초안을 미리 확보하기 위한 `search_g2b_prespec` Tool 확장 (동일 `G2B_API_KEY` 사용 가능, 포털에서 별도 활용신청 필요)
* **국책 R&D (NTIS RSS/API) 연동:** AI·디지털트윈·제조 SW R&D 과제 탐색용 `src/clients/ntis.rs` 추가
* **비-API 포털 분리 운영:** 스마트공장 사업관리시스템(`smart-factory.kr`)이나 공기업 자체 조달(한전 SRM, 국방 D2B 등)처럼 웹 크롤링(Playwright 등)이 필요한 경우, 가벼운 `bid-mcp`에 섞지 않고 별도의 크롤링 전용 MCP 서버로 분리하는 구성을 권장합니다.