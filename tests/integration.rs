//! Consolidated integration-test entrypoint.
//!
//! Each case file in `tests/cases/` is pulled in as a module so the whole
//! suite compiles and links as a single test binary. Cargo's default layout
//! treats every `tests/*.rs` as a separate binary, which forces ~70 link
//! steps for any lib change. One binary collapses that into one link.

#[path = "cases/scenario_conversation.rs"]
mod scenario_conversation;

#[path = "cases/scenario_greetings.rs"]
mod scenario_greetings;

#[path = "cases/scenario_learning.rs"]
mod scenario_learning;

#[path = "cases/test_affective_tom.rs"]
mod test_affective_tom;

#[path = "cases/test_anticipation_forecast.rs"]
mod test_anticipation_forecast;

#[path = "cases/test_becomes_substrate.rs"]
mod test_becomes_substrate;

#[path = "cases/test_belief_state.rs"]
mod test_belief_state;

#[path = "cases/test_bite_excludes_dead.rs"]
mod test_bite_excludes_dead;

#[path = "cases/test_brain_cadence_split.rs"]
mod test_brain_cadence_split;

#[path = "cases/test_brain_target_respected.rs"]
mod test_brain_target_respected;

#[path = "cases/test_campfire_fuel.rs"]
mod test_campfire_fuel;

#[path = "cases/test_campfire_ownership.rs"]
mod test_campfire_ownership;

#[path = "cases/test_cooking.rs"]
mod test_cooking;

#[path = "cases/test_culture.rs"]
mod test_culture;

#[path = "cases/test_default_sim_survival.rs"]
mod test_default_sim_survival;

#[path = "cases/test_defend_self.rs"]
mod test_defend_self;

#[path = "cases/test_deposit_and_take.rs"]
mod test_deposit_and_take;

#[path = "cases/test_despawn_cancels_action.rs"]
mod test_despawn_cancels_action;

#[path = "cases/test_eat_harvest_cycle.rs"]
mod test_eat_harvest_cycle;

#[path = "cases/test_effort_model.rs"]
mod test_effort_model;

#[path = "cases/test_emits_effect_substrate.rs"]
mod test_emits_effect_substrate;

#[path = "cases/test_entity_emotions.rs"]
mod test_entity_emotions;

#[path = "cases/test_flocking.rs"]
mod test_flocking;

#[path = "cases/test_food_security_drive.rs"]
mod test_food_security_drive;

#[path = "cases/test_fresh_agent_urgency_bootstrap.rs"]
mod test_fresh_agent_urgency_bootstrap;

#[path = "cases/test_graze.rs"]
mod test_graze;

#[path = "cases/test_harvest_empty_belief_update.rs"]
mod test_harvest_empty_belief_update;

#[path = "cases/test_harvest_knowledge.rs"]
mod test_harvest_knowledge;

#[path = "cases/test_harvestable_materials.rs"]
mod test_harvestable_materials;

#[path = "cases/test_human_actions.rs"]
mod test_human_actions;

#[path = "cases/test_hunger_timing.rs"]
mod test_hunger_timing;

#[path = "cases/test_hunting_loop.rs"]
mod test_hunting_loop;

#[path = "cases/test_item_properties.rs"]
mod test_item_properties;

#[path = "cases/test_labor_accumulation.rs"]
mod test_labor_accumulation;

#[path = "cases/test_locomotion_intensity.rs"]
mod test_locomotion_intensity;

#[path = "cases/test_look_for_fallback.rs"]
mod test_look_for_fallback;

#[path = "cases/test_main_menu.rs"]
mod test_main_menu;

#[path = "cases/test_movement.rs"]
mod test_movement;

#[path = "cases/test_multi_sense_perception.rs"]
mod test_multi_sense_perception;

#[path = "cases/test_observability.rs"]
mod test_observability;

#[path = "cases/test_other_regarding.rs"]
mod test_other_regarding;

#[path = "cases/test_perception_cache.rs"]
mod test_perception_cache;

#[path = "cases/test_plan_invalidation.rs"]
mod test_plan_invalidation;

#[path = "cases/test_plan_memory_integration.rs"]
mod test_plan_memory_integration;

#[path = "cases/test_player_controlled_marker.rs"]
mod test_player_controlled_marker;

#[path = "cases/test_reactive_drift.rs"]
mod test_reactive_drift;

#[path = "cases/test_recipe_and_build.rs"]
mod test_recipe_and_build;

#[path = "cases/test_relationship_decay.rs"]
mod test_relationship_decay;

#[path = "cases/test_rest_completion.rs"]
mod test_rest_completion;

#[path = "cases/test_rest_quality_drive.rs"]
mod test_rest_quality_drive;

#[path = "cases/test_satiated_no_eat_plans.rs"]
mod test_satiated_no_eat_plans;

#[path = "cases/test_satiation_gate.rs"]
mod test_satiation_gate;

#[path = "cases/test_second_human_group.rs"]
mod test_second_human_group;

#[path = "cases/test_sim_events.rs"]
mod test_sim_events;

#[path = "cases/test_skills.rs"]
mod test_skills;

#[path = "cases/test_sleep_prep.rs"]
mod test_sleep_prep;

#[path = "cases/test_sleep_pressure.rs"]
mod test_sleep_pressure;

#[path = "cases/test_sleep_wake_cycle.rs"]
mod test_sleep_wake_cycle;

#[path = "cases/test_smart_combat.rs"]
mod test_smart_combat;

#[path = "cases/test_species_capabilities.rs"]
mod test_species_capabilities;

#[path = "cases/test_stamina_alertness_split.rs"]
mod test_stamina_alertness_split;

#[path = "cases/test_temperature_grid.rs"]
mod test_temperature_grid;

#[path = "cases/test_theory_of_mind.rs"]
mod test_theory_of_mind;

#[path = "cases/test_thirst_drain.rs"]
mod test_thirst_drain;

#[path = "cases/test_unified_death.rs"]
mod test_unified_death;

#[path = "cases/test_walker_path_blocked.rs"]
mod test_walker_path_blocked;

#[path = "cases/test_warmth_drive.rs"]
mod test_warmth_drive;

#[path = "cases/test_wolf.rs"]
mod test_wolf;

#[path = "cases/test_wolf_devours_corpse.rs"]
mod test_wolf_devours_corpse;

#[path = "cases/test_world_entity_properties.rs"]
mod test_world_entity_properties;
