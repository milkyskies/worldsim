//! Locked 24-color palette resource. Single source of truth for every drawn color.
//!
//! Reads: assets/palette.ron (embedded via include_str! at compile time)
//! Writes: Palette resource
//! Upstream: PalettePlugin (added by main.rs run_windowed and by TestWorld)
//! Downstream: every renderer (creature spawns, terrain tiles, UI accents)

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Canonical RON source. Baked into the binary so headless and tests have
/// the same palette as the windowed game without filesystem access.
const PALETTE_RON: &str = include_str!("../assets/palette.ron");

/// Every named color slot in the game. The simulation never references raw
/// RGB - every Sprite/Mesh color goes through one of these.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum PaletteColor {
    SkinPale,
    SkinFair,
    SkinTan,
    SkinMedium,
    SkinDark,
    SkinDeep,
    FurWhite,
    FurLightGrey,
    FurGrey,
    FurSlate,
    FurCharcoal,
    FurBlack,
    LeafBright,
    LeafForest,
    LeafBush,
    LeafDeep,
    BloodFresh,
    BloodDried,
    ScarLight,
    ScarDark,
    WaterShallow,
    WaterDeep,
    AccentFlame,
    AccentBerry,
}

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    colors: HashMap<PaletteColor, (f32, f32, f32)>,
}

impl Palette {
    pub fn srgb(&self, slot: PaletteColor) -> Color {
        let (r, g, b) = self.lookup(slot);
        Color::srgb(r, g, b)
    }

    pub fn srgba(&self, slot: PaletteColor, alpha: f32) -> Color {
        let (r, g, b) = self.lookup(slot);
        Color::srgba(r, g, b, alpha)
    }

    /// Standard ground shadow under entities. Centralized so every spawn
    /// fn drops the same visual instead of each one hand-rolling FurBlack at 0.35.
    pub fn shadow(&self) -> Color {
        self.srgba(PaletteColor::FurBlack, 0.35)
    }

    fn lookup(&self, slot: PaletteColor) -> (f32, f32, f32) {
        *self.colors.get(&slot).unwrap_or_else(|| {
            panic!(
                "palette is missing slot {slot:?} - palette.ron and PaletteColor enum are out of sync"
            )
        })
    }
}

impl Default for Palette {
    /// Loads the embedded RON. Panics on parse failure - the embedded source
    /// is part of the binary, so a parse failure is a programmer error, not
    /// runtime input.
    fn default() -> Self {
        ron::from_str(PALETTE_RON)
            .unwrap_or_else(|e| panic!("embedded palette.ron failed to parse: {e}"))
    }
}

pub struct PalettePlugin;

impl Plugin for PalettePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Palette>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgba_applies_alpha() {
        let p = Palette::default();
        let c = p.srgba(PaletteColor::FurGrey, 0.35);
        let srgba = c.to_srgba();
        assert!((srgba.alpha - 0.35).abs() < 1e-5);
    }
}
