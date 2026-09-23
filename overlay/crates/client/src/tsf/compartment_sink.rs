use windows::{
    core::{Interface as _, GUID, VARIANT},
    Win32::UI::TextServices::{
        ITfCompartment, ITfCompartmentEventSink, ITfCompartmentEventSink_Impl,
        ITfCompartmentMgr, ITfSource, ITfThreadMgr, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
    },
};

use anyhow::Result;

use super::factory::{TextServiceFactory, TextServiceFactory_Impl};
use crate::engine::{
    client_action::ClientAction, composition::CompositionState, input_mode::InputMode,
    state::IMEState,
};

fn open_close(thread_mgr: ITfThreadMgr) -> Result<ITfCompartment> {
    Ok(unsafe {
        thread_mgr
            .cast::<ITfCompartmentMgr>()?
            .GetCompartment(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE)?
    })
}

fn read_open(compartment: &ITfCompartment) -> Result<bool> {
    let value = unsafe { compartment.GetValue()? };
    Ok(i32::try_from(&value).unwrap_or(0) != 0)
}

impl TextServiceFactory {
    /// Follow the keyboard open/close compartment so tools that toggle the IME
    /// through IMM32 (alt-ime-ahk etc.) switch between Latin and Kana.
    pub fn advise_open_close_sink(&self) -> Result<()> {
        let compartment = {
            let text_service = self.borrow()?;
            open_close(text_service.thread_mgr()?)?
        };

        let cookie = unsafe {
            let sink = self.borrow()?.this::<ITfCompartmentEventSink>()?;
            compartment
                .cast::<ITfSource>()?
                .AdviseSink(&ITfCompartmentEventSink::IID, &sink)?
        };
        IMEState::get()?
            .cookies
            .insert(GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, cookie);

        let open = read_open(&compartment)?;
        IMEState::get()?.input_mode = InputMode::for_open_state(open);
        Ok(())
    }

    pub fn unadvise_open_close_sink(&self) -> Result<()> {
        let cookie = IMEState::get()?
            .cookies
            .remove(&GUID_COMPARTMENT_KEYBOARD_OPENCLOSE);
        if let Some(cookie) = cookie {
            let compartment = open_close(self.borrow()?.thread_mgr()?)?;
            unsafe { compartment.cast::<ITfSource>()?.UnadviseSink(cookie)? };
        }
        Ok(())
    }

    /// Mirror our own mode switches back into the compartment so that
    /// `ImmGetOpenStatus` reports the truth. Re-entry into OnChange is a no-op
    /// because the mode already matches.
    pub fn publish_open_state(&self, mode: &InputMode) -> Result<()> {
        let (compartment, tid) = {
            let text_service = self.borrow()?;
            (open_close(text_service.thread_mgr()?)?, text_service.tid)
        };
        if read_open(&compartment)? == mode.is_open() {
            return Ok(());
        }
        let value = VARIANT::from(i32::from(mode.is_open()));
        unsafe { compartment.SetValue(tid, &value)? };
        Ok(())
    }

    fn ensure_context(&self) -> Result<()> {
        if self.borrow()?.context.is_some() {
            return Ok(());
        }
        let context = unsafe {
            let text_service = self.borrow()?;
            text_service.thread_mgr()?.GetFocus()?.GetTop()?
        };
        self.borrow_mut()?.context = Some(context);
        Ok(())
    }
}

impl ITfCompartmentEventSink_Impl for TextServiceFactory_Impl {
    #[macros::anyhow]
    fn OnChange(&self, rguid: *const GUID) -> Result<()> {
        if rguid.is_null() || unsafe { *rguid } != GUID_COMPARTMENT_KEYBOARD_OPENCLOSE {
            return Ok(());
        }

        let open = read_open(&open_close(self.borrow()?.thread_mgr()?)?)?;
        let current = IMEState::get()?.input_mode.clone();
        let target = InputMode::for_open_state(open);
        if target == current {
            return Ok(());
        }

        self.ensure_context()?;
        let composing = self.borrow()?.borrow_composition()?.state != CompositionState::None;
        let mut actions = Vec::new();
        if composing {
            actions.push(ClientAction::EndComposition);
        }
        actions.push(ClientAction::SetIMEMode(target));
        self.handle_action(&actions, CompositionState::None)?;

        Ok(())
    }
}
