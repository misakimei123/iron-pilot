use core::fmt;
use core::str::FromStr;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ironpilot_application::{
    AI_TRADING_PROMPT_VERSION_V1, AiPlanRejectionFeedback, AiPromptError, AiTradingPrompt,
    LlmLimits,
};
use ironpilot_domain::{
    AiDecisionContext, AiDecisionContextId, AiProviderResponseId, AiRawResponse, AiTradingPlan,
    DecisionContextError, DomainDecimal,
};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use reqwest::{Client, StatusCode, Url, redirect};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use uuid::Uuid;

pub const DEEPSEEK_API_BASE_URL: &str = "https://api.deepseek.com/";
pub const DEEPSEEK_API_KEY_ENV: &str = "IRONPILOT_DEEPSEEK_API_KEY";
pub const DEEPSEEK_PROVIDER_NAME: &str = "deepseek";
pub const MAX_DEEPSEEK_RESPONSE_BYTES: usize = 128 * 1_024;
pub const MAX_DEEPSEEK_OUTPUT_TOKENS: u32 = 16_384;
pub const MAX_DEEPSEEK_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

const MIN_API_KEY_LENGTH: usize = 8;
const MAX_API_KEY_LENGTH: usize = 512;
const MIN_OUTPUT_TOKENS: u32 = 256;
const MIN_REQUEST_TIMEOUT: Duration = Duration::from_millis(100);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TOKENS_PER_MILLION: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeepSeekModel {
    V4Flash,
    V4Pro,
}

