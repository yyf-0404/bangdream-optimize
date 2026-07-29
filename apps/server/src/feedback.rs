use async_trait::async_trait;
use lettre::{
    message::{header::ContentType, Attachment, Mailbox, MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_CATEGORY_CHARS: usize = 32;
const MAX_SUBJECT_CHARS: usize = 120;
const MAX_CONTENT_CHARS: usize = 10_000;
const MAX_CONTEXT_CHARS: usize = 80;
const MAX_USER_AGENT_CHARS: usize = 512;
pub const MAX_ATTACHMENT_COUNT: usize = 3;
pub const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_TOTAL_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FeedbackRequest {
    pub category: String,
    pub subject: String,
    pub content: String,
    pub contact_email: Option<String>,
    pub context: Option<FeedbackContext>,
    pub website: String,
}

impl Default for FeedbackRequest {
    fn default() -> Self {
        Self {
            category: String::new(),
            subject: String::new(),
            content: String::new(),
            contact_email: None,
            context: None,
            website: String::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FeedbackContext {
    pub runtime: String,
    pub app_version: String,
    pub page: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackReceipt {
    pub feedback_id: String,
}

#[derive(Debug, Clone)]
pub struct FeedbackSubmission {
    pub feedback_id: String,
    pub category: String,
    pub subject: String,
    pub content: String,
    pub contact_email: Option<Mailbox>,
    pub context: FeedbackContextSummary,
    pub user_agent: String,
}

#[derive(Debug, Clone, Default)]
pub struct FeedbackContextSummary {
    pub runtime: String,
    pub app_version: String,
    pub page: String,
}

#[derive(Debug, Clone)]
pub struct FeedbackAttachment {
    pub file_name: String,
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

impl FeedbackAttachment {
    pub fn validate(file_name: Option<&str>, bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("附件不能为空".to_owned());
        }
        if bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(format!(
                "单个附件不能超过 {} MiB",
                MAX_ATTACHMENT_BYTES / 1024 / 1024
            ));
        }
        let file_name = sanitize_file_name(file_name.unwrap_or_default())?;
        let content_type = attachment_content_type(&file_name)
            .ok_or_else(|| format!("不支持的附件类型：{file_name}"))?;
        Ok(Self {
            file_name,
            content_type,
            bytes,
        })
    }
}

pub fn validate_attachment_set(attachments: &[FeedbackAttachment]) -> Result<(), String> {
    if attachments.len() > MAX_ATTACHMENT_COUNT {
        return Err(format!("最多只能上传 {MAX_ATTACHMENT_COUNT} 个附件"));
    }
    let total_bytes = attachments
        .iter()
        .map(|attachment| attachment.bytes.len())
        .sum::<usize>();
    if total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
        return Err(format!(
            "附件总大小不能超过 {} MiB",
            MAX_TOTAL_ATTACHMENT_BYTES / 1024 / 1024
        ));
    }
    Ok(())
}

fn sanitize_file_name(file_name: &str) -> Result<String, String> {
    let file_name = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    if file_name.is_empty() {
        return Err("附件文件名不能为空".to_owned());
    }
    if file_name.chars().any(char::is_control) {
        return Err("附件文件名包含无效字符".to_owned());
    }
    if file_name.chars().count() > 120 {
        return Err("附件文件名不能超过 120 个字符".to_owned());
    }
    Ok(file_name.to_owned())
}

fn attachment_content_type(file_name: &str) -> Option<&'static str> {
    let extension = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "txt" | "log" => Some("text/plain"),
        "json" => Some("application/json"),
        "zip" => Some("application/zip"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

impl FeedbackSubmission {
    pub fn validate(
        request: FeedbackRequest,
        feedback_id: String,
        user_agent: Option<&str>,
    ) -> Result<Self, String> {
        let category = required_trimmed("反馈类型", request.category, MAX_CATEGORY_CHARS)?;
        if !matches!(
            category.as_str(),
            "problem" | "suggestion" | "data" | "other"
        ) {
            return Err("不支持的反馈类型".to_owned());
        }
        let subject = required_trimmed("标题", request.subject, MAX_SUBJECT_CHARS)?;
        let content = required_trimmed("反馈内容", request.content, MAX_CONTENT_CHARS)?;
        let contact_email = request
            .contact_email
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(|value| {
                if value.chars().count() > 254 {
                    return Err("联系邮箱过长".to_owned());
                }
                value
                    .parse::<Mailbox>()
                    .map_err(|_| "联系邮箱格式无效".to_owned())
            })
            .transpose()?;
        let context = request.context.unwrap_or_default();

        Ok(Self {
            feedback_id,
            category,
            subject,
            content,
            contact_email,
            context: FeedbackContextSummary {
                runtime: trimmed_with_limit(context.runtime, MAX_CONTEXT_CHARS),
                app_version: trimmed_with_limit(context.app_version, MAX_CONTEXT_CHARS),
                page: trimmed_with_limit(context.page, MAX_CONTEXT_CHARS),
            },
            user_agent: trimmed_with_limit(user_agent.unwrap_or_default(), MAX_USER_AGENT_CHARS),
        })
    }

    fn category_label(&self) -> &'static str {
        match self.category.as_str() {
            "problem" => "问题反馈",
            "suggestion" => "功能建议",
            "data" => "数据问题",
            _ => "其他",
        }
    }
}

fn required_trimmed(label: &str, value: String, max_chars: usize) -> Result<String, String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(format!("{label}不能为空"));
    }
    if value.chars().count() > max_chars {
        return Err(format!("{label}不能超过 {max_chars} 个字符"));
    }
    Ok(value)
}

fn trimmed_with_limit(value: impl AsRef<str>, max_chars: usize) -> String {
    value.as_ref().trim().chars().take(max_chars).collect()
}

#[async_trait]
pub trait FeedbackSender: Send + Sync {
    async fn send(
        &self,
        submission: &FeedbackSubmission,
        attachments: &[FeedbackAttachment],
    ) -> Result<(), String>;
}

pub struct SmtpFeedbackSender {
    mailer: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    to: Mailbox,
}

impl SmtpFeedbackSender {
    pub fn from_env() -> Result<Option<Arc<dyn FeedbackSender>>, String> {
        let Some(host) = non_empty_env("BANGDREAM_OPTIMIZE_FEEDBACK_SMTP_HOST") else {
            return Ok(None);
        };
        let username = required_env("BANGDREAM_OPTIMIZE_FEEDBACK_SMTP_USERNAME")?;
        let password = required_env("BANGDREAM_OPTIMIZE_FEEDBACK_SMTP_PASSWORD")?;
        let port = non_empty_env("BANGDREAM_OPTIMIZE_FEEDBACK_SMTP_PORT")
            .map(|value| {
                value.parse::<u16>().map_err(|_| {
                    "BANGDREAM_OPTIMIZE_FEEDBACK_SMTP_PORT must be a valid port".to_owned()
                })
            })
            .transpose()?
            .unwrap_or(465);
        let from_address =
            non_empty_env("BANGDREAM_OPTIMIZE_FEEDBACK_FROM").unwrap_or_else(|| username.clone());
        let to_address =
            non_empty_env("BANGDREAM_OPTIMIZE_FEEDBACK_TO").unwrap_or_else(|| from_address.clone());
        let from = Mailbox::new(
            Some("BangDream Optimize".to_owned()),
            from_address
                .parse()
                .map_err(|_| "BANGDREAM_OPTIMIZE_FEEDBACK_FROM is invalid".to_owned())?,
        );
        let to = to_address
            .parse::<Mailbox>()
            .map_err(|_| "BANGDREAM_OPTIMIZE_FEEDBACK_TO is invalid".to_owned())?;
        let mailer = AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
            .map_err(|err| format!("failed to configure feedback SMTP TLS: {err}"))?
            .port(port)
            .credentials(Credentials::new(username, password))
            .timeout(Some(Duration::from_secs(10)))
            .build();

        Ok(Some(Arc::new(Self { mailer, from, to })))
    }

    fn build_message(
        &self,
        submission: &FeedbackSubmission,
        attachments: &[FeedbackAttachment],
    ) -> Result<Message, String> {
        let mut builder = Message::builder()
            .from(self.from.clone())
            .to(self.to.clone())
            .subject(format!(
                "[反馈][{}][{}] {}",
                submission.category_label(),
                submission.feedback_id,
                submission.subject
            ));
        if let Some(contact_email) = submission.contact_email.clone() {
            builder = builder.reply_to(contact_email);
        }
        let mut multipart = MultiPart::mixed().singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_PLAIN)
                .body(feedback_body(submission, attachments)),
        );
        for attachment in attachments {
            let content_type = ContentType::parse(attachment.content_type)
                .map_err(|err| format!("failed to parse attachment content type: {err}"))?;
            multipart = multipart.singlepart(
                Attachment::new(attachment.file_name.clone())
                    .body(attachment.bytes.clone(), content_type),
            );
        }
        builder
            .multipart(multipart)
            .map_err(|err| format!("failed to build feedback email: {err}"))
    }
}

