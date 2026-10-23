use crate::window_input::{Action, Update, WindowInput, keycode};
use glidepod_runtime::{
    display::{AppWindow, Frame},
    input::{GamepadState, InputEvent},
};
use gpui::{prelude::*, *};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::Ordering},
    time::Duration,
};

pub struct AndroidWindow {
    input: WindowInput,
    frames: Arc<Mutex<Option<Result<Frame, String>>>>,
    image: Option<Arc<RenderImage>>,
    retired: Vec<Arc<RenderImage>>,
    error: Option<String>,
    frame_error: Option<String>,
    focus: FocusHandle,
    width: u32,
    height: u32,
    pub(crate) display_id: u32,
    bounds: Bounds<Pixels>,
    touch_down: bool,
    touch_second: bool,
    mouse_buttons: u32,
    held_keys: HashMap<String, u32>,
    modifiers: Modifiers,
    composition: String,
    game_controls: bool,
    mouse_mode: bool,
    controller: Option<glidepod_macos::gamepad::GamepadSnapshot>,
    _activation: Subscription,
}
pub fn open(
    session: AppWindow,
    title: String,
    game: bool,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<AndroidWindow>> {
    let bounds = Bounds::centered(None, size(px(480.), px(760.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(280.), px(360.))),
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| AndroidWindow::new(session, game, window, cx)),
    )
}
impl AndroidWindow {
    fn new(session: AppWindow, game: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let width = session.info.width;
        let height = session.info.height;
        let display_id = session.info.display_id;
        let session = Arc::new(session);
        let mut input = WindowInput::new(session.clone());
        let frames = Arc::new(Mutex::new(None));
        let output = frames.clone();
        let stopped = input.stopped.clone();
        std::thread::spawn(move || {
            let mut sequence = 0;
            while !stopped.load(Ordering::Acquire) {
                match session.frame(sequence) {
                    Ok(Some(frame)) => {
                        sequence = frame.sequence;
                        *output.lock().unwrap() = Some(Ok(frame));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        *output.lock().unwrap() = Some(Err(format!("{e:#}")));
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
                std::thread::sleep(Duration::from_millis(16));
            }
        });
        let focus = cx.focus_handle();
        window.focus(&focus);
        if window.is_window_active() {
            input.activate();
        }
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.clear_local_input();
            if window.is_window_active() {
                this.input.activate();
            } else {
                this.input.deactivate();
            }
            cx.notify();
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let frame = this.frames.lock().unwrap().take();
                        if let Some(frame) = frame {
                            match frame {
                                Ok(mut frame) => {
                                    glidepod_runtime::display::swap_red_blue(&mut frame.rgba);
                                    if let Some(old) = this.image.take() {
                                        this.retired.push(old);
                                    }
                                    if let Some(buffer) = image::RgbaImage::from_raw(
                                        frame.width,
                                        frame.height,
                                        frame.rgba,
                                    ) {
                                        this.image =
                                            Some(Arc::new(RenderImage::new([image::Frame::new(
                                                buffer,
                                            )])));
                                        this.frame_error = None;
                                    }
                                }
                                Err(e) => this.frame_error = Some(e),
                            }
                            cx.notify();
                        }
                        while let Ok(update) = this.input.updates.try_recv() {
                            match update {
                                Update::Ready => this.error = None,
                                Update::Error(e) => this.error = Some(e),
                                Update::Clipboard(t) => {
                                    cx.write_to_clipboard(ClipboardItem::new_string(t))
                                }
                            }
                            cx.notify();
                        }
                        if this.input.active() {
                            let controller = glidepod_macos::gamepad::poll();
                            if controller != this.controller {
                                if this.controller.map(|p| p.identity)
                                    != controller.map(|p| p.identity)
                                {
                                    this.send(Action::Device(GamepadState::default().into()));
                                }
                                if let Some(p) = controller {
                                    this.send(Action::Device(
                                        GamepadState {
                                            buttons: p.buttons,
                                            axes: p.axes,
                                        }
                                        .into(),
                                    ));
                                }
                                this.controller = controller;
                                cx.notify();
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            input,
            frames,
            image: None,
            retired: Vec::new(),
            error: None,
            frame_error: None,
            focus,
            width,
            height,
            display_id,
            bounds: Bounds::default(),
            touch_down: false,
            touch_second: false,
            mouse_buttons: 0,
            held_keys: HashMap::new(),
            modifiers: Modifiers::default(),
            composition: String::new(),
            game_controls: game,
            mouse_mode: false,
            controller: None,
            _activation: activation,
        }
    }
    fn clear_local_input(&mut self) {
        self.touch_down = false;
        self.touch_second = false;
        self.mouse_buttons = 0;
        self.held_keys.clear();
        self.modifiers = Modifiers::default();
        self.composition.clear();
        self.controller = None;
    }
    fn reset(&mut self) {
        self.clear_local_input();
        self.input.deactivate();
        self.input.activate();
    }
    fn send(&mut self, action: Action) {
        if let Err(e) = self.input.send(action) {
            self.error = Some(e);
        }
    }
    fn point(&self, p: Point<Pixels>, clamp: bool) -> Option<(f32, f32)> {
        if (!clamp && !self.bounds.contains(&p))
            || self.bounds.size.width <= px(0.)
            || self.bounds.size.height <= px(0.)
        {
            return None;
        }
        let x = (p.x - self.bounds.origin.x) / self.bounds.size.width * self.width as f32;
        let y = (p.y - self.bounds.origin.y) / self.bounds.size.height * self.height as f32;
        Some((
            x.clamp(0., (self.width - 1) as f32),
            y.clamp(0., (self.height - 1) as f32),
        ))
    }
    fn touch(&mut self, p: (f32, f32), second: bool, initial: bool) {
        let mut points = vec![(0, p.0, p.1)];
        if second {
            points.push((
                1,
                (self.width - 1) as f32 - p.0,
                (self.height - 1) as f32 - p.1,
            ));
        }
        let event = InputEvent::Touch { points };
        if initial || second != self.touch_second {
            self.send(Action::Device(event));
        } else {
            self.send(Action::Motion(event));
        }
        self.touch_second = second;
    }
    fn meta(modifiers: Modifiers) -> u32 {
        u32::from(modifiers.shift)
            | (u32::from(modifiers.alt) << 1)
            | (u32::from(modifiers.control) << 12)
    }
    fn modifiers(&mut self, next: Modifiers) {
        for (name, code, before, after) in [
            ("shift", 59, self.modifiers.shift, next.shift),
            ("alt", 57, self.modifiers.alt, next.alt),
            ("control", 113, self.modifiers.control, next.control),
        ] {
            if before != after {
                if after {
                    self.held_keys.insert(name.into(), code);
                } else {
                    self.held_keys.remove(name);
                }
                self.send(Action::Device(InputEvent::Key {
                    code,
                    down: after,
                    meta: Self::meta(next),
                }));
            }
        }
        self.modifiers = next;
    }
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.input.active() {
            return;
        }
        let key = &event.keystroke;
        let name = key.key.to_lowercase();
        if key.modifiers.platform {
            if key.modifiers.shift && matches!(name.as_str(), "k" | "m") {
                self.reset();
                if name == "k" {
                    self.game_controls = !self.game_controls;
                } else {
                    self.mouse_mode = !self.mouse_mode;
                }
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
                return;
            }
            let action = match name.as_str() {
                "v" => cx
                    .read_from_clipboard()
                    .and_then(|c| c.text())
                    .map(Action::Text),
                "c" => Some(Action::Edit("copy")),
                "x" => Some(Action::Edit("cut")),
                "a" => Some(Action::Edit("select_all")),
                _ => None,
            };
            if let Some(action) = action {
                self.send(action);
                cx.stop_propagation();
                window.prevent_default();
            }
            return;
        }
        if !self.composition.is_empty() && !self.game_controls {
            return;
        }
        let navigation = matches!(
            name.as_str(),
            "backspace"
                | "delete"
                | "enter"
                | "escape"
                | "tab"
                | "left"
                | "right"
                | "up"
                | "down"
                | "home"
                | "end"
                | "pageup"
                | "pagedown"
        );
        if (self.game_controls || navigation || key.modifiers.control)
            && let Some(code) = keycode(&name)
        {
            self.modifiers(key.modifiers);
            self.held_keys.insert(name, code);
            self.send(Action::Device(InputEvent::Key {
                code,
                down: true,
                meta: Self::meta(key.modifiers),
            }));
            cx.stop_propagation();
            window.prevent_default();
        }
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window) {
        window.focus(&self.focus);
        if !window.is_window_active() {
            return;
        }
        if !self.input.active() {
            self.clear_local_input();
            self.input.activate();
        }
        if let Some(p) = self.point(event.position, false) {
            if event.button == MouseButton::Left && !self.mouse_mode {
                self.touch_down = true;
                self.touch(p, event.modifiers.alt, true);
            } else {
                self.mouse_buttons |= match event.button {
                    MouseButton::Left => 1,
                    MouseButton::Right => 2,
                    MouseButton::Middle => 4,
                    _ => 0,
                };
                self.send(Action::Device(InputEvent::Mouse {
                    state: (p.0, p.1, self.mouse_buttons, 0., 0.),
                }));
            }
        }
    }
    fn mouse_up(&mut self, event: &MouseUpEvent) {
        if event.button == MouseButton::Left && self.touch_down {
            self.touch_down = false;
            self.send(Action::Device(InputEvent::Touch { points: vec![] }));
        } else if self.mouse_buttons != 0 {
            self.mouse_buttons &= !match event.button {
                MouseButton::Left => 1,
                MouseButton::Right => 2,
                MouseButton::Middle => 4,
                _ => 0,
            };
            if let Some(p) = self.point(event.position, true) {
                self.send(Action::Device(InputEvent::Mouse {
                    state: (p.0, p.1, self.mouse_buttons, 0., 0.),
                }));
            }
        }
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent) {
        if !self.input.active() {
            return;
        }
        if let Some(p) = self.point(event.position, self.touch_down || self.mouse_buttons != 0) {
            if self.touch_down {
                self.touch(p, event.modifiers.alt, false);
            } else if self.mouse_mode || self.mouse_buttons != 0 {
                self.send(Action::Motion(InputEvent::Mouse {
                    state: (p.0, p.1, self.mouse_buttons, 0., 0.),
                }));
            }
        }
    }
}
impl Render for AndroidWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.retired.drain(..) {
            let _ = window.drop_image(image);
        }
        let available = window.viewport_size();
        let footer = if self.error.is_some() || self.frame_error.is_some() {
            52.
        } else {
            28.
        };
        let scale = (available.width / px(self.width as f32))
            .min((available.height - px(footer)) / px(self.height as f32))
            .max(0.01);
        let dimensions = size(
            px(self.width as f32 * scale),
            px(self.height as f32 * scale),
        );
        let entity = cx.entity().downgrade();
        let pointer_entity = entity.clone();
        let input_entity = cx.entity();
        let input_focus = self.focus.clone();
        let raw = self.game_controls;
        let mut root = div()
            .size_full()
            .bg(rgb(0x151715))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, window, cx| {
                if let Some(code) = this.held_keys.remove(&event.keystroke.key.to_lowercase()) {
                    this.send(Action::Device(InputEvent::Key {
                        code,
                        down: false,
                        meta: Self::meta(event.keystroke.modifiers),
                    }));
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, _| {
                if this.input.active() && (this.game_controls || !this.held_keys.is_empty()) {
                    this.modifiers(event.modifiers);
                }
            }));
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            root = root
                .on_mouse_up(
                    button,
                    cx.listener(|this, event: &MouseUpEvent, _, _| this.mouse_up(event)),
                )
                .on_mouse_up_out(
                    button,
                    cx.listener(|this, event: &MouseUpEvent, _, _| this.mouse_up(event)),
                );
        }
        let mut surface = div()
            .w(dimensions.width)
            .h(dimensions.height)
            .relative()
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                if let Some(p) = this.point(event.position, false) {
                    let delta = event.delta.pixel_delta(px(30.));
                    this.send(Action::Device(InputEvent::Mouse {
                        state: (
                            p.0,
                            p.1,
                            this.mouse_buttons,
                            (f32::from(delta.x) / 30.).clamp(-100., 100.),
                            (f32::from(delta.y) / 30.).clamp(-100., 100.),
                        ),
                    }));
                    cx.stop_propagation();
                }
            }));
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            surface = surface.on_mouse_down(
                button,
                cx.listener(|this, e: &MouseDownEvent, w, _| this.mouse_down(e, w)),
            );
        }
        root.child(
            surface
                .children(
                    self.image
                        .clone()
                        .map(|image| img(image).size_full().object_fit(ObjectFit::Contain)),
                )
                .child(
                    canvas(
                        move |bounds, _, cx| {
                            let _ = entity.update(cx, |this, _| this.bounds = bounds);
                        },
                        move |bounds, _, window, cx| {
                            // A held drag keeps moving when it leaves the surface or window.
                            // The ordinary div move handler only runs inside its hitbox.
                            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    let _ =
                                        pointer_entity.update(cx, |this, _| this.mouse_move(event));
                                }
                            });
                            if !raw {
                                window.handle_input(
                                    &input_focus,
                                    ElementInputHandler::new(bounds, input_entity),
                                    cx,
                                );
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                ),
        )
        .child(
            div()
                .h(px(28.))
                .w_full()
                .px(px(10.))
                .text_size(px(11.))
                .text_color(rgb(0xb5bdb5))
                .flex()
                .gap(px(14.))
                .items_center()
                .child(
                    div()
                        .id("keyboard-mode")
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(0xffffff)))
                        .child(if self.game_controls {
                            "Keys: Game"
                        } else {
                            "Keys: Text"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reset();
                            this.game_controls = !this.game_controls;
                            window.focus(&this.focus);
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id("pointer-mode")
                        .cursor_pointer()
                        .hover(|s| s.text_color(rgb(0xffffff)))
                        .child(if self.mouse_mode { "Mouse" } else { "Touch" })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.reset();
                            this.mouse_mode = !this.mouse_mode;
                            window.focus(&this.focus);
                            cx.notify();
                        })),
                )
                .when(self.controller.is_some(), |row| {
                    row.child("Controller connected")
                }),
        )
        .children(
            self.error
                .clone()
                .or_else(|| self.frame_error.clone())
                .map(|e| {
                    div()
                        .h(px(24.))
                        .w_full()
                        .px(px(10.))
                        .text_size(px(11.))
                        .text_color(rgb(0xf0bb93))
                        .overflow_hidden()
                        .child(e)
                }),
        )
    }
}

// Composition stays on macOS until commit. Android owns the actual document and selection.
impl EntityInputHandler for AndroidWindow {
    fn text_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        actual: &mut Option<std::ops::Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text = self.composition.encode_utf16().collect::<Vec<_>>();
        if range.end > text.len() {
            return None;
        }
        *actual = Some(range.clone());
        String::from_utf16(&text[range]).ok()
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.composition.encode_utf16().count();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }
    fn marked_text_range(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        (!self.composition.is_empty()).then(|| 0..self.composition.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composition.clear();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition.clear();
        if !text.is_empty() && !self.game_controls {
            self.send(Action::Text(text.into()));
        }
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<std::ops::Range<usize>>,
        text: &str,
        _: Option<std::ops::Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.composition = text.into();
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: std::ops::Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(Bounds::new(
            point(bounds.left(), bounds.bottom() - px(28.)),
            size(px(240.), px(28.)),
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}
