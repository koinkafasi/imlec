use crate::particle::EmitKind;

/// What a physical key press means for the effect. Backends map their native
/// key codes onto this; the engine never sees platform codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyClass {
    /// Produces or moves text: letters, digits, space, enter, arrows.
    Text,
    /// Removes text: backspace, delete.
    Delete,
    /// Modifiers and everything else that should not spawn particles.
    Ignore,
}

impl KeyClass {
    pub fn emit_kind(self) -> Option<EmitKind> {
        match self {
            KeyClass::Text => Some(EmitKind::Typing),
            KeyClass::Delete => Some(EmitKind::Deleting),
            KeyClass::Ignore => None,
        }
    }
}

/// A pointer position in global virtual-desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerPos {
    pub x: f32,
    pub y: f32,
}

/// Anything that can report where to spawn particles.
pub trait PointerSource {
    fn position(&mut self) -> Option<PointerPos>;
}
