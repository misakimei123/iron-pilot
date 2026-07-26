use core::fmt;
use core::str::FromStr;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use ironpilot_application::{
    AuthorizedEmergencyCommand, EmergencyCommandKind, MAX_EMERGENCY_COMMAND_TTL_MILLIS,
};
use ironpilot_domain::{DomainDecimal, EmergencyActionId};
use sha2::{Digest, Sha256};
use sqlx::Row;
use teloxide_core::payloads::{GetUpdatesSetters, SendMessageSetters};
use teloxide_core::requests::Requester;
use teloxide_core::types::{AllowedUpdate, ChatId, Update, UpdateKind};
use teloxide_core::{Bot, RequestError};
use teloxide_reqwest::{Client as TelegramHttpClient, Url, redirect};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{SqliteRepository, StorageError};

pub const TELEGRAM_READONLY_VERSION_V1: &str = "ironpilot-telegram-readonly-v1";
pub const TELEGRAM_BOT_API_BASE_URL: &str = "https://api.telegram.org/";
pub const TELEGRAM_BOT_TOKEN_ENV: &str = "IRONPILOT_TELEGRAM_BOT_TOKEN";
pub const MAX_TELEGRAM_MESSAGE_CHARS: usize = 4_096;
pub const MAX_TELEGRAM_UPDATES_PER_POLL: u8 = 32;
pub const MAX_TELEGRAM_QUERY_ROWS: u8 = 20;
pub const MAX_TELEGRAM_NOTIFICATION_EVENTS: u8 = 32;
pub const MAX_TELEGRAM_READONLY_CHATS: usize = 8;
pub const TELEGRAM_EMERGENCY_VERSION_V1: &str = "ironpilot-telegram-emergency-v1";
pub const MAX_TELEGRAM_EMERGENCY_OPERATORS: usize = 8;
pub const MAX_TELEGRAM_PENDING_EMERGENCIES: usize = 16;

const MIN_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_LONG_POLL_TIMEOUT_SECONDS: u64 = 25;
const MAX_STORED_TEXT_BYTES: i64 = 8_192;
const MIN_EMERGENCY_CONFIRMATION_TTL: Duration = Duration::from_secs(10);
const MAX_EMERGENCY_CONFIRMATION_TTL: Duration = Duration::from_secs(120);
const MIN_EMERGENCY_COMMAND_TTL: Duration = Duration::from_secs(10);

pub struct TelegramReadOnlyConfig {
    bot_token: Box<str>,
    allowed_chat_ids: BTreeSet<i64>,
    request_timeout: Duration,
    api_base_url: Url,
}

impl TelegramReadOnlyConfig {
    pub fn from_environment(
        allowed_chat_ids: Vec<i64>,
        request_timeout: Duration,
    ) -> Result<Self, TelegramReadOnlyError> {
        let bot_token = std::env::var(TELEGRAM_BOT_TOKEN_ENV)
            .map_err(|_| TelegramReadOnlyError::InvalidConfiguration)?;
        Self::new(bot_token, allowed_chat_ids, request_timeout)
    }

    pub fn new(
        bot_token: impl Into<Box<str>>,
        allowed_chat_ids: Vec<i64>,
        request_timeout: Duration,
    ) -> Result<Self, TelegramReadOnlyError> {
        Self::with_base_url(
            bot_token,
            allowed_chat_ids,
            request_timeout,
            TELEGRAM_BOT_API_BASE_URL,
            false,
        )
    }

    fn with_base_url(
        bot_token: impl Into<Box<str>>,
        allowed_chat_ids: Vec<i64>,
        request_timeout: Duration,
        api_base_url: &str,
        allow_http_for_tests: bool,
    ) -> Result<Self, TelegramReadOnlyError> {
        let bot_token = bot_token.into();
        validate_bot_token(&bot_token)?;
        if allowed_chat_ids.is_empty()
            || allowed_chat_ids.len() > MAX_TELEGRAM_READONLY_CHATS
            || allowed_chat_ids.contains(&0)
        {
            return Err(TelegramReadOnlyError::InvalidConfiguration);
        }
        let supplied_chat_count = allowed_chat_ids.len();
        let allowed_chat_ids = allowed_chat_ids.into_iter().collect::<BTreeSet<_>>();
        if allowed_chat_ids.len() != supplied_chat_count {
            return Err(TelegramReadOnlyError::InvalidConfiguration);
        }
        if !(MIN_REQUEST_TIMEOUT..=MAX_REQUEST_TIMEOUT).contains(&request_timeout) {
            return Err(TelegramReadOnlyError::InvalidConfiguration);
        }
        let api_base_url =
            Url::parse(api_base_url).map_err(|_| TelegramReadOnlyError::InvalidConfiguration)?;
        let is_loopback_http = allow_http_for_tests
            && api_base_url.scheme() == "http"
            && api_base_url
                .host_str()
                .is_some_and(|host| matches!(host, "127.0.0.1" | "localhost" | "::1"));
        if (api_base_url.scheme() != "https" && !is_loopback_http)
            || api_base_url.cannot_be_a_base()
            || api_base_url.host_str().is_none()
            || api_base_url.query().is_some()
            || api_base_url.fragment().is_some()
            || api_base_url.username() != ""
            || api_base_url.password().is_some()
            || api_base_url.path() != "/"
        {
            return Err(TelegramReadOnlyError::InvalidConfiguration);
        }
        Ok(Self {
            bot_token,
            allowed_chat_ids,
            request_timeout,
            api_base_url,
        })
    }

