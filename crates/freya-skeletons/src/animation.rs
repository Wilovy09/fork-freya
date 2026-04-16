use freya_components::theming::{
    component_themes::ColorsSheet,
    macros::{
        Preference,
        ResolvablePreference,
    },
};

/// Animation style for the skeleton placeholder.
#[derive(PartialEq, Clone, Default, Debug)]
pub enum SkeletonAnimation {
    /// Fades opacity in and out repeatedly (default).
    #[default]
    Pulse,
    /// A bright band sweeps from left to right.
    Shimmer,
}

impl ResolvablePreference<SkeletonAnimation> for Preference<SkeletonAnimation> {
    fn resolve(&self, _: &ColorsSheet) -> SkeletonAnimation {
        match self {
            Self::Reference(_) => panic!("Only Colors support references."),
            Self::Specific(v) => v.clone(),
        }
    }
}
