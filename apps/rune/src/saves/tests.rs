use super::*;

fn temp_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "gba-native-rune-saves-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn synthetic_slots_create_select_export_and_delete_through_core_library() {
    let root = temp_root("crud");
    let store = SaveStore::at(root.clone());
    assert!(store.slots().unwrap().is_empty());
    store.create().unwrap();
    store.create().unwrap();
    let slots = store.slots().unwrap();
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[0].name, "slot1");
    assert!(slots[0].default);
    store.set_default("slot2").unwrap();
    let path = root.join("chosen-export.sav");
    assert!(
        store
            .export_path("slot2", &path)
            .unwrap()
            .contains("chosen-export.sav")
    );
    assert_eq!(std::fs::read(&path).unwrap(), SaveSlots::synthetic_image(1));
    store.delete("slot1").unwrap();
    assert_eq!(store.slots().unwrap()[0].name, "slot2");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn import_only_accepts_exact_generated_synthetic_save_images() {
    let root = temp_root("import");
    let store = SaveStore::at(root.clone());
    std::fs::create_dir_all(&root).unwrap();
    let import_path = root.join(gba_native::frontend::saves::IMPORT_FILE);
    std::fs::write(&import_path, [1, 2, 3]).unwrap();
    assert!(store.import_path(&import_path).is_err());

    let mut wrong_magic = SaveSlots::synthetic_image(8);
    wrong_magic[..8].copy_from_slice(b"NOTASAVE");
    std::fs::write(&import_path, wrong_magic).unwrap();
    assert!(store.import_path(&import_path).is_err());

    let mut altered_payload = SaveSlots::synthetic_image(8);
    altered_payload[100] ^= 1;
    std::fs::write(&import_path, altered_payload).unwrap();
    assert!(store.import_path(&import_path).is_err());
    assert!(store.slots().unwrap().is_empty());

    let synthetic = SaveSlots::synthetic_image(8);
    std::fs::write(&import_path, &synthetic).unwrap();
    assert!(
        store
            .import_path(&import_path)
            .unwrap()
            .contains("synthetic fixture")
    );
    assert_eq!(store.slots().unwrap()[0].name, "imported");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_views_list_and_export_core_valid_raw_images_unchanged() {
    let root = temp_root("shared-raw-saves");
    let core_slots = SaveSlots::new(root.clone());
    assert!(matches!(
        core_slots.import("empty", &[]),
        Err(gba_native::save::SaveError::InvalidSaveLength(0))
    ));
    assert!(matches!(
        core_slots.import("oversized", &vec![0; MAX_SAVE_BYTES + 1]),
        Err(gba_native::save::SaveError::InvalidSaveLength(_))
    ));
    let one_byte = [0xA7];
    let multi_byte = [0x13, 0x00, 0xFF, 0x42, 0xA1, 0x55, 0x80];
    core_slots.import("one-byte", &one_byte).unwrap();
    core_slots.import("raw-multi", &multi_byte).unwrap();

    let rune = SaveStore::at(root.clone());
    let slots = rune.slots().unwrap();
    assert_eq!(slots.len(), 2);
    assert_eq!((slots[0].name.as_str(), slots[0].bytes), ("one-byte", 1));
    assert_eq!(
        (slots[1].name.as_str(), slots[1].bytes),
        ("raw-multi", multi_byte.len())
    );
    rune.set_default("raw-multi").unwrap();
    assert!(rune.slots().unwrap()[1].default);

    for (name, expected) in [
        ("one-byte", one_byte.as_slice()),
        ("raw-multi", &multi_byte),
    ] {
        let path = root.join(format!("{name}-export.sav"));
        rune.export_path(name, &path).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), expected);
    }
    rune.delete("one-byte").unwrap();
    assert_eq!(rune.slots().unwrap().len(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn creating_after_invalid_slot2_preserves_its_corrupt_bytes() {
    let root = temp_root("slot-collision");
    let rune = SaveStore::at(root.clone());
    rune.create().unwrap();

    let corrupt: [u8; 0] = [];
    let slot_path = root
        .join(gba_native::frontend::saves::SYNTHETIC_ROM_SHA)
        .join("slot2.sav");
    std::fs::write(&slot_path, corrupt).unwrap();

    let visible = rune.slots().unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "slot1");
    rune.create().unwrap();

    assert_eq!(std::fs::read(slot_path).unwrap(), corrupt);
    let core_slots = SaveSlots::new(root.clone());
    assert_eq!(
        core_slots.load("slot3").unwrap(),
        SaveSlots::synthetic_image(2)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_import_bounds_reads_before_validating_the_fixture() {
    let root = temp_root("bounded-import");
    let store = SaveStore::at(root.clone());
    std::fs::create_dir_all(&root).unwrap();
    let oversized = root.join("oversized.sav");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len((MAX_SAVE_BYTES + 1) as u64)
        .unwrap();

    assert!(
        store
            .import_path(&oversized)
            .unwrap_err()
            .contains("exceeds")
    );
    assert!(store.slots().unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_profiles_isolate_slots_and_reload_registered_profiles() {
    let root = temp_root("profiles");
    let mut store = SaveStore::at(root.clone());
    store.create().unwrap();
    store.select_profile("default").unwrap();
    assert_eq!(store.slots().unwrap().len(), 1);

    let occupied = root
        .join(gba_native::frontend::saves::SYNTHETIC_ROM_SHA)
        .join("profiles/profile1");
    std::fs::create_dir_all(&occupied).unwrap();
    std::fs::write(occupied.join("sentinel"), b"keep").unwrap();
    let id = store.create_profile().unwrap();
    assert_eq!(id, "profile2");
    assert_eq!(std::fs::read(occupied.join("sentinel")).unwrap(), b"keep");
    store.create().unwrap();
    assert_eq!(store.slots().unwrap().len(), 1);
    assert_eq!(
        store.profiles().unwrap().as_slice(),
        std::slice::from_ref(&id)
    );

    let imported = SaveSlots::synthetic_image(7);
    let import_path = root.join("profile-import.sav");
    std::fs::write(&import_path, &imported).unwrap();
    store.import_path(&import_path).unwrap();
    let exported = root.join("profile-export.sav");
    store.export_path("imported", &exported).unwrap();
    assert_eq!(std::fs::read(&exported).unwrap(), imported);
    assert_eq!(store.slots().unwrap().len(), 2);

    store.select_profile("default").unwrap();
    assert_eq!(store.slots().unwrap().len(), 1);
    store.select_profile(&id).unwrap();
    assert_eq!(store.slots().unwrap().len(), 2);

    let profile_dir = root
        .join(gba_native::frontend::saves::SYNTHETIC_ROM_SHA)
        .join(format!("profiles/{id}"));
    assert_eq!(
        std::fs::read(profile_dir.join("imported.sav")).unwrap(),
        imported
    );
    std::fs::remove_dir_all(root).unwrap();
}