impl DeepSeekModel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V4Flash => "deepseek-v4-flash",
            Self::V4Pro => "deepseek-v4-pro",
        }
    }

    #[must_use]
    pub fn official_pricing_snapshot(self) -> DeepSeekPricing {
        match self {
            Self::V4Flash => {
                DeepSeekPricing::new(decimal("0.0028"), decimal("0.14"), decimal("0.28"))
            }
            Self::V4Pro => {
                DeepSeekPricing::new(decimal("0.003625"), decimal("0.435"), decimal("0.87"))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeepSeekPricing {
    cache_hit_input_per_million_usd: DomainDecimal,
    cache_miss_input_per_million_usd: DomainDecimal,
    output_per_million_usd: DomainDecimal,
}

impl DeepSeekPricing {
    #[must_use]
    pub const fn new(
        cache_hit_input_per_million_usd: DomainDecimal,
        cache_miss_input_per_million_usd: DomainDecimal,
        output_per_million_usd: DomainDecimal,
    ) -> Self {
        Self {
            cache_hit_input_per_million_usd,
            cache_miss_input_per_million_usd,
            output_per_million_usd,
        }
    }

    fn validate(self) -> Result<Self, DeepSeekProviderError> {
        if self.cache_hit_input_per_million_usd < DomainDecimal::ZERO
            || self.cache_miss_input_per_million_usd < DomainDecimal::ZERO
            || self.output_per_million_usd < DomainDecimal::ZERO
        {
            return Err(DeepSeekProviderError::configuration(
                "DeepSeek token prices must not be negative",
            ));
        }
        Ok(self)
    }

    fn calculate(self, usage: DeepSeekUsage) -> Result<DomainDecimal, DeepSeekProviderError> {
        let divisor = Decimal::from(TOKENS_PER_MILLION);
        let hit = Decimal::from(usage.prompt_cache_hit_tokens)
            .checked_mul(self.cache_hit_input_per_million_usd.as_decimal())
            .and_then(|value| value.checked_div(divisor));
        let miss = Decimal::from(usage.prompt_cache_miss_tokens)
            .checked_mul(self.cache_miss_input_per_million_usd.as_decimal())
            .and_then(|value| value.checked_div(divisor));
        let output = Decimal::from(usage.completion_tokens)
            .checked_mul(self.output_per_million_usd.as_decimal())
            .and_then(|value| value.checked_div(divisor));
        let cost = hit
            .and_then(|value| value.checked_add(miss?))
            .and_then(|value| value.checked_add(output?))
            .ok_or_else(|| {
                DeepSeekProviderError::configuration("DeepSeek cost calculation overflowed")
            })?;
        DomainDecimal::from_str(&cost.to_string()).map_err(|_| {
            DeepSeekProviderError::configuration(
                "DeepSeek cost cannot be represented as an exact decimal",
            )
        })
    }

    fn maximum_cost(
        self,
        input_tokens: u64,
        output_tokens: u32,
    ) -> Result<DomainDecimal, DeepSeekProviderError> {
        self.calculate(DeepSeekUsage {
            prompt_tokens: input_tokens,
            completion_tokens: u64::from(output_tokens),
            prompt_cache_hit_tokens: 0,
            prompt_cache_miss_tokens: input_tokens,
            total_tokens: input_tokens.saturating_add(u64::from(output_tokens)),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeepSeekBudgetLimits {
    daily_call_limit: u32,
    daily_token_limit: u64,
    daily_cost_limit_usd: DomainDecimal,
}

impl DeepSeekBudgetLimits {
    pub fn new(
        daily_call_limit: u32,
        daily_token_limit: u64,
        daily_cost_limit_usd: DomainDecimal,
    ) -> Result<Self, DeepSeekProviderError> {
        if daily_call_limit == 0
            || daily_token_limit == 0
            || daily_cost_limit_usd <= DomainDecimal::ZERO
        {
            return Err(DeepSeekProviderError::configuration(
                "DeepSeek daily call, token, and cost budgets must be positive",
            ));
        }
        Ok(Self {
            daily_call_limit,
            daily_token_limit,
            daily_cost_limit_usd,
        })
    }

    pub fn from_runtime_limits(limits: &LlmLimits) -> Result<Self, DeepSeekProviderError> {
        Self::new(
            limits.daily_call_limit(),
            u64::from(limits.daily_token_limit()),
            limits.daily_cost_limit_usd(),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeepSeekProviderConfig {
    model: DeepSeekModel,
    max_output_tokens: u32,
    request_timeout: Duration,
    pricing: DeepSeekPricing,
}

impl DeepSeekProviderConfig {
    pub fn new(
        model: DeepSeekModel,
        max_output_tokens: u32,
        request_timeout: Duration,
        pricing: DeepSeekPricing,
    ) -> Result<Self, DeepSeekProviderError> {
        if !(MIN_OUTPUT_TOKENS..=MAX_DEEPSEEK_OUTPUT_TOKENS).contains(&max_output_tokens) {
            return Err(DeepSeekProviderError::configuration(
                "DeepSeek max output tokens is outside the bounded range",
            ));
        }
        if !(MIN_REQUEST_TIMEOUT..=MAX_DEEPSEEK_REQUEST_TIMEOUT).contains(&request_timeout) {
            return Err(DeepSeekProviderError::configuration(
                "DeepSeek request timeout is outside the bounded range",
            ));
        }
        Ok(Self {
            model,
            max_output_tokens,
            request_timeout,
            pricing: pricing.validate()?,
        })
    }

    pub fn v4_pro_default() -> Result<Self, DeepSeekProviderError> {
        Self::new(
            DeepSeekModel::V4Pro,
            8_192,
            Duration::from_secs(45),
            DeepSeekModel::V4Pro.official_pricing_snapshot(),
        )
    }

    #[must_use]
    pub const fn model(self) -> DeepSeekModel {
        self.model
    }

    #[must_use]
    pub const fn max_output_tokens(self) -> u32 {
        self.max_output_tokens
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeepSeekUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    prompt_cache_hit_tokens: u64,
    prompt_cache_miss_tokens: u64,
    total_tokens: u64,
}

impl DeepSeekUsage {
    #[must_use]
    pub const fn prompt_tokens(self) -> u64 {
        self.prompt_tokens
    }

    #[must_use]
    pub const fn completion_tokens(self) -> u64 {
        self.completion_tokens
    }

    #[must_use]
    pub const fn prompt_cache_hit_tokens(self) -> u64 {
        self.prompt_cache_hit_tokens
    }

    #[must_use]
    pub const fn prompt_cache_miss_tokens(self) -> u64 {
        self.prompt_cache_miss_tokens
    }

    #[must_use]
    pub const fn total_tokens(self) -> u64 {
        self.total_tokens
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeepSeekBudgetSnapshot {
    utc_day: u64,
    calls_used: u32,
    tokens_used_or_reserved: u64,
    cost_used_or_reserved_usd: DomainDecimal,
}

impl DeepSeekBudgetSnapshot {
    #[must_use]
    pub const fn utc_day(self) -> u64 {
        self.utc_day
    }

    #[must_use]
    pub const fn calls_used(self) -> u32 {
        self.calls_used
    }

    #[must_use]
    pub const fn tokens_used_or_reserved(self) -> u64 {
        self.tokens_used_or_reserved
    }

    #[must_use]
    pub const fn cost_used_or_reserved_usd(self) -> DomainDecimal {
        self.cost_used_or_reserved_usd
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeepSeekAttemptOutcome {
    Plan,
    EmptyOutput,
    TruncatedOutput,
    InvalidPlan,
    ProviderRefusal,
    HttpError,
    TransportError,
    Timeout,
}

impl DeepSeekAttemptOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "PLAN",
            Self::EmptyOutput => "EMPTY_OUTPUT",
            Self::TruncatedOutput => "TRUNCATED_OUTPUT",
            Self::InvalidPlan => "INVALID_PLAN",
            Self::ProviderRefusal => "PROVIDER_REFUSAL",
            Self::HttpError => "HTTP_ERROR",
            Self::TransportError => "TRANSPORT_ERROR",
            Self::Timeout => "TIMEOUT",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeepSeekAttemptEvidence {
    attempt_id: AiProviderResponseId,
    context_id: AiDecisionContextId,
    prompt_version: &'static str,
    prompt_hash: Box<str>,
    model: Box<str>,
    is_replan: bool,
    requested_at_unix_millis: u64,
    received_at_unix_millis: Option<u64>,
    latency_millis: u64,
    raw_request: Box<str>,
    raw_response: Option<Box<str>>,
    vendor_response_id: Option<Box<str>>,
    finish_reason: Option<Box<str>>,
    usage: Option<DeepSeekUsage>,
    cost_usd: Option<DomainDecimal>,
    outcome: DeepSeekAttemptOutcome,
}

impl DeepSeekAttemptEvidence {
    #[must_use]
    pub const fn attempt_id(&self) -> AiProviderResponseId {
        self.attempt_id
    }

    #[must_use]
    pub const fn context_id(&self) -> AiDecisionContextId {
        self.context_id
    }

    #[must_use]
    pub const fn prompt_version(&self) -> &'static str {
        self.prompt_version
    }

    #[must_use]
    pub fn prompt_hash(&self) -> &str {
        &self.prompt_hash
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub const fn is_replan(&self) -> bool {
        self.is_replan
    }

    #[must_use]
    pub const fn requested_at_unix_millis(&self) -> u64 {
        self.requested_at_unix_millis
    }

    #[must_use]
    pub const fn received_at_unix_millis(&self) -> Option<u64> {
        self.received_at_unix_millis
    }

    #[must_use]
    pub const fn latency_millis(&self) -> u64 {
        self.latency_millis
    }

    #[must_use]
    pub fn raw_request(&self) -> &str {
        &self.raw_request
    }

    #[must_use]
    pub fn raw_response(&self) -> Option<&str> {
        self.raw_response.as_deref()
    }

    #[must_use]
    pub fn vendor_response_id(&self) -> Option<&str> {
        self.vendor_response_id.as_deref()
    }

    #[must_use]
    pub fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }

    #[must_use]
    pub const fn usage(&self) -> Option<DeepSeekUsage> {
        self.usage
    }

    #[must_use]
    pub const fn cost_usd(&self) -> Option<DomainDecimal> {
        self.cost_usd
    }

    #[must_use]
    pub const fn outcome(&self) -> DeepSeekAttemptOutcome {
        self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeepSeekPlanGeneration {
    prompt: AiTradingPrompt,
    raw_response: AiRawResponse,
    plan: AiTradingPlan,
    evidence: DeepSeekAttemptEvidence,
}

impl DeepSeekPlanGeneration {
    #[must_use]
    pub const fn prompt(&self) -> &AiTradingPrompt {
        &self.prompt
    }

    #[must_use]
    pub const fn raw_response(&self) -> &AiRawResponse {
        &self.raw_response
    }

    #[must_use]
    pub const fn plan(&self) -> &AiTradingPlan {
        &self.plan
    }

    #[must_use]
    pub const fn evidence(&self) -> &DeepSeekAttemptEvidence {
        &self.evidence
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeepSeekProviderErrorKind {
    InvalidConfiguration,
    InvalidPrompt,
    ExpiredContext,
    ConcurrencyExhausted,
    CallBudgetExhausted,
    TokenBudgetExhausted,
    CostBudgetExhausted,
    ReplanLimitExceeded,
    ReplanProvenanceMismatch,
    Timeout,
    Transport,
    Http,
    ResponseTooLarge,
    InvalidResponse,
    EmptyOutput,
    TruncatedOutput,
    ProviderRefusal,
    InvalidPlan,
    BudgetAccounting,
    Clock,
}

#[derive(Clone, Debug)]
pub struct DeepSeekProviderError {
    kind: DeepSeekProviderErrorKind,
    message: Box<str>,
    evidence: Option<Box<DeepSeekAttemptEvidence>>,
}

impl DeepSeekProviderError {
    fn configuration(message: impl Into<Box<str>>) -> Self {
        Self {
            kind: DeepSeekProviderErrorKind::InvalidConfiguration,
            message: message.into(),
            evidence: None,
        }
    }

    fn new(kind: DeepSeekProviderErrorKind, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            message: message.into(),
            evidence: None,
        }
    }

    fn with_evidence(
        kind: DeepSeekProviderErrorKind,
        message: impl Into<Box<str>>,
        evidence: DeepSeekAttemptEvidence,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            evidence: Some(Box::new(evidence)),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> DeepSeekProviderErrorKind {
        self.kind
    }

    #[must_use]
    pub fn evidence(&self) -> Option<&DeepSeekAttemptEvidence> {
        self.evidence.as_deref()
    }
}

impl fmt::Display for DeepSeekProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DeepSeekProviderError {}

#[derive(Clone)]
pub struct DeepSeekAiTradingPlanProvider {
    client: Client,
    endpoint: Url,
    config: DeepSeekProviderConfig,
    budget_limits: DeepSeekBudgetLimits,
    budget: Arc<Mutex<BudgetState>>,
    concurrency: Arc<Semaphore>,
    replanned_contexts: Arc<Mutex<BTreeSet<AiDecisionContextId>>>,
}

impl DeepSeekAiTradingPlanProvider {
    pub fn from_environment(
        config: DeepSeekProviderConfig,
        budget_limits: DeepSeekBudgetLimits,
    ) -> Result<Self, DeepSeekProviderError> {
        let api_key = std::env::var(DEEPSEEK_API_KEY_ENV).map_err(|_| {
            DeepSeekProviderError::configuration(
                "IRONPILOT_DEEPSEEK_API_KEY is missing or is not valid Unicode",
            )
        })?;
        Self::new(api_key, config, budget_limits)
    }

    pub fn new(
        api_key: impl AsRef<str>,
        config: DeepSeekProviderConfig,
        budget_limits: DeepSeekBudgetLimits,
    ) -> Result<Self, DeepSeekProviderError> {
        Self::with_base_url(
            api_key.as_ref(),
            DEEPSEEK_API_BASE_URL,
            config,
            budget_limits,
            false,
        )
    }

    fn with_base_url(
        api_key: &str,
        base_url: &str,
        config: DeepSeekProviderConfig,
        budget_limits: DeepSeekBudgetLimits,
        allow_loopback_http: bool,
    ) -> Result<Self, DeepSeekProviderError> {
        validate_api_key(api_key)?;
        let base_url = Url::parse(base_url).map_err(|error| {
            DeepSeekProviderError::configuration(
                format!("invalid DeepSeek API base URL: {error}").into_boxed_str(),
            )
        })?;
        let is_loopback_http = allow_loopback_http
            && base_url.scheme() == "http"
            && base_url
                .host_str()
                .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"));
        if (!is_loopback_http && base_url.scheme() != "https")
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.path() != "/"
            || base_url.query().is_some()
            || base_url.fragment().is_some()
        {
            return Err(DeepSeekProviderError::configuration(
                "DeepSeek base URL must be an HTTPS origin",
            ));
        }
        let endpoint = base_url.join("chat/completions").map_err(|error| {
            DeepSeekProviderError::configuration(
                format!("cannot construct DeepSeek endpoint: {error}").into_boxed_str(),
            )
        })?;
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| {
                DeepSeekProviderError::configuration("DeepSeek API key is not a valid HTTP value")
            })?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT.min(config.request_timeout))
            .timeout(config.request_timeout)
            .redirect(redirect::Policy::none())
            .default_headers(headers)
            .user_agent(concat!("ironpilot/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| {
                DeepSeekProviderError::configuration(
                    format!("cannot construct DeepSeek client: {error}").into_boxed_str(),
                )
            })?;
        Ok(Self {
            client,
            endpoint,
            config,
            budget_limits,
            budget: Arc::new(Mutex::new(BudgetState::default())),
            concurrency: Arc::new(Semaphore::new(1)),
            replanned_contexts: Arc::new(Mutex::new(BTreeSet::new())),
        })
    }

    pub async fn generate_plan(
        &self,
        context: &AiDecisionContext,
    ) -> Result<DeepSeekPlanGeneration, DeepSeekProviderError> {
        let prompt = AiTradingPrompt::initial(context).map_err(prompt_error)?;
        self.execute(context, prompt).await
    }

    pub async fn replan_after_rejection<I, S>(
        &self,
        context: &AiDecisionContext,
        rejected_plan: &AiTradingPlan,
        reasons: I,
    ) -> Result<DeepSeekPlanGeneration, DeepSeekProviderError>
    where
        I: IntoIterator<Item = S>,
        S: Into<Box<str>>,
    {
        let feedback =
            AiPlanRejectionFeedback::new(context, rejected_plan, reasons).map_err(|error| {
                match error {
                    AiPromptError::ReplanProvenanceMismatch => DeepSeekProviderError::new(
                        DeepSeekProviderErrorKind::ReplanProvenanceMismatch,
                        error.to_string().into_boxed_str(),
                    ),
                    _ => prompt_error(error),
                }
            })?;
        {
            let mut replanned = self.replanned_contexts.lock().map_err(|_| {
                DeepSeekProviderError::new(
                    DeepSeekProviderErrorKind::ReplanLimitExceeded,
                    "DeepSeek replan state is unavailable",
                )
            })?;
            if !replanned.insert(context.context_id()) {
                return Err(DeepSeekProviderError::new(
                    DeepSeekProviderErrorKind::ReplanLimitExceeded,
                    "the Decision Context has already consumed its one replan",
                ));
            }
        }
        let prompt = AiTradingPrompt::replan(context, &feedback).map_err(prompt_error)?;
        self.execute(context, prompt).await
    }

    #[must_use]
    pub fn budget_snapshot(&self) -> DeepSeekBudgetSnapshot {
        let now = current_unix_millis().unwrap_or(0);
        let day = utc_day(now);
        let mut budget = self
            .budget
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        budget.reset_if_new_day(day);
        budget.snapshot()
    }

    async fn execute(
        &self,
        context: &AiDecisionContext,
        prompt: AiTradingPrompt,
    ) -> Result<DeepSeekPlanGeneration, DeepSeekProviderError> {
        let requested_at = current_unix_millis()?;
        if requested_at < context.as_of_unix_millis() || context.is_expired_at(requested_at) {
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::ExpiredContext,
                "AI Decision Context is future-dated or expired before the DeepSeek request",
            ));
        }
        let _permit = self.concurrency.clone().try_acquire_owned().map_err(|_| {
            DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::ConcurrencyExhausted,
                "DeepSeek concurrency budget is exhausted",
            )
        })?;
        let input_token_bound = prompt.conservative_input_token_bound();
        let reserved_tokens =
            input_token_bound.saturating_add(u64::from(self.config.max_output_tokens));
        let reserved_cost = self
            .config
            .pricing
            .maximum_cost(input_token_bound, self.config.max_output_tokens)?;
        let reservation = self.reserve_budget(requested_at, reserved_tokens, reserved_cost)?;
        let attempt_id =
            AiProviderResponseId::new(Uuid::new_v4()).expect("random UUID v4 must be non-nil");
        let raw_request = build_request(&prompt, self.config);
        let start = Instant::now();
        let response_result = self
            .client
            .post(self.endpoint.clone())
            .body(raw_request.clone())
            .send()
            .await;
        let mut response = match response_result {
            Ok(response) => response,
            Err(error) => {
                self.release_reservation(reservation);
                let outcome = if error.is_timeout() {
                    DeepSeekAttemptOutcome::Timeout
                } else {
                    DeepSeekAttemptOutcome::TransportError
                };
                let kind = if error.is_timeout() {
                    DeepSeekProviderErrorKind::Timeout
                } else {
                    DeepSeekProviderErrorKind::Transport
                };
                let evidence = attempt_evidence(
                    attempt_id,
                    context,
                    &prompt,
                    self.config,
                    requested_at,
                    None,
                    elapsed_millis(start),
                    raw_request,
                    None,
                    None,
                    None,
                    None,
                    None,
                    outcome,
                );
                return Err(DeepSeekProviderError::with_evidence(
                    kind,
                    format!("DeepSeek request failed: {error}").into_boxed_str(),
                    evidence,
                ));
            }
        };
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_DEEPSEEK_RESPONSE_BYTES as u64)
        {
            self.release_reservation(reservation);
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::ResponseTooLarge,
                "DeepSeek response exceeds the bounded body limit",
            ));
        }
        let mut body = Vec::new();
        loop {
            let chunk = match response.chunk().await {
                Ok(chunk) => chunk,
                Err(error) => {
                    self.release_reservation(reservation);
                    return Err(DeepSeekProviderError::new(
                        if error.is_timeout() {
                            DeepSeekProviderErrorKind::Timeout
                        } else {
                            DeepSeekProviderErrorKind::Transport
                        },
                        format!("cannot read DeepSeek response: {error}").into_boxed_str(),
                    ));
                }
            };
            let Some(chunk) = chunk else {
                break;
            };
            if body.len().saturating_add(chunk.len()) > MAX_DEEPSEEK_RESPONSE_BYTES {
                self.release_reservation(reservation);
                return Err(DeepSeekProviderError::new(
                    DeepSeekProviderErrorKind::ResponseTooLarge,
                    "DeepSeek response exceeds the bounded body limit",
                ));
            }
            body.extend_from_slice(&chunk);
        }
        let received_at = match current_unix_millis() {
            Ok(received_at) => received_at,
            Err(error) => {
                self.release_reservation(reservation);
                return Err(error);
            }
        };
        let latency_millis = elapsed_millis(start);
        let raw_response = String::from_utf8(body).map_err(|_| {
            self.release_reservation(reservation);
            DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::InvalidResponse,
                "DeepSeek response is not UTF-8 JSON",
            )
        })?;
        if !status.is_success() {
            self.release_reservation(reservation);
            let evidence = attempt_evidence(
                attempt_id,
                context,
                &prompt,
                self.config,
                requested_at,
                Some(received_at),
                latency_millis,
                raw_request,
                Some(raw_response),
                None,
                None,
                None,
                None,
                DeepSeekAttemptOutcome::HttpError,
            );
            return Err(DeepSeekProviderError::with_evidence(
                DeepSeekProviderErrorKind::Http,
                http_error_message(status),
                evidence,
            ));
        }
        let decoded: ChatCompletionResponse =
            serde_json::from_str(&raw_response).map_err(|error| {
                self.release_reservation(reservation);
                DeepSeekProviderError::new(
                    DeepSeekProviderErrorKind::InvalidResponse,
                    format!("cannot decode DeepSeek response: {error}").into_boxed_str(),
                )
            })?;
        let usage = match decoded.usage.validate() {
            Ok(usage) => usage,
            Err(error) => {
                self.release_reservation(reservation);
                return Err(error);
            }
        };
        let cost = match self.config.pricing.calculate(usage) {
            Ok(cost) => cost,
            Err(error) => {
                self.release_reservation(reservation);
                return Err(error);
            }
        };
        self.finalize_reservation(reservation, usage.total_tokens, cost)?;
        if decoded.model.as_ref() != self.config.model.as_str() {
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::InvalidResponse,
                "DeepSeek response model does not match the requested model",
            ));
        }
        if decoded.choices.len() != 1 || decoded.choices[0].index != 0 {
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::InvalidResponse,
                "DeepSeek response must contain exactly choice index 0",
            ));
        }
        let choice = &decoded.choices[0];
        let base_evidence = |outcome, raw_response: String| {
            attempt_evidence(
                attempt_id,
                context,
                &prompt,
                self.config,
                requested_at,
                Some(received_at),
                latency_millis,
                raw_request.clone(),
                Some(raw_response),
                Some(decoded.id.clone()),
                Some(choice.finish_reason.clone()),
                Some(usage),
                Some(cost),
                outcome,
            )
        };
        if choice.finish_reason.as_ref() == "length" {
            return Err(DeepSeekProviderError::with_evidence(
                DeepSeekProviderErrorKind::TruncatedOutput,
                "DeepSeek output was truncated",
                base_evidence(DeepSeekAttemptOutcome::TruncatedOutput, raw_response),
            ));
        }
        if choice.finish_reason.as_ref() != "stop" {
            return Err(DeepSeekProviderError::with_evidence(
                DeepSeekProviderErrorKind::ProviderRefusal,
                format!(
                    "DeepSeek did not complete a plan: finish_reason={}",
                    choice.finish_reason
                )
                .into_boxed_str(),
                base_evidence(DeepSeekAttemptOutcome::ProviderRefusal, raw_response),
            ));
        }
        let content = choice.message.content.as_deref().unwrap_or_default().trim();
        if content.is_empty() {
            return Err(DeepSeekProviderError::with_evidence(
                DeepSeekProviderErrorKind::EmptyOutput,
                "DeepSeek returned no AITradingPlan",
                base_evidence(DeepSeekAttemptOutcome::EmptyOutput, raw_response),
            ));
        }
        let plan = AiTradingPlan::from_json(content).map_err(|error| {
            DeepSeekProviderError::with_evidence(
                DeepSeekProviderErrorKind::InvalidPlan,
                format!("DeepSeek returned an invalid AITradingPlan: {error}").into_boxed_str(),
                base_evidence(DeepSeekAttemptOutcome::InvalidPlan, raw_response.clone()),
            )
        })?;
        if plan.context_id() != context.context_id()
            || plan.instrument_id() != context.instrument_id()
            || plan.valid_until_unix_millis() <= received_at
            || context.is_expired_at(received_at)
        {
            return Err(DeepSeekProviderError::with_evidence(
                DeepSeekProviderErrorKind::InvalidPlan,
                "DeepSeek plan provenance or validity does not match the active Context",
                base_evidence(DeepSeekAttemptOutcome::InvalidPlan, raw_response),
            ));
        }
        let domain_response = AiRawResponse::new(
            attempt_id,
            context.context_id(),
            DEEPSEEK_PROVIDER_NAME,
            decoded.model,
            received_at,
            raw_response.clone(),
        )
        .map_err(domain_response_error)?;
        let evidence = base_evidence(DeepSeekAttemptOutcome::Plan, raw_response);
        Ok(DeepSeekPlanGeneration {
            prompt,
            raw_response: domain_response,
            plan,
            evidence,
        })
    }

    fn reserve_budget(
        &self,
        now_unix_millis: u64,
        tokens: u64,
        cost: DomainDecimal,
    ) -> Result<BudgetReservation, DeepSeekProviderError> {
        let mut state = self.budget.lock().map_err(|_| {
            DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::BudgetAccounting,
                "DeepSeek budget state is unavailable",
            )
        })?;
        state.reset_if_new_day(utc_day(now_unix_millis));
        if state.calls_used >= self.budget_limits.daily_call_limit {
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::CallBudgetExhausted,
                "DeepSeek daily call budget is exhausted",
            ));
        }
        if state
            .tokens_used_or_reserved
            .checked_add(tokens)
            .is_none_or(|value| value > self.budget_limits.daily_token_limit)
        {
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::TokenBudgetExhausted,
                "DeepSeek daily token budget is exhausted",
            ));
        }
        let next_cost = state
            .cost_used_or_reserved_usd
            .checked_add(cost)
            .ok_or_else(|| {
                DeepSeekProviderError::new(
                    DeepSeekProviderErrorKind::BudgetAccounting,
                    "DeepSeek daily cost budget overflowed",
                )
            })?;
        if next_cost > self.budget_limits.daily_cost_limit_usd {
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::CostBudgetExhausted,
                "DeepSeek daily cost budget is exhausted",
            ));
        }
        state.calls_used += 1;
        state.tokens_used_or_reserved += tokens;
        state.cost_used_or_reserved_usd = next_cost;
        Ok(BudgetReservation { tokens, cost })
    }

    fn release_reservation(&self, reservation: BudgetReservation) {
        let mut state = self
            .budget
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.tokens_used_or_reserved = state
            .tokens_used_or_reserved
            .saturating_sub(reservation.tokens);
        state.cost_used_or_reserved_usd = state
            .cost_used_or_reserved_usd
            .checked_sub(reservation.cost)
            .unwrap_or(DomainDecimal::ZERO);
    }

    fn finalize_reservation(
        &self,
        reservation: BudgetReservation,
        actual_tokens: u64,
        actual_cost: DomainDecimal,
    ) -> Result<(), DeepSeekProviderError> {
        if actual_tokens > reservation.tokens || actual_cost > reservation.cost {
            self.release_reservation(reservation);
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::BudgetAccounting,
                "DeepSeek reported usage beyond the conservative reservation",
            ));
        }
        let mut state = self.budget.lock().map_err(|_| {
            DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::BudgetAccounting,
                "DeepSeek budget state is unavailable",
            )
        })?;
        state.tokens_used_or_reserved = state
            .tokens_used_or_reserved
            .saturating_sub(reservation.tokens)
            .saturating_add(actual_tokens);
        state.cost_used_or_reserved_usd = state
            .cost_used_or_reserved_usd
            .checked_sub(reservation.cost)
            .and_then(|value| value.checked_add(actual_cost))
            .ok_or_else(|| {
                DeepSeekProviderError::new(
                    DeepSeekProviderErrorKind::BudgetAccounting,
                    "DeepSeek budget reconciliation failed",
                )
            })?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
