use crate::tsf::factory::TextServiceFactory;

use windows::{
    core::Interface,
    Win32::UI::TextServices::{ITfLangBarItemButton, ITfLangBarItemMgr},
};

use anyhow::Result;

#[derive(Default, Clone, PartialEq, Debug)]
pub enum InputMode {
    #[default]
    Latin,
    Kana,
}

impl InputMode {
    pub fn toggle(&self) -> Self {
        match self {
            InputMode::Latin => InputMode::Kana,
            InputMode::Kana => InputMode::Latin,
        }
    }

    /// Mode for the keyboard open/close compartment, which tools such as
    /// alt-ime-ahk flip through IMM32 (`ImmSetOpenStatus`).
    pub fn for_open_state(open: bool) -> Self {
        if open {
            InputMode::Kana
        } else {
            InputMode::Latin
        }
    }

    pub fn is_open(&self) -> bool {
        *self == InputMode::Kana
    }

    /// Mode for the dedicated IME on/off keys. AutoHotkey sends these instead of
    /// flipping the open/close compartment, whose change is not delivered to us
    /// in Chromium-based apps.
    pub fn for_ime_key(vk: usize) -> Option<Self> {
        match vk {
            0x16 => Some(InputMode::Kana),  // VK_IME_ON
            0x1A => Some(InputMode::Latin), // VK_IME_OFF
            _ => None,
        }
    }
}

impl TextServiceFactory {
    pub fn update_lang_bar(&self) -> Result<()> {
        let text_service = self.borrow()?;
        let thread_mgr = text_service.thread_mgr()?;

        unsafe {
            thread_mgr
                .cast::<ITfLangBarItemMgr>()?
                .RemoveItem(&text_service.this::<ITfLangBarItemButton>()?)?;

            thread_mgr
                .cast::<ITfLangBarItemMgr>()?
                .AddItem(&text_service.this::<ITfLangBarItemButton>()?)?;
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::InputMode;

    #[test]
    fn open_state_round_trips() {
        for mode in [InputMode::Latin, InputMode::Kana] {
            assert_eq!(InputMode::for_open_state(mode.is_open()), mode);
            assert_eq!(mode.toggle().toggle(), mode);
        }
    }

    #[test]
    fn ime_keys_pick_a_mode() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_IME_OFF, VK_IME_ON};
        assert_eq!(InputMode::for_ime_key(VK_IME_ON.0 as usize), Some(InputMode::Kana));
        assert_eq!(InputMode::for_ime_key(VK_IME_OFF.0 as usize), Some(InputMode::Latin));
        assert_eq!(InputMode::for_ime_key(0xF3), None);
        assert_eq!(InputMode::for_ime_key(0x41), None);
    }
}
