use super::input_mode::InputMode;

#[derive(Debug, PartialEq)]
pub enum ClientAction {
    StartComposition,
    EndComposition,

    AppendText(String),
    RemoveText,
    ShrinkText(String),

    SetTextWithType(SetTextType),

    MoveCursor(i32),
    SetSelection(SetSelectionType),

    /// Before committing a live conversion, wait for Jev's judgement of the whole
    /// buffer so that what gets committed keeps its English words.
    Settle,

    SetIMEMode(InputMode),
}

#[derive(Debug, PartialEq)]
pub enum SetSelectionType {
    Up,
    Down,
    Number(i32),
}

#[derive(Debug, PartialEq)]
pub enum SetTextType {
    Hiragana,     // F6
    Katakana,     // F7
    HalfKatakana, // F8
    FullLatin,    // F9
    HalfLatin,    // F10
}
