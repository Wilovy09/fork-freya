pub mod animation;

use std::time::Duration;

pub use animation::SkeletonAnimation;
use freya_animation::prelude::*;
use freya_components::{
    define_theme,
    get_theme,
    theming::{
        component_themes::{
            ColorsSheet,
            Theme,
        },
        macros::Preference,
    },
};
use freya_core::prelude::*;
use torin::{
    position::Position,
    size::Size,
};

pub mod prelude {
    pub use crate::{
        Skeleton,
        SkeletonAnimation,
        SkeletonExt,
        SkeletonStyleThemePartial,
        SkeletonStyleThemePartialExt,
        register_skeleton_themes,
    };
}

define_theme! {
    for = Skeleton;
    theme_field = theme;

    %[component]
    pub SkeletonStyle {
        %[fields]
        /// Background color of the placeholder. Defaults to the theme's `surface_primary`.
        background: Color,
        /// Shimmer highlight color. Defaults to `text_placeholder` at reduced opacity.
        shimmer_color: Color,
        /// Duration of one animation cycle.
        duration: Duration,
        /// Animation style: [`SkeletonAnimation::Pulse`] or [`SkeletonAnimation::Shimmer`].
        animation: SkeletonAnimation,
        /// Corner radius of the placeholder shape.
        corner_radius: CornerRadius,
        /// Starting X position of the shimmer band (pixels, can be negative).
        shimmer_from: f32,
        /// Ending X position of the shimmer band (pixels).
        shimmer_to: f32,
        /// Width of the shimmer band in pixels.
        shimmer_width: f32,
    }
}

/// Register the default skeleton theme preferences into a [`Theme`].
///
/// Call this when building your theme before providing it via `use_init_theme` or
/// `use_init_root_theme`.
///
/// # Example
///
/// ```rust,no_run
/// # use freya::prelude::*;
/// # use freya_skeletons::prelude::*;
/// fn app() -> impl IntoElement {
///     use_init_root_theme(|| {
///         let mut theme = light_theme();
///         register_skeleton_themes(&mut theme);
///         theme
///     });
///     rect()
/// }
/// ```
pub fn register_skeleton_themes(theme: &mut Theme) {
    let colors: &ColorsSheet = &theme.colors.clone();
    theme.set(
        "skeleton",
        SkeletonStyleThemePreference {
            background: Preference::Reference("surface_primary"),
            shimmer_color: Preference::Specific(colors.text_placeholder.with_a(90)),
            duration: Preference::Specific(Duration::from_millis(1000)),
            animation: Preference::Specific(SkeletonAnimation::Pulse),
            corner_radius: Preference::Specific(CornerRadius::new_all(4.)),
            shimmer_from: Preference::Specific(-250.),
            shimmer_to: Preference::Specific(900.),
            shimmer_width: Preference::Specific(200.),
        },
    );
}

/// Skeleton loading placeholder with a configurable theme.
///
/// Uses the active theme's `surface_primary` and `text_placeholder` colors by default.
/// Override any field via the individual setters or `.theme()`.
///
/// Requires the skeleton theme to be registered via [`register_skeleton_themes`] before use.
///
/// # Example
///
/// ```rust,no_run
/// # use freya::prelude::*;
/// # use freya_skeletons::prelude::*;
/// # use std::time::Duration;
/// fn app() -> impl IntoElement {
///     let loading = use_state(|| true);
///     rect().width(Size::px(200.)).height(Size::px(80.)).child(
///         Skeleton::new()
///             .loading(*loading.read())
///             .animation(SkeletonAnimation::Shimmer)
///             .duration(Duration::from_millis(1200))
///             .child("Some content"),
///     )
/// }
/// ```
#[derive(PartialEq)]
pub struct Skeleton {
    pub(crate) theme: Option<SkeletonStyleThemePartial>,
    loading: bool,
    elements: Vec<Element>,
    key: DiffKey,
}

impl KeyExt for Skeleton {
    fn write_key(&mut self) -> &mut DiffKey {
        &mut self.key
    }
}

