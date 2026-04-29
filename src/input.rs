use crate::app::UiCommand;
use crate::classifier::{MovementClassifier, apply_counter_strafe_thresholds};
use crate::config::MovementKeys;
use rdev::{Button, Event, EventType, Key, listen};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use winit::event_loop::EventLoopProxy;

pub fn start_input_listener(proxy: EventLoopProxy<UiCommand>, movement_keys: MovementKeys) {
    match thread::Builder::new()
        .name("dstrafe-input".to_owned())
        .spawn(move || {
            let classifier = MovementClassifier::new(
                movement_keys.vertical_pair(),
                movement_keys.horizontal_pair(),
            )
            .unwrap_or_default();
            let mut listener = InputListener {
                classifier,
                movement_keys,
                proxy,
            };

            if let Err(error) = listen(move |event| listener.handle_event(event)) {
                log::error!("global input listener stopped: {error:?}");
            }
        }) {
        Ok(_handle) => {}
        Err(error) => log::error!("failed to start global input listener: {error}"),
    }
}

struct InputListener {
    classifier: MovementClassifier,
    movement_keys: MovementKeys,
    proxy: EventLoopProxy<UiCommand>,
}

impl InputListener {
    fn handle_event(&mut self, event: Event) {
        let timestamp_ms = timestamp_ms(event.time);

        match event.event_type {
            EventType::KeyPress(Key::F6) => self.send(UiCommand::ToggleVisible),
            EventType::KeyPress(Key::F8) => self.send(UiCommand::Exit),
            EventType::KeyPress(Key::Equal) => self.send(UiCommand::IncreaseSize),
            EventType::KeyPress(Key::Minus) => self.send(UiCommand::DecreaseSize),
            EventType::KeyPress(key) => {
                if let Some(key) = key_to_char(key)
                    && self.movement_keys.contains(key)
                {
                    self.classifier.on_press(key, timestamp_ms);
                }
            }
            EventType::KeyRelease(key) => {
                if let Some(key) = key_to_char(key)
                    && self.movement_keys.contains(key)
                {
                    self.classifier.on_release(key, timestamp_ms);
                }
            }
            EventType::ButtonPress(Button::Left) => {
                let result =
                    apply_counter_strafe_thresholds(self.classifier.classify_shot(timestamp_ms));
                self.send(UiCommand::Shot(result));
            }
            _ => {}
        }
    }

    fn send(&self, command: UiCommand) {
        if self.proxy.send_event(command).is_err() {
            log::debug!("event loop is no longer accepting input events");
        }
    }
}

fn timestamp_ms(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64() * 1000.0)
        .unwrap_or_default()
}

fn key_to_char(key: Key) -> Option<char> {
    match key {
        Key::KeyA => Some('A'),
        Key::KeyB => Some('B'),
        Key::KeyC => Some('C'),
        Key::KeyD => Some('D'),
        Key::KeyE => Some('E'),
        Key::KeyF => Some('F'),
        Key::KeyG => Some('G'),
        Key::KeyH => Some('H'),
        Key::KeyI => Some('I'),
        Key::KeyJ => Some('J'),
        Key::KeyK => Some('K'),
        Key::KeyL => Some('L'),
        Key::KeyM => Some('M'),
        Key::KeyN => Some('N'),
        Key::KeyO => Some('O'),
        Key::KeyP => Some('P'),
        Key::KeyQ => Some('Q'),
        Key::KeyR => Some('R'),
        Key::KeyS => Some('S'),
        Key::KeyT => Some('T'),
        Key::KeyU => Some('U'),
        Key::KeyV => Some('V'),
        Key::KeyW => Some('W'),
        Key::KeyX => Some('X'),
        Key::KeyY => Some('Y'),
        Key::KeyZ => Some('Z'),
        Key::Num0 | Key::Kp0 => Some('0'),
        Key::Num1 | Key::Kp1 => Some('1'),
        Key::Num2 | Key::Kp2 => Some('2'),
        Key::Num3 | Key::Kp3 => Some('3'),
        Key::Num4 | Key::Kp4 => Some('4'),
        Key::Num5 | Key::Kp5 => Some('5'),
        Key::Num6 | Key::Kp6 => Some('6'),
        Key::Num7 | Key::Kp7 => Some('7'),
        Key::Num8 | Key::Kp8 => Some('8'),
        Key::Num9 | Key::Kp9 => Some('9'),
        _ => None,
    }
}
