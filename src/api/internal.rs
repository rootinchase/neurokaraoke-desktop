use serde::{Deserialize, Deserializer};
use std::sync::Arc;
use uuid::Uuid;


// WHY????? I DON'T KNOW??????
#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum PossiblyWithId {
    NoId(Arc<str>),
    Id { id: Option<Uuid>, name: Arc<str> },
}

impl From<PossiblyWithId> for Arc<str> {
    fn from(value: PossiblyWithId) -> Self {
        match value {
            PossiblyWithId::NoId(name) => name,
            PossiblyWithId::Id { name, .. } => name,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MaybeArtists {
    // The API might send a single string or an array
    Single(Arc<str>),
    List(Vec<PossiblyWithId>),
    Optional(Option<Vec<PossiblyWithId>>),
}

pub fn deserialize_artists<'de, D>(d: D) -> Result<Arc<[Arc<str>]>, D::Error>
where
    D: Deserializer<'de>,
{
    let artists = match MaybeArtists::deserialize(d)? {
        MaybeArtists::Single(name) => vec![PossiblyWithId::NoId(name)],
        MaybeArtists::List(artists) => artists,
        MaybeArtists::Optional(artists) => artists.unwrap_or_default(),
    };

    Ok(artists
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>()
        .into())
}