struct BudgetReservation {
    tokens: u64,
    cost: DomainDecimal,
}

#[derive(Clone, Copy, Debug)]
struct BudgetState {
    utc_day: u64,
    calls_used: u32,
    tokens_used_or_reserved: u64,
    cost_used_or_reserved_usd: DomainDecimal,
}

impl Default for BudgetState {
    fn default() -> Self {
        Self {
            utc_day: 0,
            calls_used: 0,
            tokens_used_or_reserved: 0,
            cost_used_or_reserved_usd: DomainDecimal::ZERO,
        }
    }
}

impl BudgetState {
    fn reset_if_new_day(&mut self, day: u64) {
        if self.utc_day != day {
            *self = Self {
                utc_day: day,
                ..Self::default()
            };
        }
    }

    const fn snapshot(self) -> DeepSeekBudgetSnapshot {
        DeepSeekBudgetSnapshot {
            utc_day: self.utc_day,
            calls_used: self.calls_used,
            tokens_used_or_reserved: self.tokens_used_or_reserved,
            cost_used_or_reserved_usd: self.cost_used_or_reserved_usd,
        }
    }
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    response_format: ResponseFormat,
    thinking: ThinkingMode,
    max_tokens: u32,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Clone, Copy, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Clone, Copy, Serialize)]