#[async_trait]
impl FeedbackSender for SmtpFeedbackSender {
    async fn send(
        &self,
        submission: &FeedbackSubmission,
        attachments: &[FeedbackAttachment],
    ) -> Result<(), String> {
        let message = self.build_message(submission, attachments)?;
        self.mailer
            .send(message)
            .await
            .map(|_| ())
            .map_err(|err| format!("SMTP submission failed: {err}"))
    }
}

fn feedback_body(submission: &FeedbackSubmission, attachments: &[FeedbackAttachment]) -> String {
    let contact = submission
        .contact_email
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "未填写".to_owned());
    format!(
        "反馈编号：{}\n反馈类型：{}\n联系邮箱：{}\n运行环境：{}\n应用版本：{}\n当前页面：{}\nUser-Agent：{}\n附件：{}\n\n标题：{}\n\n反馈内容：\n{}\n",
        submission.feedback_id,
        submission.category_label(),
        contact,
        display_or_unknown(&submission.context.runtime),
        display_or_unknown(&submission.context.app_version),
        display_or_unknown(&submission.context.page),
        display_or_unknown(&submission.user_agent),
        attachment_summary(attachments),
        submission.subject,
        submission.content,
    )
}

fn attachment_summary(attachments: &[FeedbackAttachment]) -> String {
    if attachments.is_empty() {
        return "无".to_owned();
    }
    attachments
        .iter()
        .map(|attachment| {
            format!(
                "{} ({} bytes)",
                attachment.file_name,
                attachment.bytes.len()
            )
        })
        .collect::<Vec<_>>()
        .join("、")
}

