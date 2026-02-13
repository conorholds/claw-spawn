use crate::domain::{AlgorithmMode, AssetFocus, Persona, StrictnessLevel, SubscriptionTier};

pub(super) fn parse_subscription_tier(tier: &str) -> Option<SubscriptionTier> {
    match tier {
        "free" => Some(SubscriptionTier::Free),
        "basic" => Some(SubscriptionTier::Basic),
        "pro" => Some(SubscriptionTier::Pro),
        _ => None,
    }
}

pub(super) fn parse_persona(persona: &str) -> Option<Persona> {
    match persona {
        "beginner" => Some(Persona::Beginner),
        "tweaker" => Some(Persona::Tweaker),
        "quant_lite" => Some(Persona::QuantLite),
        _ => None,
    }
}

pub(super) fn parse_asset_focus(asset_focus: &str) -> Option<AssetFocus> {
    match asset_focus {
        "majors" => Some(AssetFocus::Majors),
        "memes" => Some(AssetFocus::Memes),
        _ => None,
    }
}

pub(super) fn parse_algorithm(algorithm: &str) -> Option<AlgorithmMode> {
    match algorithm {
        "trend" => Some(AlgorithmMode::Trend),
        "mean_reversion" => Some(AlgorithmMode::MeanReversion),
        "breakout" => Some(AlgorithmMode::Breakout),
        _ => None,
    }
}

pub(super) fn parse_strictness(strictness: &str) -> Option<StrictnessLevel> {
    match strictness {
        "low" => Some(StrictnessLevel::Low),
        "medium" => Some(StrictnessLevel::Medium),
        "high" => Some(StrictnessLevel::High),
        _ => None,
    }
}
