//! Register only the requested combinations, never a keyboard hook.
use crate::config::{KeyBinding, Shortcuts};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT},
};

#[derive(Default)]
pub struct Hotkeys(Vec<(i32, KeyBinding, &'static str)>);
impl Hotkeys {
    pub fn replace(&mut self, hwnd: usize, settings: &Shortcuts) -> Result<(), &'static str> {
        let hwnd = hwnd as HWND;
        settings.validate().map_err(|_| "invalid_request")?;
        let mut next: Vec<(i32, KeyBinding, &'static str)> = Vec::new();
        let mut added = Vec::new();
        for (binding, action) in [
            (settings.history, "--toggle-history"),
            (settings.inbox, "--toggle-inbox"),
        ] {
            let Some(binding) = binding else { continue };
            let id = if let Some(old) = self.0.iter().find(|v| v.1 == binding) {
                old.0
            } else {
                let id = (1003..1007)
                    .find(|id| !self.0.iter().chain(next.iter()).any(|v| v.0 == *id))
                    .ok_or("shortcut_error")?;
                if unsafe {
                    RegisterHotKey(hwnd, id, binding.modifiers | MOD_NOREPEAT, binding.key)
                } == 0
                {
                    for id in added {
                        unsafe {
                            UnregisterHotKey(hwnd, id);
                        }
                    }
                    return Err("shortcut_conflict");
                }
                added.push(id);
                id
            };
            next.push((id, binding, action));
        }
        for old in &self.0 {
            if !next.iter().any(|v| v.0 == old.0) {
                unsafe {
                    UnregisterHotKey(hwnd, old.0);
                }
            }
        }
        self.0 = next;
        Ok(())
    }
    pub fn action(&self, id: usize) -> Option<&'static str> {
        self.0.iter().find(|v| v.0 as usize == id).map(|v| v.2)
    }
    pub fn clear(&mut self, hwnd: usize) {
        let hwnd = hwnd as HWND;
        for (id, _, _) in self.0.drain(..) {
            unsafe {
                UnregisterHotKey(hwnd, id);
            }
        }
    }
}
