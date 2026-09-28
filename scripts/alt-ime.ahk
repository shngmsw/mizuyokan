; Left Alt alone: IME off (A). Right Alt alone: IME on (あ). Same keys as
; alt-ime-ahk, plus VK_IME_OFF / VK_IME_ON: Chromium-based apps (Chrome,
; VS Code, Slack...) do not pass the IMM32 open/close change on to the IME,
; but the keys do reach it (key_event_sink.rs).
; Needs AutoHotkey v2.
#Requires AutoHotkey v2.0
#SingleInstance Force
InstallKeybdHook

; Alt down and up alone would open the window menu; vk07 is unassigned.
*~LAlt::Send "{Blind}{vk07}"
*~RAlt::Send "{Blind}{vk07}"

~LAlt up:: {
    if (A_PriorKey = "LAlt")
        SetIme(false)
}

~RAlt up:: {
    if (A_PriorKey = "RAlt")
        SetIme(true)
}

SetIme(open) {
    Send open ? "{vk16}" : "{vk1A}"
    hwnd := FocusedWindow()
    if !hwnd
        return
    ; WM_IME_CONTROL, IMC_SETOPENSTATUS
    try SendMessage 0x283, 0x006, open, DllCall("imm32\ImmGetDefaultIMEWnd", "Ptr", hwnd, "Ptr")
}

FocusedWindow() {
    hwnd := WinExist("A")
    size := 4 + 4 + A_PtrSize * 6 + 16
    info := Buffer(size, 0)
    NumPut("UInt", size, info)
    if DllCall("GetGUIThreadInfo", "UInt", 0, "Ptr", info)
        hwnd := NumGet(info, 8 + A_PtrSize, "Ptr") || hwnd
    return hwnd
}
