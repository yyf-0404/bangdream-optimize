use aes::{
    cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit},
    Aes128,
};
use bangdream_optimize_core::{
    AreaItemConfig, CharacterBonusConfig, PlayerCardConfig, PlayerConfig, StatRate,
};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const API_BASE_URL: &str = "https://l3-prod-all-bd.bilibiligame.net/api";
const LOGIN_URL: &str = "https://l3-prod-all-bd.bilibiligame.net/api/user/login";
const DEFAULT_KEY: &[u8; 16] = b"wakakabiliwakaka";
const DEFAULT_IV: &[u8; 16] = b"biliwakakawohaha";
const BLOCK_SIZE: usize = 16;
const DEFAULT_DATA_VERSION: &str = "9.4.1.9";
const DEFAULT_MASTER_VERSION: &str = "2026061513000000";
const DEFAULT_PERSIST_PATH: &str = "var/bangdream-account/persist.json";

#[derive(Debug, Clone)]
pub struct BangDreamAccountImporter {
    persist_path: PathBuf,
    cards_dir: Option<PathBuf>,
    episode_ids_cache: Arc<RwLock<BTreeMap<u64, [u64; 2]>>>,
}

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub user_id: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct Persist {
    device_id: String,
    sdk: PersistSdk,
    unity: PersistUnity,
}

#[derive(Debug, Clone, Deserialize)]
struct PersistSdk {
    uid: String,
    access_key: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PersistUnity {
    unity_channel_id: String,
    unity_platform_id: String,
    client_platform: String,
    login_platform: String,
    operating_system: String,
    device_model: String,
    client_version: String,
    udid_short: String,
    client_package: String,
}

#[derive(Debug, Clone)]
struct LoginSession {
    token: String,
    request_id: String,
    persist: Persist,
}

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("failed to read Bang Dream persist file: {0}")]
    ReadPersist(#[from] std::io::Error),
    #[error("failed to parse Bang Dream persist file: {0}")]
    ParsePersist(#[from] serde_json::Error),
    #[error("Bang Dream import is missing required persist field: {0}")]
    MissingPersistField(&'static str),
    #[error("Bang Dream request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Bang Dream API returned HTTP {status}: {context}")]
    HttpStatus { status: u16, context: String },
    #[error("Bang Dream API response is missing header {0}")]
    MissingHeader(&'static str),
    #[error("Bang Dream encrypted payload is invalid: {0}")]
    Crypto(String),
    #[error("Bang Dream protobuf decode failed: {0}")]
    Protobuf(String),
}

impl BangDreamAccountImporter {
    pub fn new(persist_path: impl Into<PathBuf>) -> Result<Self, ImportError> {
        Ok(Self {
            persist_path: persist_path.into(),
            cards_dir: None,
            episode_ids_cache: Arc::new(RwLock::new(BTreeMap::new())),
        })
    }

    pub fn with_cards_dir(mut self, cards_dir: Option<PathBuf>) -> Self {
        self.cards_dir = cards_dir;
        self
    }

    pub fn persist_path(&self) -> &Path {
        &self.persist_path
    }

    pub fn import_player_config(
        &self,
        request: ImportRequest,
    ) -> Result<PlayerConfig, ImportError> {
        let persist = read_persist(&self.persist_path)?;
        validate_persist(&persist)?;
        let client = Client::builder().timeout(Duration::from_secs(30)).build()?;
        let session = self.login(&client, persist)?;
        let suite_user = self.fetch_suite_user(&client, &session, request.user_id)?;
        suite_user_to_player_config(request.user_id, &suite_user, |card_id| {
            self.episode_ids_for_card(card_id)
        })
    }

    fn episode_ids_for_card(&self, card_id: u64) -> Option<[u64; 2]> {
        if let Ok(cache) = self.episode_ids_cache.read() {
            if let Some(cached) = cache.get(&card_id) {
                return Some(*cached);
            }
        }

        let episode_ids = self
            .cards_dir
            .as_deref()
            .and_then(|cards_dir| load_card_episode_ids(cards_dir, card_id));
        if let (Some(episode_ids), Ok(mut cache)) = (episode_ids, self.episode_ids_cache.write()) {
            cache.insert(card_id, episode_ids);
        }
        episode_ids
    }

    fn login(&self, client: &Client, persist: Persist) -> Result<LoginSession, ImportError> {
        let plain = build_login_request(&persist);
        let body = aes_encrypt_iso10126(&plain)?;
        let request_id = request_id();
        let response = client
            .post(LOGIN_URL)
            .headers(login_headers(&persist, &request_id))
            .body(body)
            .send()?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.bytes()?.to_vec();
        if !status.is_success() {
            return Err(ImportError::HttpStatus {
                status: status.as_u16(),
                context: "Unity login failed".to_owned(),
            });
        }
        let _plain = aes_decrypt_iso10126(&body)?;
        let token =
            header_value(&headers, "x-token").ok_or(ImportError::MissingHeader("X-Token"))?;
        let next_request_id = header_value(&headers, "x-requestid").unwrap_or(request_id);
        Ok(LoginSession {
            token,
            request_id: next_request_id,
            persist,
        })
    }

    fn fetch_suite_user(
        &self,
        client: &Client,
        session: &LoginSession,
        user_id: u64,
    ) -> Result<Vec<u8>, ImportError> {
        let mut request_id = session.request_id.clone();
        let endpoint = format!("/suite/user/{user_id}");
        let url = format!("{API_BASE_URL}{endpoint}");
        let mut last_plain = Vec::new();

        for _attempt in 0..=2 {
            let response = client
                .get(&url)
                .headers(authed_get_headers(session, &request_id))
                .send()?;
            let status = response.status();
            let body = response.bytes()?.to_vec();
            let plain = aes_decrypt_iso10126(&body)?;
            if let Some(hint) = request_id_hint(&plain) {
                request_id = hint;
                last_plain = plain;
                continue;
            }
            if !status.is_success() {
                return Err(ImportError::HttpStatus {
                    status: status.as_u16(),
                    context: format!("GET {endpoint} failed"),
                });
            }
            return Ok(plain);
        }

        Err(ImportError::Protobuf(format!(
            "suite user request-id retry exhausted; last response len={}",
            last_plain.len()
        )))
    }
}

pub fn persist_path_from_env() -> PathBuf {
    std::env::var("BANGDREAM_OPTIMIZE_BD_PERSIST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("BANGDREAM_OPTIMIZE_BD_PERSIST_DIR")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(|dir| PathBuf::from(dir).join("persist.json"))
        })
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PERSIST_PATH))
}