struct ThinkingMode {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    id: Box<str>,
    choices: Vec<ChatChoice>,
    model: Box<str>,
    usage: WireUsage,
}

#[derive(Deserialize)]
struct ChatChoice {
    index: u32,
    finish_reason: Box<str>,
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    content: Option<Box<str>>,
}

#[derive(Clone, Copy, Deserialize)]
struct WireUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    prompt_cache_hit_tokens: u64,
    prompt_cache_miss_tokens: u64,
    total_tokens: u64,
}

impl WireUsage {
    fn validate(self) -> Result<DeepSeekUsage, DeepSeekProviderError> {
        if self
            .prompt_cache_hit_tokens
            .checked_add(self.prompt_cache_miss_tokens)
            != Some(self.prompt_tokens)
            || self.prompt_tokens.checked_add(self.completion_tokens) != Some(self.total_tokens)
        {
            return Err(DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::InvalidResponse,
                "DeepSeek usage token totals are inconsistent",
            ));
        }
        Ok(DeepSeekUsage {
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            prompt_cache_hit_tokens: self.prompt_cache_hit_tokens,
            prompt_cache_miss_tokens: self.prompt_cache_miss_tokens,
            total_tokens: self.total_tokens,
        })
    }
}

fn build_request(prompt: &AiTradingPrompt, config: DeepSeekProviderConfig) -> String {
    serde_json::to_string(&ChatCompletionRequest {
        model: config.model.as_str(),
        messages: [
            ChatMessage {
                role: "system",
                content: prompt.system_message(),
            },
            ChatMessage {
                role: "user",
                content: prompt.user_message(),
            },
        ],
        response_format: ResponseFormat {
            kind: "json_object",
        },
        thinking: ThinkingMode { kind: "enabled" },
        max_tokens: config.max_output_tokens,
        stream: false,
    })
    .expect("DeepSeek request must serialize")
}

