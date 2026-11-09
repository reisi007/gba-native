use gba_native::frontend::saves::SaveSlot;
use rune::Tx;

use super::AppState;

#[derive(Debug)]
pub(super) struct BrowserSaveState {
    profiles: Vec<String>,
    selected_profile: String,
    pending: Option<PendingSave>,
    started: bool,
}

#[derive(Debug)]
struct PendingSave {
    id: u32,
    kind: PendingKind,
    profile: String,
}

#[derive(Debug)]
enum PendingKind {
    Profiles,
    Slots,
    CreateProfile,
    CreateSlot,
    Delete(String),
    SetDefault(String),
    Import,
    Export,
}

impl Default for BrowserSaveState {
    fn default() -> Self {
        Self {
            profiles: vec!["default".to_owned()],
            selected_profile: "default".to_owned(),
            pending: None,
            started: false,
        }
    }
}

impl BrowserSaveState {
    pub(super) fn profiles(&self) -> &[String] {
        &self.profiles
    }

    pub(super) fn selected_profile(&self) -> &str {
        &self.selected_profile
    }

    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    fn start(
        &mut self,
        store: &crate::saves::SaveStore,
        kind: PendingKind,
        operation: &str,
        name: &str,
        value: &str,
        status: &mut Option<String>,
    ) {
        if self.pending.is_some() {
            return;
        }
        let profile = self.selected_profile.clone();
        let id = store.start_browser_operation(operation, &profile, name, value);
        self.pending = Some(PendingSave {
            id,
            kind,
            profile: profile.clone(),
        });
        *status = Some(match operation {
            "profiles" => "Loading browser save profiles…".to_owned(),
            "slots" => format!("Loading profile {profile}…"),
            "create-profile" => "Creating browser save profile…".to_owned(),
            "create" => "Creating save slot…".to_owned(),
            "delete" => format!("Deleting save slot {name}…"),
            "default" => format!("Updating the default save slot to {name}…"),
            "import" => "Importing save fixture…".to_owned(),
            "export" => format!("Exporting save slot {name}…"),
            _ => "Saving…".to_owned(),
        });
    }

    fn load_profiles(&mut self, store: &crate::saves::SaveStore, status: &mut Option<String>) {
        self.start(store, PendingKind::Profiles, "profiles", "", "", status);
    }

    fn load_slots(&mut self, store: &crate::saves::SaveStore, status: &mut Option<String>) {
        self.start(store, PendingKind::Slots, "slots", "", "", status);
    }

    pub(super) fn show(&mut self, store: &crate::saves::SaveStore, status: &mut Option<String>) {
        if !self.started {
            self.started = true;
            self.load_profiles(store, status);
        } else if self.pending.is_none() {
            self.load_slots(store, status);
        }
    }

    pub(super) fn select_profile(
        &mut self,
        store: &crate::saves::SaveStore,
        profile: &str,
        slots: &mut Vec<SaveSlot>,
        status: &mut Option<String>,
    ) {
        if self.pending.is_some() || !self.profiles.iter().any(|known| known == profile) {
            return;
        }
        if self.selected_profile == profile {
            return;
        }
        self.selected_profile = profile.to_owned();
        slots.clear();
        self.load_slots(store, status);
    }

    pub(super) fn create_profile(
        &mut self,
        store: &crate::saves::SaveStore,
        status: &mut Option<String>,
    ) {
        self.start(
            store,
            PendingKind::CreateProfile,
            "create-profile",
            "",
            "",
            status,
        );
    }

    pub(super) fn create_slot(
        &mut self,
        store: &crate::saves::SaveStore,
        status: &mut Option<String>,
    ) {
        self.start(store, PendingKind::CreateSlot, "create", "", "", status);
    }

    pub(super) fn delete_slot(
        &mut self,
        store: &crate::saves::SaveStore,
        name: &str,
        status: &mut Option<String>,
    ) {
        self.start(
            store,
            PendingKind::Delete(name.to_owned()),
            "delete",
            name,
            "",
            status,
        );
    }

    pub(super) fn set_default(
        &mut self,
        store: &crate::saves::SaveStore,
        name: &str,
        status: &mut Option<String>,
    ) {
        self.start(
            store,
            PendingKind::SetDefault(name.to_owned()),
            "default",
            name,
            "",
            status,
        );
    }

    pub(super) fn import(&mut self, store: &crate::saves::SaveStore, status: &mut Option<String>) {
        self.start(store, PendingKind::Import, "import", "", "", status);
    }

