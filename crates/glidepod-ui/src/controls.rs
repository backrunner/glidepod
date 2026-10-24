//! Prototype-sized controls. Keep GPUI's focus/activation behavior while owning
//! typography and state colors instead of inheriting component defaults.
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, Selectable, StyledExt,
    button::{Button as GpuiButton, ButtonCustomVariant, ButtonVariants},
};

#[derive(Default, Clone, Copy)]
enum Kind {
    #[default]
    Normal,
    Primary,
    Ghost,
    Navigation(bool),
    Segment(bool),
}

#[derive(IntoElement)]
pub(super) struct Button {
    inner: GpuiButton,
    style: StyleRefinement,
    kind: Kind,
    small: bool,
    disabled: bool,
    label: Option<SharedString>,
    icon: Option<Icon>,
    icon_size: Option<Pixels>,
    shortcut: Option<SharedString>,
    badge: Option<SharedString>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            inner: GpuiButton::new(id),
            style: StyleRefinement::default(),
            kind: Kind::Normal,
            small: false,
            disabled: false,
            label: None,
            icon: None,
            icon_size: None,
            shortcut: None,
            badge: None,
        }
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn icon_size(mut self, size: Pixels) -> Self {
        self.icon_size = Some(size);
        self
    }
    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.inner = self.inner.tooltip(tooltip);
        self
    }
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
    pub fn primary(mut self) -> Self {
        self.kind = Kind::Primary;
        self
    }
    pub fn ghost(mut self) -> Self {
        self.kind = Kind::Ghost;
        self
    }
    pub fn navigation(mut self, selected: bool) -> Self {
        self.kind = Kind::Navigation(selected);
        self
    }
    pub fn segment(mut self, selected: bool) -> Self {
        self.kind = Kind::Segment(selected);
        self
    }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.inner = self.inner.on_click(f);
        self
    }
}
impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let dark = cx.theme().mode.is_dark();
        let p = super::palette(dark);
        let transparent: Hsla = rgba(0).into();
        let mut bg: Hsla = rgb(p.panel).into();
        let mut fg: Hsla = rgb(p.text).into();
        let mut border: Hsla = rgb(if dark { 0x4e5e51 } else { 0xcbd4cb }).into();
        let mut hover: Hsla = rgb(if dark { 0x354438 } else { 0xf1f5ef }).into();
        let mut height = if self.small { 28. } else { 32. };
        let mut font = if self.small { 11. } else { 12. };
        let mut padding = if self.small { 9. } else { 11. };
        let mut icon_size = 16.;
        let mut gap = 6.;
        let mut radius = 6.;
        let mut selected = false;
        let mut weight = FontWeight::MEDIUM;
        match self.kind {
            Kind::Normal => {}
            Kind::Primary => {
                bg = rgb(p.accent).into();
                border = bg;
                fg = rgb(if dark { 0x192d20 } else { 0xffffff }).into();
                hover = rgb(if dark { 0xb0c9b4 } else { 0x284e3e }).into();
            }
            Kind::Ghost => {
                bg = transparent;
                border = transparent;
                fg = rgb(p.muted).into();
            }
            Kind::Navigation(active) => {
                selected = active;
                height = 36.;
                font = 12.;
                padding = 12.;
                icon_size = 18.;
                gap = 11.;
                weight = if active {
                    FontWeight(550.)
                } else {
                    FontWeight::NORMAL
                };
                bg = if active {
                    rgb(p.selected).into()
                } else {
                    transparent
                };
                fg = rgb(if active {
                    if dark { 0xc5dbc6 } else { 0x2a4f3c }
                } else if dark {
                    0xacb8aa
                } else {
                    0x66756b
                })
                .into();
                border = transparent;
                hover = rgb(if dark { 0x303d32 } else { 0xe5ebe3 }).into();
            }
            Kind::Segment(active) => {
                selected = active;
                height = 28.;
                font = 11.;
                padding = 12.;
                radius = 5.;
                weight = if active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                };
                bg = if active {
                    rgb(if dark { 0x405343 } else { p.panel }).into()
                } else {
                    transparent
                };
                fg = rgb(if active {
                    if dark { 0xe2eadf } else { p.text }
                } else if dark {
                    0x9bac9b
                } else {
                    0x647165
                })
                .into();
                border = transparent;
                hover = rgb(if dark { 0x324135 } else { 0xe1e8de }).into();
            }
        }
        // Explicit child typography also avoids the dependency's red hover text.
        let content = div()
            .flex()
            .relative()
            .flex_shrink_0()
            .when(matches!(self.kind, Kind::Navigation(_)), |d| {
                d.w(px(if window.viewport_size().width <= px(980.) {
                    125.
                } else {
                    153.
                }))
            })
            .items_center()
            .gap(px(gap))
            .text_size(px(font))
            .line_height(px(18.))
            .font_weight(weight)
            .text_color(fg)
            .children(self.icon.map(|icon| {
                icon.size(self.icon_size.unwrap_or(px(icon_size)))
                    .text_color(fg)
            }))
            .children(self.label.map(|label| div().flex_shrink_0().child(label)))
            .children(self.badge.map(|badge| {
                div()
                    .absolute()
                    .right_0()
                    .top_0()
                    .text_size(px(10.))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(rgb(p.muted))
                    .opacity(0.75)
                    .child(badge)
            }))
            .children(self.shortcut.map(|shortcut| {
                div()
                    .ml(px(5.))
                    .text_size(px(10.))
                    .font_weight(FontWeight::NORMAL)
                    .opacity(0.65)
                    .child(shortcut)
            }));
        self.inner
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(bg)
                    .foreground(fg)
                    .border(border)
                    .hover(hover)
                    .active(bg),
            )
            .selected(selected)
            .disabled(self.disabled)
            .h(px(height))
            .px(px(padding))
            .rounded(px(radius))
            .when(
                matches!(self.kind, Kind::Navigation(_) | Kind::Segment(_)),
                |b| b.border_0(),
            )
            .when(matches!(self.kind, Kind::Navigation(_)), |b| {
                b.w_full().justify_start()
            })
            .when(matches!(self.kind, Kind::Segment(true)), |b| {
                b.shadow(vec![BoxShadow {
                    color: rgba(0x223c2918).into(),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(3.),
                    spread_radius: px(0.),
                }])
            })
            .when(self.disabled, |b| {
                b.bg(bg).border_color(border).opacity(0.45)
            })
            .refine_style(&self.style)
            .child(content)
    }
}

