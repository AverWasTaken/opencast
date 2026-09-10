//! Persisted Windows virtual-key shortcut, independent of the UI and OS bindings.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    pub modifiers: u32,
    pub key: u32,
}
impl Default for Shortcut {
    fn default() -> Self {
        Self {
            modifiers: 1,
            key: 32,
        }
    }
}
impl Shortcut {
    pub fn valid(self) -> bool {
        let key = matches!(self.key, 8|9|13|19|20|27|32..=40|45|46|48..=57|65..=90|96..=111|112..=135|186..=192|219..=222);
        key && self.modifiers & !15 == 0
            && (self.modifiers & 11 != 0 || (112..=135).contains(&self.key))
    }
    pub fn label(self) -> String {
        let mut keys = Vec::new();
        for (flag, name) in [(2, "Ctrl"), (1, "Alt"), (4, "Shift"), (8, "Win")] {
            if self.modifiers & flag != 0 {
                keys.push(name.to_owned());
            }
        }
        keys.push(match self.key {
            48..=57 | 65..=90 => char::from_u32(self.key).unwrap().to_string(),
            112..=135 => format!("F{}", self.key - 111),
            8 => "Backspace".into(),
            9 => "Tab".into(),
            13 => "Enter".into(),
            27 => "Escape".into(),
            32 => "Space".into(),
            33 => "Page Up".into(),
            34 => "Page Down".into(),
            35 => "End".into(),
            36 => "Home".into(),
            37 => "Left".into(),
            38 => "Up".into(),
            39 => "Right".into(),
            40 => "Down".into(),
            45 => "Insert".into(),
            46 => "Delete".into(),
            186 => ";".into(),
            187 => "=".into(),
            188 => ",".into(),
            189 => "-".into(),
            190 => ".".into(),
            191 => "/".into(),
            192 => "`".into(),
            219 => "[".into(),
            220 => "\\".into(),
            221 => "]".into(),
            222 => "'".into(),
            _ => format!("Key {}", self.key),
        });
        keys.join(" + ")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_custom_combinations() {
        assert!(Shortcut::default().valid());
        assert!(
            Shortcut {
                modifiers: 6,
                key: 75
            }
            .valid()
        );
        assert!(
            Shortcut {
                modifiers: 0,
                key: 119
            }
            .valid()
        );
        assert!(
            !Shortcut {
                modifiers: 0,
                key: 65
            }
            .valid()
        );
        assert!(
            !Shortcut {
                modifiers: 4,
                key: 65
            }
            .valid()
        );
        assert!(
            !Shortcut {
                modifiers: 1,
                key: 0
            }
            .valid()
        );
        assert!(
            !Shortcut {
                modifiers: 32,
                key: 65
            }
            .valid()
        );
    }
    #[test]
    fn labels_and_roundtrips() {
        let key = Shortcut {
            modifiers: 6,
            key: 75,
        };
        assert_eq!(key.label(), "Ctrl + Shift + K");
        assert_eq!(
            serde_json::from_str::<Shortcut>(&serde_json::to_string(&key).unwrap()).unwrap(),
            key
        );
    }
}
