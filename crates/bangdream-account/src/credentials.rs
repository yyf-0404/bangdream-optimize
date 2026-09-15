//! One SDK login, one game login, then only that account's own suite.
use super::{
    aes_decrypt_iso10126, aes_encrypt_iso10126, field_bytes, field_bytes_value, field_string,
    field_u64, load_card_episode_ids, parse_fields, suite_user_to_player_config, ImportError,
    PlayerConfig,
};
use crate::{
    apk::{self, ClientVersion},
    version_config::VersionConfig,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use md5::{Digest, Md5};
use rand::{rngs::OsRng, RngCore};
use reqwest::{blocking::Client, header::HeaderMap, Method};
use rsa::{pkcs8::DecodePublicKey, Pkcs1v15Encrypt, RsaPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::Read,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const SDK_HOST: &str = "https://line1-sdk-center-login-sh.biligame.net";
const GAME_BASE: &str = "https://l3-prod-all-bd.bilibiligame.net/api";
const SDK_KEY: &str = "0e814339fb45488db2f0a3462fe17690";
const REQUEST_KEY: &str = "44859ad705d454676f9184af633e6e4f";
const DEVICE_ID: &str = "116c041772c0a98614cc0ab7f5d4c6b9";
const BUVID: &str = "XX6AB116C041772C0A98614CC0AB7F5D4C6B9";
const PACKAGE: &str = "com.bilibili.star.bili";
const MAX_SUITE: usize = 32 * 1024 * 1024;

static IMPORT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

type Form = BTreeMap<String, String>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountChannel {
    Android,
    Ios,
}
impl AccountChannel {
    fn game(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Android => ("1", "2", "Android"),
            Self::Ios => ("1000", "1", "iOS"),
        }
    }
    fn bd_id(self) -> &'static str {
        match self {
            Self::Android => "ec85a677-2f0e-4121-951f-da641cef481a-2ec55091-9b91-4687-a50b-6cc",
            Self::Ios => "801dec5f-93c9-4e0d-9aea-03db2d57da4b-72e7c926-9d82-498b-99c9-a54",
        }
    }
}