impl ChildrenExt for Skeleton {
    fn get_children(&mut self) -> &mut Vec<Element> {
        &mut self.elements
    }
}

impl Default for Skeleton {
    fn default() -> Self {
        Self::new()
    }
}

impl Skeleton {
    pub fn new() -> Self {
        Self {
            theme: None,
            loading: false,
            elements: Vec::new(),
            key: DiffKey::None,
        }
    }

    /// Whether to show the skeleton placeholder instead of real content.
    pub fn loading(mut self, loading: impl Into<bool>) -> Self {
        self.loading = loading.into();
        self
    }

    /// Override the full theme partial at once.
    /// Prefer individual setters (`.background()`, `.animation()`, etc.) for partial overrides.
    pub fn theme(mut self, theme: SkeletonStyleThemePartial) -> Self {
        self.theme = Some(theme);
        self
    }
}

impl Component for Skeleton {
    fn render(&self) -> impl IntoElement {
        let loading = self.loading;
        let elements = self.elements.clone();

        let theme = get_theme!(&self.theme, SkeletonStyleThemePreference, "skeleton");

        let anim_key = (
            theme.animation.clone(),
            theme.duration,
            theme.shimmer_from.to_bits(),
            theme.shimmer_to.to_bits(),
        );
        let animation = use_animation_with_dependencies(
            &anim_key,
            |conf, (animation, duration, shimmer_from_bits, shimmer_to_bits)| {
                conf.on_creation(OnCreation::Run);
                conf.on_change(OnChange::Rerun);
                let ms = duration.as_millis() as u64;
                match animation {
                    SkeletonAnimation::Pulse => {
                        conf.on_finish(OnFinish::reverse());
                        AnimNum::new(0.4, 1.0).time(ms)
                    }
                    SkeletonAnimation::Shimmer => {
                        conf.on_finish(OnFinish::restart());
                        AnimNum::new(
                            f32::from_bits(*shimmer_from_bits),
                            f32::from_bits(*shimmer_to_bits),
                        )
                        .time(ms)
                    }
                }
            },
        );

        let value = animation.get().value();
        let is_pulse = theme.animation == SkeletonAnimation::Pulse;

        rect()
            .expanded()
            .maybe(loading, |r| {
                r.background(theme.background)
                    .corner_radius(theme.corner_radius)
                    .overflow(Overflow::Clip)
                    .maybe(is_pulse, |r| r.opacity(value))
                    .maybe(!is_pulse, |r| {
                        r.child(
                            rect()
                                .position(Position::new_absolute().left(value))
                                .width(Size::px(theme.shimmer_width))
                                .height(Size::fill())
                                .background(theme.shimmer_color),
                        )
                    })
            })
            .maybe(!loading, |r| r.children(elements))
    }

    fn render_key(&self) -> DiffKey {
        self.key.clone().or(self.default_key())
    }
}

/// Trait for applying a static (non-animated) skeleton style directly on builder elements.
///
/// When `loading` is true, clears children and applies the theme's `surface_primary` background.
/// Place `.skeleton()` **after** all `.child()` calls as it clears children when loading.
///
/// For animated skeletons, use the [`Skeleton`] component instead.
///
/// # Example
///
/// ```rust,no_run
/// # use freya::prelude::*;
/// # use freya_skeletons::prelude::*;
/// fn app() -> impl IntoElement {
///     let loading = use_state(|| true);
///     rect()
///         .width(Size::px(200.))
///         .height(Size::px(20.))
///         .child("content")
///         .skeleton(*loading.read())
/// }
/// ```
pub trait SkeletonExt: StyleExt + ChildrenExt + Sized {
    fn skeleton(mut self, loading: impl Into<bool>) -> Self {
        if loading.into() {
            self.get_children().clear();
            let theme = get_theme!(
                &None::<SkeletonStyleThemePartial>,
                SkeletonStyleThemePreference,
                "skeleton"
            );
            self.background(theme.background)
        } else {
            self
        }
    }
}

impl<T: StyleExt + ChildrenExt> SkeletonExt for T {}