    fn long_poll_timeout_seconds(&self) -> u64 {
        self.request_timeout
            .as_secs()
            .saturating_sub(1)
            .clamp(1, MAX_LONG_POLL_TIMEOUT_SECONDS)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TelegramReadOnlyCommand {
    Help,
    Status,
    Events { limit: u8 },
    Plans { limit: u8 },
    Plan { action_id: Option<Box<str>> },
    Validations { limit: u8 },
    Validation { action_id: Option<Box<str>> },
    Positions,
    Orders { limit: u8 },
    Trades { limit: u8 },
    Authorization,
    Unsupported,
}

impl TelegramReadOnlyCommand {
    pub fn parse(text: &str) -> Result<Option<Self>, TelegramReadOnlyError> {
        let text = text.trim();
        if text.is_empty() || !text.starts_with('/') {
            return Ok(None);
        }
        if text.len() > 256 || text.chars().any(|character| character == '\0') {
            return Err(TelegramReadOnlyError::InvalidCommand);
        }
        let mut fields = text.split_whitespace();
        let command = fields
            .next()
            .expect("a non-empty command has a first token")
            .split('@')
            .next()
            .expect("split always has a first field")
            .to_ascii_lowercase();
        let arguments = fields.collect::<Vec<_>>();
        let no_arguments = || {
            if arguments.is_empty() {
                Ok(())
            } else {
                Err(TelegramReadOnlyError::InvalidCommand)
            }
        };
        match command.as_str() {
            "/start" | "/help" => {
                no_arguments()?;
                Ok(Some(Self::Help))
            }
            "/status" => {
                no_arguments()?;
                Ok(Some(Self::Status))
            }
            "/events" => Ok(Some(Self::Events {
                limit: parse_optional_limit(&arguments)?,
            })),
            "/plans" => Ok(Some(Self::Plans {
                limit: parse_optional_limit(&arguments)?,
            })),
            "/plan" => Ok(Some(Self::Plan {
                action_id: parse_optional_identifier(&arguments)?,
            })),
            "/validations" => Ok(Some(Self::Validations {
                limit: parse_optional_limit(&arguments)?,
            })),
            "/validation" => Ok(Some(Self::Validation {
                action_id: parse_optional_identifier(&arguments)?,
            })),
            "/positions" => {
                no_arguments()?;
                Ok(Some(Self::Positions))
            }
            "/orders" => Ok(Some(Self::Orders {
                limit: parse_optional_limit(&arguments)?,
            })),
            "/trades" => Ok(Some(Self::Trades {
                limit: parse_optional_limit(&arguments)?,
            })),
            "/authorization" => {
                no_arguments()?;
                Ok(Some(Self::Authorization))
            }
            _ => Ok(Some(Self::Unsupported)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelegramReadOnlyText(Box<str>);

impl TelegramReadOnlyText {
    fn new(value: String) -> Result<Self, TelegramReadOnlyError> {
        let value = sanitize_and_bound_text(&value);
        if value.is_empty() {
            return Err(TelegramReadOnlyError::InvalidResponse);
        }
        Ok(Self(value.into_boxed_str()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TelegramPollReport {
    next_offset: Option<i64>,
    received_updates: u8,
    authorized_commands: u8,
    replies_sent: u8,
}

impl TelegramPollReport {
    #[must_use]
    pub const fn next_offset(self) -> Option<i64> {
        self.next_offset
    }

    #[must_use]
    pub const fn received_updates(self) -> u8 {
        self.received_updates
    }

    #[must_use]
    pub const fn authorized_commands(self) -> u8 {
        self.authorized_commands
    }

    #[must_use]
    pub const fn replies_sent(self) -> u8 {
        self.replies_sent
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TelegramNotificationReport {
    next_audit_sequence: i64,
    confirmed_events: u8,
    messages_sent: u16,
}

impl TelegramNotificationReport {
    #[must_use]
    pub const fn next_audit_sequence(self) -> i64 {
        self.next_audit_sequence
    }

    #[must_use]
    pub const fn confirmed_events(self) -> u8 {
        self.confirmed_events
    }

    #[must_use]
    pub const fn messages_sent(self) -> u16 {
        self.messages_sent
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TelegramEmergencyCommand {
    BeginCloseAll,
    ConfirmCloseAll { nonce: Box<str> },
}

impl TelegramEmergencyCommand {
    pub fn parse(text: &str) -> Result<Option<Self>, TelegramReadOnlyError> {
        let text = text.trim();
        if text.is_empty() || !text.starts_with('/') {
            return Ok(None);
        }
        if text.len() > 256 || text.chars().any(|character| character == '\0') {
            return Err(TelegramReadOnlyError::InvalidCommand);
        }
        let mut fields = text.split_whitespace();
        let command = fields
            .next()
            .expect("a non-empty command has a first token")
            .split('@')
            .next()
            .expect("split always has a first field")
            .to_ascii_lowercase();
        let arguments = fields.collect::<Vec<_>>();
        match command.as_str() {
            "/emergency_close_all" => {
                if arguments.is_empty() {
                    Ok(Some(Self::BeginCloseAll))
                } else {
                    Err(TelegramReadOnlyError::InvalidCommand)
                }
            }
            "/confirm_emergency_close_all" => match arguments.as_slice() {
                [nonce]
                    if nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
                {
                    Ok(Some(Self::ConfirmCloseAll {
                        nonce: nonce.to_ascii_lowercase().into_boxed_str(),
                    }))
                }
                _ => Err(TelegramReadOnlyError::InvalidCommand),
            },
            _ => Ok(None),
        }
    }
}

pub struct TelegramEmergencyPolicy {
    allowed_user_ids: BTreeSet<u64>,
    confirmation_ttl: Duration,
    command_ttl: Duration,
}

impl TelegramEmergencyPolicy {
    pub fn new(
        allowed_user_ids: Vec<u64>,
        confirmation_ttl: Duration,
        command_ttl: Duration,
    ) -> Result<Self, TelegramReadOnlyError> {
        if allowed_user_ids.is_empty()
            || allowed_user_ids.len() > MAX_TELEGRAM_EMERGENCY_OPERATORS
            || allowed_user_ids.contains(&0)
            || !(MIN_EMERGENCY_CONFIRMATION_TTL..=MAX_EMERGENCY_CONFIRMATION_TTL)
                .contains(&confirmation_ttl)
            || command_ttl < MIN_EMERGENCY_COMMAND_TTL
            || command_ttl.as_millis() > u128::from(MAX_EMERGENCY_COMMAND_TTL_MILLIS)
        {
            return Err(TelegramReadOnlyError::InvalidConfiguration);
        }
        let supplied_user_count = allowed_user_ids.len();
        let allowed_user_ids = allowed_user_ids.into_iter().collect::<BTreeSet<_>>();
        if allowed_user_ids.len() != supplied_user_count {
            return Err(TelegramReadOnlyError::InvalidConfiguration);
        }
        Ok(Self {
            allowed_user_ids,
            confirmation_ttl,
            command_ttl,
        })
    }
}

pub struct TelegramEmergencySession {
    policy: TelegramEmergencyPolicy,
    pending: Mutex<BTreeMap<(i64, u64), PendingEmergencyChallenge>>,
}

impl TelegramEmergencySession {
    #[must_use]
    pub fn new(policy: TelegramEmergencyPolicy) -> Self {
        Self {
            policy,
            pending: Mutex::new(BTreeMap::new()),
        }
    }

    fn is_authorized_user(&self, user_id: u64) -> bool {
        self.policy.allowed_user_ids.contains(&user_id)
    }

    async fn handle(
        &self,
        chat_id: i64,
        user_id: u64,
        update_id: i64,
        command: TelegramEmergencyCommand,
        now_unix_millis: u64,
    ) -> Result<TelegramEmergencyOutcome, TelegramReadOnlyError> {
        if !self.policy.allowed_user_ids.contains(&user_id) {
            return TelegramEmergencyOutcome::reply("Emergency authorization rejected.");
        }
        let key = (chat_id, user_id);
        let mut pending = self.pending.lock().await;
        pending.retain(|_, challenge| challenge.expires_at_unix_millis > now_unix_millis);
        match command {
            TelegramEmergencyCommand::BeginCloseAll => {
                if pending.len() >= MAX_TELEGRAM_PENDING_EMERGENCIES && !pending.contains_key(&key)
                {
                    return Err(TelegramReadOnlyError::EmergencyCapacityExceeded);
                }
                let action_id = EmergencyActionId::new(Uuid::new_v4())
                    .map_err(|_| TelegramReadOnlyError::InvalidResponse)?;
                let nonce = Uuid::new_v4().simple().to_string();
                let nonce_hash = Sha256::digest(nonce.as_bytes()).into();
                let expires_at_unix_millis = now_unix_millis
                    .checked_add(duration_millis(self.policy.confirmation_ttl)?)
                    .ok_or(TelegramReadOnlyError::InvalidCommand)?;
                let authorization_evidence_hash = telegram_authorization_hash(
                    chat_id,
                    user_id,
                    update_id,
                    action_id,
                    now_unix_millis,
                );
                pending.insert(
                    key,
                    PendingEmergencyChallenge {
                        action_id,
                        authorization_evidence_hash,
                        nonce_hash,
                        expires_at_unix_millis,
                    },
                );
                TelegramEmergencyOutcome::reply(&format!(
                    "Emergency close requires a second confirmation.\n\
                     No order or position has changed.\n\
                     Challenge expires at: {expires_at_unix_millis}\n\
                     Confirm with:\n/confirm_emergency_close_all {nonce}"
                ))
            }
            TelegramEmergencyCommand::ConfirmCloseAll { nonce } => {
                let Some(challenge) = pending.remove(&key) else {
                    return TelegramEmergencyOutcome::reply(
                        "No active emergency confirmation. Start again with /emergency_close_all.",
                    );
                };
                let supplied_nonce_hash: [u8; 32] = Sha256::digest(nonce.as_bytes()).into();
                if challenge.expires_at_unix_millis <= now_unix_millis
                    || supplied_nonce_hash != challenge.nonce_hash
                {
                    return TelegramEmergencyOutcome::reply(
                        "Emergency confirmation rejected. The challenge is no longer valid.",
                    );
                }
                let expires_at_unix_millis = now_unix_millis
                    .checked_add(duration_millis(self.policy.command_ttl)?)
                    .ok_or(TelegramReadOnlyError::InvalidCommand)?;
                let authorized = AuthorizedEmergencyCommand::new(
                    challenge.action_id,
                    EmergencyCommandKind::CloseAllManagedExposure,
                    format!("telegram:user:{user_id}:chat:{chat_id}"),
                    challenge.authorization_evidence_hash,
                    challenge.nonce_hash,
                    now_unix_millis,
                    expires_at_unix_millis,
                )
                .map_err(|_| TelegramReadOnlyError::InvalidCommand)?;
                Ok(TelegramEmergencyOutcome {
                    reply: TelegramReadOnlyText::new(
                        "Emergency confirmation accepted. The authorized command is ready for the Emergency Core; no direct Telegram order was created.".to_owned(),
                    )?,
                    authorized_command: Some(authorized),
                })
            }
        }
    }
}

struct PendingEmergencyChallenge {
    action_id: EmergencyActionId,
    authorization_evidence_hash: [u8; 32],
    nonce_hash: [u8; 32],
    expires_at_unix_millis: u64,
}

struct TelegramEmergencyOutcome {
    reply: TelegramReadOnlyText,
    authorized_command: Option<AuthorizedEmergencyCommand>,
}

impl TelegramEmergencyOutcome {
    fn reply(value: &str) -> Result<Self, TelegramReadOnlyError> {
        Ok(Self {
            reply: TelegramReadOnlyText::new(value.to_owned())?,
            authorized_command: None,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TelegramEmergencyPollReport {
    next_offset: Option<i64>,
    received_updates: u8,
    emergency_attempts: u8,
    replies_sent: u8,
    authorized_commands: Vec<AuthorizedEmergencyCommand>,
}

impl TelegramEmergencyPollReport {
    #[must_use]
    pub const fn next_offset(&self) -> Option<i64> {
        self.next_offset
    }

    #[must_use]
    pub const fn received_updates(&self) -> u8 {
        self.received_updates
    }

    #[must_use]
    pub const fn emergency_attempts(&self) -> u8 {
        self.emergency_attempts
    }

    #[must_use]
    pub const fn replies_sent(&self) -> u8 {
        self.replies_sent
    }

    #[must_use]
    pub fn authorized_commands(&self) -> &[AuthorizedEmergencyCommand] {
        &self.authorized_commands
    }
}

pub struct TelegramReadOnlyAdapter {
    bot: Bot,
    config: TelegramReadOnlyConfig,
}

impl TelegramReadOnlyAdapter {
    pub fn new(config: TelegramReadOnlyConfig) -> Result<Self, TelegramReadOnlyError> {
        let client = TelegramHttpClient::builder()
            .connect_timeout(config.request_timeout.min(Duration::from_secs(5)))
            .timeout(config.request_timeout)
            .redirect(redirect::Policy::none())
            .user_agent(concat!("ironpilot/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| TelegramReadOnlyError::InvalidConfiguration)?;
        let bot = Bot::with_client(config.bot_token.to_string(), client)
            .set_api_url(config.api_base_url.clone());
        Ok(Self { bot, config })
    }

    pub async fn execute_command(
        &self,
        repository: &SqliteRepository,
        command: &TelegramReadOnlyCommand,
    ) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
        match command {
            TelegramReadOnlyCommand::Help => TelegramReadOnlyText::new(help_text().to_owned()),
            TelegramReadOnlyCommand::Status => status_text(repository).await,
            TelegramReadOnlyCommand::Events { limit } => events_text(repository, *limit).await,
            TelegramReadOnlyCommand::Plans { limit } => plans_text(repository, *limit).await,
            TelegramReadOnlyCommand::Plan { action_id } => {
                plan_text(repository, action_id.as_deref()).await
            }
            TelegramReadOnlyCommand::Validations { limit } => {
                validations_text(repository, *limit).await
            }
            TelegramReadOnlyCommand::Validation { action_id } => {
                validation_text(repository, action_id.as_deref()).await
            }
            TelegramReadOnlyCommand::Positions => positions_text(repository).await,
            TelegramReadOnlyCommand::Orders { limit } => orders_text(repository, *limit).await,
            TelegramReadOnlyCommand::Trades { limit } => trades_text(repository, *limit).await,
            TelegramReadOnlyCommand::Authorization => authorization_text(repository).await,
            TelegramReadOnlyCommand::Unsupported => TelegramReadOnlyText::new(
                "Unsupported. This adapter is read-only; strategy and emergency controls are unavailable.\n\n"
                    .to_owned()
                    + help_text(),
            ),
        }
    }

    pub async fn poll_once(
        &self,
        repository: &SqliteRepository,
        offset: Option<i64>,
    ) -> Result<TelegramPollReport, TelegramReadOnlyError> {
        let (updates, sdk_offset) = self.fetch_updates(offset).await?;
        let mut authorized_commands = 0_u8;
        let mut replies_sent = 0_u8;
        for update in &updates {
            let UpdateKind::Message(message) = &update.kind else {
                continue;
            };
            if !self.config.allowed_chat_ids.contains(&message.chat.id.0) {
                continue;
            }
            let Some(text) = message.text() else {
                continue;
            };
            let command = match TelegramReadOnlyCommand::parse(text) {
                Ok(Some(command)) => command,
                Ok(None) => continue,
                Err(_) => {
                    self.send_message(
                        message.chat.id.0,
                        &TelegramReadOnlyText::new(
                            "Invalid read-only command. Use /help.".to_owned(),
                        )?,
                    )
                    .await?;
                    authorized_commands = authorized_commands.saturating_add(1);
                    replies_sent = replies_sent.saturating_add(1);
                    continue;
                }
            };
            authorized_commands = authorized_commands.saturating_add(1);
            let response = self.execute_command(repository, &command).await?;
            self.send_message(message.chat.id.0, &response).await?;
            replies_sent = replies_sent.saturating_add(1);
        }
        let next_offset = updates
            .last()
            .map(|update| {
                i64::from(update.id.0)
                    .checked_add(1)
                    .ok_or(TelegramReadOnlyError::InvalidResponse)
            })
            .transpose()?
            .or(sdk_offset.map(i64::from));
        Ok(TelegramPollReport {
            next_offset,
            received_updates: u8::try_from(updates.len())
                .map_err(|_| TelegramReadOnlyError::InvalidResponse)?,
            authorized_commands,
            replies_sent,
        })
    }

    pub async fn poll_once_with_emergency(
        &self,
        repository: &SqliteRepository,
        emergency: &TelegramEmergencySession,
        offset: Option<i64>,
        now_unix_millis: u64,
    ) -> Result<TelegramEmergencyPollReport, TelegramReadOnlyError> {
        if now_unix_millis == 0 {
            return Err(TelegramReadOnlyError::InvalidCommand);
        }
        let (updates, sdk_offset) = self.fetch_updates(offset).await?;
        let mut emergency_attempts = 0_u8;
        let mut replies_sent = 0_u8;
        let mut authorized_commands = Vec::new();
        for update in &updates {
            let UpdateKind::Message(message) = &update.kind else {
                continue;
            };
            let chat_id = message.chat.id.0;
            if !self.config.allowed_chat_ids.contains(&chat_id) {
                continue;
            }
            let Some(text) = message.text() else {
                continue;
            };
            match TelegramEmergencyCommand::parse(text) {
                Ok(Some(command)) => {
                    emergency_attempts = emergency_attempts.saturating_add(1);
                    let Some(user_id) = message.from.as_ref().map(|user| user.id.0) else {
                        self.send_message(
                            chat_id,
                            &TelegramReadOnlyText::new(
                                "Emergency authorization rejected.".to_owned(),
                            )?,
                        )
                        .await?;
                        replies_sent = replies_sent.saturating_add(1);
                        continue;
                    };
                    let outcome = emergency
                        .handle(
                            chat_id,
                            user_id,
                            i64::from(update.id.0),
                            command,
                            now_unix_millis,
                        )
                        .await?;
                    self.send_message(chat_id, &outcome.reply).await?;
                    replies_sent = replies_sent.saturating_add(1);
                    if let Some(command) = outcome.authorized_command {
                        authorized_commands.push(command);
                    }
                }
                Ok(None) => {
                    let command = match TelegramReadOnlyCommand::parse(text) {
                        Ok(Some(command)) => command,
                        Ok(None) => continue,
                        Err(_) => {
                            self.send_message(
                                chat_id,
                                &TelegramReadOnlyText::new(
                                    "Invalid command. Use /help.".to_owned(),
                                )?,
                            )
                            .await?;
                            replies_sent = replies_sent.saturating_add(1);
                            continue;
                        }
                    };
                    let response = self.execute_command(repository, &command).await?;
                    let response = if matches!(command, TelegramReadOnlyCommand::Help)
                        && message
                            .from
                            .as_ref()
                            .is_some_and(|user| emergency.is_authorized_user(user.id.0))
                    {
                        TelegramReadOnlyText::new(format!(
                            "{}\n\nProtected emergency control:\n/emergency_close_all - begin identity-bound two-step confirmation",
                            response.as_str()
                        ))?
                    } else {
                        response
                    };
                    self.send_message(chat_id, &response).await?;
                    replies_sent = replies_sent.saturating_add(1);
                }
                Err(_) => {
                    emergency_attempts = emergency_attempts.saturating_add(1);
                    self.send_message(
                        chat_id,
                        &TelegramReadOnlyText::new(
                            "Invalid emergency command. Start again with /emergency_close_all."
                                .to_owned(),
                        )?,
                    )
                    .await?;
                    replies_sent = replies_sent.saturating_add(1);
                }
            }
        }
        let next_offset = updates
            .last()
            .map(|update| {
                i64::from(update.id.0)
                    .checked_add(1)
                    .ok_or(TelegramReadOnlyError::InvalidResponse)
            })
            .transpose()?
            .or(sdk_offset.map(i64::from));
        Ok(TelegramEmergencyPollReport {
            next_offset,
            received_updates: u8::try_from(updates.len())
                .map_err(|_| TelegramReadOnlyError::InvalidResponse)?,
            emergency_attempts,
            replies_sent,
            authorized_commands,
        })
    }

    pub async fn notify_confirmed_events(
        &self,
        repository: &SqliteRepository,
        after_audit_sequence: i64,
        limit: u8,
    ) -> Result<TelegramNotificationReport, TelegramReadOnlyError> {
        if after_audit_sequence < 0 || limit == 0 || limit > MAX_TELEGRAM_NOTIFICATION_EVENTS {
            return Err(TelegramReadOnlyError::InvalidLimit);
        }
        let rows = sqlx::query(
            "
            SELECT sequence, occurred_at, category, subject_id,
                   substr(payload_json, 1, ?) AS payload_json
            FROM audit_log
            WHERE sequence > ?
            ORDER BY sequence
            LIMIT ?
            ",
        )
        .bind(MAX_STORED_TEXT_BYTES)
        .bind(after_audit_sequence)
        .bind(i64::from(limit))
        .fetch_all(&repository.pool)
        .await
        .map_err(storage)?;
        let mut next_audit_sequence = after_audit_sequence;
        let mut messages_sent = 0_u16;
        for row in &rows {
            let sequence: i64 = row.try_get("sequence").map_err(storage)?;
            let text = TelegramReadOnlyText::new(format!(
                "Confirmed event\nsequence: {sequence}\ntime: {}\ncategory: {}\nsubject: {}\npayload: {}",
                row.try_get::<i64, _>("occurred_at").map_err(storage)?,
                row.try_get::<String, _>("category").map_err(storage)?,
                row.try_get::<Option<String>, _>("subject_id")
                    .map_err(storage)?
                    .unwrap_or_else(|| "-".to_owned()),
                row.try_get::<String, _>("payload_json").map_err(storage)?
            ))?;
            for chat_id in &self.config.allowed_chat_ids {
                self.send_message(*chat_id, &text).await?;
                messages_sent = messages_sent.saturating_add(1);
            }
            next_audit_sequence = sequence;
        }
        Ok(TelegramNotificationReport {
            next_audit_sequence,
            confirmed_events: u8::try_from(rows.len())
                .map_err(|_| TelegramReadOnlyError::InvalidResponse)?,
            messages_sent,
        })
    }

    async fn send_message(
        &self,
        chat_id: i64,
        text: &TelegramReadOnlyText,
    ) -> Result<(), TelegramReadOnlyError> {
        let request = self
            .bot
            .send_message(ChatId(chat_id), text.as_str())
            .protect_content(true);
        tokio::time::timeout(self.config.request_timeout, request)
            .await
            .map_err(|_| TelegramReadOnlyError::Timeout)?
            .map_err(classify_sdk_error)?;
        Ok(())
    }

    async fn fetch_updates(
        &self,
        offset: Option<i64>,
    ) -> Result<(Vec<Update>, Option<i32>), TelegramReadOnlyError> {
        if offset.is_some_and(|value| value < 0) {
            return Err(TelegramReadOnlyError::InvalidCommand);
        }
        let offset = offset
            .map(i32::try_from)
            .transpose()
            .map_err(|_| TelegramReadOnlyError::InvalidCommand)?;
        let mut request = self
            .bot
            .get_updates()
            .limit(MAX_TELEGRAM_UPDATES_PER_POLL)
            .timeout(
                u32::try_from(self.config.long_poll_timeout_seconds())
                    .expect("bounded long-poll timeout fits u32"),
            )
            .allowed_updates([AllowedUpdate::Message]);
        if let Some(offset) = offset {
            request = request.offset(offset);
        }
        let updates = tokio::time::timeout(self.config.request_timeout, request)
            .await
            .map_err(|_| TelegramReadOnlyError::Timeout)?
            .map_err(classify_sdk_error)?;
        if updates.len() > usize::from(MAX_TELEGRAM_UPDATES_PER_POLL)
            || updates.windows(2).any(|pair| pair[0].id >= pair[1].id)
        {
            return Err(TelegramReadOnlyError::InvalidResponse);
        }
        Ok((updates, offset))
    }
}

fn duration_millis(value: Duration) -> Result<u64, TelegramReadOnlyError> {
    u64::try_from(value.as_millis()).map_err(|_| TelegramReadOnlyError::InvalidConfiguration)
}

fn telegram_authorization_hash(
    chat_id: i64,
    user_id: u64,
    update_id: i64,
    action_id: EmergencyActionId,
    issued_at_unix_millis: u64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for value in [
        TELEGRAM_EMERGENCY_VERSION_V1.as_bytes(),
        &chat_id.to_be_bytes(),
        &user_id.to_be_bytes(),
        &update_id.to_be_bytes(),
        action_id.to_string().as_bytes(),
        &issued_at_unix_millis.to_be_bytes(),
    ] {
        hasher.update(value.len().to_be_bytes());
        hasher.update(value);
    }
    hasher.finalize().into()
}

fn parse_optional_limit(arguments: &[&str]) -> Result<u8, TelegramReadOnlyError> {
    match arguments {
        [] => Ok(10),
        [value] => {
            let value = value
                .parse::<u8>()
                .map_err(|_| TelegramReadOnlyError::InvalidLimit)?;
            if value == 0 || value > MAX_TELEGRAM_QUERY_ROWS {
                return Err(TelegramReadOnlyError::InvalidLimit);
            }
            Ok(value)
        }
        _ => Err(TelegramReadOnlyError::InvalidCommand),
    }
}

fn parse_optional_identifier(
    arguments: &[&str],
) -> Result<Option<Box<str>>, TelegramReadOnlyError> {
    match arguments {
        [] => Ok(None),
        [value]
            if !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || byte == b'-') =>
        {
            Ok(Some((*value).into()))
        }
        _ => Err(TelegramReadOnlyError::InvalidCommand),
    }
}

fn validate_limit(limit: u8) -> Result<i64, TelegramReadOnlyError> {
    if limit == 0 || limit > MAX_TELEGRAM_QUERY_ROWS {
        return Err(TelegramReadOnlyError::InvalidLimit);
    }
    Ok(i64::from(limit))
}

fn help_text() -> &'static str {
    "IronPilot read-only commands:\n/status\n/events [1-20]\n/plans [1-20]\n/plan [action_id]\n/validations [1-20]\n/validation [action_id]\n/positions\n/orders [1-20]\n/trades [1-20]\n/authorization\n\nNo strategy or emergency control is exposed."
}

async fn status_text(
    repository: &SqliteRepository,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let state: Option<(String, i64)> =
        sqlx::query_as("SELECT state, updated_at FROM system_state WHERE singleton_id = 1")
            .fetch_optional(&repository.pool)
            .await
            .map_err(storage)?;
    let active_plans: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM trade_plans WHERE state NOT IN ('REJECTED', 'CANCELLED', 'CLOSED')",
    )
    .fetch_one(&repository.pool)
    .await
    .map_err(storage)?;
    let open_orders: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM paper_orders WHERE state NOT IN ('FILLED', 'CANCELLED', 'EXPIRED', 'REJECTED')",
    )
    .fetch_one(&repository.pool)
    .await
    .map_err(storage)?;
    let state_text = state.map_or_else(
        || "not initialized".to_owned(),
        |(state, updated_at)| format!("{state} (updated {updated_at})"),
    );
    TelegramReadOnlyText::new(format!(
        "IronPilot status\nruntime: {state_text}\nactive trade plans: {active_plans}\nopen paper orders: {open_orders}"
    ))
}

async fn events_text(
    repository: &SqliteRepository,
    limit: u8,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let rows = sqlx::query(
        "
        SELECT sequence, occurred_at, category, subject_id
        FROM audit_log
        ORDER BY sequence DESC
        LIMIT ?
        ",
    )
    .bind(validate_limit(limit)?)
    .fetch_all(&repository.pool)
    .await
    .map_err(storage)?;
    let mut text = String::from("Confirmed events");
    for row in rows {
        text.push_str(&format!(
            "\n#{} {} {} {}",
            row.try_get::<i64, _>("sequence").map_err(storage)?,
            row.try_get::<i64, _>("occurred_at").map_err(storage)?,
            row.try_get::<String, _>("category").map_err(storage)?,
            row.try_get::<Option<String>, _>("subject_id")
                .map_err(storage)?
                .unwrap_or_else(|| "-".to_owned())
        ));
    }
    TelegramReadOnlyText::new(text)
}

async fn plans_text(
    repository: &SqliteRepository,
    limit: u8,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let rows = sqlx::query(
        "
        SELECT ledger.action_id, ledger.trade_plan_id, plans.action,
               plans.instrument_id, ledger.recorded_at, plans.plan_hash
        FROM ai_trade_plan_ledger AS ledger
        JOIN ai_trading_plans AS plans ON plans.ai_plan_id = ledger.ai_plan_id
        ORDER BY ledger.recorded_at DESC, ledger.action_id DESC
        LIMIT ?
        ",
    )
    .bind(validate_limit(limit)?)
    .fetch_all(&repository.pool)
    .await
    .map_err(storage)?;
    let mut text = String::from("AI plans");
    for row in rows {
        text.push_str(&format!(
            "\n{} {} {} action={} trade_plan={} hash={}",
            row.try_get::<i64, _>("recorded_at").map_err(storage)?,
            row.try_get::<String, _>("instrument_id").map_err(storage)?,
            row.try_get::<String, _>("action").map_err(storage)?,
            row.try_get::<String, _>("action_id").map_err(storage)?,
            row.try_get::<String, _>("trade_plan_id").map_err(storage)?,
            row.try_get::<String, _>("plan_hash").map_err(storage)?
        ));
    }
    TelegramReadOnlyText::new(text)
}

async fn plan_text(
    repository: &SqliteRepository,
    action_id: Option<&str>,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let row = if let Some(action_id) = action_id {
        sqlx::query(
            "
            SELECT ledger.action_id, ledger.trade_plan_id, plans.action, plans.plan_hash,
                   substr(responses.raw_response, 1, ?) AS raw_response
            FROM ai_trade_plan_ledger AS ledger
            JOIN ai_trading_plans AS plans ON plans.ai_plan_id = ledger.ai_plan_id
            JOIN ai_provider_responses AS responses ON responses.response_id = ledger.response_id
            WHERE ledger.action_id = ?
            ",
        )
        .bind(MAX_STORED_TEXT_BYTES)
        .bind(action_id)
        .fetch_optional(&repository.pool)
        .await
        .map_err(storage)?
    } else {
        sqlx::query(
            "
            SELECT ledger.action_id, ledger.trade_plan_id, plans.action, plans.plan_hash,
                   substr(responses.raw_response, 1, ?) AS raw_response
            FROM ai_trade_plan_ledger AS ledger
            JOIN ai_trading_plans AS plans ON plans.ai_plan_id = ledger.ai_plan_id
            JOIN ai_provider_responses AS responses ON responses.response_id = ledger.response_id
            ORDER BY ledger.recorded_at DESC, ledger.action_id DESC
            LIMIT 1
            ",
        )
        .bind(MAX_STORED_TEXT_BYTES)
        .fetch_optional(&repository.pool)
        .await
        .map_err(storage)?
    };
    let Some(row) = row else {
        return TelegramReadOnlyText::new("AI plan: not found".to_owned());
    };
    TelegramReadOnlyText::new(format!(
        "AI raw plan\naction_id: {}\ntrade_plan_id: {}\naction: {}\nplan_hash: {}\nraw:\n{}",
        row.try_get::<String, _>("action_id").map_err(storage)?,
        row.try_get::<String, _>("trade_plan_id").map_err(storage)?,
        row.try_get::<String, _>("action").map_err(storage)?,
        row.try_get::<String, _>("plan_hash").map_err(storage)?,
        row.try_get::<String, _>("raw_response").map_err(storage)?
    ))
}

async fn validations_text(
    repository: &SqliteRepository,
    limit: u8,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let rows = sqlx::query(
        "
        SELECT action_id, outcome, authorized_maximum_loss_quote,
               recalculated_maximum_loss_quote, validated_at, validation_hash
        FROM execution_validations
        ORDER BY validated_at DESC, action_id DESC
        LIMIT ?
        ",
    )
    .bind(validate_limit(limit)?)
    .fetch_all(&repository.pool)
    .await
    .map_err(storage)?;
    let mut text = String::from("Validation results");
    for row in rows {
        text.push_str(&format!(
            "\n{} {} action={} authorized={} recalculated={} hash={}",
            row.try_get::<i64, _>("validated_at").map_err(storage)?,
            row.try_get::<String, _>("outcome").map_err(storage)?,
            row.try_get::<String, _>("action_id").map_err(storage)?,
            row.try_get::<String, _>("authorized_maximum_loss_quote")
                .map_err(storage)?,
            row.try_get::<Option<String>, _>("recalculated_maximum_loss_quote")
                .map_err(storage)?
                .unwrap_or_else(|| "-".to_owned()),
            row.try_get::<String, _>("validation_hash")
                .map_err(storage)?
        ));
    }
    TelegramReadOnlyText::new(text)
}

async fn validation_text(
    repository: &SqliteRepository,
    action_id: Option<&str>,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let row = if let Some(action_id) = action_id {
        sqlx::query(
            "
            SELECT action_id, outcome, authorized_maximum_loss_quote,
                   recalculated_maximum_loss_quote, validation_hash,
                   substr(evidence_json, 1, ?) AS evidence_json
            FROM execution_validations
            WHERE action_id = ?
            ",
        )
        .bind(MAX_STORED_TEXT_BYTES)
        .bind(action_id)
        .fetch_optional(&repository.pool)
        .await
        .map_err(storage)?
    } else {
        sqlx::query(
            "
            SELECT action_id, outcome, authorized_maximum_loss_quote,
                   recalculated_maximum_loss_quote, validation_hash,
                   substr(evidence_json, 1, ?) AS evidence_json
            FROM execution_validations
            ORDER BY validated_at DESC, action_id DESC
            LIMIT 1
            ",
        )
        .bind(MAX_STORED_TEXT_BYTES)
        .fetch_optional(&repository.pool)
        .await
        .map_err(storage)?
    };
    let Some(row) = row else {
        return TelegramReadOnlyText::new("Validation: not found".to_owned());
    };
    TelegramReadOnlyText::new(format!(
        "Validation detail\naction_id: {}\noutcome: {}\nauthorized maximum loss: {}\nrecalculated maximum loss: {}\nvalidation_hash: {}\nevidence / rejection reasons:\n{}",
        row.try_get::<String, _>("action_id").map_err(storage)?,
        row.try_get::<String, _>("outcome").map_err(storage)?,
        row.try_get::<String, _>("authorized_maximum_loss_quote")
            .map_err(storage)?,
        row.try_get::<Option<String>, _>("recalculated_maximum_loss_quote")
            .map_err(storage)?
            .unwrap_or_else(|| "-".to_owned()),
        row.try_get::<String, _>("validation_hash")
            .map_err(storage)?,
        row.try_get::<String, _>("evidence_json").map_err(storage)?
    ))
}

async fn positions_text(
    repository: &SqliteRepository,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let rows = sqlx::query(
        "
        SELECT trade_plan_id, instrument_id,
               json_extract(payload_json, '$.base_asset') AS base_asset,
               json_extract(payload_json, '$.remaining_quantity') AS remaining_quantity
        FROM managed_lots
        WHERE closed_at IS NULL
        ORDER BY instrument_id, trade_plan_id, opened_at, managed_lot_id
        LIMIT 101
        ",
    )
    .fetch_all(&repository.pool)
    .await
    .map_err(storage)?;
    if rows.len() > 100 {
        return Err(TelegramReadOnlyError::ReadModelTooLarge);
    }
    let mut positions: BTreeMap<(String, String, String), DomainDecimal> = BTreeMap::new();
    for row in rows {
        let key = (
            row.try_get("instrument_id").map_err(storage)?,
            row.try_get("trade_plan_id").map_err(storage)?,
            row.try_get("base_asset").map_err(storage)?,
        );
        let quantity = DomainDecimal::from_str(
            &row.try_get::<String, _>("remaining_quantity")
                .map_err(storage)?,
        )
        .map_err(|_| TelegramReadOnlyError::InvalidStoredData)?;
        let current = positions.get(&key).copied().unwrap_or(DomainDecimal::ZERO);
        positions.insert(
            key,
            current
                .checked_add(quantity)
                .ok_or(TelegramReadOnlyError::InvalidStoredData)?,
        );
    }
    let mut text = String::from("Managed positions");
    for ((instrument, trade_plan, base_asset), quantity) in positions {
        text.push_str(&format!(
            "\n{instrument} {base_asset}={} trade_plan={trade_plan}",
            quantity.as_decimal().normalize()
        ));
    }
    TelegramReadOnlyText::new(text)
}

async fn orders_text(
    repository: &SqliteRepository,
    limit: u8,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let rows = sqlx::query(
        "
        SELECT orders.order_id, orders.state, orders.updated_at,
               specs.instrument_id, specs.role, specs.side, specs.order_type,
               specs.quantity, specs.limit_price, specs.trigger_price,
               specs.filled_quantity
        FROM paper_orders AS orders
        JOIN paper_order_specs AS specs ON specs.order_id = orders.order_id
        ORDER BY orders.updated_at DESC, orders.order_id DESC
        LIMIT ?
        ",
    )
    .bind(validate_limit(limit)?)
    .fetch_all(&repository.pool)
    .await
    .map_err(storage)?;
    let mut text = String::from("Paper orders");
    for row in rows {
        text.push_str(&format!(
            "\n{} {} {} {} {} {} qty={} filled={} limit={} trigger={} id={}",
            row.try_get::<i64, _>("updated_at").map_err(storage)?,
            row.try_get::<String, _>("instrument_id").map_err(storage)?,
            row.try_get::<String, _>("role").map_err(storage)?,
            row.try_get::<String, _>("side").map_err(storage)?,
            row.try_get::<String, _>("order_type").map_err(storage)?,
            row.try_get::<String, _>("state").map_err(storage)?,
            row.try_get::<Option<String>, _>("quantity")
                .map_err(storage)?
                .unwrap_or_else(|| "-".to_owned()),
            row.try_get::<String, _>("filled_quantity")
                .map_err(storage)?,
            row.try_get::<Option<String>, _>("limit_price")
                .map_err(storage)?
                .unwrap_or_else(|| "-".to_owned()),
            row.try_get::<Option<String>, _>("trigger_price")
                .map_err(storage)?
                .unwrap_or_else(|| "-".to_owned()),
            row.try_get::<String, _>("order_id").map_err(storage)?
        ));
    }
    TelegramReadOnlyText::new(text)
}

async fn trades_text(
    repository: &SqliteRepository,
    limit: u8,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let rows = sqlx::query(
        "
        SELECT fills.fill_id, fills.occurred_at, specs.instrument_id,
               specs.role, specs.side,
               substr(fills.payload_json, 1, ?) AS payload_json
        FROM fills
        JOIN paper_order_specs AS specs ON specs.order_id = fills.order_id
        ORDER BY fills.occurred_at DESC, fills.fill_id DESC
        LIMIT ?
        ",
    )
    .bind(MAX_STORED_TEXT_BYTES)
    .bind(validate_limit(limit)?)
    .fetch_all(&repository.pool)
    .await
    .map_err(storage)?;
    let mut text = String::from("Paper trades");
    for row in rows {
        text.push_str(&format!(
            "\n{} {} {} {} fill={} facts={}",
            row.try_get::<i64, _>("occurred_at").map_err(storage)?,
            row.try_get::<String, _>("instrument_id").map_err(storage)?,
            row.try_get::<String, _>("role").map_err(storage)?,
            row.try_get::<String, _>("side").map_err(storage)?,
            row.try_get::<String, _>("fill_id").map_err(storage)?,
            row.try_get::<String, _>("payload_json").map_err(storage)?
        ));
    }
    TelegramReadOnlyText::new(text)
}

async fn authorization_text(
    repository: &SqliteRepository,
) -> Result<TelegramReadOnlyText, TelegramReadOnlyError> {
    let context = sqlx::query(
        "
        SELECT instrument_id, as_of, valid_until, maximum_loss_quote, context_hash
        FROM ai_decision_contexts
        ORDER BY as_of DESC, context_id DESC
        LIMIT 1
        ",
    )
    .fetch_optional(&repository.pool)
    .await
    .map_err(storage)?;
    let validation = sqlx::query(
        "
        SELECT outcome, authorized_maximum_loss_quote,
               substr(evidence_json, 1, ?) AS evidence_json
        FROM execution_validations
        ORDER BY validated_at DESC, action_id DESC
        LIMIT 1
        ",
    )
    .bind(MAX_STORED_TEXT_BYTES)
    .fetch_optional(&repository.pool)
    .await
    .map_err(storage)?;
    let context_text = if let Some(row) = context {
        format!(
            "instrument: {}\nContext as_of/valid_until: {}/{}\nuser maximum loss quote: {}\ncontext_hash: {}",
            row.try_get::<String, _>("instrument_id").map_err(storage)?,
            row.try_get::<i64, _>("as_of").map_err(storage)?,
            row.try_get::<i64, _>("valid_until").map_err(storage)?,
            row.try_get::<String, _>("maximum_loss_quote")
                .map_err(storage)?,
            row.try_get::<String, _>("context_hash").map_err(storage)?
        )
    } else {
        "No Decision Context authorization recorded.".to_owned()
    };
    let validation_text = if let Some(row) = validation {
        format!(
            "\nlatest validation: {}\nauthorized maximum loss quote: {}\nrejection evidence: {}",
            row.try_get::<String, _>("outcome").map_err(storage)?,
            row.try_get::<String, _>("authorized_maximum_loss_quote")
                .map_err(storage)?,
            row.try_get::<String, _>("evidence_json").map_err(storage)?
        )
    } else {
        "\nNo Validation recorded.".to_owned()
    };
    TelegramReadOnlyText::new(format!(
        "User authorization\n{context_text}{validation_text}"
    ))
}

fn sanitize_and_bound_text(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character == '\n' || character == '\t' || !character.is_control() {
                character
            } else {
                '\u{fffd}'
            }
        })
        .collect::<String>();
    if sanitized.chars().count() <= MAX_TELEGRAM_MESSAGE_CHARS {
        return sanitized;
    }
    let suffix = "\n[truncated]";
    let keep = MAX_TELEGRAM_MESSAGE_CHARS.saturating_sub(suffix.chars().count());
    sanitized.chars().take(keep).chain(suffix.chars()).collect()
}

fn validate_bot_token(token: &str) -> Result<(), TelegramReadOnlyError> {
    let Some((bot_id, secret)) = token.split_once(':') else {
        return Err(TelegramReadOnlyError::InvalidConfiguration);
    };
    if bot_id.is_empty()
        || !bot_id.bytes().all(|byte| byte.is_ascii_digit())
        || !(20..=256).contains(&secret.len())
        || !secret
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(TelegramReadOnlyError::InvalidConfiguration);
    }
    Ok(())
}

fn classify_sdk_error(error: RequestError) -> TelegramReadOnlyError {
    match error {
        RequestError::Network(error) if error.is_timeout() => TelegramReadOnlyError::Timeout,
        RequestError::Network(_) | RequestError::Io(_) => TelegramReadOnlyError::Transport,
        RequestError::InvalidJson { .. } => TelegramReadOnlyError::InvalidResponse,
        RequestError::Api(_) | RequestError::MigrateToChatId(_) | RequestError::RetryAfter(_) => {
            TelegramReadOnlyError::RemoteRejected
        }
    }
}

fn storage(error: sqlx::Error) -> TelegramReadOnlyError {
    TelegramReadOnlyError::Storage(StorageError::from(error))
}

#[derive(Debug)]
pub enum TelegramReadOnlyError {
    InvalidConfiguration,
    InvalidCommand,
    InvalidLimit,
    InvalidStoredData,
    ReadModelTooLarge,
    Timeout,
    Transport,
    InvalidResponse,
    RemoteRejected,
    EmergencyCapacityExceeded,
    Storage(StorageError),
}

impl fmt::Display for TelegramReadOnlyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => {
                formatter.write_str("Telegram read-only configuration is invalid")
            }
            Self::InvalidCommand => formatter.write_str("Telegram read-only command is invalid"),
            Self::InvalidLimit => formatter.write_str("Telegram read-only limit is invalid"),
            Self::InvalidStoredData => {
                formatter.write_str("Telegram read model contains invalid stored data")
            }
            Self::ReadModelTooLarge => {
                formatter.write_str("Telegram read model exceeds its bounded size")
            }
            Self::Timeout => formatter.write_str("Telegram Bot API request timed out"),
            Self::Transport => formatter.write_str("Telegram Bot API transport failed"),
            Self::InvalidResponse => formatter.write_str("Telegram Bot API response is invalid"),
            Self::RemoteRejected => formatter.write_str("Telegram Bot API rejected the request"),
            Self::EmergencyCapacityExceeded => {
                formatter.write_str("Telegram emergency confirmation capacity is exhausted")
            }
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TelegramReadOnlyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::{Value, json};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    const TOKEN: &str = concat!(
        "123456789",
        ":",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ_",
        "1234567890"
    );

    #[test]
    fn configuration_commands_and_message_bounds_are_strictly_read_only() {
        assert!(TelegramReadOnlyConfig::new(TOKEN, vec![1], Duration::from_secs(2)).is_ok());
        assert!(TelegramReadOnlyConfig::new(TOKEN, vec![1, 1], Duration::from_secs(2)).is_err());
        assert!(
            TelegramReadOnlyConfig::with_base_url(
                TOKEN,
                vec![1],
                Duration::from_secs(2),
                "http://127.0.0.1:1234/",
                false
            )
            .is_err()
        );
        assert!(matches!(
            TelegramReadOnlyCommand::parse("/events 20"),
            Ok(Some(TelegramReadOnlyCommand::Events { limit: 20 }))
        ));
        assert!(matches!(
            TelegramReadOnlyCommand::parse("/plan 00000000-0000-0000-0000-000000000001"),
            Ok(Some(TelegramReadOnlyCommand::Plan { action_id: Some(_) }))
        ));
        assert_eq!(
            TelegramReadOnlyCommand::parse("/pause").expect("command should parse"),
            Some(TelegramReadOnlyCommand::Unsupported)
        );
        assert_eq!(
            TelegramReadOnlyCommand::parse("/emergency_close_all").expect("command should parse"),
            Some(TelegramReadOnlyCommand::Unsupported)
        );
        assert!(TelegramReadOnlyCommand::parse("/events 21").is_err());
        assert!(TelegramReadOnlyCommand::parse("/status now").is_err());
        assert_eq!(
            TelegramReadOnlyCommand::parse("ordinary text").expect("text should parse"),
            None
        );
        let oversized = "x".repeat(MAX_TELEGRAM_MESSAGE_CHARS + 100);
        let text = TelegramReadOnlyText::new(oversized).expect("text should truncate");
        assert_eq!(text.as_str().chars().count(), MAX_TELEGRAM_MESSAGE_CHARS);
        assert!(text.as_str().ends_with("[truncated]"));
        let error = TelegramReadOnlyConfig::new(
            "secret-token-that-must-not-appear",
            vec![1],
            Duration::from_secs(2),
        )
        .err()
        .expect("invalid token should fail");
        assert!(!error.to_string().contains("secret-token"));
    }

    #[tokio::test]
    async fn emergency_confirmation_is_identity_bound_one_time_and_ttl_bounded() {
        assert!(
            TelegramEmergencyPolicy::new(vec![7], Duration::from_secs(60), Duration::from_secs(60))
                .is_ok()
        );
        assert!(
            TelegramEmergencyPolicy::new(
                vec![7, 7],
                Duration::from_secs(60),
                Duration::from_secs(60)
            )
            .is_err()
        );
        assert!(matches!(
            TelegramEmergencyCommand::parse("/emergency_close_all"),
            Ok(Some(TelegramEmergencyCommand::BeginCloseAll))
        ));
        assert!(
            TelegramEmergencyCommand::parse("/confirm_emergency_close_all not-a-nonce").is_err()
        );

        let session = TelegramEmergencySession::new(
            TelegramEmergencyPolicy::new(vec![7], Duration::from_secs(60), Duration::from_secs(60))
                .expect("policy"),
        );
        let challenge = session
            .handle(1, 7, 10, TelegramEmergencyCommand::BeginCloseAll, 1_000)
            .await
            .expect("challenge");
        let nonce = challenge
            .reply
            .as_str()
            .split_whitespace()
            .last()
            .expect("nonce")
            .to_owned();
        assert_eq!(nonce.len(), 32);
        assert!(challenge.authorized_command.is_none());

        let wrong_user = session
            .handle(
                1,
                8,
                11,
                TelegramEmergencyCommand::ConfirmCloseAll {
                    nonce: nonce.clone().into_boxed_str(),
                },
                1_500,
            )
            .await
            .expect("wrong user rejection");
        assert!(wrong_user.authorized_command.is_none());
        let confirmed = session
            .handle(
                1,
                7,
                12,
                TelegramEmergencyCommand::ConfirmCloseAll {
                    nonce: nonce.into_boxed_str(),
                },
                1_500,
            )
            .await
            .expect("confirmation");
        let command = confirmed.authorized_command.expect("authorized command");
        assert_eq!(
            command.kind(),
            EmergencyCommandKind::CloseAllManagedExposure
        );
        assert_eq!(command.authorization_subject(), "telegram:user:7:chat:1");
        assert!(command.is_valid_at(1_500));
        let replay = session
            .handle(
                1,
                7,
                13,
                TelegramEmergencyCommand::ConfirmCloseAll {
                    nonce: "00000000000000000000000000000000".into(),
                },
                1_501,
            )
            .await
            .expect("replay rejection");
        assert!(replay.authorized_command.is_none());

        let second = session
            .handle(1, 7, 14, TelegramEmergencyCommand::BeginCloseAll, 2_000)
            .await
            .expect("second challenge");
        let second_nonce = second
            .reply
            .as_str()
            .split_whitespace()
            .last()
            .expect("second nonce");
        let wrong_nonce = session
            .handle(
                1,
                7,
                15,
                TelegramEmergencyCommand::ConfirmCloseAll {
                    nonce: "ffffffffffffffffffffffffffffffff".into(),
                },
                2_001,
            )
            .await
            .expect("wrong nonce");
        assert!(wrong_nonce.authorized_command.is_none());
        let invalidated = session
            .handle(
                1,
                7,
                16,
                TelegramEmergencyCommand::ConfirmCloseAll {
                    nonce: second_nonce.to_owned().into_boxed_str(),
                },
                2_002,
            )
            .await
            .expect("challenge must be consumed");
        assert!(invalidated.authorized_command.is_none());
    }

    #[tokio::test]
    async fn sdk_poll_constructs_only_confirmed_commands_without_business_writes() {
        let (repository, temp_path) = fixture().await;
        let session = TelegramEmergencySession::new(
            TelegramEmergencyPolicy::new(vec![7], Duration::from_secs(60), Duration::from_secs(60))
                .expect("policy"),
        );
        let challenge = session
            .handle(1, 7, 10, TelegramEmergencyCommand::BeginCloseAll, 1_000)
            .await
            .expect("challenge");
        let nonce = challenge
            .reply
            .as_str()
            .split_whitespace()
            .last()
            .expect("nonce");
        let updates = json!({
            "ok": true,
            "result": [
                telegram_update_from(
                    11,
                    1,
                    7,
                    &format!("/confirm_emergency_close_all {nonce}")
                ),
                telegram_update_from(12, 1, 8, "/emergency_close_all"),
                telegram_update_from(13, 99, 7, "/emergency_close_all"),
                telegram_update_from(14, 1, 7, "/help"),
                telegram_update(15, 1, "/emergency_close_all")
            ]
        })
        .to_string();
        let send_ok = json!({"ok": true, "result": telegram_message(1, 1, "sent")}).to_string();
        let (base_url, server) = spawn_http_server(vec![
            updates,
            send_ok.clone(),
            send_ok.clone(),
            send_ok.clone(),
            send_ok,
        ])
        .await;
        let adapter = TelegramReadOnlyAdapter::new(
            TelegramReadOnlyConfig::with_base_url(
                TOKEN,
                vec![1],
                Duration::from_secs(2),
                &base_url,
                true,
            )
            .expect("config"),
        )
        .expect("adapter");
        let before = business_counts(&repository).await;
        let report = adapter
            .poll_once_with_emergency(&repository, &session, Some(11), 1_500)
            .await
            .expect("emergency poll");
        assert_eq!(report.next_offset(), Some(16));
        assert_eq!(report.received_updates(), 5);
        assert_eq!(report.emergency_attempts(), 3);
        assert_eq!(report.replies_sent(), 4);
        assert_eq!(report.authorized_commands().len(), 1);
        assert_eq!(
            report.authorized_commands()[0].authorization_subject(),
            "telegram:user:7:chat:1"
        );
        assert_eq!(
            business_counts(&repository).await,
            before,
            "Telegram confirmation may only construct a command"
        );

        let requests = server.await.expect("server");
        assert_eq!(requests.len(), 5);
        assert!(requests[0].contains(&format!("/bot{TOKEN}/GetUpdates")));
        let texts = requests[1..]
            .iter()
            .map(|request| request_json(request)["text"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(
            texts
                .iter()
                .any(|text| text.contains("confirmation accepted"))
        );
        assert!(
            texts
                .iter()
                .any(|text| text.contains("authorization rejected"))
        );
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.contains("authorization rejected"))
                .count(),
            2
        );
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Protected emergency control"))
        );
        for request in &requests[1..] {
            assert_eq!(request_json(request)["protect_content"], true);
        }
        repository.close().await;
        std::fs::remove_dir_all(temp_path).expect("remove Telegram test directory");
    }

    #[tokio::test]
    async fn every_required_read_model_is_bounded_and_performs_zero_business_writes() {
        let (repository, temp_path) = fixture().await;
        seed_read_model(&repository).await;
        let config = TelegramReadOnlyConfig::new(TOKEN, vec![1], Duration::from_secs(2))
            .expect("config should be valid");
        let adapter = TelegramReadOnlyAdapter::new(config).expect("adapter should build");
        let before = business_counts(&repository).await;
        let cases = [
            (TelegramReadOnlyCommand::Status, "ENTRY_ENABLED"),
            (
                TelegramReadOnlyCommand::Events { limit: 10 },
                "CONFIRMED_TEST_EVENT",
            ),
            (TelegramReadOnlyCommand::Plans { limit: 10 }, "OPEN_LONG"),
            (
                TelegramReadOnlyCommand::Plan { action_id: None },
                "raw-plan-marker",
            ),
            (TelegramReadOnlyCommand::Validations { limit: 10 }, "REJECT"),
            (
                TelegramReadOnlyCommand::Validation { action_id: None },
                "OVER_AUTHORIZATION",
            ),
            (TelegramReadOnlyCommand::Positions, "BTC=0.1"),
            (
                TelegramReadOnlyCommand::Orders { limit: 10 },
                "ENTRY BUY LIMIT",
            ),
            (
                TelegramReadOnlyCommand::Trades { limit: 10 },
                "execution_price",
            ),
            (
                TelegramReadOnlyCommand::Authorization,
                "user maximum loss quote: 25",
            ),
        ];
        for (command, expected) in cases {
            let text = adapter
                .execute_command(&repository, &command)
                .await
                .expect("read-only query should succeed");
            assert!(
                text.as_str().contains(expected),
                "response did not contain {expected}: {}",
                text.as_str()
            );
            assert!(text.as_str().chars().count() <= MAX_TELEGRAM_MESSAGE_CHARS);
        }
        assert_eq!(
            business_counts(&repository).await,
            before,
            "Telegram queries must not mutate trading or audit state"
        );
        repository.close().await;
        std::fs::remove_dir_all(temp_path).expect("remove Telegram test directory");
    }

    #[tokio::test]
    async fn polling_and_confirmed_notifications_use_allowlists_offsets_and_no_controls() {
        let (repository, temp_path) = fixture().await;
        seed_read_model(&repository).await;
        let updates = json!({
            "ok": true,
            "result": [
                telegram_update(10, 1, "/status"),
                telegram_update(11, 99, "/orders"),
                telegram_update(12, 1, "/emergency"),
                telegram_update(13, 1, "hello")
            ]
        })
        .to_string();
        let send_ok = json!({"ok": true, "result": telegram_message(1, 1, "sent")}).to_string();
        let (base_url, server) = spawn_http_server(vec![
            updates,
            send_ok.clone(),
            send_ok.clone(),
            send_ok.clone(),
            send_ok,
        ])
        .await;
        let config = TelegramReadOnlyConfig::with_base_url(
            TOKEN,
            vec![1, 2],
            Duration::from_secs(2),
            &base_url,
            true,
        )
        .expect("test config should be valid");
        let adapter = TelegramReadOnlyAdapter::new(config).expect("adapter should build");
        let before = business_counts(&repository).await;
        let poll = adapter
            .poll_once(&repository, Some(10))
            .await
            .expect("poll should succeed");
        assert_eq!(poll.next_offset(), Some(14));
        assert_eq!(poll.received_updates(), 4);
        assert_eq!(poll.authorized_commands(), 2);
        assert_eq!(poll.replies_sent(), 2);
        let notifications = adapter
            .notify_confirmed_events(&repository, 0, 10)
            .await
            .expect("notification batch should succeed");
        assert_eq!(notifications.next_audit_sequence(), 1);
        assert_eq!(notifications.confirmed_events(), 1);
        assert_eq!(notifications.messages_sent(), 2);
        assert_eq!(business_counts(&repository).await, before);

        let requests = server.await.expect("mock Telegram server should finish");
        assert_eq!(requests.len(), 5);
        assert!(
            requests[0].contains(&format!("/bot{TOKEN}/GetUpdates")),
            "{}",
            requests[0]
        );
        let poll_body = request_json(&requests[0]);
        assert_eq!(poll_body["offset"], 10);
        assert_eq!(poll_body["limit"], MAX_TELEGRAM_UPDATES_PER_POLL);
        assert_eq!(poll_body["timeout"], 1);
        assert_eq!(poll_body["allowed_updates"], json!(["message"]));
        let reply_texts = requests[1..]
            .iter()
            .map(|request| request_json(request)["text"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(
            reply_texts
                .iter()
                .any(|text| text.contains("IronPilot status"))
        );
        assert!(
            reply_texts
                .iter()
                .any(|text| text.contains("read-only") && text.contains("unavailable"))
        );
        assert_eq!(
            reply_texts
                .iter()
                .filter(|text| text.contains("Confirmed event"))
                .count(),
            2
        );
        for request in &requests[1..] {
            let body = request_json(request);
            assert_eq!(body["protect_content"], true);
        }
        repository.close().await;
        std::fs::remove_dir_all(temp_path).expect("remove Telegram test directory");
    }

    #[tokio::test]
    async fn invalid_and_rejected_sdk_responses_fail_closed_without_secret_disclosure() {
        let (repository, temp_path) = fixture().await;
        let (invalid_url, invalid_server) = spawn_http_server(vec!["not-json".to_owned()]).await;
        let invalid = TelegramReadOnlyAdapter::new(
            TelegramReadOnlyConfig::with_base_url(
                TOKEN,
                vec![1],
                Duration::from_secs(2),
                &invalid_url,
                true,
            )
            .expect("test config should be valid"),
        )
        .expect("adapter should build")
        .poll_once(&repository, None)
        .await
        .expect_err("invalid SDK response must fail");
        assert!(matches!(invalid, TelegramReadOnlyError::InvalidResponse));
        assert!(!invalid.to_string().contains(TOKEN));
        let _ = invalid_server
            .await
            .expect("invalid response mock server should finish");

        let (rejected_url, rejected_server) = spawn_http_server(vec![
            json!({
                "ok": false,
                "error_code": 403,
                "description": "Forbidden: bot was blocked by the user"
            })
            .to_string(),
        ])
        .await;
        let rejected = TelegramReadOnlyAdapter::new(
            TelegramReadOnlyConfig::with_base_url(
                TOKEN,
                vec![1],
                Duration::from_secs(2),
                &rejected_url,
                true,
            )
            .expect("test config should be valid"),
        )
        .expect("adapter should build")
        .poll_once(&repository, None)
        .await
        .expect_err("remote rejection must fail");
        assert!(matches!(rejected, TelegramReadOnlyError::RemoteRejected));
        assert!(!rejected.to_string().contains(TOKEN));
        let _ = rejected_server
            .await
            .expect("rejected mock server should finish");
        repository.close().await;
        std::fs::remove_dir_all(temp_path).expect("remove Telegram test directory");
    }

    async fn fixture() -> (SqliteRepository, PathBuf) {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::AcqRel);
        let temp_path = std::env::temp_dir().join(format!(
            "ironpilot-p3-07a-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp_path).expect("create Telegram test directory");
        let repository = SqliteRepository::connect(temp_path.join("telegram.sqlite3"), 1)
            .await
            .expect("open Telegram test database");
        (repository, temp_path)
    }

    async fn seed_read_model(repository: &SqliteRepository) {
        let context_id = id(1);
        let response_id = id(2);
        let ai_plan_id = id(3);
        let trade_plan_id = id(4);
        let action_id = id(5);
        let order_intent_id = id(6);
        let order_id = id(7);
        sqlx::query(
            "INSERT INTO system_state(singleton_id, state, updated_at) VALUES (1, 'ENTRY_ENABLED', 100)",
        )
        .execute(&repository.pool)
        .await
        .expect("seed state");
        sqlx::query(
            "INSERT INTO audit_log(audit_entry_id, occurred_at, category, subject_id, payload_json) VALUES (?, 101, 'CONFIRMED_TEST_EVENT', ?, '{\"confirmed\":true}')",
        )
        .bind(id(20))
        .bind(&action_id)
        .execute(&repository.pool)
        .await
        .expect("seed audit");
        sqlx::query(
            "INSERT INTO ai_decision_contexts(context_id, schema_version, instrument_id, as_of, valid_until, maximum_loss_quote, context_hash, payload_json) VALUES (?, '1.0', 'bybit:spot:BTCUSDT', 100, 200, '25', 'context-hash', '{}')",
        )
        .bind(&context_id)
        .execute(&repository.pool)
        .await
        .expect("seed Context");
        sqlx::query(
            "INSERT INTO ai_provider_responses(response_id, context_id, provider, model, received_at, response_hash, raw_response) VALUES (?, ?, 'recorded', 'stub', 110, 'response-hash', '{\"raw\":\"raw-plan-marker\"}')",
        )
        .bind(&response_id)
        .bind(&context_id)
        .execute(&repository.pool)
        .await
        .expect("seed provider response");
        sqlx::query(
            "INSERT INTO ai_trading_plans(ai_plan_id, context_id, response_id, schema_version, instrument_id, action, created_at, valid_until, plan_hash, payload_json) VALUES (?, ?, ?, '3.0', 'bybit:spot:BTCUSDT', 'OPEN_LONG', 110, 200, 'plan-hash', '{}')",
        )
        .bind(&ai_plan_id)
        .bind(&context_id)
        .bind(&response_id)
        .execute(&repository.pool)
        .await
        .expect("seed AI plan");
        sqlx::query(
            "INSERT INTO trade_plans(trade_plan_id, instrument_id, state, created_at, updated_at, payload_json) VALUES (?, 'bybit:spot:BTCUSDT', 'ACTIVE', 110, 130, '{}')",
        )
        .bind(&trade_plan_id)
        .execute(&repository.pool)
        .await
        .expect("seed TradePlan");
        sqlx::query(
            "INSERT INTO trade_plan_actions(action_id, trade_plan_id, action_type, state, created_at, expires_at, payload_json) VALUES (?, ?, 'OPEN_LONG', 'VALIDATION_REJECTED', 110, 200, '{}')",
        )
        .bind(&action_id)
        .bind(&trade_plan_id)
        .execute(&repository.pool)
        .await
        .expect("seed action");
        sqlx::query(
            "INSERT INTO ai_trade_plan_ledger(action_id, trade_plan_id, context_id, response_id, ai_plan_id, context_hash, response_hash, plan_hash, recorded_at) VALUES (?, ?, ?, ?, ?, 'context-hash', 'response-hash', 'plan-hash', 110)",
        )
        .bind(&action_id)
        .bind(&trade_plan_id)
        .bind(&context_id)
        .bind(&response_id)
        .bind(&ai_plan_id)
        .execute(&repository.pool)
        .await
        .expect("seed ledger");
        sqlx::query(
            "INSERT INTO execution_validations(action_id, trade_plan_id, ai_plan_id, validator_version, outcome, context_hash, plan_hash, recalculated_maximum_loss_quote, authorized_maximum_loss_quote, validated_at, validation_hash, evidence_json) VALUES (?, ?, ?, 'validator-v1', 'REJECT', 'context-hash', 'plan-hash', '30', '25', 120, 'validation-hash', '{\"rejections\":[{\"code\":\"OVER_AUTHORIZATION\"}]}')",
        )
        .bind(&action_id)
        .bind(&trade_plan_id)
        .bind(&ai_plan_id)
        .execute(&repository.pool)
        .await
        .expect("seed validation");
        sqlx::query(
            "INSERT INTO paper_execution_submissions(action_id, trade_plan_id, venue, command, validation_hash, source_plan_hash, request_hash, created_at, payload_json) VALUES (?, ?, 'PAPER', 'OPEN_LONG', 'validation-hash', 'plan-hash', 'request-hash', 121, '{}')",
        )
        .bind(&action_id)
        .bind(&trade_plan_id)
        .execute(&repository.pool)
        .await
        .expect("seed submission");
        sqlx::query(
            "INSERT INTO order_intents(order_intent_id, action_id, state, created_at, payload_json) VALUES (?, ?, 'SUBMITTED', 121, '{}')",
        )
        .bind(&order_intent_id)
        .bind(&action_id)
        .execute(&repository.pool)
        .await
        .expect("seed intent");
        sqlx::query(
            "INSERT INTO paper_orders(order_id, order_intent_id, state, created_at, updated_at, payload_json) VALUES (?, ?, 'PARTIALLY_FILLED', 121, 130, '{}')",
        )
        .bind(&order_id)
        .bind(&order_intent_id)
        .execute(&repository.pool)
        .await
        .expect("seed order");
        sqlx::query(
            "INSERT INTO paper_order_specs(order_id, action_id, trade_plan_id, instrument_id, role, take_profit_index, side, order_type, quantity, limit_price, trigger_price, time_in_force, expires_at, max_slippage_quote, decision_as_of, submitted_at, filled_quantity, accumulated_quote, accumulated_fee_quote) VALUES (?, ?, ?, 'bybit:spot:BTCUSDT', 'ENTRY', NULL, 'BUY', 'LIMIT', '0.1', '210', NULL, 'GTC', 200, '1', 100, 121, '0.1', '21', '0.01')",
        )
        .bind(&order_id)
        .bind(&action_id)
        .bind(&trade_plan_id)
        .execute(&repository.pool)
        .await
        .expect("seed order spec");
        sqlx::query(
            "INSERT INTO fills(fill_id, order_id, occurred_at, payload_json) VALUES (?, ?, 130, '{\"execution_price\":\"210\",\"fee_quote\":\"0.01\"}')",
        )
        .bind(id(8))
        .bind(&order_id)
        .execute(&repository.pool)
        .await
        .expect("seed fill");
        sqlx::query(
            "INSERT INTO managed_lots(managed_lot_id, trade_plan_id, instrument_id, opened_at, closed_at, payload_json) VALUES (?, ?, 'bybit:spot:BTCUSDT', 130, NULL, '{\"schema_version\":\"ironpilot-managed-lot-v1\",\"base_asset\":\"BTC\",\"initial_quantity\":\"0.1\",\"remaining_quantity\":\"0.1\",\"source_fill_id\":\"source\"}')",
        )
        .bind(id(9))
        .bind(&trade_plan_id)
        .execute(&repository.pool)
        .await
        .expect("seed managed lot");
    }

    async fn business_counts(repository: &SqliteRepository) -> (i64, i64, i64, i64, i64) {
        (
            sqlx::query_scalar("SELECT COUNT(*) FROM audit_log")
                .fetch_one(&repository.pool)
                .await
                .expect("audit count"),
            sqlx::query_scalar("SELECT COUNT(*) FROM trade_plan_actions")
                .fetch_one(&repository.pool)
                .await
                .expect("action count"),
            sqlx::query_scalar("SELECT COUNT(*) FROM order_intents")
                .fetch_one(&repository.pool)
                .await
                .expect("intent count"),
            sqlx::query_scalar("SELECT COUNT(*) FROM fills")
                .fetch_one(&repository.pool)
                .await
                .expect("fill count"),
            sqlx::query_scalar("SELECT COUNT(*) FROM managed_lots")
                .fetch_one(&repository.pool)
                .await
                .expect("lot count"),
        )
    }

    fn id(value: u128) -> String {
        format!("{value:032x}")
    }

    fn telegram_update(update_id: u32, chat_id: i64, text: &str) -> Value {
        json!({
            "update_id": update_id,
            "message": telegram_message(i32::try_from(update_id).expect("fixture ID fits i32"), chat_id, text)
        })
    }

    fn telegram_update_from(update_id: u32, chat_id: i64, user_id: u64, text: &str) -> Value {
        let mut message = telegram_message(
            i32::try_from(update_id).expect("fixture ID fits i32"),
            chat_id,
            text,
        );
        message["from"] = json!({
            "id": user_id,
            "is_bot": false,
            "first_name": "Emergency Operator"
        });
        json!({
            "update_id": update_id,
            "message": message
        })
    }

    fn telegram_message(message_id: i32, chat_id: i64, text: &str) -> Value {
        json!({
            "message_id": message_id,
            "date": 1,
            "chat": {
                "id": chat_id,
                "type": "private",
                "first_name": "IronPilot Test"
            },
            "text": text
        })
    }

    async fn spawn_http_server(
        responses: Vec<String>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock Telegram server");
        let address = listener.local_addr().expect("mock server address");
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response_body in responses {
                let (mut stream, _) = listener.accept().await.expect("accept request");
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 2_048];
                let header_end = loop {
                    let read = stream.read(&mut buffer).await.expect("read request");
                    assert!(read > 0, "request closed before headers");
                    bytes.extend_from_slice(&buffer[..read]);
                    if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        break index + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..header_end].to_vec())
                    .expect("request headers should be UTF-8");
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .map(str::parse::<usize>)
                    })
                    .transpose()
                    .expect("content length should parse")
                    .unwrap_or(0);
                while bytes.len() < header_end + content_length {
                    let read = stream.read(&mut buffer).await.expect("read body");
                    assert!(read > 0, "request closed before body");
                    bytes.extend_from_slice(&buffer[..read]);
                }
                requests.push(
                    String::from_utf8(bytes[..header_end + content_length].to_vec())
                        .expect("request should be UTF-8"),
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    response_body.len(),
                    response_body
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write response");
            }
            requests
        });
        (format!("http://{address}/"), server)
    }

    fn request_json(request: &str) -> Value {
        let (_, body) = request
            .split_once("\r\n\r\n")
            .expect("request should have a body");
        serde_json::from_str(body).expect("request body should be JSON")
    }
}