// No Debug/Serialize: credentials must not enter traces, diagnostics or saved profiles.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialImportRequest {
    pub account: String,
    pub password: String,
    pub channel: AccountChannel,
}
impl CredentialImportRequest {
    pub fn validate(&self) -> Result<(), ImportError> {
        if self.account.trim().is_empty()
            || self.account.len() > 320
            || self.account.chars().any(char::is_control)
        {
            return Err(fail("请输入有效的 Bilibili 账号"));
        }
        if self.password.is_empty() || self.password.len() > 1024 {
            return Err(fail("请输入有效的密码"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialImportResult {
    pub player: PlayerConfig,
    pub game_uid: u64,
    pub name: String,
    pub rank: u64,
    pub channel: AccountChannel,
}

#[derive(Clone, Default)]
pub struct CredentialImporter {
    cards_dir: Option<PathBuf>,
    versions: VersionConfig,
}
impl CredentialImporter {
    pub fn new(cards_dir: Option<PathBuf>) -> Self {
        Self {
            cards_dir,
            versions: VersionConfig::default(),
        }
    }
    pub fn with_version_config(mut self, path: impl Into<PathBuf>) -> Self {
        self.versions = VersionConfig::new(path);
        self
    }
    pub fn import(
        &self,
        request: CredentialImportRequest,
    ) -> Result<CredentialImportResult, ImportError> {
        request.validate()?;
        let _guard = IMPORT_LOCK
            .try_lock()
            .map_err(|_| fail("已有账号正在读取，请稍后再试"))?;
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| fail("无法初始化国服连接"))?;
        let http = HttpTransport(client.clone());
        let (version, application) = self.versions.resolve(
            |version| application_versions(&http, version, request.channel),
            || apk::discover(&client),
        )?;
        import_preflighted(&http, request, &version, application, |card_id| {
            self.cards_dir
                .as_deref()
                .and_then(|dir| load_card_episode_ids(dir, card_id))
        })
    }
}

pub(crate) fn fail(message: impl Into<String>) -> ImportError {
    ImportError::Account(message.into())
}
pub(crate) fn bounded_body(
    response: reqwest::blocking::Response,
    max: usize,
) -> Result<Vec<u8>, ImportError> {
    if response.content_length().is_some_and(|n| n > max as u64) {
        return Err(fail("国服响应超过大小限制"));
    }
    let mut body = Vec::new();
    response
        .take(max as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|_| fail("国服响应读取失败，请稍后重试"))?;
    if body.len() > max {
        return Err(fail("国服响应超过大小限制"));
    }
    Ok(body)
}

trait Transport {
    fn game(
        &self,
        method: Method,
        endpoint: &str,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
        max: usize,
    ) -> Result<(HeaderMap, Vec<u8>), ImportError>;
    fn sdk(&self, endpoint: &str, form: Form) -> Result<Value, ImportError>;
}
struct HttpTransport(Client);
impl Transport for HttpTransport {
    fn game(
        &self,
        method: Method,
        endpoint: &str,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
        max: usize,
    ) -> Result<(HeaderMap, Vec<u8>), ImportError> {
        let mut request = self
            .0
            .request(method, format!("{GAME_BASE}{endpoint}"))
            .headers(headers);
        if let Some(body) = body {
            request = request.body(body);
        }
        let response = request
            .send()
            .map_err(|_| fail("连接国服超时或失败，本次登录已停止，请稍后重试"))?;
        if !response.status().is_success() {
            return Err(fail(format!(
                "国服接口返回 HTTP {}，本次登录已停止",
                response.status().as_u16()
            )));
        }
        let headers = response.headers().clone();
        let plain = aes_decrypt_iso10126(&bounded_body(response, max)?)
            .map_err(|_| fail("国服返回的数据无法解密，请检查游戏是否维护或更新"))?;
        Ok((headers, plain))
    }
    fn sdk(&self, endpoint: &str, mut form: Form) -> Result<Value, ImportError> {
        form.insert("sign".into(), sdk_sign(&form));
        let response = self
            .0
            .post(format!("{SDK_HOST}{endpoint}"))
            .header("User-Agent", "Mozilla/5.0 BiliGSCSDK")
            .form(&form)
            .send()
            .map_err(|_| fail("连接 Bilibili 登录服务失败，本次登录已停止"))?;
        if !response.status().is_success() {
            return Err(fail(format!(
                "Bilibili 登录服务返回 HTTP {}",
                response.status().as_u16()
            )));
        }
        let value: Value = serde_json::from_slice(&bounded_body(response, 1024 * 1024)?)
            .map_err(|_| fail("Bilibili 登录响应格式异常"))?;
        let code = value
            .get("code")
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()));
        if code != Some(0) {
            // Never relay an upstream response/body: it may contain account or session data.
            return Err(fail(format!("Bilibili 登录未成功（代码 {}）。请核对账号密码；如账号要求验证码或安全验证，请先在官方客户端完成后重试。",
                code.map(|n| n.to_string()).unwrap_or_else(|| "未知".into()))));
        }
        Ok(value)
    }
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn sdk_params(channel: AccountChannel, v: &ClientVersion, time: u128) -> Form {
    let values = [
        ("app_id", "330"),
        ("game_id", "330"),
        ("merchant_id", "1"),
        ("server_id", "557"),
        ("channel_id", "1"),
        ("platform", "3"),
        ("platform_type", "3"),
        ("sdk_type", "1"),
        ("sdk_log_type", "1"),
        ("sdk_ver", "6.19.5"),
        ("version", "3"),
        ("domain", "line1-sdk-center-login-sh.biligame.net"),
        ("original_domain", SDK_HOST),
        ("cur_buvid", BUVID),
        ("old_buvid", BUVID),
        ("udid", BUVID),
        ("bd_id", channel.bd_id()),
        ("apk_sign", "4502a02a00395dec05a4134ad593224d"),
        ("app_ver", &v.client),
        ("version_code", &v.code),
        ("current_env", "0"),
        ("domain_switch_count", "0"),
        ("ad_info", "{\"server_type\":\"0\"}"),
        ("ad_ext", "{\"server_type\":\"0\"}"),
    ];
    let mut form: Form = values
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    form.insert("timestamp".into(), time.to_string());
    form
}
fn sdk_sign(form: &Form) -> String {
    let mut hash = Md5::new();
    for (key, value) in form {
        if !["item_name", "item_desc", "feign_sign", "token", "sign"]
            .contains(&key.to_ascii_lowercase().as_str())
        {
            hash.update(value.as_bytes());
        }
    }
    hash.update(SDK_KEY.as_bytes());
    format!("{:x}", hash.finalize())
}
fn find_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value {
        Value::Object(map) => map
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .or_else(|| map.values().find_map(|v| find_string(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find_string(v, key)),
        _ => None,
    }
}
fn sdk_login(
    http: &impl Transport,
    request: CredentialImportRequest,
    version: &ClientVersion,
) -> Result<(String, String), ImportError> {
    let mut form = sdk_params(request.channel, version, timestamp());
    form.insert("cipher_type".into(), "bili_login_rsa".into());
    let cipher = http.sdk("/api/external/issue/cipher/v3", form)?;
    let prefix =
        find_string(&cipher, "hash").ok_or_else(|| fail("登录服务没有返回密码加密参数"))?;
    let pem = find_string(&cipher, "cipher_key").ok_or_else(|| fail("登录服务没有返回密码公钥"))?;
    let base64: String = pem
        .replace("-----BEGIN PUBLIC KEY-----", "")
        .replace("-----END PUBLIC KEY-----", "")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let der = STANDARD
        .decode(base64)
        .map_err(|_| fail("登录服务的密码公钥格式异常"))?;
    let public_key =
        RsaPublicKey::from_public_key_der(&der).map_err(|_| fail("登录服务的密码公钥格式异常"))?;
    let encrypted = public_key
        .encrypt(
            &mut OsRng,
            Pkcs1v15Encrypt,
            format!("{prefix}{}", request.password).as_bytes(),
        )
        .map_err(|_| fail("密码加密失败，请检查密码长度"))?;
    let mut form = sdk_params(request.channel, version, timestamp());
    form.insert("user_id".into(), request.account.trim().into());
    form.insert("pwd".into(), STANDARD.encode(encrypted));
    let value = http.sdk("/api/external/login/v3", form)?;
    let uid = value
        .get("uid")
        .and_then(|v| {
            v.as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(String::from))
        })
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| fail("登录服务没有返回有效账号编号"))?;
    let key = value
        .get("access_key")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail("登录服务没有返回登录凭证"))?;
    Ok((uid, key.into()))
}
fn game_headers(
    version: &ClientVersion,
    channel: AccountChannel,
) -> Result<HeaderMap, ImportError> {
    let (channel, platform, client) = channel.game();
    let agent = format!(
        "UnityPlayer/{} (UnityWebRequest/1.0, libcurl/8.10.1-DEV)",
        version.unity
    );
    let mut headers = HeaderMap::new();
    for (key, value) in [
        ("content-type", "application/octet-stream"),
        ("accept", "application/octet-stream"),
        ("user-agent", agent.as_str()),
        ("x-unity-version", &version.unity),
        ("x-clientversion", &version.client),
        ("x-channelid", channel),
        ("x-platformid", platform),
        ("x-clientplatform", client),
        ("x-deviceid", DEVICE_ID),
    ] {
        headers.insert(
            key,
            value.parse().map_err(|_| fail("国服客户端版本格式异常"))?,
        );
    }
    Ok(headers)
}
fn text_field(fields: &[super::ProtoField], number: u64) -> Option<String> {
    String::from_utf8(field_bytes_value(fields, number)?.to_vec()).ok()
}
fn login_body(uid: &str, access_key: &str, version: &ClientVersion) -> Vec<u8> {
    let mut udid = Vec::new();
    field_string(&mut udid, 1, "12345");
    field_string(&mut udid, 2, DEVICE_ID);
    let mut body = Vec::new();
    for (i, value) in [
        uid,
        access_key,
        "Android",
        "HXY MP08",
        "Android OS 12 / API-31 (SP1A.210812.016/2207251529)",
        &version.client,
    ]
    .iter()
    .enumerate()
    {
        field_string(&mut body, i as u64 + 1, value);
    }
    field_bytes(&mut body, 7, &udid);
    field_string(&mut body, 8, PACKAGE);
    body
}
struct ApplicationVersion {
    data: String,
    master: String,
}
fn application_versions(
    http: &impl Transport,
    version: &ClientVersion,
    channel: AccountChannel,
) -> Result<ApplicationVersion, ImportError> {
    version.validate()?;
    let (_, application) = http.game(
        Method::GET,
        "/application",
        game_headers(version, channel)?,
        None,
        1024 * 1024,
    )?;
    let application = parse_fields(&application)?;
    if text_field(&application, 1).as_deref() != Some(version.client.as_str()) {
        return Err(fail("游戏客户端版本已更新"));
    }
    let data = text_field(&application, 2)
        .filter(|s| apk::dotted_version(s))
        .ok_or_else(|| fail("国服数据版本格式异常"))?;
    let master = text_field(&application, 10)
        .filter(|s| s.len() == 16 && s.bytes().all(|b| b.is_ascii_digit()))
        .ok_or_else(|| fail("国服主数据版本格式异常"))?;
    Ok(ApplicationVersion { data, master })
}
#[cfg(test)]
fn import_once(
    http: &impl Transport,
    request: CredentialImportRequest,
    version: &ClientVersion,
    episodes: impl FnMut(u64) -> Option<[u64; 2]>,
) -> Result<CredentialImportResult, ImportError> {
    request.validate()?;
    let application = application_versions(http, version, request.channel)?;
    import_preflighted(http, request, version, application, episodes)
}
fn import_preflighted(
    http: &impl Transport,
    request: CredentialImportRequest,
    version: &ClientVersion,
    application: ApplicationVersion,
    episodes: impl FnMut(u64) -> Option<[u64; 2]>,
) -> Result<CredentialImportResult, ImportError> {
    let channel = request.channel;
    let mut headers = game_headers(version, channel)?;
    let ApplicationVersion { data, master } = application;
    headers.insert(
        "x-dataversion",
        data.parse().map_err(|_| fail("国服数据版本格式异常"))?,
    );
    let (sdk_uid, access_key) = sdk_login(http, request, version)?;
    let mut random = [0u8; 16];
    OsRng.fill_bytes(&mut random);
    let rid: String = random.iter().map(|b| format!("{b:02x}")).collect();
    headers.insert("x-requestid", rid.parse().unwrap());
    let (response, login) = http.game(
        Method::POST,
        "/user/login",
        headers.clone(),
        Some(aes_encrypt_iso10126(&login_body(
            &sdk_uid,
            &access_key,
            version,
        ))?),
        1024 * 1024,
    )?;
    let uid = field_u64(&parse_fields(&login)?, 1)
        .filter(|n| *n > 0 && *n <= i64::MAX as u64)
        .ok_or_else(|| fail("游戏登录没有返回有效玩家 ID"))?;
    let mut token = response
        .get("x-token")
        .filter(|v| !v.is_empty())
        .cloned()
        .ok_or_else(|| fail("游戏登录没有返回会话凭证"))?;
    let nonce = response
        .get("x-requestid")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail("游戏登录没有返回请求序号"))?;
    let rid = format!("{:x}", Md5::digest(format!("{REQUEST_KEY}{nonce}")));
    token.set_sensitive(true);
    headers.insert("x-token", token);
    headers.insert("x-requestid", rid.parse().unwrap());
    headers.insert("x-masterdataversion", master.parse().unwrap());
    // Use only the UID returned by this login. Never fetch a caller-supplied player ID.
    let (_, suite) = http.game(
        Method::GET,
        &format!("/suite/user/{uid}"),
        headers,
        None,
        MAX_SUITE,
    )?;
    let root = parse_fields(&suite)?;
    let user =
        parse_fields(field_bytes_value(&root, 1).ok_or_else(|| fail("国服没有返回用户资料"))?)?;
    let registration =
        parse_fields(field_bytes_value(&user, 1).ok_or_else(|| fail("国服没有返回用户身份"))?)?;
    let data =
        parse_fields(field_bytes_value(&user, 2).ok_or_else(|| fail("国服没有返回用户等级"))?)?;
    if field_u64(&registration, 1) != Some(uid) || field_u64(&data, 1) != Some(uid) {
        return Err(fail("登录账号与返回资料不一致，已停止导入"));
    }
    let player = suite_user_to_player_config(uid, &suite, episodes)?;
    Ok(CredentialImportResult {
        player,
        game_uid: uid,
        name: text_field(&registration, 3).unwrap_or_default(),
        rank: field_u64(&data, 2).unwrap_or_default(),
        channel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::{pkcs8::EncodePublicKey, RsaPrivateKey};
    use std::{cell::RefCell, sync::OnceLock};
    fn version() -> ClientVersion {
        ClientVersion {
            client: "9.4.4".into(),
            code: "940400".into(),
            unity: "2022.3.62f1".into(),
        }
    }
    fn request(channel: AccountChannel) -> CredentialImportRequest {
        CredentialImportRequest {
            account: "test@example.invalid".into(),
            password: "sample-password".into(),
            channel,
        }
    }
    fn integer(bytes: &mut Vec<u8>, field: u64, value: u64) {
        super::super::write_varint(bytes, field << 3);
        super::super::write_varint(bytes, value);
    }
    fn key() -> &'static RsaPrivateKey {
        static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
        KEY.get_or_init(|| RsaPrivateKey::new(&mut OsRng, 1024).unwrap())
    }
    struct Mock {
        calls: RefCell<Vec<String>>,
        channel: AccountChannel,
        wrong_uid: bool,
        missing_token: bool,
        fail_sdk: bool,
    }
    impl Mock {
        fn new(channel: AccountChannel) -> Self {
            Self {
                calls: RefCell::new(vec![]),
                channel,
                wrong_uid: false,
                missing_token: false,
                fail_sdk: false,
            }
        }
    }
    impl Transport for Mock {
        fn sdk(&self, path: &str, form: Form) -> Result<Value, ImportError> {
            self.calls.borrow_mut().push(path.into());
            assert_eq!(form["platform"], "3");
            assert_eq!(form["channel_id"], "1");
            assert_eq!(form["bd_id"], self.channel.bd_id());
            if self.fail_sdk {
                return Err(fail("verification required"));
            }
            if path.ends_with("cipher/v3") {
                let der = RsaPublicKey::from(key()).to_public_key_der().unwrap();
                return Ok(
                    serde_json::json!({"data":{"hash":"prefix","cipher_key":STANDARD.encode(der.as_ref())}}),
                );
            }
            assert_eq!(form["user_id"], "test@example.invalid");
            let plain = key()
                .decrypt(Pkcs1v15Encrypt, &STANDARD.decode(&form["pwd"]).unwrap())
                .unwrap();
            assert_eq!(plain, b"prefixsample-password");
            Ok(serde_json::json!({"uid":123,"access_key":"test-access-key"}))
        }
        fn game(
            &self,
            method: Method,
            path: &str,
            headers: HeaderMap,
            body: Option<Vec<u8>>,
            _: usize,
        ) -> Result<(HeaderMap, Vec<u8>), ImportError> {
            self.calls.borrow_mut().push(path.into());
            let (channel, platform, client) = self.channel.game();
            assert_eq!(headers["x-channelid"], channel);
            assert_eq!(headers["x-platformid"], platform);
            assert_eq!(headers["x-clientplatform"], client);
            assert!(!headers.contains_key("x-signature"));
            let mut result = Vec::new();
            let mut response = HeaderMap::new();
            match path {
                "/application" => {
                    assert_eq!(method, Method::GET);
                    field_string(&mut result, 1, "9.4.4");
                    field_string(&mut result, 2, "9.4.4.1");
                    field_string(&mut result, 10, "2026091400000000");
                }
                "/user/login" => {
                    assert_eq!(method, Method::POST);
                    assert!(!headers.contains_key("x-token"));
                    assert_eq!(headers["x-requestid"].len(), 32);
                    let plain = aes_decrypt_iso10126(&body.unwrap()).unwrap();
                    let fields = parse_fields(&plain).unwrap();
                    assert_eq!(text_field(&fields, 1).as_deref(), Some("123"));
                    assert_eq!(text_field(&fields, 3).as_deref(), Some("Android"));
                    integer(&mut result, 1, 42);
                    if !self.missing_token {
                        response.insert("x-token", "test-session-token".parse().unwrap());
                    }
                    response.insert("x-requestid", "server-nonce".parse().unwrap());
                }
                "/suite/user/42" => {
                    assert_eq!(method, Method::GET);
                    assert!(body.is_none());
                    assert_eq!(headers["x-token"], "test-session-token");
                    assert_eq!(
                        headers["x-requestid"],
                        format!("{:x}", Md5::digest(format!("{REQUEST_KEY}server-nonce")))
                    );
                    assert_eq!(headers["x-masterdataversion"], "2026091400000000");
                    let mut registration = Vec::new();
                    integer(&mut registration, 1, if self.wrong_uid { 43 } else { 42 });
                    field_string(&mut registration, 3, "测试玩家");
                    let mut data = Vec::new();
                    integer(&mut data, 1, 42);
                    integer(&mut data, 2, 268);
                    let mut user = Vec::new();
                    field_bytes(&mut user, 1, &registration);
                    field_bytes(&mut user, 2, &data);
                    field_bytes(&mut result, 1, &user);
                }
                _ => panic!("unexpected endpoint"),
            }
            Ok((response, result))
        }
    }
    #[test]
    fn sdk_sign_sorts_values_before_encoding_and_skips_excluded_fields() {
        let form = Form::from([
            ("z".into(), "a+b&中文".into()),
            ("a".into(), "123".into()),
            ("Token".into(), "ignored".into()),
        ]);
        assert_eq!(
            sdk_sign(&form),
            format!("{:x}", Md5::digest(format!("123a+b&中文{SDK_KEY}")))
        );
    }
    #[test]
    fn android_and_ios_use_one_login_and_only_own_suite() {
        for channel in [AccountChannel::Android, AccountChannel::Ios] {
            let mock = Mock::new(channel);
            let result = import_once(&mock, request(channel), &version(), |_| None).unwrap();
            assert_eq!(result.game_uid, 42);
            assert_eq!(result.player.player_id, 42);
            assert_eq!(result.rank, 268);
            assert_eq!(result.name, "测试玩家");
            assert_eq!(result.channel, channel);
            assert_eq!(
                *mock.calls.borrow(),
                vec![
                    "/application",
                    "/api/external/issue/cipher/v3",
                    "/api/external/login/v3",
                    "/user/login",
                    "/suite/user/42"
                ]
            );
            let json = serde_json::to_string(&result).unwrap();
            assert!(!json.contains("sample-password"));
            assert!(!json.contains("test-session-token"));
            assert!(!json.contains("test-access-key"));
        }
    }
    #[test]
    fn different_account_suite_is_never_converted() {
        let mut mock = Mock::new(AccountChannel::Android);
        mock.wrong_uid = true;
        assert!(
            import_once(&mock, request(mock.channel), &version(), |_| None)
                .unwrap_err()
                .to_string()
                .contains("不一致")
        );
        assert_eq!(mock.calls.borrow().len(), 5);
    }
    #[test]
    fn missing_session_token_stops_before_suite_and_does_not_retry() {
        let mut mock = Mock::new(AccountChannel::Ios);
        mock.missing_token = true;
        assert!(import_once(&mock, request(mock.channel), &version(), |_| None).is_err());
        assert_eq!(mock.calls.borrow().len(), 4);
    }
    #[test]
    fn sdk_rejection_stops_without_game_login() {
        let mut mock = Mock::new(AccountChannel::Android);
        mock.fail_sdk = true;
        assert!(import_once(&mock, request(mock.channel), &version(), |_| None).is_err());
        assert_eq!(mock.calls.borrow().len(), 2);
    }
    #[test]
    fn requests_require_explicit_channel_and_reject_caller_uid() {
        for value in [
            serde_json::json!({"account":"a","password":"p"}),
            serde_json::json!({"account":"a","password":"p","channel":"android","user_id":123}),
        ] {
            assert!(serde_json::from_value::<CredentialImportRequest>(value).is_err());
        }
        let mut req = request(AccountChannel::Android);
        req.password.clear();
        assert!(req.validate().is_err());
        assert!(parse_fields(&[0]).is_err());
        assert!(parse_fields(&[8, 255, 255, 255, 255, 255, 255, 255, 255, 255, 2]).is_err());
    }
    #[test]
    fn refreshed_version_is_saved_before_one_account_login() {
        let path =
            std::env::temp_dir().join(format!("cn-login-version-{:016x}.json", OsRng.next_u64()));
        let config = VersionConfig::new(&path);
        let mut old = version();
        old.client = "9.4.3".into();
        config.save(&old).unwrap();
        let mock = Mock::new(AccountChannel::Android);
        let discoveries = std::cell::Cell::new(0);
        let (current, application) = config
            .resolve(
                |v| application_versions(&mock, v, mock.channel),
                || {
                    discoveries.set(discoveries.get() + 1);
                    Ok(version())
                },
            )
            .unwrap();
        assert_eq!(config.load().unwrap(), version());
        let result =
            import_preflighted(&mock, request(mock.channel), &current, application, |_| {
                None
            })
            .unwrap();
        assert_eq!(result.game_uid, 42);
        assert_eq!(discoveries.get(), 1);
        let calls = mock.calls.borrow();
        assert_eq!(
            calls
                .iter()
                .filter(|s| s.as_str() == "/application")
                .count(),
            2
        );
        assert_eq!(
            calls.iter().filter(|s| s.as_str() == "/user/login").count(),
            1
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    #[ignore = "public network only: fixed defaults and /application; no APK or login"]
    fn public_fixed_version_preflight() {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .unwrap();
        let path =
            std::env::temp_dir().join(format!("cn-fixed-preflight-{:016x}.json", OsRng.next_u64()));
        let config = VersionConfig::new(&path);
        let http = HttpTransport(client);
        for channel in [AccountChannel::Android, AccountChannel::Ios] {
            let (version, _) = config
                .resolve(
                    |v| application_versions(&http, v, channel),
                    || panic!("fixed defaults failed; APK must not be needed"),
                )
                .unwrap();
            eprintln!(
                "Fixed client accepted for {channel:?}: {} / {} / {}",
                version.client, version.code, version.unity
            );
        }
        assert!(!path.exists());
    }
    #[test]
    #[ignore = "public network only: official APK metadata and /application, no account login"]
    fn public_version_preflight() {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .unwrap();
        let version = apk::discover(&client).unwrap();
        eprintln!("Public client metadata: {:?}", version);
        for channel in [AccountChannel::Android, AccountChannel::Ios] {
            let (_, body) = HttpTransport(client.clone())
                .game(
                    Method::GET,
                    "/application",
                    game_headers(&version, channel).unwrap(),
                    None,
                    1024 * 1024,
                )
                .unwrap();
            let fields = parse_fields(&body).unwrap();
            assert_eq!(
                text_field(&fields, 1).as_deref(),
                Some(version.client.as_str())
            );
            assert!(text_field(&fields, 2).is_some_and(|s| apk::dotted_version(&s)));
            assert_eq!(text_field(&fields, 10).unwrap().len(), 16);
        }
    }
}
