//! Desktop saves page state: the shared slot library plus selection.
//!
//! The slots, validation and default handling come from
//! `gba_native::frontend::saves`; this module only owns the desktop page state
//! and the keyboard actions. It is pure state plus pixel composition, so it can
//! be unit-tested without a window.

use std::path::PathBuf;

use gba_native::frontend::ui::{self, SavesAction};
use gba_native::frontend::{RgbaImage, SaveSlot, SaveSlots};

pub struct SavesState {
    library: SaveSlots,
    slots: Vec<SaveSlot>,
    selected: usize,
    open: bool,
    status: Option<String>,
}

impl SavesState {
    pub fn new(root: PathBuf) -> Self {
        let mut state = Self {
            library: SaveSlots::new(root),
            slots: Vec::new(),
            selected: 0,
            open: false,
            status: None,
        };
        state.reload();
        state
    }

    pub fn open(&mut self) {
        self.reload();
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    #[cfg(test)]
    pub fn slots(&self) -> &[SaveSlot] {
        &self.slots
    }

    #[cfg(test)]
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn compose(&self) -> RgbaImage {
        ui::saves_page(&self.slots, self.selected)
    }

    pub fn select(&mut self, delta: isize) {
        if self.slots.is_empty() {
            return;
        }
        let count = self.slots.len() as isize;
        let next = (self.selected as isize + delta).rem_euclid(count);
        self.selected = next as usize;
    }

    pub fn apply(&mut self, action: SavesAction) {
        match action {
            SavesAction::Close => self.open = false,
            SavesAction::New => {
                let seed = self.slots.len() as u8;
                let name = format!("slot{}", self.slots.len() + 1);
                if let Err(error) = self.library.store(&name, &SaveSlots::synthetic_image(seed)) {
                    self.status = Some(format!("cannot create {name}: {error}"));
                }
            }
            SavesAction::Import => match self.library.import_from_file() {
                Ok(name) => {
                    self.status = Some(format!(
                        "imported {name} from {}",
                        self.library
                            .root()
                            .join(gba_native::frontend::IMPORT_FILE)
                            .display()
                    ));
                }
                Err(error) => {
                    self.status = Some(format!(
                        "import failed; place a raw image at {} ({error})",
                        self.library
                            .root()
                            .join(gba_native::frontend::IMPORT_FILE)
                            .display()
                    ));
                }
            },
            SavesAction::Export => {
                if let Some(name) = self.slots.get(self.selected).map(|slot| slot.name.clone()) {
                    match self.library.export_to_file(&name) {
                        Ok(path) => {
                            self.status = Some(format!("exported {name} to {}", path.display()));
                        }
                        Err(error) => self.status = Some(format!("cannot export {name}: {error}")),
                    }
                }
            }
            SavesAction::Use | SavesAction::MakeDefault => {
                if let Some(name) = self.slots.get(self.selected).map(|slot| slot.name.clone()) {
                    if let Err(error) = self.library.set_default(&name) {
                        self.status = Some(format!("cannot select {name}: {error}"));
                    } else if action == SavesAction::Use {
                        self.open = false;
                    }
                }
            }
            SavesAction::Delete => {
                if let Some(name) = self.slots.get(self.selected).map(|slot| slot.name.clone())
                    && let Err(error) = self.library.delete(&name)
                {
                    self.status = Some(format!("cannot delete {name}: {error}"));
                }
            }
        }
        self.reload();
    }

    fn reload(&mut self) {
        match self.library.slots() {
            Ok(slots) => {
                self.slots = slots;
                if self.selected >= self.slots.len() {
                    self.selected = self.slots.len().saturating_sub(1);
                }
            }
            Err(error) => {
                self.slots.clear();
                self.status = Some(format!("cannot list save slots: {error}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(tag: &str) -> (SavesState, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "gba-native-desktop-saves-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ));
        (SavesState::new(root.clone()), root)
    }

    #[test]
    fn creating_using_and_deleting_slots_round_trips() {
        let (mut state, root) = state("roundtrip");
        state.open();
        state.apply(SavesAction::New);
        state.apply(SavesAction::New);
        assert_eq!(state.slots().len(), 2);
        assert!(state.slots()[0].default);

        state.select(1);
        state.apply(SavesAction::MakeDefault);
        assert!(state.slots()[1].default);

        assert!(state.is_open());
        state.apply(SavesAction::Use);
        assert!(!state.is_open(), "USE returns to the shell");
        assert!(state.slots()[1].default);

        state.open();
        state.apply(SavesAction::Delete);
        assert_eq!(state.slots().len(), 1);

        let bytes = SaveSlots::synthetic_image(6);
        std::fs::create_dir_all(state.library.root()).expect("saves dir");
        std::fs::write(
            state.library.root().join(gba_native::frontend::IMPORT_FILE),
            &bytes,
        )
        .expect("import file");
        state.apply(SavesAction::Import);
        assert!(state.slots().iter().any(|slot| slot.name == "imported"));
        state.selected = state
            .slots()
            .iter()
            .position(|slot| slot.name == "imported")
            .expect("imported slot");
        state.apply(SavesAction::Export);
        let exported = root.join("export-imported.sav");
        assert_eq!(std::fs::read(exported).expect("exported"), bytes);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn selection_wraps_and_closing_keeps_the_slots() {
        let (mut state, root) = state("selection");
        state.open();
        state.apply(SavesAction::New);
        state.apply(SavesAction::New);
        state.select(5);
        assert_eq!(state.selected(), 1);
        state.select(-3);
        assert_eq!(state.selected(), 0);
        state.apply(SavesAction::Close);
        assert!(!state.is_open());
        state.open();
        assert_eq!(state.slots().len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }
}
