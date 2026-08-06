use std::sync::Arc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{DeserializeAs, SerializeAs};
use tokio::sync::Mutex;

// for use with #[serde_as(as = ...)]
pub struct IntoAs<T>(std::marker::PhantomData<T>);
impl<'de, U, T: Deserialize<'de> + Into<U>> DeserializeAs<'de, U> for IntoAs<T> {
    fn deserialize_as<D>(deserializer: D) -> Result<U, D::Error>
    where
        D: Deserializer<'de>
    {
        Ok(T::deserialize(deserializer)?.into())
    }
}
impl<U, T: Serialize> SerializeAs<U> for IntoAs<T>
where
    for<'a> T: From<&'a U>,
{
    fn serialize_as<S>(source: &U, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer
    {
        T::from(source).serialize(serializer)
    }
}

pub struct AsArcMutex<T>(std::marker::PhantomData<T>);
impl<'de, T: Deserialize<'de>> DeserializeAs<'de, Arc<Mutex<T>>> for AsArcMutex<T> {
    fn deserialize_as<D>(deserializer: D) -> Result<Arc<Mutex<T>>, D::Error>
    where
        D: Deserializer<'de>
    {
        Ok(Arc::new(Mutex::new(T::deserialize(deserializer)?)))
    }
}
impl<T: Serialize> SerializeAs<Arc<Mutex<T>>> for AsArcMutex<T> {
    fn serialize_as<S>(source: &Arc<Mutex<T>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer
    {
        (*source.blocking_lock()).serialize(serializer)
    }
}