pub(super) fn segmented(dark: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(2.))
        .p(px(2.))
        .border_1()
        .rounded(px(7.))
        .bg(rgb(if dark { 0x28342a } else { 0xe9eee7 }))
        .border_color(rgb(if dark { 0x435045 } else { 0xdfe5dd }))
}

#[derive(IntoElement)]
pub(super) struct Switch {
    inner: GpuiButton,
    checked: bool,
    disabled: bool,
}
impl Switch {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            inner: GpuiButton::new(id),
            checked: false,
            disabled: false,
        }
    }
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.inner = self.inner.tooltip(tooltip);
        self
    }
    pub fn on_click(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        let next = !self.checked;
        self.inner = self.inner.on_click(move |_, w, cx| f(&next, w, cx));
        self
    }
}
impl Disableable for Switch {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
impl RenderOnce for Switch {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let bg: Hsla = rgb(if self.checked { 0x4e775e } else { 0xd6ded4 }).into();
        let border: Hsla = rgb(if self.checked { 0x4e775e } else { 0xccd5cc }).into();
        self.inner
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(bg)
                    .active(bg)
                    .hover(bg)
                    .border(border),
            )
            .disabled(self.disabled)
            .selected(true)
            .w(px(32.))
            .h(px(20.))
            .p_0()
            .rounded(px(10.))
            .when(self.disabled, |b| {
                b.bg(bg).border_color(border).opacity(0.45)
            })
            .child(
                div().relative().w(px(30.)).h(px(18.)).child(
                    div()
                        .absolute()
                        .top(px(1.))
                        .left(px(if self.checked { 13. } else { 1. }))
                        .size(px(16.))
                        .rounded_full()
                        .shadow(vec![BoxShadow {
                            color: rgba(0x00000022).into(),
                            offset: point(px(0.), px(1.)),
                            blur_radius: px(3.),
                            spread_radius: px(0.),
                        }])
                        .bg(rgb(0xffffff)),
                ),
            )
    }
}