fn read_persist(path: &Path) -> Result<Persist, ImportError> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn validate_persist(persist: &Persist) -> Result<(), ImportError> {
    if persist.device_id.trim().is_empty() {
        return Err(ImportError::MissingPersistField("device_id"));
    }
    if persist.sdk.uid.trim().is_empty() {
        return Err(ImportError::MissingPersistField("sdk.uid"));
    }
    if persist.sdk.access_key.trim().is_empty() {
        return Err(ImportError::MissingPersistField("sdk.access_key"));
    }
    Ok(())
}

fn login_headers(persist: &Persist, request_id: &str) -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    insert_header(&mut headers, "user-agent", unity_user_agent());
    insert_header(&mut headers, "content-type", "application/octet-stream");
    insert_header(&mut headers, "accept", "application/octet-stream");
    insert_header(
        &mut headers,
        "x-clientversion",
        &persist.unity.client_version,
    );
    insert_header(&mut headers, "x-requestid", request_id);
    insert_header(&mut headers, "x-channelid", &persist.unity.unity_channel_id);
    insert_header(
        &mut headers,
        "x-platformid",
        &persist.unity.unity_platform_id,
    );
    insert_header(&mut headers, "x-deviceid", &persist.device_id);
    insert_header(
        &mut headers,
        "x-clientplatform",
        &persist.unity.client_platform,
    );
    insert_header(&mut headers, "x-unity-version", "2022.3.62f3c1");
    headers
}

fn authed_get_headers(session: &LoginSession, request_id: &str) -> reqwest::header::HeaderMap {
    let persist = &session.persist;
    let mut headers = login_headers(persist, request_id);
    insert_header(&mut headers, "x-token", &session.token);
    insert_header(&mut headers, "x-dataversion", DEFAULT_DATA_VERSION);
    insert_header(&mut headers, "x-masterdataversion", DEFAULT_MASTER_VERSION);
    headers
}

fn insert_header(headers: &mut reqwest::header::HeaderMap, key: &'static str, value: &str) {
    if let Ok(value) = reqwest::header::HeaderValue::from_str(value) {
        headers.insert(reqwest::header::HeaderName::from_static(key), value);
    }
}

fn unity_user_agent() -> &'static str {
    "UnityPlayer/2022.3.62f3c1 (UnityWebRequest/1.0, libcurl/8.10.1-DEV)"
}

