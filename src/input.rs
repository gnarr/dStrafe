use crate::app::UiCommand;
use crate::classifier::MovementClassifier;
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
                shortcuts: ShortcutState::default(),
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
    shortcuts: ShortcutState,
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
                if let Some(command) = self.shortcuts.on_key_press(key) {
                    self.send(command);
                } else if let Some(key) = key_to_char(key)
                    && self.movement_keys.contains(key)
                {
                    self.classifier.on_press(key, timestamp_ms);
                }
            }
            EventType::KeyRelease(key) => {
                self.shortcuts.on_key_release(key);

                if let Some(key) = key_to_char(key)
                    && self.movement_keys.contains(key)
                {
                    self.classifier.on_release(key, timestamp_ms);
                }
            }
            EventType::ButtonPress(Button::Left) => {
                let result = self.classifier.classify_shot(timestamp_ms);
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

#[derive(Default)]
struct ShortcutState {
    left_ctrl_down: bool,
    right_ctrl_down: bool,
    f7_down: bool,
}

impl ShortcutState {
    fn on_key_press(&mut self, key: Key) -> Option<UiCommand> {
        match key {
            Key::ControlLeft => self.left_ctrl_down = true,
            Key::ControlRight => self.right_ctrl_down = true,
            Key::F7 => {
                if self.f7_down {
                    return None;
                }

                self.f7_down = true;
                if self.is_ctrl_down() {
                    return Some(UiCommand::ToggleSecondDisplayFullscreen);
                }
            }
            _ => {}
        }

        None
    }

    fn on_key_release(&mut self, key: Key) {
        match key {
            Key::ControlLeft => self.left_ctrl_down = false,
            Key::ControlRight => self.right_ctrl_down = false,
            Key::F7 => self.f7_down = false,
            _ => {}
        }
    }

    fn is_ctrl_down(&self) -> bool {
        self.left_ctrl_down || self.right_ctrl_down
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

#[cfg(test)]
mod tests {
    use super::ShortcutState;
    use crate::app::UiCommand;
    use rdev::Key;

    #[test]
    fn left_control_f7_toggles_fullscreen() {
        let mut shortcuts = ShortcutState::default();

        assert_eq!(shortcuts.on_key_press(Key::ControlLeft), None);
        assert_eq!(
            shortcuts.on_key_press(Key::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
    }

    #[test]
    fn right_control_f7_toggles_fullscreen() {
        let mut shortcuts = ShortcutState::default();

        assert_eq!(shortcuts.on_key_press(Key::ControlRight), None);
        assert_eq!(
            shortcuts.on_key_press(Key::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
    }

    #[test]
    fn f7_without_control_does_not_toggle_fullscreen() {
        let mut shortcuts = ShortcutState::default();

        assert_eq!(shortcuts.on_key_press(Key::F7), None);
    }

    #[test]
    fn releasing_one_control_keeps_other_control_active() {
        let mut shortcuts = ShortcutState::default();

        shortcuts.on_key_press(Key::ControlLeft);
        shortcuts.on_key_press(Key::ControlRight);
        shortcuts.on_key_release(Key::ControlLeft);

        assert_eq!(
            shortcuts.on_key_press(Key::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
    }

    #[test]
    fn held_f7_does_not_repeat_fullscreen_toggle() {
        let mut shortcuts = ShortcutState::default();

        shortcuts.on_key_press(Key::ControlLeft);

        assert_eq!(
            shortcuts.on_key_press(Key::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
        assert_eq!(shortcuts.on_key_press(Key::F7), None);

        shortcuts.on_key_release(Key::F7);

        assert_eq!(
            shortcuts.on_key_press(Key::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
    }
}
