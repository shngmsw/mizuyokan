use windows::{
    core::GUID,
    Win32::{
        Foundation::{BOOL, LPARAM, WPARAM},
        UI::{
            Input::KeyboardAndMouse::VK_OEM_3,
            TextServices::{ITfContext, ITfKeyEventSink_Impl, TF_MOD_ALT, TF_PRESERVEDKEY},
        },
    },
};

use anyhow::Result;

use super::factory::TextServiceFactory_Impl;
use crate::engine::{
    client_action::ClientAction, composition::CompositionState, state::IMEState,
};

pub const GUID_PRESERVEDKEY_TOGGLE: GUID =
    GUID::from_u128(0x4f7f0a5e_6b1c_4d3a_9a41_6d2f3c8e1b70);

/// Alt+` toggles the input mode on keyboards without Zenkaku/Hankaku (e.g. US layout),
/// same as Microsoft IME. Alt combinations never reach OnKeyDown, so it must be preserved.
pub const PRESERVEDKEY_TOGGLE: TF_PRESERVEDKEY = TF_PRESERVEDKEY {
    uVKey: VK_OEM_3.0 as u32,
    uModifiers: TF_MOD_ALT,
};

// sink (aka event listener) for key events
impl ITfKeyEventSink_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    #[tracing::instrument]
    fn OnTestKeyDown(
        &self,
        pic: Option<&ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        // this function checks if the key event will be handled by "OnKeyUp" function
        // so we need to return TRUE if we want to handle the key event
        let result = self.process_key(pic, wparam)?.is_some();

        Ok(result.into())
    }

    #[macros::anyhow]
    #[tracing::instrument]
    fn OnKeyDown(&self, pic: Option<&ITfContext>, wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        // this function is called when a key is pressed
        // we can handle key events here
        let result = self.handle_key(pic, wparam)?;

        Ok(result.into())
    }

    #[macros::anyhow]
    fn OnTestKeyUp(
        &self,
        _pic: Option<&ITfContext>,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        // same as OnTestKeyDown
        Ok(false.into())
    }

    #[macros::anyhow]
    fn OnKeyUp(&self, _pic: Option<&ITfContext>, _wparam: WPARAM, _lparam: LPARAM) -> Result<BOOL> {
        // this function is called when a key is released
        // but we handle key events in OnKeyDown function
        // so just return S_OK
        Ok(false.into())
    }

    #[macros::anyhow]
    fn OnPreservedKey(&self, pic: Option<&ITfContext>, rguid: *const GUID) -> Result<BOOL> {
        if rguid.is_null() || unsafe { *rguid } != GUID_PRESERVEDKEY_TOGGLE {
            return Ok(false.into());
        }
        let Some(context) = pic else {
            return Ok(false.into());
        };
        self.borrow_mut()?.context = Some(context.clone());

        let composing = self.borrow()?.borrow_composition()?.state != CompositionState::None;
        let next = IMEState::get()?.input_mode.toggle();
        let mut actions = Vec::new();
        if composing {
            actions.push(ClientAction::EndComposition);
        }
        actions.push(ClientAction::SetIMEMode(next));
        self.handle_action(&actions, CompositionState::None)?;

        Ok(true.into())
    }

    #[macros::anyhow]
    fn OnSetFocus(&self, _fforeground: BOOL) -> Result<()> {
        Ok(())
    }
}
