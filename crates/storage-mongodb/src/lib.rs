use async_trait::async_trait;
use bangdream_optimize_core::PlayerConfig;
use bangdream_optimize_data::{DataError, PlayerConfigStore};
use mongodb::{
    bson::{doc, Bson, Document},
    Client, Collection,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MongoStoreError {
    #[error("mongodb error: {0}")]
    Mongo(#[from] mongodb::error::Error),
}

pub struct MongoPlayerConfigStore {
    collection: Collection<PlayerConfig>,
}

impl MongoPlayerConfigStore {
    pub async fn connect(uri: &str, db_name: &str) -> Result<Self, MongoStoreError> {
        let client = Client::with_uri_str(uri).await?;
        let collection = client.database(db_name).collection("players");
        Ok(Self { collection })
    }

    pub async fn get(&self, player_id: i64) -> Result<Option<PlayerConfig>, MongoStoreError> {
        Ok(self.collection.find_one(doc! { "_id": player_id }).await?)
    }

    pub async fn sample_calculation_player_id(&self) -> Result<Option<i64>, MongoStoreError> {
        let filter = doc! {
            "currentEvent": { "$exists": true },
            "eventSongs": { "$exists": true, "$ne": doc! {} },
            "cardList": { "$exists": true, "$ne": doc! {} },
        };
        Ok(self
            .collection
            .find_one(filter)
            .await?
            .map(|player| player.player_id))
    }

    pub async fn large_calculation_players(
        &self,
        min_cards: i32,
        limit: i64,
    ) -> Result<Vec<LargePlayerSummary>, MongoStoreError> {
        let pipeline = vec![
            doc! {
                "$match": {
                    "currentEvent": { "$exists": true },
                    "eventSongs": { "$exists": true, "$ne": doc! {} },
                    "cardList": { "$exists": true, "$ne": doc! {} },
                }
            },
            doc! {
                "$project": {
                    "_id": 1,
                    "playerId": 1,
                    "currentEvent": 1,
                    "cardCount": { "$size": { "$objectToArray": "$cardList" } },
                    "eventSongCount": { "$size": { "$objectToArray": "$eventSongs" } },
                }
            },
            doc! { "$match": { "cardCount": { "$gte": min_cards } } },
            doc! { "$sort": { "cardCount": -1 } },
            doc! { "$limit": limit.max(1) },
        ];

        let mut cursor = self.collection.aggregate(pipeline).await?;
        let mut players = Vec::new();
        while cursor.advance().await? {
            let document: Document = cursor.deserialize_current()?;
            players.push(LargePlayerSummary::from_document(document));
        }

        Ok(players)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargePlayerSummary {
    pub player_id: i64,
    pub current_event: Option<u32>,
    pub card_count: i32,
    pub event_song_count: i32,
}

impl LargePlayerSummary {
    fn from_document(document: Document) -> Self {
        Self {
            player_id: integer_field(&document, "playerId")
                .or_else(|| integer_field(&document, "_id"))
                .unwrap_or_default(),
            current_event: integer_field(&document, "currentEvent").map(|value| value as u32),
            card_count: integer_field(&document, "cardCount").unwrap_or_default() as i32,
            event_song_count: integer_field(&document, "eventSongCount").unwrap_or_default() as i32,
        }
    }
}

fn integer_field(document: &Document, key: &str) -> Option<i64> {
    match document.get(key)? {
        Bson::Int32(value) => Some(*value as i64),
        Bson::Int64(value) => Some(*value),
        Bson::Double(value) => Some(*value as i64),
        Bson::String(value) => value.parse().ok(),
        _ => None,
    }
}

#[async_trait]
impl PlayerConfigStore for MongoPlayerConfigStore {
    async fn get_player_config(&self, player_id: i64) -> Result<Option<PlayerConfig>, DataError> {
        self.get(player_id).await.map_err(|err| DataError::Storage {
            message: err.to_string(),
        })
    }
}