    pub(super) fn export(
        &mut self,
        store: &crate::saves::SaveStore,
        name: &str,
        status: &mut Option<String>,
    ) {
        self.start(store, PendingKind::Export, "export", name, "", status);
    }

    pub(super) fn poll(
        &mut self,
        store: &crate::saves::SaveStore,
        slots: &mut Vec<SaveSlot>,
        selected_slot: &mut usize,
        status: &mut Option<String>,
        tx: &mut Tx,
    ) -> bool {
        if !self.started {
            self.started = true;
            self.load_profiles(store, status);
        }
        let mut changed = false;
        if let Some(result) = store.take_browser_result() {
            changed = self.finish(result, store, slots, selected_slot, status);
        }
        if self.pending.is_some() {
            tx.wake_after(std::time::Duration::from_millis(40));
        }
        changed
    }

    fn finish(
        &mut self,
        raw: String,
        store: &crate::saves::SaveStore,
        slots: &mut Vec<SaveSlot>,
        selected_slot: &mut usize,
        status: &mut Option<String>,
    ) -> bool {
        let mut fields = raw.splitn(3, '\t');
        let Some(id) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
            *status = Some("Browser save operation returned an invalid response".to_owned());
            self.pending = None;
            return true;
        };
        let succeeded = fields.next() == Some("1");
        let payload = fields.next().unwrap_or_default();
        let Some(pending) = self.pending.take() else {
            return false;
        };
        if pending.id != id {
            self.pending = Some(pending);
            return false;
        }
        if !succeeded {
            *status = Some(format!("Browser save operation failed: {payload}"));
            return true;
        }

        match pending.kind {
            PendingKind::Profiles => {
                self.profiles = payload
                    .lines()
                    .filter(|name| valid_profile(name))
                    .map(str::to_owned)
                    .collect();
                if !self.profiles.iter().any(|profile| profile == "default") {
                    self.profiles.insert(0, "default".to_owned());
                }
                if !self
                    .profiles
                    .iter()
                    .any(|profile| profile == &self.selected_profile)
                {
                    self.selected_profile = "default".to_owned();
                }
                self.load_slots(store, status);
            }
            PendingKind::Slots => {
                let previous = slots.get(*selected_slot).map(|slot| slot.name.clone());
                *slots = payload.lines().filter_map(parse_slot).collect();
                *selected_slot = previous
                    .and_then(|name| slots.iter().position(|slot| slot.name == name))
                    .unwrap_or(0);
                *status = Some(format!("Loaded profile {}", pending.profile));
            }
            PendingKind::CreateProfile => {
                if valid_profile(payload) {
                    self.selected_profile = payload.to_owned();
                    *status = Some(format!("Created profile {payload}"));
                    self.load_profiles(store, status);
                } else {
                    *status =
                        Some("Browser save operation returned an invalid profile id".to_owned());
                }
            }
            PendingKind::CreateSlot => {
                *status = Some(format!("Created save slot {payload}"));
                self.load_slots(store, status);
            }
            PendingKind::Delete(name) => {
                *status = Some(format!("Deleted save slot {name}"));
                self.load_slots(store, status);
            }
            PendingKind::SetDefault(name) => {
                *status = Some(format!("{name} is now the default slot"));
                self.load_slots(store, status);
            }
            PendingKind::Import => {
                *status = Some("Imported synthetic save fixture".to_owned());
                self.load_slots(store, status);
            }
            PendingKind::Export => *status = Some(payload.to_owned()),
        }
        true
    }
}

pub(super) fn poll(state: &mut AppState, tx: &mut Tx) -> bool {
    state.browser_saves.poll(
        &state.save_store,
        &mut state.save_slots,
        &mut state.selected_save_slot,
        &mut state.save_status,
        tx,
    )
}

pub(super) fn select_profile(state: &mut AppState, profile: &str) {
    state.browser_saves.select_profile(
        &state.save_store,
        profile,
        &mut state.save_slots,
        &mut state.save_status,
    );
}

pub(super) fn create_profile(state: &mut AppState) {
    state
        .browser_saves
        .create_profile(&state.save_store, &mut state.save_status);
}

fn valid_profile(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn parse_slot(line: &str) -> Option<SaveSlot> {
    let mut fields = line.split('\t');
    let name = fields.next()?;
    let bytes = fields.next()?.parse().ok()?;
    let default = match fields.next()? {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    if fields.next().is_some() || !valid_slot_name(name) || bytes == 0 || bytes > 128 * 1024 {
        return None;
    }
    Some(SaveSlot {
        name: name.to_owned(),
        bytes,
        default,
    })
}

fn valid_slot_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
