#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChatComposer {
    text: String,
}

impl ChatComposer {
    pub fn set_text(&mut self, text: String) {
        self.text = text;
    }

    pub fn can_send(&self) -> bool {
        !self.text.trim().is_empty()
    }

    pub fn press_enter(&mut self, shift: bool) -> ChatComposerAction {
        if shift {
            self.text.push('\n');
            return ChatComposerAction::InsertNewline;
        }
        let text = self.text.trim().to_string();
        if text.is_empty() {
            return ChatComposerAction::None;
        }
        self.text.clear();
        ChatComposerAction::Submit(text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatComposerAction {
    Submit(String),
    InsertNewline,
    None,
}
