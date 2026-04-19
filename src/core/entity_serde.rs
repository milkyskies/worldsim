//! Serde helper: serialize Bevy `Entity` as `{"name": "...", "id": "..."}`
//! using a thread-local name resolver.
//!
//! Reads: nothing (pure serialization)
//! Writes: nothing
//! Upstream: nothing
//! Downstream: `event_log::collect_event_log` wraps each serialize call with
//!             `with_resolver`, so Entity fields anywhere in a SimEvent produce
//!             `{name, id}` objects with agent names filled in.

use bevy::ecs::entity::Entity;
use serde::Serializer;
use serde::ser::SerializeStruct;
use std::cell::RefCell;

type Resolver = Box<dyn Fn(Entity) -> String>;

thread_local! {
    static ENTITY_RESOLVER: RefCell<Option<Resolver>> = const { RefCell::new(None) };
}

/// Run `f` with a thread-local resolver installed. Entity fields serialized
/// inside `f` will include the resolved name; outside the closure,
/// serialization falls back to the stable `Entity` debug string in both
/// `name` and `id` slots.
pub fn with_resolver<R>(resolver: impl Fn(Entity) -> String + 'static, f: impl FnOnce() -> R) -> R {
    ENTITY_RESOLVER.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(resolver));
    });
    let out = f();
    ENTITY_RESOLVER.with(|cell| {
        *cell.borrow_mut() = None;
    });
    out
}

/// Stable id string — matches Bevy's Debug format (`<index>v<generation>`).
pub fn entity_id_str(entity: Entity) -> String {
    format!("{entity:?}")
}

fn resolve_name(entity: Entity) -> String {
    ENTITY_RESOLVER.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|r| r(entity))
            .unwrap_or_else(|| entity_id_str(entity))
    })
}

/// `serialize_with` target for `Entity` fields. Emits `{name, id}`.
pub fn serialize_entity<S: Serializer>(entity: &Entity, ser: S) -> Result<S::Ok, S::Error> {
    let mut s = ser.serialize_struct("EntityRef", 2)?;
    s.serialize_field("name", &resolve_name(*entity))?;
    s.serialize_field("id", &entity_id_str(*entity))?;
    s.end()
}

/// Same but for `Option<Entity>`.
pub fn serialize_entity_opt<S: Serializer>(
    entity: &Option<Entity>,
    ser: S,
) -> Result<S::Ok, S::Error> {
    match entity {
        Some(e) => serialize_entity(e, ser),
        None => ser.serialize_none(),
    }
}

/// Same but for `Vec<Entity>` — emits `[{name, id}, …]`.
pub fn serialize_entity_vec<S: Serializer>(
    entities: &Vec<Entity>,
    ser: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = ser.serialize_seq(Some(entities.len()))?;
    for e in entities {
        seq.serialize_element(&EntityRef {
            name: resolve_name(*e),
            id: entity_id_str(*e),
        })?;
    }
    seq.end()
}

#[derive(serde::Serialize)]
struct EntityRef {
    name: String,
    id: String,
}

/// Summarize `Arc<Vec<BrainProposal>>` for the event log — only the four
/// displayed fields (brain, action name, urgency, reasoning), avoiding
/// the need to derive `Serialize` on `ActionTemplate` and its tree of
/// sub-types.
pub fn serialize_brain_proposals<S: Serializer>(
    proposals: &std::sync::Arc<Vec<crate::agent::brains::proposal::BrainProposal>>,
    ser: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut seq = ser.serialize_seq(Some(proposals.len()))?;
    for p in proposals.iter() {
        seq.serialize_element(&BrainProposalSummary {
            brain: format!("{:?}", p.brain),
            action: p.action.name.clone(),
            urgency: p.urgency,
            reasoning: &p.reasoning,
        })?;
    }
    seq.end()
}

#[derive(serde::Serialize)]
struct BrainProposalSummary<'a> {
    brain: String,
    action: String,
    urgency: f32,
    reasoning: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::entity::Entity;
    use serde::Serialize;

    #[derive(Serialize)]
    struct Wrap {
        #[serde(serialize_with = "serialize_entity")]
        e: Entity,
    }

    #[test]
    fn entity_serializes_with_resolved_name_inside_resolver_scope() {
        let e = Entity::from_raw_u32(7).unwrap();
        let out = with_resolver(
            |_| "Alice".to_string(),
            || serde_json::to_string(&Wrap { e }).unwrap(),
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["e"]["name"], "Alice");
        assert_eq!(v["e"]["id"], entity_id_str(e));
    }

    #[test]
    fn entity_falls_back_to_id_without_resolver() {
        let e = Entity::from_raw_u32(7).unwrap();
        let out = serde_json::to_string(&Wrap { e }).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["e"]["name"], v["e"]["id"]);
    }
}
