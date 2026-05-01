use crate::app::UiCommand;
use crate::classifier::MovementClassifier;
use crate::config::MovementKeys;
use std::thread;
use std::time::Instant;
use winit::event_loop::EventLoopProxy;

pub fn start_input_listener(proxy: EventLoopProxy<UiCommand>, movement_keys: MovementKeys) {
    match thread::Builder::new()
        .name("dstrafe-input".to_owned())
        .spawn(move || {
            let clock = InputClock::start();
            let mut listener = InputListener::new(proxy, movement_keys);

            if let Err(error) =
                platform::listen(move |event| listener.handle_event(event, clock.timestamp_ms()))
            {
                log::error!("global input listener stopped: {error}");
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
    fn new(proxy: EventLoopProxy<UiCommand>, movement_keys: MovementKeys) -> Self {
        let classifier = MovementClassifier::new(
            movement_keys.vertical_pair(),
            movement_keys.horizontal_pair(),
        )
        .unwrap_or_default();

        Self {
            classifier,
            movement_keys,
            proxy,
            shortcuts: ShortcutState::default(),
        }
    }

    fn handle_event(&mut self, event: InputEvent, timestamp_ms: f64) {
        match event {
            InputEvent::KeyPress(InputKey::F6) => self.send(UiCommand::ToggleVisible),
            InputEvent::KeyPress(InputKey::F8) => self.send(UiCommand::Exit),
            InputEvent::KeyPress(InputKey::Equal) => self.send(UiCommand::IncreaseSize),
            InputEvent::KeyPress(InputKey::Minus) => self.send(UiCommand::DecreaseSize),
            InputEvent::KeyPress(key) => {
                if let Some(command) = self.shortcuts.on_key_press(key) {
                    self.send(command);
                } else if let Some(key) = key.character()
                    && self.movement_keys.contains(key)
                {
                    self.classifier.on_press(key, timestamp_ms);
                }
            }
            InputEvent::KeyRelease(key) => {
                self.shortcuts.on_key_release(key);

                if let Some(key) = key.character()
                    && self.movement_keys.contains(key)
                {
                    self.classifier.on_release(key, timestamp_ms);
                }
            }
            InputEvent::LeftMouseDown => {
                let result = self.classifier.classify_shot(timestamp_ms);
                self.send(UiCommand::Shot(result));
            }
        }
    }

    fn send(&self, command: UiCommand) {
        if self.proxy.send_event(command).is_err() {
            log::debug!("event loop is no longer accepting input events");
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct InputClock {
    start: Instant,
}

impl InputClock {
    fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    fn timestamp_ms(self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputEvent {
    KeyPress(InputKey),
    KeyRelease(InputKey),
    LeftMouseDown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputKey {
    Character(char),
    F6,
    F7,
    F8,
    Equal,
    Minus,
    ControlLeft,
    ControlRight,
}

impl InputKey {
    fn character(self) -> Option<char> {
        match self {
            Self::Character(key) => Some(key),
            _ => None,
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
    fn on_key_press(&mut self, key: InputKey) -> Option<UiCommand> {
        match key {
            InputKey::ControlLeft => self.left_ctrl_down = true,
            InputKey::ControlRight => self.right_ctrl_down = true,
            InputKey::F7 => {
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

    fn on_key_release(&mut self, key: InputKey) {
        match key {
            InputKey::ControlLeft => self.left_ctrl_down = false,
            InputKey::ControlRight => self.right_ctrl_down = false,
            InputKey::F7 => self.f7_down = false,
            _ => {}
        }
    }

    fn is_ctrl_down(&self) -> bool {
        self.left_ctrl_down || self.right_ctrl_down
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{InputEvent, InputKey};
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::sync::mpsc::{self, Sender};
    use std::thread;
    use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_CONTROL, VK_F6, VK_F7, VK_F8, VK_LCONTROL, VK_NUMPAD0, VK_NUMPAD9, VK_OEM_MINUS,
        VK_OEM_PLUS, VK_RCONTROL,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG,
        SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
        WM_LBUTTONDOWN, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    static EVENT_SENDER: Mutex<Option<Sender<InputEvent>>> = Mutex::new(None);
    static KEY_HOOK: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
    static MOUSE_HOOK: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

    pub fn listen<T>(callback: T) -> Result<(), String>
    where
        T: FnMut(InputEvent) + Send + 'static,
    {
        let module_handle = current_module_handle()?;

        let (event_sender, event_receiver) = mpsc::channel();
        set_event_sender(event_sender)?;
        let event_worker = match start_event_worker(callback, event_receiver) {
            Ok(worker) => worker,
            Err(error) => {
                clear_event_sender();
                return Err(error);
            }
        };

        let key_hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(raw_keyboard_callback),
                module_handle,
                0,
            )
        };
        if key_hook.is_null() {
            let error = unsafe { GetLastError() };
            stop_event_worker(event_worker);
            return Err(format!("keyboard hook failed with Windows error {error}"));
        }
        KEY_HOOK.store(key_hook, Ordering::Relaxed);

        let mouse_hook =
            unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(raw_mouse_callback), module_handle, 0) };
        if mouse_hook.is_null() {
            let error = unsafe { GetLastError() };
            uninstall_hooks();
            stop_event_worker(event_worker);
            return Err(format!("mouse hook failed with Windows error {error}"));
        }
        MOUSE_HOOK.store(mouse_hook, Ordering::Relaxed);

        let result = run_message_loop();
        uninstall_hooks();
        stop_event_worker(event_worker);
        result
    }

    fn current_module_handle() -> Result<*mut c_void, String> {
        let module_handle = unsafe { GetModuleHandleW(null()) };
        if module_handle.is_null() {
            let error = unsafe { GetLastError() };
            return Err(format!(
                "failed to get current module handle with Windows error {error}"
            ));
        }

        Ok(module_handle)
    }

    fn start_event_worker<T>(
        mut callback: T,
        event_receiver: mpsc::Receiver<InputEvent>,
    ) -> Result<thread::JoinHandle<()>, String>
    where
        T: FnMut(InputEvent) + Send + 'static,
    {
        thread::Builder::new()
            .name("dstrafe-input-events".to_owned())
            .spawn(move || {
                for event in event_receiver {
                    callback(event);
                }
            })
            .map_err(|error| format!("failed to start input event worker: {error}"))
    }

    fn set_event_sender(sender: Sender<InputEvent>) -> Result<(), String> {
        let mut slot = EVENT_SENDER
            .lock()
            .map_err(|_| "input event sender lock is poisoned".to_owned())?;
        *slot = Some(sender);
        Ok(())
    }

    fn clear_event_sender() {
        if let Ok(mut slot) = EVENT_SENDER.lock() {
            *slot = None;
        }
    }

    fn stop_event_worker(worker: thread::JoinHandle<()>) {
        clear_event_sender();
        let _ = worker.join();
    }

    fn run_message_loop() -> Result<(), String> {
        let mut message = MSG::default();

        loop {
            let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
            match result {
                -1 => {
                    let error = unsafe { GetLastError() };
                    return Err(format!("message loop failed with Windows error {error}"));
                }
                0 => return Ok(()),
                _ => {}
            }
        }
    }

    fn uninstall_hooks() {
        let key_hook = KEY_HOOK.swap(null_mut(), Ordering::Relaxed);
        if !key_hook.is_null() {
            unsafe {
                UnhookWindowsHookEx(key_hook);
            }
        }

        let mouse_hook = MOUSE_HOOK.swap(null_mut(), Ordering::Relaxed);
        if !mouse_hook.is_null() {
            unsafe {
                UnhookWindowsHookEx(mouse_hook);
            }
        }
    }

    unsafe extern "system" fn raw_keyboard_callback(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32
            && let Some(event) = keyboard_event(wparam, lparam)
        {
            dispatch(event);
        }

        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }

    unsafe extern "system" fn raw_mouse_callback(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code == HC_ACTION as i32
            && let Some(event) = mouse_event(wparam)
        {
            dispatch(event);
        }

        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }

    fn keyboard_event(wparam: WPARAM, lparam: LPARAM) -> Option<InputEvent> {
        let pressed = match wparam as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => true,
            WM_KEYUP | WM_SYSKEYUP => false,
            _ => return None,
        };

        let hook = unsafe { *(lparam as *const KBDLLHOOKSTRUCT) };
        let key = key_from_keyboard_hook(&hook)?;

        Some(if pressed {
            InputEvent::KeyPress(key)
        } else {
            InputEvent::KeyRelease(key)
        })
    }

    fn mouse_event(wparam: WPARAM) -> Option<InputEvent> {
        match wparam as u32 {
            WM_LBUTTONDOWN => Some(InputEvent::LeftMouseDown),
            _ => None,
        }
    }

    fn dispatch(event: InputEvent) {
        let sender = EVENT_SENDER
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().cloned());

        if let Some(sender) = sender {
            let _ = sender.send(event);
        }
    }

    fn key_from_keyboard_hook(hook: &KBDLLHOOKSTRUCT) -> Option<InputKey> {
        if hook.vkCode == u32::from(VK_CONTROL) {
            Some(control_key_from_hook(hook))
        } else {
            key_from_virtual_key(hook.vkCode)
        }
    }

    fn key_from_virtual_key(vk_code: u32) -> Option<InputKey> {
        match vk_code {
            code if code == u32::from(VK_F6) => Some(InputKey::F6),
            code if code == u32::from(VK_F7) => Some(InputKey::F7),
            code if code == u32::from(VK_F8) => Some(InputKey::F8),
            code if code == u32::from(VK_OEM_PLUS) => Some(InputKey::Equal),
            code if code == u32::from(VK_OEM_MINUS) => Some(InputKey::Minus),
            code if code == u32::from(VK_LCONTROL) => Some(InputKey::ControlLeft),
            code if code == u32::from(VK_RCONTROL) => Some(InputKey::ControlRight),
            code @ 0x30..=0x39 => Some(InputKey::Character((code as u8) as char)),
            code @ 0x41..=0x5A => Some(InputKey::Character((code as u8) as char)),
            code if (u32::from(VK_NUMPAD0)..=u32::from(VK_NUMPAD9)).contains(&code) => {
                let digit = b'0' + (code - u32::from(VK_NUMPAD0)) as u8;
                Some(InputKey::Character(digit as char))
            }
            _ => None,
        }
    }

    fn control_key_from_hook(hook: &KBDLLHOOKSTRUCT) -> InputKey {
        if hook.flags & LLKHF_EXTENDED != 0 {
            InputKey::ControlRight
        } else {
            InputKey::ControlLeft
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn keyboard_hook(vk_code: u32, flags: u32) -> KBDLLHOOKSTRUCT {
            KBDLLHOOKSTRUCT {
                vkCode: vk_code,
                scanCode: 0x1d,
                flags,
                time: 0,
                dwExtraInfo: 0,
            }
        }

        #[test]
        fn generic_control_without_extended_flag_maps_to_left_control() {
            let hook = keyboard_hook(u32::from(VK_CONTROL), 0);

            assert_eq!(key_from_keyboard_hook(&hook), Some(InputKey::ControlLeft));
        }

        #[test]
        fn generic_control_with_extended_flag_maps_to_right_control() {
            let hook = keyboard_hook(u32::from(VK_CONTROL), LLKHF_EXTENDED);

            assert_eq!(key_from_keyboard_hook(&hook), Some(InputKey::ControlRight));
        }

        #[test]
        fn side_specific_control_codes_do_not_depend_on_extended_flag() {
            let left_hook = keyboard_hook(u32::from(VK_LCONTROL), LLKHF_EXTENDED);
            let right_hook = keyboard_hook(u32::from(VK_RCONTROL), 0);

            assert_eq!(
                key_from_keyboard_hook(&left_hook),
                Some(InputKey::ControlLeft)
            );
            assert_eq!(
                key_from_keyboard_hook(&right_hook),
                Some(InputKey::ControlRight)
            );
        }

        #[test]
        fn shortcut_virtual_keys_map_to_input_keys() {
            let cases = [
                (u32::from(VK_F6), InputKey::F6),
                (u32::from(VK_F7), InputKey::F7),
                (u32::from(VK_F8), InputKey::F8),
                (u32::from(VK_OEM_PLUS), InputKey::Equal),
                (u32::from(VK_OEM_MINUS), InputKey::Minus),
            ];

            for (vk_code, expected) in cases {
                assert_eq!(key_from_virtual_key(vk_code), Some(expected));
            }
        }

        #[test]
        fn top_row_digits_map_to_character_digits() {
            for digit in 0..=9 {
                let vk_code = 0x30 + digit;
                let expected = InputKey::Character((b'0' + digit as u8) as char);

                assert_eq!(key_from_virtual_key(vk_code), Some(expected));
            }
        }

        #[test]
        fn numpad_digits_map_to_character_digits() {
            for digit in 0..=9 {
                let vk_code = u32::from(VK_NUMPAD0) + digit;
                let expected = InputKey::Character((b'0' + digit as u8) as char);

                assert_eq!(key_from_virtual_key(vk_code), Some(expected));
            }
        }

        #[test]
        fn movement_letters_map_to_uppercase_characters() {
            for letter in b'A'..=b'Z' {
                let expected = InputKey::Character(letter as char);

                assert_eq!(key_from_virtual_key(u32::from(letter)), Some(expected));
            }
        }

        #[test]
        fn generic_control_virtual_key_is_not_mapped_without_hook_flags() {
            assert_eq!(key_from_virtual_key(u32::from(VK_CONTROL)), None);
        }

        #[test]
        fn unrelated_virtual_keys_are_ignored() {
            assert_eq!(key_from_virtual_key(0x1B), None);
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::{InputEvent, InputKey};
    use rdev::{Button, Event, EventType, Key, listen as rdev_listen};

    pub fn listen<T>(mut callback: T) -> Result<(), String>
    where
        T: FnMut(InputEvent) + Send + 'static,
    {
        rdev_listen(move |event| {
            if let Some(input_event) = map_event(&event) {
                callback(input_event);
            }
        })
        .map_err(|error| format!("{error:?}"))
    }

    fn map_event(event: &Event) -> Option<InputEvent> {
        match event.event_type {
            EventType::KeyPress(key) => key_from_rdev(key).map(InputEvent::KeyPress),
            EventType::KeyRelease(key) => key_from_rdev(key).map(InputEvent::KeyRelease),
            EventType::ButtonPress(Button::Left) => Some(InputEvent::LeftMouseDown),
            _ => None,
        }
    }

    fn key_from_rdev(key: Key) -> Option<InputKey> {
        match key {
            Key::F6 => Some(InputKey::F6),
            Key::F7 => Some(InputKey::F7),
            Key::F8 => Some(InputKey::F8),
            Key::Equal => Some(InputKey::Equal),
            Key::Minus => Some(InputKey::Minus),
            Key::ControlLeft => Some(InputKey::ControlLeft),
            Key::ControlRight => Some(InputKey::ControlRight),
            Key::KeyA => Some(InputKey::Character('A')),
            Key::KeyB => Some(InputKey::Character('B')),
            Key::KeyC => Some(InputKey::Character('C')),
            Key::KeyD => Some(InputKey::Character('D')),
            Key::KeyE => Some(InputKey::Character('E')),
            Key::KeyF => Some(InputKey::Character('F')),
            Key::KeyG => Some(InputKey::Character('G')),
            Key::KeyH => Some(InputKey::Character('H')),
            Key::KeyI => Some(InputKey::Character('I')),
            Key::KeyJ => Some(InputKey::Character('J')),
            Key::KeyK => Some(InputKey::Character('K')),
            Key::KeyL => Some(InputKey::Character('L')),
            Key::KeyM => Some(InputKey::Character('M')),
            Key::KeyN => Some(InputKey::Character('N')),
            Key::KeyO => Some(InputKey::Character('O')),
            Key::KeyP => Some(InputKey::Character('P')),
            Key::KeyQ => Some(InputKey::Character('Q')),
            Key::KeyR => Some(InputKey::Character('R')),
            Key::KeyS => Some(InputKey::Character('S')),
            Key::KeyT => Some(InputKey::Character('T')),
            Key::KeyU => Some(InputKey::Character('U')),
            Key::KeyV => Some(InputKey::Character('V')),
            Key::KeyW => Some(InputKey::Character('W')),
            Key::KeyX => Some(InputKey::Character('X')),
            Key::KeyY => Some(InputKey::Character('Y')),
            Key::KeyZ => Some(InputKey::Character('Z')),
            Key::Num0 | Key::Kp0 => Some(InputKey::Character('0')),
            Key::Num1 | Key::Kp1 => Some(InputKey::Character('1')),
            Key::Num2 | Key::Kp2 => Some(InputKey::Character('2')),
            Key::Num3 | Key::Kp3 => Some(InputKey::Character('3')),
            Key::Num4 | Key::Kp4 => Some(InputKey::Character('4')),
            Key::Num5 | Key::Kp5 => Some(InputKey::Character('5')),
            Key::Num6 | Key::Kp6 => Some(InputKey::Character('6')),
            Key::Num7 | Key::Kp7 => Some(InputKey::Character('7')),
            Key::Num8 | Key::Kp8 => Some(InputKey::Character('8')),
            Key::Num9 | Key::Kp9 => Some(InputKey::Character('9')),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InputKey, ShortcutState};
    use crate::app::UiCommand;

    #[test]
    fn left_control_f7_toggles_fullscreen() {
        let mut shortcuts = ShortcutState::default();

        assert_eq!(shortcuts.on_key_press(InputKey::ControlLeft), None);
        assert_eq!(
            shortcuts.on_key_press(InputKey::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
    }

    #[test]
    fn right_control_f7_toggles_fullscreen() {
        let mut shortcuts = ShortcutState::default();

        assert_eq!(shortcuts.on_key_press(InputKey::ControlRight), None);
        assert_eq!(
            shortcuts.on_key_press(InputKey::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
    }

    #[test]
    fn f7_without_control_does_not_toggle_fullscreen() {
        let mut shortcuts = ShortcutState::default();

        assert_eq!(shortcuts.on_key_press(InputKey::F7), None);
    }

    #[test]
    fn releasing_one_control_keeps_other_control_active() {
        let mut shortcuts = ShortcutState::default();

        shortcuts.on_key_press(InputKey::ControlLeft);
        shortcuts.on_key_press(InputKey::ControlRight);
        shortcuts.on_key_release(InputKey::ControlLeft);

        assert_eq!(
            shortcuts.on_key_press(InputKey::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
    }

    #[test]
    fn held_f7_does_not_repeat_fullscreen_toggle() {
        let mut shortcuts = ShortcutState::default();

        shortcuts.on_key_press(InputKey::ControlLeft);

        assert_eq!(
            shortcuts.on_key_press(InputKey::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
        assert_eq!(shortcuts.on_key_press(InputKey::F7), None);

        shortcuts.on_key_release(InputKey::F7);

        assert_eq!(
            shortcuts.on_key_press(InputKey::F7),
            Some(UiCommand::ToggleSecondDisplayFullscreen)
        );
    }
}