#[allow(clippy::too_many_arguments)]
fn attempt_evidence(
    attempt_id: AiProviderResponseId,
    context: &AiDecisionContext,
    prompt: &AiTradingPrompt,
    config: DeepSeekProviderConfig,
    requested_at_unix_millis: u64,
    received_at_unix_millis: Option<u64>,
    latency_millis: u64,
    raw_request: String,
    raw_response: Option<String>,
    vendor_response_id: Option<Box<str>>,
    finish_reason: Option<Box<str>>,
    usage: Option<DeepSeekUsage>,
    cost_usd: Option<DomainDecimal>,
    outcome: DeepSeekAttemptOutcome,
) -> DeepSeekAttemptEvidence {
    DeepSeekAttemptEvidence {
        attempt_id,
        context_id: context.context_id(),
        prompt_version: AI_TRADING_PROMPT_VERSION_V1,
        prompt_hash: prompt.prompt_hash().to_string().into_boxed_str(),
        model: config.model.as_str().into(),
        is_replan: prompt.is_replan(),
        requested_at_unix_millis,
        received_at_unix_millis,
        latency_millis,
        raw_request: raw_request.into_boxed_str(),
        raw_response: raw_response.map(String::into_boxed_str),
        vendor_response_id,
        finish_reason,
        usage,
        cost_usd,
        outcome,
    }
}

fn prompt_error(error: AiPromptError) -> DeepSeekProviderError {
    DeepSeekProviderError::new(
        DeepSeekProviderErrorKind::InvalidPrompt,
        error.to_string().into_boxed_str(),
    )
}

fn domain_response_error(error: DecisionContextError) -> DeepSeekProviderError {
    DeepSeekProviderError::new(
        DeepSeekProviderErrorKind::InvalidResponse,
        format!("cannot record DeepSeek raw response: {error}").into_boxed_str(),
    )
}

fn validate_api_key(api_key: &str) -> Result<(), DeepSeekProviderError> {
    if !(MIN_API_KEY_LENGTH..=MAX_API_KEY_LENGTH).contains(&api_key.len())
        || api_key.chars().any(char::is_whitespace)
        || api_key.chars().any(char::is_control)
    {
        return Err(DeepSeekProviderError::configuration(
            "DeepSeek API key is empty or invalid",
        ));
    }
    Ok(())
}

fn current_unix_millis() -> Result<u64, DeepSeekProviderError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            DeepSeekProviderError::new(
                DeepSeekProviderErrorKind::Clock,
                "system clock is before the Unix epoch",
            )
        })?
        .as_millis();
    u64::try_from(millis).map_err(|_| {
        DeepSeekProviderError::new(
            DeepSeekProviderErrorKind::Clock,
            "system clock cannot be represented in milliseconds",
        )
    })
}

const fn utc_day(unix_millis: u64) -> u64 {
    unix_millis / 86_400_000
}

