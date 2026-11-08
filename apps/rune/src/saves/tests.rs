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
    let corrupt: [u8; 0] = [];
    let slot_path = root
        .join(gba_native::frontend::saves::SYNTHETIC_ROM_SHA)
        .join("slot2.sav");
    std::fs::create_dir_all(slot_path.parent().unwrap()).unwrap();
    std::fs::write(&slot_path, corrupt).unwrap();

    let rune = SaveStore::at(root.clone());
    assert!(rune.slots().unwrap().is_empty());
    rune.create().unwrap();

    assert_eq!(std::fs::read(slot_path).unwrap(), corrupt);
    let core_slots = SaveSlots::new(root.clone());
    assert_eq!(
        core_slots.load("slot3").unwrap(),
        SaveSlots::synthetic_image(1)
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
