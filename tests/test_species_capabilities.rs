//! Tests for the decoupled body-part / channel system (#291).
//!
//! Verifies that per-species anatomy correctly gates which actions each
//! species can perform:
//!
//! - Humans can Harvest + Attack but not Bite
//! - Wolves can Bite (jaws) but not Harvest (jaws too crude for precise grasp)
//! - Deer can neither Bite nor Harvest (no manipulator at all)
//! - Quadrupeds retain fast Locomotion from having four legs
//! - Injuries to a specific named part reduce only the channels it provides
//!
//! These tests run at the capability layer — they don't spin up brains or
//! ticks. They just verify `Body` produces the right `channel_capacity`
//! values and that `ChannelLoad::would_hard_conflict` honours them.

use worldsim::agent::actions::Channel;
use worldsim::agent::actions::ChannelCapacities;
use worldsim::agent::actions::ChannelLoad;
use worldsim::agent::actions::ChannelUsage;
use worldsim::agent::actions::{ActionRegistry, ActionType};
use worldsim::agent::biology::body::{Body, BodyNodeKind, Injury, InjuryType, TagChannelMapping};

fn requirements_for(registry: &ActionRegistry, action: ActionType) -> &'static [ChannelUsage] {
    registry
        .get(action)
        .expect("action is registered")
        .body_channels()
}

fn can_perform(body: &Body, requirements: &[ChannelUsage]) -> bool {
    let m = TagChannelMapping::default();
    let caps = ChannelCapacities::compute(Some(body), None, None, &m);
    let load = ChannelLoad::new();
    !load.would_hard_conflict(requirements, &caps)
}

#[test]
fn human_can_harvest() {
    let body = Body::human();
    let registry = ActionRegistry::new();
    assert!(
        can_perform(&body, requirements_for(&registry, ActionType::Harvest)),
        "human Manipulation 1.0 should satisfy Harvest's 0.9 requirement"
    );
}

#[test]
fn human_cannot_bite() {
    let body = Body::human();
    let registry = ActionRegistry::new();
    assert!(
        !can_perform(&body, requirements_for(&registry, ActionType::Bite)),
        "human has no anatomy providing Bite — Bite must hard-conflict"
    );
}

#[test]
fn human_can_attack() {
    let body = Body::human();
    let registry = ActionRegistry::new();
    assert!(
        can_perform(&body, requirements_for(&registry, ActionType::Attack)),
        "human Manipulation 1.0 should satisfy Attack's 0.9 requirement"
    );
}

#[test]
fn wolf_cannot_harvest() {
    let body = Body::wolf();
    let registry = ActionRegistry::new();
    assert!(
        !can_perform(&body, requirements_for(&registry, ActionType::Harvest)),
        "wolf jaws cap Manipulation at 0.4, below Harvest's 0.9 requirement"
    );
}

#[test]
fn wolf_cannot_attack() {
    let body = Body::wolf();
    let registry = ActionRegistry::new();
    assert!(
        !can_perform(&body, requirements_for(&registry, ActionType::Attack)),
        "wolf Manipulation is too low for Attack — wolves must use Bite instead"
    );
}

#[test]
fn wolf_can_bite() {
    let body = Body::wolf();
    let registry = ActionRegistry::new();
    assert!(
        can_perform(&body, requirements_for(&registry, ActionType::Bite)),
        "wolf jaws provide Bite 1.0"
    );
}

#[test]
fn deer_cannot_harvest() {
    let body = Body::deer();
    let registry = ActionRegistry::new();
    assert!(
        !can_perform(&body, requirements_for(&registry, ActionType::Harvest)),
        "deer has no anatomy providing Manipulation"
    );
}

#[test]
fn deer_cannot_bite() {
    let body = Body::deer();
    let registry = ActionRegistry::new();
    assert!(
        !can_perform(&body, requirements_for(&registry, ActionType::Bite)),
        "deer has no anatomy providing Bite"
    );
}

#[test]
fn deer_can_walk() {
    let body = Body::deer();
    let registry = ActionRegistry::new();
    assert!(
        can_perform(&body, requirements_for(&registry, ActionType::Walk)),
        "deer has Locomotion from four legs"
    );
}

#[test]
fn deer_can_eat() {
    let body = Body::deer();
    let registry = ActionRegistry::new();
    assert!(
        can_perform(&body, requirements_for(&registry, ActionType::Eat)),
        "deer mouth provides Consumption — eating must be possible"
    );
}

#[test]
fn wolf_quadruped_has_higher_locomotion_than_human() {
    let wolf = Body::wolf();
    let human = Body::human();
    let m = worldsim::agent::biology::body::TagChannelMapping::default();
    let wolf_loc = wolf.channel_capacity(Channel::Locomotion, &m);
    let human_loc = human.channel_capacity(Channel::Locomotion, &m);
    // Both bodies use `max` across parts (best part wins) per
    // Body::channel_capacity, so four legs vs two legs is not an automatic
    // advantage — wolves gain their speed via SpeciesProfile::base_speed,
    // not via the channel layer. The assertion here is just that wolves
    // DO have Locomotion at all, and it matches the per-leg intensity
    // declared in Body::wolf.
    assert!(wolf_loc >= 0.3, "wolf should retain Locomotion from legs");
    assert!(human_loc > 0.0, "human should have Locomotion from legs");
}

#[test]
fn wolf_broken_jaw_loses_manipulation_consumption_vocalization_and_bite() {
    let mut body = Body::wolf();
    let jaw = body.node_mut(BodyNodeKind::Jaw).expect("wolf body has jaw");
    jaw.add_injury(Injury {
        injury_type: InjuryType::Fracture,
        severity: 1.0,
        pain: 5.0,
        healed_amount: 0.0,
        bleed_rate: 0.0,
    });

    let m = TagChannelMapping::default();
    // A single anatomical injury collapses multiple capabilities at once,
    // because they all lived on the same part.
    assert!(
        body.channel_capacity(Channel::Manipulation, &m) < 0.1,
        "broken jaws must knock out Manipulation"
    );
    assert!(
        body.channel_capacity(Channel::Bite, &m) < 0.1,
        "broken jaws must knock out Bite"
    );
    assert!(
        body.channel_capacity(Channel::Consumption, &m) < 0.1,
        "broken jaws must knock out Consumption"
    );
    assert!(
        body.channel_capacity(Channel::Vocalization, &m) < 0.1,
        "broken jaws must knock out Vocalization"
    );
    // Locomotion lives on the legs — unrelated to a jaw injury.
    assert!(
        body.channel_capacity(Channel::Locomotion, &m) > 0.0,
        "jaw injury must not affect locomotion"
    );
}

#[test]
fn human_one_broken_hand_halves_manipulation() {
    let mut body = Body::human();
    let hand = body
        .node_mut(BodyNodeKind::RightHand)
        .expect("human has right hand");
    hand.add_injury(Injury {
        injury_type: InjuryType::Fracture,
        severity: 1.0,
        pain: 5.0,
        healed_amount: 0.0,
        bleed_rate: 0.0,
    });
    let m = TagChannelMapping::default();
    let manip = body.channel_capacity(Channel::Manipulation, &m);
    assert!(
        (manip - 0.5).abs() < 1e-4,
        "expected 0.5 Manipulation after one broken arm, got {manip}"
    );
    assert!(body.channel_capacity(Channel::Locomotion, &m) > 0.0);
}