fn display_or_unknown(value: &str) -> &str {
    if value.is_empty() {
        "未知"
    } else {
        value
    }
}

fn required_env(key: &'static str) -> Result<String, String> {
    non_empty_env(key).ok_or_else(|| format!("{key} is required when feedback SMTP is enabled"))
}

fn non_empty_env(key: &'static str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[derive(Clone)]
pub struct FeedbackRateLimiter {
    attempts: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    max_attempts: usize,
    window: Duration,
}

impl Default for FeedbackRateLimiter {
    fn default() -> Self {
        Self::new(3, Duration::from_secs(10 * 60))
    }
}

impl FeedbackRateLimiter {
    pub fn new(max_attempts: usize, window: Duration) -> Self {
        Self {
            attempts: Arc::new(Mutex::new(HashMap::new())),
            max_attempts: max_attempts.max(1),
            window,
        }
    }

    pub fn from_env() -> Self {
        let max_attempts = env_usize("BANGDREAM_OPTIMIZE_FEEDBACK_RATE_LIMIT")
            .unwrap_or(3)
            .max(1);
        let window_seconds =
            env_usize("BANGDREAM_OPTIMIZE_FEEDBACK_RATE_WINDOW_SECONDS").unwrap_or(600);
        Self::new(
            max_attempts,
            Duration::from_secs(window_seconds.max(1) as u64),
        )
    }

    pub fn check(&self, client_key: String) -> Result<(), u64> {
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
        let mut attempts = self.attempts.lock().unwrap_or_else(|err| err.into_inner());
        let entries = attempts.entry(client_key).or_default();
        while entries.front().is_some_and(|entry| *entry <= cutoff) {
            entries.pop_front();
        }
        if entries.len() >= self.max_attempts {
            let retry_after = entries
                .front()
                .and_then(|entry| entry.checked_add(self.window))
                .and_then(|deadline| deadline.checked_duration_since(now))
                .map(|duration| duration.as_secs().max(1))
                .unwrap_or(1);
            return Err(retry_after);
        }
        entries.push_back(now);
        Ok(())
    }
}

fn env_usize(key: &'static str) -> Option<usize> {
    non_empty_env(key).and_then(|value| value.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> FeedbackRequest {
        FeedbackRequest {
            category: "suggestion".to_owned(),
            subject: "增加反馈功能".to_owned(),
            content: "希望可以从前端发送反馈。".to_owned(),
            contact_email: Some("user@example.com".to_owned()),
            context: Some(FeedbackContext {
                runtime: "browser".to_owned(),
                app_version: "0.3.3".to_owned(),
                page: "activity".to_owned(),
            }),
            website: String::new(),
        }
    }

    #[test]
    fn validates_and_formats_feedback() {
        let feedback =
            FeedbackSubmission::validate(request(), "F-1".to_owned(), Some("Test Agent")).unwrap();
        let body = feedback_body(&feedback, &[]);
        assert!(body.contains("反馈编号：F-1"));
        assert!(body.contains("反馈类型：功能建议"));
        assert!(body.contains("user@example.com"));
        assert!(body.contains("运行环境：browser"));
    }

    #[test]
    fn rejects_invalid_email_and_oversized_content() {
        let mut invalid_email = request();
        invalid_email.contact_email = Some("bad\nBcc: attacker@example.com".to_owned());
        assert!(FeedbackSubmission::validate(invalid_email, "F-1".to_owned(), None).is_err());

        let mut oversized = request();
        oversized.content = "a".repeat(MAX_CONTENT_CHARS + 1);
        assert!(FeedbackSubmission::validate(oversized, "F-1".to_owned(), None).is_err());
    }

    #[test]
    fn limiter_rejects_requests_after_the_configured_count() {
        let limiter = FeedbackRateLimiter::new(2, Duration::from_secs(60));
        assert!(limiter.check("client".to_owned()).is_ok());
        assert!(limiter.check("client".to_owned()).is_ok());
        assert!(limiter.check("client".to_owned()).is_err());
        assert!(limiter.check("other".to_owned()).is_ok());
    }

    #[test]
    fn validates_supported_attachment_types_and_limits() {
        let diagnostic =
            FeedbackAttachment::validate(Some("../../diagnostic.json"), br#"{"ok":true}"#.to_vec())
                .unwrap();
        assert_eq!(diagnostic.file_name, "diagnostic.json");
        assert_eq!(diagnostic.content_type, "application/json");

        let unsupported = FeedbackAttachment::validate(Some("program.exe"), vec![1, 2, 3]);
        assert!(unsupported.is_err());

        let too_many = vec![diagnostic; MAX_ATTACHMENT_COUNT + 1];
        assert!(validate_attachment_set(&too_many).is_err());
    }

    #[tokio::test]
    async fn builds_multipart_email_with_attachment() {
        let sender = SmtpFeedbackSender {
            mailer: AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous("localhost").build(),
            from: "feedback@example.com".parse().unwrap(),
            to: "owner@example.com".parse().unwrap(),
        };
        let submission =
            FeedbackSubmission::validate(request(), "F-attachment".to_owned(), None).unwrap();
        let attachment =
            FeedbackAttachment::validate(Some("diagnostic.json"), br#"{"ok":true}"#.to_vec())
                .unwrap();

        let message = sender.build_message(&submission, &[attachment]).unwrap();
        let formatted = String::from_utf8(message.formatted()).unwrap();
        assert!(formatted.contains("Content-Type: multipart/mixed"));
        assert!(formatted.contains("filename=\"diagnostic.json\""));
        assert!(formatted.contains("Content-Type: application/json"));
    }
}