fn elapsed_millis(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn http_error_message(status: StatusCode) -> Box<str> {
    let class = match status.as_u16() {
        400 | 422 => "invalid request",
        401 => "authentication failed",
        402 => "insufficient provider balance",
        429 => "rate limited",
        500 => "provider server error",
        503 => "provider overloaded",
        _ => "unexpected HTTP status",
    };
    format!("DeepSeek {class} ({status})").into_boxed_str()
}

fn decimal(value: &str) -> DomainDecimal {
    DomainDecimal::from_str(value).expect("fixed DeepSeek price must be an exact decimal")
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;
    use std::time::Duration;

    use ironpilot_application::{AuditEntry, UnixMillis};
    use ironpilot_domain::{
        AssetCode, AuditEntryId, ClosedCandle, ExchangeAssetBalance, ExchangeServerTime,
        FEATURE_CANDLE_WINDOW, InstrumentId, InstrumentRulesSnapshot, InstrumentTradingStatus,
        LocalAssetBalance, MarketDataSource, MarketFeatureEngine, MarketTimeframe,
        PortfolioReconciler, RulesHash, RuntimeInstanceId, TopOfBook,
        validated_spot_instrument_rules,
    };
    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;
    use crate::persistence::{PersistenceEffect, SqliteRepository};

    const TEST_API_KEY: &str = "test-deepseek-api-key";

    #[test]
    fn current_pricing_calculates_cache_hit_miss_and_output_exactly() {
        let usage = DeepSeekUsage {
            prompt_tokens: 1_000_000,
            completion_tokens: 1_000_000,
            prompt_cache_hit_tokens: 250_000,
            prompt_cache_miss_tokens: 750_000,
            total_tokens: 2_000_000,
        };
        assert_eq!(
            DeepSeekModel::V4Pro
                .official_pricing_snapshot()
                .calculate(usage)
                .expect("cost should calculate")
                .to_string(),
            "1.19715625"
        );
    }

    #[test]
    fn malformed_usage_totals_fail_closed() {
        let usage = WireUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            prompt_cache_hit_tokens: 3,
            prompt_cache_miss_tokens: 6,
            total_tokens: 15,
        };
        assert_eq!(
            usage
                .validate()
                .expect_err("inconsistent usage must fail")
                .kind(),
            DeepSeekProviderErrorKind::InvalidResponse
        );
    }

    #[test]
    fn api_key_and_production_origin_are_strict() {
        let config = DeepSeekProviderConfig::v4_pro_default().expect("default config");
        let budget =
            DeepSeekBudgetLimits::new(1, 100_000, decimal("1.00")).expect("budget is valid");
        assert!(DeepSeekAiTradingPlanProvider::new("", config, budget).is_err());
        assert!(
            DeepSeekAiTradingPlanProvider::with_base_url(
                "test-api-key",
                "http://example.com/",
                config,
                budget,
                false
            )
            .is_err()
        );
    }

    #[test]
    fn prompt_contains_raw_market_features_account_rules_and_authorization() {
        let context = decision_context(1);
        let prompt = AiTradingPrompt::initial(&context).expect("prompt must build");
        let payload: Value =
            serde_json::from_str(prompt.user_message()).expect("prompt must be JSON");

        assert_eq!(
            payload["decision_context"]["market"]["candles_15m"]
                .as_array()
                .map(Vec::len),
            Some(FEATURE_CANDLE_WINDOW)
        );
        assert_eq!(
            payload["decision_context"]["market"]["candles_1h"]
                .as_array()
                .map(Vec::len),
            Some(FEATURE_CANDLE_WINDOW)
        );
        assert!(
            payload["decision_context"]["market"]["features"]["primary_15m"]["rsi"].is_string()
        );
        assert!(payload["decision_context"]["instrument_rules"]["price_tick"].is_string());
        assert!(payload["decision_context"]["account"]["portfolio"]["assets"].is_array());
        assert_eq!(
            payload["decision_context"]["user_authorization"]["maximum_loss_quote"],
            "25.00"
        );
        let complete_prompt = format!("{}{}", prompt.system_message(), prompt.user_message());
        for forbidden in [
            "strategy_space",
            "Strategy Space",
            "materializer",
            "Materializer",
            "risk_tier",
            "entry_anchor",
        ] {
            assert!(
                !complete_prompt.contains(forbidden),
                "prompt must not inject {forbidden}"
            );
        }
    }

    #[tokio::test]
    async fn exact_open_long_is_parsed_with_raw_usage_cost_and_latency_evidence() {
        let context = decision_context(2);
        let plan_json = plan_json(&context, "OPEN_LONG", 20);
        let (base_url, server) =
            spawn_http_server(vec![MockResponse::ok(completion_body(&plan_json, "stop"))]).await;
        let provider = test_provider(&base_url, 4, Duration::from_secs(2));

        let generated = provider
            .generate_plan(&context)
            .await
            .expect("complete AI plan must generate");
        assert_eq!(generated.plan().action().as_str(), "OPEN_LONG");
        assert_eq!(
            generated
                .plan()
                .order()
                .expect("OPEN_LONG has order")
                .limit_price()
                .expect("LIMIT has price")
                .to_string(),
            "210.00"
        );
        assert_eq!(
            generated
                .plan()
                .protective_stop()
                .expect("OPEN_LONG has stop")
                .trigger_price()
                .to_string(),
            "200.00"
        );
        assert_eq!(generated.plan().take_profits().len(), 2);
        assert_eq!(generated.raw_response().provider(), DEEPSEEK_PROVIDER_NAME);
        assert_eq!(
            generated
                .evidence()
                .usage()
                .map(DeepSeekUsage::total_tokens),
            Some(15)
        );
        assert!(
            generated
                .evidence()
                .cost_usd()
                .is_some_and(|cost| cost > DomainDecimal::ZERO)
        );
        assert_eq!(generated.evidence().outcome(), DeepSeekAttemptOutcome::Plan);
        assert_eq!(
            generated.evidence().prompt_version(),
            AI_TRADING_PROMPT_VERSION_V1
        );
        let requests = server.await.expect("mock server");
        assert_eq!(requests.len(), 1);
        let request: Value = serde_json::from_str(&requests[0]).expect("request JSON");
        assert_eq!(request["response_format"]["type"], "json_object");
        assert_eq!(request["thinking"]["type"], "enabled");
        assert_eq!(request["stream"], false);
        assert!(!generated.evidence().raw_request().contains(TEST_API_KEY));

        let temp_path =
            std::env::temp_dir().join(format!("ironpilot-p3-04-{}", Uuid::new_v4().hyphenated()));
        std::fs::create_dir(&temp_path).expect("create provider evidence temp directory");
        let repository = SqliteRepository::connect(temp_path.join("evidence.sqlite3"), 1)
            .await
            .expect("open evidence repository");
        let owner = stable_id::<RuntimeInstanceId>(8_000);
        let lease_at = timestamp(generated.evidence().requested_at_unix_millis());
        repository
            .acquire_instance_lease(owner, lease_at, Duration::from_secs(60))
            .await
            .expect("acquire evidence lease");
        let occurred_at = timestamp(
            generated
                .evidence()
                .received_at_unix_millis()
                .expect("successful attempt received time"),
        );
        let audit = AuditEntry::new(
            stable_id::<AuditEntryId>(8_001),
            occurred_at,
            "AI_PROVIDER_ATTEMPT_RECORDED",
            Some(generated.evidence().attempt_id().to_string()),
            json!({
                "outcome": generated.evidence().outcome().as_str(),
                "prompt_hash": generated.evidence().prompt_hash()
            }),
        )
        .expect("provider audit");
        assert_eq!(
            repository
                .persist_ai_provider_attempt(owner, &context, generated.evidence(), &audit)
                .await
                .expect("persist provider evidence"),
            PersistenceEffect::Applied
        );
        assert_eq!(
            repository
                .persist_ai_provider_attempt(owner, &context, generated.evidence(), &audit)
                .await
                .expect("duplicate provider evidence"),
            PersistenceEffect::DuplicateNoEffect
        );
        let stored: (String, i64, i64, String) = sqlx::query_as(
            "SELECT prompt_hash, total_tokens, latency_millis, outcome
             FROM ai_provider_attempts WHERE attempt_id = ?",
        )
        .bind(generated.evidence().attempt_id().to_string())
        .fetch_one(repository.pool())
        .await
        .expect("read provider evidence");
        assert_eq!(stored.0, generated.evidence().prompt_hash());
        assert_eq!(stored.1, 15);
        assert!(stored.2 >= 0);
        assert_eq!(stored.3, "PLAN");

        let mut rolled_back = generated.evidence().clone();
        rolled_back.attempt_id = stable_id(8_002);
        assert!(
            repository
                .persist_ai_provider_attempt(owner, &context, &rolled_back, &audit)
                .await
                .is_err(),
            "duplicate audit ID must roll back the candidate attempt"
        );
        let rolled_back_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ai_provider_attempts WHERE attempt_id = ?")
                .bind(rolled_back.attempt_id().to_string())
                .fetch_one(repository.pool())
                .await
                .expect("count rolled-back provider attempt");
        assert_eq!(rolled_back_count, 0);
        repository.close().await;
        std::fs::remove_dir_all(&temp_path).expect("remove provider evidence temp directory");
    }

    #[tokio::test]
    async fn multiple_management_actions_parse_without_local_parameter_generation() {
        for (sequence, action) in [(30, "NO_TRADE"), (31, "HOLD"), (32, "MODIFY_PROTECTION")] {
            let context = decision_context(sequence);
            let body = completion_body(&plan_json(&context, action, sequence + 100), "stop");
            let (base_url, server) = spawn_http_server(vec![MockResponse::ok(body)]).await;
            let generated = test_provider(&base_url, 2, Duration::from_secs(2))
                .generate_plan(&context)
                .await
                .expect("action must parse");
            assert_eq!(generated.plan().action().as_str(), action);
            assert_eq!(server.await.expect("mock server").len(), 1);
        }
    }

    #[tokio::test]
    async fn empty_truncated_and_unknown_field_outputs_fail_closed() {
        let empty_context = decision_context(40);
        let truncated_context = decision_context(41);
        let unknown_context = decision_context(42);
        let mut unknown: Value =
            serde_json::from_str(&plan_json(&unknown_context, "NO_TRADE", 142)).expect("plan JSON");
        unknown["risk_tier"] = json!("medium");
        let responses = vec![
            MockResponse::ok(completion_body("", "stop")),
            MockResponse::ok(completion_body("{\"schema_version\":\"3.0\"", "length")),
            MockResponse::ok(completion_body(&unknown.to_string(), "stop")),
        ];
        let (base_url, server) = spawn_http_server(responses).await;
        let provider = test_provider(&base_url, 4, Duration::from_secs(2));

        let empty = provider
            .generate_plan(&empty_context)
            .await
            .expect_err("empty output must fail");
        assert_eq!(empty.kind(), DeepSeekProviderErrorKind::EmptyOutput);
        assert_eq!(
            empty.evidence().map(DeepSeekAttemptEvidence::outcome),
            Some(DeepSeekAttemptOutcome::EmptyOutput)
        );
        let truncated = provider
            .generate_plan(&truncated_context)
            .await
            .expect_err("truncated output must fail");
        assert_eq!(truncated.kind(), DeepSeekProviderErrorKind::TruncatedOutput);
        let unknown = provider
            .generate_plan(&unknown_context)
            .await
            .expect_err("unknown plan field must fail");
        assert_eq!(unknown.kind(), DeepSeekProviderErrorKind::InvalidPlan);
        assert_eq!(server.await.expect("mock server").len(), 3);
    }

    #[tokio::test]
    async fn timeout_returns_no_plan_and_retains_bounded_attempt_evidence() {
        let context = decision_context(50);
        let body = completion_body(&plan_json(&context, "NO_TRADE", 150), "stop");
        let (base_url, server) = spawn_http_server(vec![MockResponse {
            delay: Duration::from_millis(300),
            status: 200,
            body,
        }])
        .await;
        let provider = test_provider(&base_url, 2, Duration::from_millis(100));

        let error = provider
            .generate_plan(&context)
            .await
            .expect_err("timeout must fail closed");
        assert_eq!(error.kind(), DeepSeekProviderErrorKind::Timeout);
        assert_eq!(
            error.evidence().map(DeepSeekAttemptEvidence::outcome),
            Some(DeepSeekAttemptOutcome::Timeout)
        );
        let _ = server.await;
    }

    #[tokio::test]
    async fn exhausted_call_budget_prevents_a_second_http_request() {
        let first_context = decision_context(60);
        let second_context = decision_context(61);
        let (base_url, server) = spawn_http_server(vec![MockResponse::ok(completion_body(
            &plan_json(&first_context, "NO_TRADE", 160),
            "stop",
        ))])
        .await;
        let provider = test_provider(&base_url, 1, Duration::from_secs(2));

        provider
            .generate_plan(&first_context)
            .await
            .expect("first request is budgeted");
        let error = provider
            .generate_plan(&second_context)
            .await
            .expect_err("second request exceeds call budget");
        assert_eq!(error.kind(), DeepSeekProviderErrorKind::CallBudgetExhausted);
        assert_eq!(provider.budget_snapshot().calls_used(), 1);
        assert_eq!(server.await.expect("mock server").len(), 1);
    }

    #[tokio::test]
    async fn token_and_cost_budget_exhaustion_prevent_http_requests() {
        let context = decision_context(65);
        let config = DeepSeekProviderConfig::new(
            DeepSeekModel::V4Pro,
            8_192,
            Duration::from_secs(2),
            DeepSeekModel::V4Pro.official_pricing_snapshot(),
        )
        .expect("provider config");

        let (token_url, token_server) = spawn_http_server(Vec::new()).await;
        let token_provider = DeepSeekAiTradingPlanProvider::with_base_url(
            TEST_API_KEY,
            &token_url,
            config,
            DeepSeekBudgetLimits::new(2, 1, decimal("10.00")).expect("token budget"),
            true,
        )
        .expect("token provider");
        assert_eq!(
            token_provider
                .generate_plan(&context)
                .await
                .expect_err("token budget must block")
                .kind(),
            DeepSeekProviderErrorKind::TokenBudgetExhausted
        );
        assert!(token_server.await.expect("token server").is_empty());

        let (cost_url, cost_server) = spawn_http_server(Vec::new()).await;
        let cost_provider = DeepSeekAiTradingPlanProvider::with_base_url(
            TEST_API_KEY,
            &cost_url,
            config,
            DeepSeekBudgetLimits::new(2, 1_000_000, decimal("0.000000001")).expect("cost budget"),
            true,
        )
        .expect("cost provider");
        assert_eq!(
            cost_provider
                .generate_plan(&context)
                .await
                .expect_err("cost budget must block")
                .kind(),
            DeepSeekProviderErrorKind::CostBudgetExhausted
        );
        assert!(cost_server.await.expect("cost server").is_empty());
    }

    #[tokio::test]
    async fn rejection_feedback_allows_exactly_one_replan_for_the_context() {
        let context = decision_context(70);
        let initial_plan = plan_json(&context, "OPEN_LONG", 170);
        let replacement_plan = plan_json(&context, "NO_TRADE", 171);
        let (base_url, server) = spawn_http_server(vec![
            MockResponse::ok(completion_body(&initial_plan, "stop")),
            MockResponse::ok(completion_body(&replacement_plan, "stop")),
        ])
        .await;
        let provider = test_provider(&base_url, 4, Duration::from_secs(2));
        let initial = provider
            .generate_plan(&context)
            .await
            .expect("initial plan");
        let replacement = provider
            .replan_after_rejection(
                &context,
                initial.plan(),
                ["price tick is not exchange-compatible"],
            )
            .await
            .expect("one rejection replan is allowed");
        assert_eq!(replacement.plan().action().as_str(), "NO_TRADE");
        assert!(replacement.prompt().is_replan());

        let error = provider
            .replan_after_rejection(&context, replacement.plan(), ["simulated second rejection"])
            .await
            .expect_err("second replan must be blocked");
        assert_eq!(error.kind(), DeepSeekProviderErrorKind::ReplanLimitExceeded);
        let requests = server.await.expect("mock server");
        assert_eq!(requests.len(), 2);
        assert!(requests[1].contains("price tick is not exchange-compatible"));
        assert!(requests[1].contains(&initial.plan().plan_id().to_string()));
    }

    fn test_provider(
        base_url: &str,
        call_limit: u32,
        timeout: Duration,
    ) -> DeepSeekAiTradingPlanProvider {
        let config = DeepSeekProviderConfig::new(
            DeepSeekModel::V4Pro,
            8_192,
            timeout,
            DeepSeekModel::V4Pro.official_pricing_snapshot(),
        )
        .expect("test provider config");
        let budget = DeepSeekBudgetLimits::new(call_limit, 1_000_000, decimal("10.00"))
            .expect("test budget");
        DeepSeekAiTradingPlanProvider::with_base_url(TEST_API_KEY, base_url, config, budget, true)
            .expect("test provider")
    }

    fn decision_context(sequence: u128) -> AiDecisionContext {
        let now = current_unix_millis().expect("test clock");
        let primary_duration = MarketTimeframe::FifteenMinutes.duration_millis();
        let confirmation_duration = MarketTimeframe::OneHour.duration_millis();
        let primary_end = now - (now % primary_duration);
        let confirmation_end = primary_end - (primary_end % confirmation_duration);
        let primary = candles(MarketTimeframe::FifteenMinutes, primary_end);
        let confirmation = candles(MarketTimeframe::OneHour, confirmation_end);
        let book = TopOfBook::new(
            instrument(),
            now,
            now,
            decimal("218.9"),
            decimal("10"),
            decimal("219.1"),
            decimal("12"),
        )
        .expect("book fixture");
        let features = MarketFeatureEngine::compute(
            &primary,
            &confirmation,
            &book,
            now,
            MarketDataSource::WebSocketLive,
        )
        .expect("feature fixture");
        let rules = rules_snapshot(now);
        let portfolio = PortfolioReconciler::reconcile(
            vec![
                ExchangeAssetBalance::new(
                    AssetCode::new("BTC").expect("asset"),
                    decimal("0.5"),
                    DomainDecimal::ZERO,
                )
                .expect("exchange balance"),
                ExchangeAssetBalance::new(
                    AssetCode::new("USDT").expect("asset"),
                    decimal("1000"),
                    DomainDecimal::ZERO,
                )
                .expect("exchange balance"),
            ],
            vec![
                LocalAssetBalance::new(
                    AssetCode::new("BTC").expect("asset"),
                    decimal("0.5"),
                    decimal("0.5"),
                )
                .expect("local balance"),
                LocalAssetBalance::new(
                    AssetCode::new("USDT").expect("asset"),
                    decimal("1000"),
                    DomainDecimal::ZERO,
                )
                .expect("local balance"),
            ],
            now,
        )
        .expect("portfolio fixture");
        AiDecisionContext::new(
            stable_id(sequence),
            now,
            primary,
            confirmation,
            book,
            features,
            &rules,
            &portfolio,
            Vec::new(),
            Vec::new(),
            decimal("25.00"),
        )
        .expect("decision context fixture")
    }

    fn candles(timeframe: MarketTimeframe, end_at: u64) -> Vec<ClosedCandle> {
        let duration = timeframe.duration_millis();
        let first_open = end_at - duration * u64::try_from(FEATURE_CANDLE_WINDOW).expect("window");
        (0..FEATURE_CANDLE_WINDOW)
            .map(|index| {
                let price = 100 + i64::try_from(index).expect("index");
                ClosedCandle::new(
                    instrument(),
                    timeframe,
                    first_open + duration * u64::try_from(index).expect("index"),
                    decimal(&price.to_string()),
                    decimal(&(price + 1).to_string()),
                    decimal(&(price - 1).to_string()),
                    decimal(&price.to_string()),
                    decimal("10"),
                    decimal(&(price * 10).to_string()),
                    true,
                )
                .expect("candle fixture")
            })
            .collect()
    }

    fn rules_snapshot(now: u64) -> InstrumentRulesSnapshot {
        let rules = validated_spot_instrument_rules(
            instrument(),
            AssetCode::new("BTC").expect("asset"),
            AssetCode::new("USDT").expect("asset"),
            InstrumentTradingStatus::Trading,
            decimal("0.10"),
            decimal("0.000001"),
            decimal("0.000001"),
            decimal("5"),
            decimal("100"),
            decimal("50"),
            decimal("25"),
            decimal("0.01"),
            decimal("0.02"),
        )
        .expect("rules");
        InstrumentRulesSnapshot::new(
            vec![rules],
            ExchangeServerTime::new(now / 1_000, now * 1_000_000, now).expect("server time"),
            now,
            now + 60_000,
            RulesHash::from_sha256([9; 32]),
        )
        .expect("rules snapshot")
    }

    fn plan_json(context: &AiDecisionContext, action: &str, sequence: u128) -> String {
        let now = current_unix_millis().expect("test clock");
        let mut plan = json!({
            "schema_version": "3.0",
            "plan_id": stable_id::<ironpilot_domain::AiTradingPlanId>(sequence).to_string(),
            "context_id": context.context_id().to_string(),
            "instrument_id": context.instrument_id().to_string(),
            "action": action,
            "valid_until": now + 20_000,
            "confidence": "0.70",
            "thesis": "The supplied raw market and account facts support this AI decision.",
            "invalidation": "Re-evaluate when subsequent facts invalidate the thesis.",
            "risks": ["Market conditions can change."]
        });
        match action {
            "OPEN_LONG" => {
                plan["order"] = json!({
                    "type": "LIMIT",
                    "quantity": "0.10",
                    "limit_price": "210.00",
                    "time_in_force": "GTC",
                    "expires_at": now + 15_000,
                    "max_slippage_quote": "1.00"
                });
                plan["protective_stop"] = json!({
                    "trigger_price": "200.00",
                    "order_type": "MARKET"
                });
                plan["take_profits"] = json!([
                    {"price": "225.00", "quantity": "0.04"},
                    {"price": "230.00", "quantity": "0.06"}
                ]);
                plan["declared_max_loss_quote"] = json!("2.00");
                plan["review"] = review(now);
            }
            "NO_TRADE" => {}
            "HOLD" => {
                plan["target_trade_plan_id"] =
                    json!(stable_id::<ironpilot_domain::TradePlanId>(9_000).to_string());
                plan["review"] = review(now);
            }
            "MODIFY_PROTECTION" => {
                plan["target_trade_plan_id"] =
                    json!(stable_id::<ironpilot_domain::TradePlanId>(9_000).to_string());
                plan["protective_stop"] = json!({
                    "trigger_price": "205.00",
                    "order_type": "MARKET"
                });
                plan["declared_max_loss_quote"] = json!("1.50");
                plan["review"] = review(now);
            }
            other => panic!("unsupported test action {other}"),
        }
        plan.to_string()
    }

    fn review(now: u64) -> Value {
        json!({
            "next_review_at": now + 10_000,
            "max_holding_until": now + 60_000
        })
    }

    fn completion_body(content: &str, finish_reason: &str) -> String {
        json!({
            "id": "chatcmpl-test",
            "choices": [{
                "index": 0,
                "finish_reason": finish_reason,
                "message": {
                    "role": "assistant",
                    "content": content
                }
            }],
            "created": 1_800_000_000,
            "model": "deepseek-v4-pro",
            "system_fingerprint": "fp-test",
            "object": "chat.completion",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "prompt_cache_hit_tokens": 3,
                "prompt_cache_miss_tokens": 7,
                "total_tokens": 15
            }
        })
        .to_string()
    }

    fn instrument() -> InstrumentId {
        InstrumentId::from_str("bybit:spot:BTCUSDT").expect("instrument")
    }

    fn stable_id<T: FromStr>(value: u128) -> T
    where
        T::Err: fmt::Debug,
    {
        T::from_str(&format!("{value:032x}")).expect("stable ID")
    }

    fn timestamp(value: u64) -> UnixMillis {
        UnixMillis::new(i64::try_from(value).expect("test timestamp fits i64"))
            .expect("test timestamp")
    }

    struct MockResponse {
        delay: Duration,
        status: u16,
        body: String,
    }

    impl MockResponse {
        fn ok(body: String) -> Self {
            Self {
                delay: Duration::ZERO,
                status: 200,
                body,
            }
        }
    }

    async fn spawn_http_server(
        responses: Vec<MockResponse>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let address = listener.local_addr().expect("mock address");
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.expect("accept request");
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 8_192];
                let header_end = loop {
                    let read = stream.read(&mut buffer).await.expect("read request");
                    assert!(read > 0, "request closed before headers");
                    bytes.extend_from_slice(&buffer[..read]);
                    if let Some(index) = find_header_end(&bytes) {
                        break index;
                    }
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().expect("content length"))
                        })
                    })
                    .expect("content length header");
                let body_start = header_end + 4;
                while bytes.len() < body_start + content_length {
                    let read = stream.read(&mut buffer).await.expect("read body");
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                }
                requests.push(
                    String::from_utf8(bytes[body_start..body_start + content_length].to_vec())
                        .expect("request body is UTF-8"),
                );
                tokio::time::sleep(response.delay).await;
                let reason = if response.status == 200 {
                    "OK"
                } else {
                    "ERROR"
                };
                let wire = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.status,
                    reason,
                    response.body.len(),
                    response.body
                );
                let _ = stream.write_all(wire.as_bytes()).await;
            }
            requests
        });
        (format!("http://{address}/"), server)
    }

    fn find_header_end(bytes: &[u8]) -> Option<usize> {
        bytes.windows(4).position(|window| window == b"\r\n\r\n")
    }
}