fn header_value(headers: &reqwest::header::HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn build_login_request(persist: &Persist) -> Vec<u8> {
    let mut out = Vec::new();
    field_string(&mut out, 1, &persist.sdk.uid);
    field_string(&mut out, 2, &persist.sdk.access_key);
    field_string(&mut out, 3, &persist.unity.login_platform);
    field_string(&mut out, 4, &persist.unity.device_model);
    field_string(&mut out, 5, &persist.unity.operating_system);
    field_string(&mut out, 6, &persist.unity.client_version);
    let mut udid = Vec::new();
    field_string(&mut udid, 1, &persist.unity.udid_short);
    field_string(&mut udid, 2, &persist.device_id);
    field_bytes(&mut out, 7, &udid);
    field_string(&mut out, 8, &persist.unity.client_package);
    out
}

fn field_string(out: &mut Vec<u8>, tag: u64, value: &str) {
    field_bytes(out, tag, value.as_bytes());
}

fn field_bytes(out: &mut Vec<u8>, tag: u64, value: &[u8]) {
    write_varint(out, (tag << 3) | 2);
    write_varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{nanos:032x}")
}

fn aes_encrypt_iso10126(plain: &[u8]) -> Result<Vec<u8>, ImportError> {
    let mut data = iso10126_pad(plain);
    aes_cbc_crypt(&mut data, false)?;
    Ok(data)
}

fn aes_decrypt_iso10126(ciphertext: &[u8]) -> Result<Vec<u8>, ImportError> {
    if ciphertext.is_empty() || ciphertext.len() % BLOCK_SIZE != 0 {
        return Err(ImportError::Crypto("invalid AES block length".to_owned()));
    }
    let mut data = ciphertext.to_vec();
    aes_cbc_crypt(&mut data, true)?;
    iso10126_unpad(&data)
}

fn iso10126_pad(plain: &[u8]) -> Vec<u8> {
    let pad_len = BLOCK_SIZE - (plain.len() % BLOCK_SIZE);
    let pad_len = if pad_len == 0 { BLOCK_SIZE } else { pad_len };
    let mut out = Vec::with_capacity(plain.len() + pad_len);
    out.extend_from_slice(plain);
    out.resize(plain.len() + pad_len - 1, 0);
    out.push(pad_len as u8);
    out
}

fn iso10126_unpad(data: &[u8]) -> Result<Vec<u8>, ImportError> {
    let Some(&pad_len) = data.last() else {
        return Err(ImportError::Crypto("missing ISO10126 padding".to_owned()));
    };
    let pad_len = usize::from(pad_len);
    if !(1..=BLOCK_SIZE).contains(&pad_len) || pad_len > data.len() {
        return Err(ImportError::Crypto(format!(
            "invalid ISO10126 pad length {pad_len}"
        )));
    }
    Ok(data[..data.len() - pad_len].to_vec())
}

fn aes_cbc_crypt(data: &mut [u8], decrypt: bool) -> Result<(), ImportError> {
    if data.len() % BLOCK_SIZE != 0 {
        return Err(ImportError::Crypto(
            "AES-CBC data is not block aligned".to_owned(),
        ));
    }
    let cipher = Aes128::new(GenericArray::from_slice(DEFAULT_KEY));
    let mut previous = *DEFAULT_IV;
    for block in data.chunks_exact_mut(BLOCK_SIZE) {
        if decrypt {
            let current = block_to_array(block);
            cipher.decrypt_block(GenericArray::from_mut_slice(block));
            xor_block(block, &previous);
            previous = current;
        } else {
            xor_block(block, &previous);
            cipher.encrypt_block(GenericArray::from_mut_slice(block));
            previous = block_to_array(block);
        }
    }
    Ok(())
}

fn block_to_array(block: &[u8]) -> [u8; BLOCK_SIZE] {
    let mut out = [0; BLOCK_SIZE];
    out.copy_from_slice(block);
    out
}

fn xor_block(block: &mut [u8], other: &[u8; BLOCK_SIZE]) {
    for (left, right) in block.iter_mut().zip(other) {
        *left ^= *right;
    }
}

fn request_id_hint(plain: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(plain);
    if !text.contains("X-Requestid error") {
        return None;
    }
    for key in ["newRequestId:", "oldRequestId:"] {
        if let Some(idx) = text.find(key) {
            let start = idx + key.len();
            let candidate = text.get(start..start + 32)?;
            if candidate.chars().all(|ch| ch.is_ascii_hexdigit()) {
                return Some(candidate.to_owned());
            }
        }
    }
    None
}

fn suite_user_to_player_config<F>(
    user_id: u64,
    buf: &[u8],
    mut episode_ids_for_card: F,
) -> Result<PlayerConfig, ImportError>
where
    F: FnMut(u64) -> Option<[u64; 2]>,
{
    let mut player = PlayerConfig {
        mongo_id: None,
        player_id: i64::try_from(user_id).unwrap_or(i64::MAX),
        current_event: Some(287),
        event_songs: BTreeMap::new(),
        event_presets: BTreeMap::new(),
        event_overrides: BTreeMap::new(),
        card_list: BTreeMap::new(),
        area_item: BTreeMap::new(),
        character_bouns: BTreeMap::new(),
    };

    let fields = parse_fields(buf)?;
    let read_episode_ids = fields
        .iter()
        .find(|field| field.field == 16)
        .map(|field| import_read_episode_ids(field.bytes_value()?))
        .transpose()?;

    for field in fields {
        match field.field {
            3 => import_user_situations(
                field.bytes_value()?,
                &mut player,
                read_episode_ids.as_ref(),
                &mut episode_ids_for_card,
            )?,
            22 => import_area_items(field.bytes_value()?, &mut player)?,
            401 => import_character_potential(field.bytes_value()?, &mut player)?,
            456 => import_character_task_bonus_rates(field.bytes_value()?, &mut player)?,
            _ => {}
        }
    }

    Ok(player)
}

fn import_user_situations<F>(
    buf: &[u8],
    player: &mut PlayerConfig,
    read_episode_ids: Option<&BTreeSet<u64>>,
    episode_ids_for_card: &mut F,
) -> Result<(), ImportError>
where
    F: FnMut(u64) -> Option<[u64; 2]>,
{
    for row in map_rows(buf)? {
        let Some(card_id) = row.get_u64("f2_2") else {
            continue;
        };
        let episodes = read_episode_ids
            .and_then(|read| {
                episode_ids_for_card(card_id).map(|episode_ids| {
                    [
                        read.contains(&episode_ids[0]),
                        read.contains(&episode_ids[1]),
                    ]
                })
            })
            .or_else(|| infer_episodes_from_append_parameter(&row))
            .unwrap_or([false, false]);
        player.card_list.insert(
            card_id.to_string(),
            PlayerCardConfig {
                level: u8_from(row.get_u64("f2_3")),
                training: row.get_str("f2_7") == Some("done"),
                illust_training_status: row.get_str("f2_9") == Some("after_training"),
                episodes,
                limit_break_rank: u8_from(row.get_u64("f2_13")),
                skill_level: u8_from(row.get_u64("f2_11")).max(1),
            },
        );
    }
    Ok(())
}

fn import_read_episode_ids(buf: &[u8]) -> Result<BTreeSet<u64>, ImportError> {
    Ok(map_rows(buf)?
        .into_iter()
        .filter(|row| row.get_str("f2_3") == Some("already_read"))
        .filter_map(|row| row.get_u64("f2_2"))
        .collect())
}

fn infer_episodes_from_append_parameter(row: &Row) -> Option<[bool; 2]> {
    let append_parameter = parse_fields(row.get_bytes("f2_12")?).ok()?;
    let append = [
        field_u64(&append_parameter, 3)?,
        field_u64(&append_parameter, 4)?,
        field_u64(&append_parameter, 5)?,
    ];
    if append[0] != append[1] || append[0] != append[2] {
        return None;
    }

    let level = row.get_u64("f2_3").unwrap_or_default();
    let trained = row.get_str("f2_7") == Some("done");
    let limit_break_rank = row.get_u64("f2_13").unwrap_or_default();
    let mut possible_states = BTreeSet::new();

    for rarity in 1_u64..=5 {
        let (max_level, training_bonus, episode_bonus) = match rarity {
            1 => (20, 0, [100, 200]),
            2 => (30, 0, [150, 300]),
            3 => (50, 300, [200, 500]),
            4 | 5 => (60, 400, [250, 600]),
            _ => unreachable!(),
        };
        if level > max_level || (trained && rarity < 3) {
            continue;
        }

        let fixed = (if trained { training_bonus } else { 0 }) + rarity * limit_break_rank * 50;
        let totals = [
            ([false, false], fixed),
            ([true, false], fixed + episode_bonus[0]),
            ([false, true], fixed + episode_bonus[1]),
            ([true, true], fixed + episode_bonus[0] + episode_bonus[1]),
        ];
        for (state, total) in totals {
            if append[0] == total {
                possible_states.insert(state);
            }
        }
    }

    if possible_states.len() != 1 {
        return None;
    }
    possible_states.first().copied()
}

#[derive(Debug, Deserialize)]
struct BestdoriCardDetail {
    episodes: Option<BestdoriEpisodeEntries>,
}

#[derive(Debug, Deserialize)]
struct BestdoriEpisodeEntries {
    #[serde(default)]
    entries: Vec<BestdoriEpisode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BestdoriEpisode {
    episode_id: u64,
    episode_type: String,
    situation_id: u64,
}

fn load_card_episode_ids(cards_dir: &Path, card_id: u64) -> Option<[u64; 2]> {
    let text = fs::read_to_string(cards_dir.join(format!("{card_id}.json"))).ok()?;
    let card: BestdoriCardDetail = serde_json::from_str(&text).ok()?;
    let entries = card.episodes?.entries;
    let standard = entries
        .iter()
        .find(|episode| episode.situation_id == card_id && episode.episode_type == "standard")?
        .episode_id;
    let memorial = entries
        .iter()
        .find(|episode| episode.situation_id == card_id && episode.episode_type == "memorial")?
        .episode_id;
    Some([standard, memorial])
}

fn import_area_items(buf: &[u8], player: &mut PlayerConfig) -> Result<(), ImportError> {
    for row in map_rows(buf)? {
        let Some(category) = row.get_u64("f2_3") else {
            continue;
        };
        player.area_item.insert(
            category.to_string(),
            AreaItemConfig {
                level: u8_from(row.get_u64("f2_4")),
            },
        );
    }
    Ok(())
}

fn import_character_potential(buf: &[u8], player: &mut PlayerConfig) -> Result<(), ImportError> {
    for row in map_rows(buf)? {
        let Some(character_id) = row.get_u64("f1") else {
            continue;
        };
        let bonus = player
            .character_bouns
            .entry(character_id.to_string())
            .or_insert_with(empty_character_bonus);
        bonus.potential = StatRate {
            performance: level_to_rate(row.get_u64("f2_1")),
            technique: level_to_rate(row.get_u64("f2_2")),
            visual: level_to_rate(row.get_u64("f2_3")),
        };
    }
    Ok(())
}

fn import_character_task_bonus_rates(
    buf: &[u8],
    player: &mut PlayerConfig,
) -> Result<(), ImportError> {
    for top in parse_fields(buf)? {
        if top.field != 1 || top.wire != 2 {
            continue;
        }

        let entry = parse_fields(top.bytes_value()?)?;
        let Some(value_buf) = field_bytes_value(&entry, 2) else {
            continue;
        };

        for bonus_item in parse_fields(value_buf)? {
            if bonus_item.field != 1 || bonus_item.wire != 2 {
                continue;
            }

            let fields = parse_fields(bonus_item.bytes_value()?)?;
            let Some(character_id) = field_u64(&fields, 1) else {
                continue;
            };
            let performance = field_fixed32_as_percent_rate(&fields, 3);
            let technique = field_fixed32_as_percent_rate(&fields, 4);
            let visual = field_fixed32_as_percent_rate(&fields, 5);
            if performance == 0.0 && technique == 0.0 && visual == 0.0 {
                continue;
            }

            let bonus = player
                .character_bouns
                .entry(character_id.to_string())
                .or_insert_with(empty_character_bonus);
            bonus.character_task.performance =
                round_rate(bonus.character_task.performance + performance);
            bonus.character_task.technique = round_rate(bonus.character_task.technique + technique);
            bonus.character_task.visual = round_rate(bonus.character_task.visual + visual);
        }
    }
    Ok(())
}

fn empty_character_bonus() -> CharacterBonusConfig {
    CharacterBonusConfig {
        potential: StatRate {
            performance: 0.0,
            technique: 0.0,
            visual: 0.0,
        },
        character_task: StatRate {
            performance: 0.0,
            technique: 0.0,
            visual: 0.0,
        },
    }
}

fn level_to_rate(value: Option<u64>) -> f64 {
    match value.unwrap_or_default() {
        0 | 1 => 0.0,
        value => value as f64 / 1000.0,
    }
}

fn round_rate(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn field_u64(fields: &[ProtoField], field_no: u64) -> Option<u64> {
    fields
        .iter()
        .find(|field| field.field == field_no)
        .and_then(|field| match &field.value {
            ProtoValue::Varint(value) => Some(*value),
            ProtoValue::Fixed64(value) => Some(*value),
            ProtoValue::Fixed32(value) => Some(u64::from(*value)),
            ProtoValue::Bytes(_) => None,
        })
}

fn field_bytes_value(fields: &[ProtoField], field_no: u64) -> Option<&[u8]> {
    fields
        .iter()
        .find(|field| field.field == field_no)
        .and_then(|field| match &field.value {
            ProtoValue::Bytes(value) => Some(value.as_slice()),
            _ => None,
        })
}

fn field_fixed32_as_percent_rate(fields: &[ProtoField], field_no: u64) -> f64 {
    let Some(raw) = fields
        .iter()
        .find(|field| field.field == field_no)
        .and_then(|field| match field.value {
            ProtoValue::Fixed32(value) => Some(value),
            _ => None,
        })
    else {
        return 0.0;
    };
    f64::from(f32::from_bits(raw)) / 100.0
}

fn u8_from(value: Option<u64>) -> u8 {
    value
        .and_then(|value| u8::try_from(value).ok())
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
struct ProtoField {
    field: u64,
    wire: u8,
    value: ProtoValue,
}

#[derive(Debug, Clone)]
enum ProtoValue {
    Varint(u64),
    Bytes(Vec<u8>),
    Fixed32(u32),
    Fixed64(u64),
}

impl ProtoField {
    fn bytes_value(&self) -> Result<&[u8], ImportError> {
        match &self.value {
            ProtoValue::Bytes(value) => Ok(value),
            _ => Err(ImportError::Protobuf(format!(
                "field {} is not length-delimited",
                self.field
            ))),
        }
    }
}

#[derive(Debug, Default)]
struct Row {
    values: BTreeMap<String, RowValue>,
}

#[derive(Debug, Clone)]
enum RowValue {
    U64(u64),
    String(String),
    Bytes(Vec<u8>),
}

impl Row {
    fn insert(&mut self, key: String, value: RowValue) {
        if !self.values.contains_key(&key) {
            self.values.insert(key, value);
            return;
        }
        let mut idx = 2;
        loop {
            let candidate = format!("{key}_{idx}");
            if !self.values.contains_key(&candidate) {
                self.values.insert(candidate, value);
                return;
            }
            idx += 1;
        }
    }

    fn get_u64(&self, key: &str) -> Option<u64> {
        match self.values.get(key) {
            Some(RowValue::U64(value)) => Some(*value),
            _ => None,
        }
    }

    fn get_str(&self, key: &str) -> Option<&str> {
        match self.values.get(key) {
            Some(RowValue::String(value)) => Some(value),
            _ => None,
        }
    }

    fn get_bytes(&self, key: &str) -> Option<&[u8]> {
        match self.values.get(key) {
            Some(RowValue::Bytes(value)) => Some(value),
            _ => None,
        }
    }
}

fn map_rows(buf: &[u8]) -> Result<Vec<Row>, ImportError> {
    let mut rows = Vec::new();
    for top in parse_fields(buf)? {
        if top.field != 1 || top.wire != 2 {
            continue;
        }
        let mut row = Row::default();
        for item in parse_fields(top.bytes_value()?)? {
            if item.wire == 2 {
                let data = item.bytes_value()?;
                if let Ok(text) = std::str::from_utf8(data) {
                    if text
                        .chars()
                        .all(|ch| ch >= ' ' || matches!(ch, '\r' | '\n' | '\t'))
                    {
                        row.insert(
                            format!("f{}", item.field),
                            RowValue::String(text.to_owned()),
                        );
                        continue;
                    }
                }
                flatten_message(&format!("f{}_", item.field), data, &mut row)?;
            } else {
                insert_scalar(&mut row, format!("f{}", item.field), &item.value);
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

fn flatten_message(prefix: &str, buf: &[u8], row: &mut Row) -> Result<(), ImportError> {
    for item in parse_fields(buf)? {
        let key = format!("{prefix}{}", item.field);
        if item.wire == 2 {
            let data = item.bytes_value()?;
            if let Ok(text) = std::str::from_utf8(data) {
                if text
                    .chars()
                    .all(|ch| ch >= ' ' || matches!(ch, '\r' | '\n' | '\t'))
                {
                    row.insert(key, RowValue::String(text.to_owned()));
                    continue;
                }
            }
            row.insert(key, RowValue::Bytes(data.to_vec()));
        } else {
            insert_scalar(row, key, &item.value);
        }
    }
    Ok(())
}

fn insert_scalar(row: &mut Row, key: String, value: &ProtoValue) {
    match value {
        ProtoValue::Varint(value) => row.insert(key, RowValue::U64(*value)),
        ProtoValue::Fixed32(value) => row.insert(key, RowValue::U64(u64::from(*value))),
        ProtoValue::Fixed64(value) => row.insert(key, RowValue::U64(*value)),
        ProtoValue::Bytes(value) => row.insert(key, RowValue::Bytes(value.clone())),
    }
}

fn parse_fields(buf: &[u8]) -> Result<Vec<ProtoField>, ImportError> {
    let mut fields = Vec::new();
    let mut off = 0;
    while off < buf.len() {
        let key = read_varint(buf, &mut off)?;
        let field = key >> 3;
        let wire = (key & 7) as u8;
        let value = match wire {
            0 => ProtoValue::Varint(read_varint(buf, &mut off)?),
            1 => {
                let data = read_exact(buf, &mut off, 8)?;
                ProtoValue::Fixed64(u64::from_le_bytes(data.try_into().unwrap()))
            }
            2 => {
                let size = read_varint(buf, &mut off)? as usize;
                ProtoValue::Bytes(read_exact(buf, &mut off, size)?.to_vec())
            }
            5 => {
                let data = read_exact(buf, &mut off, 4)?;
                ProtoValue::Fixed32(u32::from_le_bytes(data.try_into().unwrap()))
            }
            _ => {
                return Err(ImportError::Protobuf(format!(
                    "unsupported wire type {wire}"
                )))
            }
        };
        fields.push(ProtoField { field, wire, value });
    }
    Ok(fields)
}

fn read_varint(buf: &[u8], off: &mut usize) -> Result<u64, ImportError> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    while *off < buf.len() {
        let b = buf[*off];
        *off += 1;
        value |= u64::from(b & 0x7f) << shift;
        if b < 0x80 {
            return Ok(value);
        }
        shift += 7;
        if shift > 63 {
            break;
        }
    }
    Err(ImportError::Protobuf("bad varint".to_owned()))
}

fn read_exact<'a>(buf: &'a [u8], off: &mut usize, size: usize) -> Result<&'a [u8], ImportError> {
    if *off + size > buf.len() {
        return Err(ImportError::Protobuf("length overrun".to_owned()));
    }
    let data = &buf[*off..*off + size];
    *off += size;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_request_matches_expected_prefix_shape() {
        let persist = Persist {
            device_id: "device".to_owned(),
            sdk: PersistSdk {
                uid: "uid".to_owned(),
                access_key: "access".to_owned(),
            },
            unity: PersistUnity {
                unity_channel_id: "1".to_owned(),
                unity_platform_id: "2".to_owned(),
                client_platform: "Android".to_owned(),
                login_platform: "Android".to_owned(),
                operating_system: "Android OS".to_owned(),
                device_model: "model".to_owned(),
                client_version: "9.4.2".to_owned(),
                udid_short: "12345".to_owned(),
                client_package: "pkg".to_owned(),
            },
        };
        let bytes = build_login_request(&persist);
        assert!(bytes.starts_with(&[0x0a, 0x03, b'u', b'i', b'd']));
    }

    #[test]
    fn aes_round_trips_iso10126_payload() {
        let payload = b"hello bang dream";
        let encrypted = aes_encrypt_iso10126(payload).unwrap();
        assert_eq!(encrypted.len() % BLOCK_SIZE, 0);
        let decrypted = aes_decrypt_iso10126(&encrypted).unwrap();
        assert_eq!(decrypted, payload);
    }

    #[test]
    fn request_id_hint_allows_protobuf_prefix_bytes() {
        let plain = b"\x08\x95\x03\x12\x00\x1a[URI:/api/suite/user/1008159056][newRequestId:b2d7ad89db834332295064f18aa4c5ed] X-Requestid error.";
        assert_eq!(
            request_id_hint(plain).as_deref(),
            Some("b2d7ad89db834332295064f18aa4c5ed")
        );
    }

    #[test]
    fn imports_character_task_bonus_rates_from_field_456() {
        let payload = field_456_payload(vec![
            bonus_rate_item(1, 0, 2.4, 2.3, 2.2),
            bonus_rate_item(1, 1, 0.4, 0.4, 0.3),
        ]);
        let mut player = empty_player();

        import_character_task_bonus_rates(&payload, &mut player).unwrap();

        let task = &player.character_bouns["1"].character_task;
        assert_close(task.performance, 0.028);
        assert_close(task.technique, 0.027);
        assert_close(task.visual, 0.025);
    }

    #[test]
    fn potential_level_one_is_zero_rate() {
        assert_eq!(level_to_rate(None), 0.0);
        assert_eq!(level_to_rate(Some(0)), 0.0);
        assert_eq!(level_to_rate(Some(1)), 0.0);
        assert_eq!(level_to_rate(Some(50)), 0.05);
    }

    #[test]
    fn imports_episode_state_from_real_user_episode_ids() {
        let situations = user_situation_map_payload(3, 200);
        let episodes = user_episode_map_payload(&[(5, "already_read")]);
        let mut payload = Vec::new();
        // The real response puts userSituationMap before userEpisodeMap. The
        // importer must therefore collect field 16 before importing field 3.
        push_bytes_field(&mut payload, 3, &situations);
        push_bytes_field(&mut payload, 16, &episodes);

        let player = suite_user_to_player_config(1008159056, &payload, |card_id| {
            (card_id == 3).then_some([5, 6])
        })
        .unwrap();

        assert_eq!(player.card_list["3"].episodes, [true, false]);
    }

    #[test]
    fn append_parameter_fallback_does_not_mark_unread_episodes_as_read() {
        let payload = user_situation_map_payload(3, 0);
        let mut player = empty_player();
        let mut resolver = |_| None;

        import_user_situations(&payload, &mut player, None, &mut resolver).unwrap();

        assert_eq!(player.card_list["3"].episodes, [false, false]);
    }

    #[test]
    fn imports_second_episode_without_first_episode() {
        let situations = user_situation_map_payload(3, 500);
        let episodes = user_episode_map_payload(&[(6, "already_read")]);
        let mut payload = Vec::new();
        push_bytes_field(&mut payload, 3, &situations);
        push_bytes_field(&mut payload, 16, &episodes);

        let player = suite_user_to_player_config(1008159056, &payload, |card_id| {
            (card_id == 3).then_some([5, 6])
        })
        .unwrap();

        assert_eq!(player.card_list["3"].episodes, [false, true]);

        let mut fallback_player = empty_player();
        let mut no_episode_mapping = |_| None;
        import_user_situations(
            &situations,
            &mut fallback_player,
            None,
            &mut no_episode_mapping,
        )
        .unwrap();
        assert_eq!(fallback_player.card_list["3"].episodes, [false, true]);
    }

    #[test]
    fn reads_only_already_read_user_episode_rows() {
        let payload = user_episode_map_payload(&[(5, "already_read"), (6, "not_read")]);

        assert_eq!(
            import_read_episode_ids(&payload).unwrap(),
            BTreeSet::from([5])
        );
    }

    #[test]
    fn loads_episode_ids_from_bestdori_card_detail() {
        let cards_dir =
            std::env::temp_dir().join(format!("bangdream-account-card-episodes-{}", request_id()));
        fs::create_dir_all(&cards_dir).unwrap();
        let importer = BangDreamAccountImporter::new(cards_dir.join("persist.json"))
            .unwrap()
            .with_cards_dir(Some(cards_dir.clone()));

        // A game-data sync may finish after the importer is constructed. Missing
        // details must therefore not be cached permanently.
        assert_eq!(importer.episode_ids_for_card(2055), None);
        fs::write(
            cards_dir.join("2055.json"),
            r#"{
                "episodes": {
                    "entries": [
                        {"episodeId":3321,"episodeType":"standard","situationId":2055},
                        {"episodeId":3322,"episodeType":"memorial","situationId":2055}
                    ]
                }
            }"#,
        )
        .unwrap();

        assert_eq!(importer.episode_ids_for_card(2055), Some([3321, 3322]));

        fs::remove_file(cards_dir.join("2055.json")).unwrap();
        fs::remove_dir(cards_dir).unwrap();
    }

    fn empty_player() -> PlayerConfig {
        PlayerConfig {
            mongo_id: None,
            player_id: 0,
            current_event: None,
            event_songs: BTreeMap::new(),
            event_presets: BTreeMap::new(),
            event_overrides: BTreeMap::new(),
            card_list: BTreeMap::new(),
            area_item: BTreeMap::new(),
            character_bouns: BTreeMap::new(),
        }
    }

    fn field_456_payload(items: Vec<Vec<u8>>) -> Vec<u8> {
        let mut value = Vec::new();
        for item in items {
            push_bytes_field(&mut value, 1, &item);
        }

        let mut entry = Vec::new();
        push_varint_field(&mut entry, 1, 1);
        push_bytes_field(&mut entry, 2, &value);

        let mut payload = Vec::new();
        push_bytes_field(&mut payload, 1, &entry);
        payload
    }

    fn user_situation_map_payload(card_id: u64, append: u64) -> Vec<u8> {
        let mut append_parameter = Vec::new();
        push_varint_field(&mut append_parameter, 1, 1008159056);
        push_varint_field(&mut append_parameter, 2, card_id);
        push_varint_field(&mut append_parameter, 3, append);
        push_varint_field(&mut append_parameter, 4, append);
        push_varint_field(&mut append_parameter, 5, append);

        let mut situation = Vec::new();
        push_varint_field(&mut situation, 1, 1008159056);
        push_varint_field(&mut situation, 2, card_id);
        push_varint_field(&mut situation, 3, 1);
        push_bytes_field(&mut situation, 7, b"not_doing");
        push_bytes_field(&mut situation, 9, b"normal");
        push_varint_field(&mut situation, 11, 1);
        push_bytes_field(&mut situation, 12, &append_parameter);
        push_varint_field(&mut situation, 13, 0);

        map_payload(card_id, &situation)
    }

    fn user_episode_map_payload(rows: &[(u64, &str)]) -> Vec<u8> {
        let mut payload = Vec::new();
        for (episode_id, status) in rows {
            let mut episode = Vec::new();
            push_varint_field(&mut episode, 1, 1008159056);
            push_varint_field(&mut episode, 2, *episode_id);
            push_bytes_field(&mut episode, 3, status.as_bytes());

            let mut entry = Vec::new();
            push_varint_field(&mut entry, 1, *episode_id);
            push_bytes_field(&mut entry, 2, &episode);
            push_bytes_field(&mut payload, 1, &entry);
        }
        payload
    }

    fn map_payload(key: u64, value: &[u8]) -> Vec<u8> {
        let mut entry = Vec::new();
        push_varint_field(&mut entry, 1, key);
        push_bytes_field(&mut entry, 2, value);

        let mut payload = Vec::new();
        push_bytes_field(&mut payload, 1, &entry);
        payload
    }

    fn bonus_rate_item(
        character_id: u64,
        bonus_type: u64,
        performance: f32,
        technique: f32,
        visual: f32,
    ) -> Vec<u8> {
        let mut item = Vec::new();
        push_varint_field(&mut item, 1, character_id);
        push_varint_field(&mut item, 2, bonus_type);
        push_fixed32_field(&mut item, 3, performance.to_bits());
        push_fixed32_field(&mut item, 4, technique.to_bits());
        push_fixed32_field(&mut item, 5, visual.to_bits());
        item
    }

    fn push_varint_field(out: &mut Vec<u8>, field: u64, value: u64) {
        push_varint(out, field << 3);
        push_varint(out, value);
    }

    fn push_bytes_field(out: &mut Vec<u8>, field: u64, value: &[u8]) {
        push_varint(out, (field << 3) | 2);
        push_varint(out, value.len() as u64);
        out.extend_from_slice(value);
    }

    fn push_fixed32_field(out: &mut Vec<u8>, field: u64, value: u32) {
        push_varint(out, (field << 3) | 5);
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.000001,
            "actual={actual} expected={expected}"
        );
    }
}
