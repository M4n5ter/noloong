use super::{MAX_TOASTS, NoloongAppView, ToastMessage, ToastTone};
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, div, prelude::*, px, relative, rgb,
};
use gpui_component::StyledExt as _;
use std::time::Instant;

const TOAST_STACK: ToastStackConfig = ToastStackConfig {
    pattern: ToastStackPattern::Diagonal,
    spread: 0.88,
    max_visible: MAX_TOASTS,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToastStackPattern {
    Diagonal,
    Fan,
    Ring,
}

impl ToastStackPattern {
    const ALL: [Self; 3] = [Self::Diagonal, Self::Fan, Self::Ring];
}

#[derive(Clone, Copy, Debug)]
struct ToastStackConfig {
    pattern: ToastStackPattern,
    spread: f32,
    max_visible: usize,
}

#[derive(Clone, Copy, Debug)]
struct ToastCardLayout {
    top: f32,
    right: f32,
    width: f32,
}

#[derive(Clone, Copy, Debug)]
struct ToastPalette {
    dot: u32,
    border: u32,
    bg: u32,
    text: u32,
}

impl ToastStackConfig {
    fn layout(&self, depth: f32) -> ToastCardLayout {
        debug_assert!(ToastStackPattern::ALL.contains(&self.pattern));

        let spread = self.spread.clamp(0.55, 1.35);
        match self.pattern {
            ToastStackPattern::Diagonal => ToastCardLayout {
                top: depth * 12.0 * spread,
                right: depth * 16.0 * spread,
                width: 360.0 - depth * 18.0 * spread,
            },
            ToastStackPattern::Fan => ToastCardLayout {
                top: depth * 9.0 * spread,
                right: depth * (18.0 + depth * 5.0) * spread,
                width: 360.0 - depth * 16.0 * spread,
            },
            ToastStackPattern::Ring => {
                let radius = depth * 22.0 * spread;
                let angle = -0.35 + depth * 0.34;
                ToastCardLayout {
                    top: radius * (1.0 + angle.sin()),
                    right: radius * (1.0 - angle.cos()) + depth * 7.0 * spread,
                    width: 360.0 - depth * 13.0 * spread,
                }
            }
        }
    }
}

impl ToastPalette {
    fn for_tone(tone: ToastTone) -> Self {
        match tone {
            ToastTone::Success => Self {
                dot: 0x9bc8b3,
                border: 0x3b514d,
                bg: 0x101a1b,
                text: 0xe7eef2,
            },
            ToastTone::Error => Self {
                dot: 0xd49aa2,
                border: 0x67484f,
                bg: 0x1c1519,
                text: 0xf3e6e8,
            },
        }
    }
}

impl NoloongAppView {
    pub(super) fn render_toasts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let mut cards = self
            .toasts
            .iter()
            .rev()
            .take(TOAST_STACK.max_visible)
            .enumerate()
            .collect::<Vec<_>>();

        cards.reverse();

        div()
            .absolute()
            .top(px(62.0))
            .right(px(26.0))
            .w(px(440.0))
            .h(px(130.0))
            .children(cards.into_iter().map(|(depth, toast)| {
                self.render_toast_card(toast, depth, now, cx)
                    .into_any_element()
            }))
    }

    fn render_toast_card(
        &self,
        toast: &ToastMessage,
        target_depth: usize,
        now: Instant,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let visual_depth = toast.visual_depth(target_depth, now);
        let layout = TOAST_STACK.layout(visual_depth);
        let palette = ToastPalette::for_tone(toast.tone);
        let depth_opacity = (1.0 - visual_depth * 0.12).max(0.58);
        let opacity = toast.opacity_at(now) * depth_opacity;
        let toast_id = toast.id;
        let can_promote = target_depth > 0 && !toast.is_animating_at(now);

        div()
            .id(SharedString::from(format!("toast-{toast_id}")))
            .absolute()
            .top(px(layout.top))
            .right(px(layout.right))
            .w(px(layout.width.max(260.0)))
            .min_h(px(44.0))
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .rounded(px(18.0))
            .border_1()
            .border_color(rgb(palette.border))
            .bg(rgb(palette.bg))
            .opacity(opacity)
            .shadow_lg()
            .occlude()
            .cursor_pointer()
            .hover(|style| {
                style
                    .bg(rgb(0x141f27))
                    .border_color(rgb(0x5f7381))
                    .text_color(rgb(0xf4f8fb))
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _window, cx| {
                cx.stop_propagation();
                if *hovered && can_promote {
                    this.promote_toast(toast_id, cx);
                }
            }))
            .child(
                div()
                    .flex_none()
                    .size(px(7.0))
                    .rounded_full()
                    .bg(rgb(palette.dot)),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_medium()
                    .line_height(relative(1.35))
                    .text_color(rgb(palette.text))
                    .child(toast.text.clone()),
            )
            .child(
                div()
                    .id(SharedString::from(format!("toast-close-{toast_id}")))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(22.0))
                    .rounded_full()
                    .text_xs()
                    .text_color(rgb(0x9aa7b4))
                    .bg(rgb(0x18222c))
                    .hover(|style| {
                        style
                            .bg(rgb(0x253241))
                            .text_color(rgb(0xf4f8fb))
                            .border_color(rgb(0x536879))
                    })
                    .border_1()
                    .border_color(rgb(0x27323d))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        cx.stop_propagation();
                        this.dismiss_toast(toast_id, cx);
                    }))
                    .child("×"),
            )
            .into_any_element()
    }
}
