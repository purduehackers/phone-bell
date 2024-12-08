use std::sync::mpsc;

use crate::hardware::PhoneHardware;

use druid::{
    theme,
    widget::{CrossAxisAlignment, Either, Flex, Image, Label, Painter},
    Color, Data, ExtEventSink, ImageBuf, Lens,
};
use druid::{AppLauncher, RenderContext, Widget, WidgetExt, WindowDesc};

#[derive(Clone, Data, Lens)]
struct UIState {
    dialing_enabled: bool,
    dialed_number: String,
    #[data(ignore)]
    dial_sender: mpsc::Sender<u8>,

    hook_state: bool,
    #[data(ignore)]
    hook_state_sender: mpsc::Sender<bool>,

    ringing: bool,
}

impl UIState {
    fn digit(&mut self, digit: u8) {
        if self.dialing_enabled {
            let _ = self.dial_sender.send(digit);
        }
    }

    fn toggle_hook(&mut self) {
        self.hook_state = !self.hook_state;
        let _ = self.hook_state_sender.send(self.hook_state);
    }
}

fn call_button() -> impl Widget<UIState> {
    let phone_call_data =
        ImageBuf::from_data(include_bytes!("../../assets/phone-call.png")).unwrap();
    let phone_data = ImageBuf::from_data(include_bytes!("../../assets/phone.png")).unwrap();

    let painter = Painter::new(|ctx, _, env| {
        let bounds = ctx.size().to_rect();

        ctx.fill(bounds, &env.get(theme::PRIMARY_DARK));

        if ctx.is_hot() {
            ctx.stroke(bounds.inset(-0.5), &Color::WHITE, 1.0);
        }

        if ctx.is_active() {
            ctx.fill(bounds, &env.get(theme::PRIMARY_LIGHT));
        }
    });

    Either::new(
        |data: &UIState, _env| data.hook_state,
        Image::new(phone_data),
        Image::new(phone_call_data),
    )
    .fix_size(36., 36.)
    .center()
    .background(painter)
    .expand()
    .on_click(move |_ctx, data: &mut UIState, _env| data.toggle_hook())
}

fn digit_button(digit: u8) -> impl Widget<UIState> {
    let painter = Painter::new(|ctx, _, env| {
        let bounds = ctx.size().to_rect();

        ctx.fill(bounds, &env.get(theme::BACKGROUND_LIGHT));

        if ctx.is_hot() {
            ctx.stroke(bounds.inset(-0.5), &Color::WHITE, 1.0);
        }

        if ctx.is_active() {
            ctx.fill(bounds, &Color::rgb8(0x71, 0x71, 0x71));
        }
    });

    Label::new(format!("{digit}"))
        .with_text_size(36.)
        .center()
        .background(painter)
        .expand()
        .on_click(move |_ctx, data: &mut UIState, _env| data.digit(digit))
}

fn flex_row_3<T: Data>(
    w1: impl Widget<T> + 'static,
    w2: impl Widget<T> + 'static,
    w3: impl Widget<T> + 'static,
) -> impl Widget<T> {
    Flex::row()
        .with_flex_child(w1, 1.0)
        .with_spacer(1.0)
        .with_flex_child(w2, 1.0)
        .with_spacer(1.0)
        .with_flex_child(w3, 1.0)
}

fn flex_row_2<T: Data>(
    w1: impl Widget<T> + 'static,
    w2: impl Widget<T> + 'static,
) -> impl Widget<T> {
    Flex::row()
        .with_flex_child(w1, 1.0)
        .with_spacer(1.0)
        .with_flex_child(w2, 2.0)
}

