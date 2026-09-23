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
}
