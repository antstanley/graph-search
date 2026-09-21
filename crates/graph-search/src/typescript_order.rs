//! Preserve authored wildcard precedence without an ordered-map dependency.
use serde::Deserialize;
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};

// Only object key order is needed. Arrays and scalar values are discarded on
// this second pass; the ordinary JSON decoder already validated the document.
#[derive(Default)]
struct ObjectOrder(Vec<(String, ObjectOrder)>);
impl ObjectOrder {
    fn last(&self, key: &str) -> Option<&Self> {
        self.0
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
}
struct OrderVisitor;
impl<'de> Visitor<'de> for OrderVisitor {
    type Value = ObjectOrder;
    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("validated JSON")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut result = Vec::new();
        while let Some(key) = map.next_key()? {
            result.push((key, map.next_value()?));
        }
        Ok(ObjectOrder(result))
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<Self::Value, S::Error> {
        while sequence.next_element::<IgnoredAny>()?.is_some() {}
        Ok(ObjectOrder::default())
    }
    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(ObjectOrder::default())
    }
    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(ObjectOrder::default())
    }
    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(ObjectOrder::default())
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(ObjectOrder::default())
    }
    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(ObjectOrder::default())
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(ObjectOrder::default())
    }
}
impl<'de> Deserialize<'de> for ObjectOrder {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(OrderVisitor)
    }
}

pub(crate) fn patterns(bytes: &[u8]) -> Result<Vec<String>, serde_json::Error> {
    let root: ObjectOrder = serde_json::from_slice(bytes)?;
    let mut seen = std::collections::BTreeSet::new();
    Ok(root
        .last("compilerOptions")
        .and_then(|options| options.last("paths"))
        .into_iter()
        .flat_map(|paths| &paths.0)
        // Updating an existing JS object property does not move its first slot.
        .filter(|(key, _)| key.contains('*') && seen.insert(key.as_str()))
        .map(|(key, _)| key.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_keep_first_property_order_but_last_duplicate_container_value() {
        let text = br#"{"compilerOptions":{"paths":{"unused*":[]}},"compilerOptions":{"paths":{"old*":[]},"paths":{"a*Z":["first"],"a*YZ":["second"],"a*Z":["last"],"exact":[]}}}"#;
        assert_eq!(patterns(text).unwrap(), ["a*Z", "a*YZ"]);
        assert_eq!(
            patterns(
                br#"{"compilerOptions":{"paths":{"a*YZ":[],"a*Z":[]}},"other":[{"ignore*":[]}]}"#
            )
            .unwrap(),
            ["a*YZ", "a*Z"]
        );
    }
}