fn ui_builder() -> impl Widget<UIState> {
    let number = Label::new(|data: &String, _env: &_| data.clone())
        .with_text_size(36.0)
        .lens(UIState::dialed_number)
        .expand_width()
        .padding(5.0);

    let bell_data = ImageBuf::from_data(include_bytes!("../../assets/bell.png")).unwrap();
    let bell_ring_data = ImageBuf::from_data(include_bytes!("../../assets/bell-ring.png")).unwrap();

    let ringer = Image::new(bell_data)
        .fix_size(36., 36.)
        .center()
        .padding(5.0);
    let ringer_active = Image::new(bell_ring_data)
        .fix_size(36., 36.)
        .center()
        .padding(5.0);

    Flex::column()
        .with_spacer(1.0)
        .with_flex_child(
            Flex::row()
                .with_spacer(1.0)
                .with_flex_child(
                    Either::new(|data: &UIState, _env| data.ringing, ringer_active, ringer),
                    1.0,
                )
                .with_spacer(1.0)
                .with_flex_child(number, 2.0)
                .with_spacer(1.0),
            1.0,
        )
        .with_spacer(1.0)
        .cross_axis_alignment(CrossAxisAlignment::End)
        .with_flex_child(
            flex_row_3(digit_button(1), digit_button(2), digit_button(3)),
            1.0,
        )
        .with_spacer(1.0)
        .with_flex_child(
            flex_row_3(digit_button(4), digit_button(5), digit_button(6)),
            1.0,
        )
        .with_spacer(1.0)
        .with_flex_child(
            flex_row_3(digit_button(7), digit_button(8), digit_button(9)),
            1.0,
        )
        .with_spacer(1.0)
        .with_flex_child(flex_row_2(digit_button(0), call_button()), 1.0)
}

pub struct Hardware {
    event_sink: ExtEventSink,

    last_dialed_number: String,
    dialed_number: String,
    dial_receiver: mpsc::Receiver<u8>,

    hook_state: bool,
    hook_state_receiver: mpsc::Receiver<bool>,
    launcher: Option<force_send_sync::Send<Launcher>>,
}

pub struct Launcher {
    launcher: AppLauncher<UIState>,
    state: UIState,
}

impl Launcher {
    pub fn go(self) {
        let _ = self.launcher.log_to_console().launch(self.state);
    }
}

impl Hardware {
    pub fn take_gui(&mut self) -> Launcher {
        self.launcher.take().expect("whered the gui go???").unwrap()
    }
}

impl PhoneHardware for Hardware {
    fn create() -> Self {
        let (sender, receiver) = mpsc::channel::<ExtEventSink>();

        let (hook_state_sender, hook_state_receiver) = mpsc::channel::<bool>();
        let (dial_sender, dial_receiver) = mpsc::channel::<u8>();

        let main_window = WindowDesc::new(ui_builder())
            .title("Phone Bell")
            .window_size((300., 500.))
            .resizable(false);

        let launcher = AppLauncher::with_window(main_window);

        let event_sink = launcher.get_external_handle();

        let _ = sender.send(event_sink);

        let state = UIState {
            dialing_enabled: true,
            dialed_number: String::from(""),
            dial_sender,

            hook_state: true,
            hook_state_sender,

            ringing: false,
        };

        // let _ = launcher.log_to_console().launch(state);

        Hardware {
            event_sink: receiver.recv().unwrap(),

            last_dialed_number: String::new(),
            dialed_number: String::new(),
            dial_receiver,

            hook_state: true,
            hook_state_receiver,
            launcher: Some(unsafe { force_send_sync::Send::new(Launcher { launcher, state }) }),
        }
    }

    fn update(&mut self) {
        if let Ok(new_hook_state) = self.hook_state_receiver.try_recv() {
            self.hook_state = new_hook_state;
        }

        if let Ok(new_digit) = self.dial_receiver.try_recv() {
            let ch: char = (b'0' + new_digit) as char;
            self.dialed_number.push(ch);
        }

        if self.dialed_number != self.last_dialed_number {
            let new_number = self.dialed_number.clone();
            self.event_sink
                .add_idle_callback(move |data: &mut UIState| {
                    data.dialed_number = new_number;
                });
            self.last_dialed_number = self.dialed_number.clone();
        }
    }

    fn ring(&mut self, enabled: bool) {
        self.event_sink
            .add_idle_callback(move |data: &mut UIState| {
                data.ringing = enabled;
            });
    }

    fn enable_dialing(&mut self, enabled: bool) {
        self.event_sink
            .add_idle_callback(move |data: &mut UIState| {
                data.dialing_enabled = enabled;
            });
    }

    fn dialed_number(&mut self) -> &mut String {
        &mut self.dialed_number
    }

    fn get_hook_state(&self) -> bool {
        self.hook_state
    }
}
