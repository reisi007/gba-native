use super::*;

fn temp_root(tag: &str) -> PathBuf {
    let unique = format!(
        "gba-native-save-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let path = std::env::temp_dir().join(unique);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn named_profiles_isolate_slots_defaults_and_restart() {
    let root = temp_root("profiles");
    let a = SaveLibrary::for_profile(&root, "alice").unwrap();
    let b = SaveLibrary::for_profile(&root, "bob").unwrap();
    let legacy = SaveLibrary::new(&root);
    legacy.store(DIGEST, "compat", b"legacy").unwrap();
    assert_eq!(
        SaveLibrary::for_profile(&root, "default")
            .unwrap()
            .load(DIGEST, "compat")
            .unwrap(),
        b"legacy"
    );
    assert_eq!(
        fs::read(root.join(DIGEST).join("compat.sav")).unwrap(),
        b"legacy"
    );
    assert_eq!(
        fs::read(root.join(DIGEST).join(OWNER_FILE)).unwrap(),
        owner_marker(DIGEST).as_bytes()
    );
    SaveLibrary::for_profile(&root, "default")
        .unwrap()
        .store(DIGEST, "default-write", b"same path")
        .unwrap();
    assert_eq!(legacy.load(DIGEST, "default-write").unwrap(), b"same path");
    a.store(DIGEST, "one", b"a").unwrap();
    b.store(DIGEST, "one", b"b").unwrap();
    assert_eq!(
        fs::read_to_string(
            root.join(DIGEST)
                .join("profiles/alice/.gba-native-profile-owner")
        )
        .unwrap(),
        format!(
            "gba-native-save-profile-owner-v1\nrom-sha256={DIGEST}\nprofile-id=alice\nsave-format=raw-backup-v1\n"
        )
    );
    assert_eq!(a.load(DIGEST, "one").unwrap(), b"a");
    assert_eq!(b.load(DIGEST, "one").unwrap(), b"b");
    assert_eq!(a.list(DIGEST).unwrap(), ["one"]);
    a.set_default(DIGEST, "one").unwrap();
    b.store(DIGEST, "two", b"c").unwrap();
    assert_eq!(b.list(DIGEST).unwrap(), ["one", "two"]);
    b.set_default(DIGEST, "two").unwrap();
    a.delete(DIGEST, "one").unwrap();
    assert_eq!(b.default_slot(DIGEST).unwrap().as_deref(), Some("two"));
    assert_eq!(
        SaveLibrary::for_profile(&root, "bob")
            .unwrap()
            .load(DIGEST, "one")
            .unwrap(),
        b"b"
    );
    assert_eq!(legacy.list(DIGEST).unwrap(), ["compat", "default-write"]);
    assert_eq!(
        SaveLibrary::for_profile(&root, "default").unwrap().root(),
        root
    );
    for invalid in ["", "..", "../x", "a/b", "x".repeat(33).as_str()] {
        assert!(matches!(
            SaveLibrary::for_profile(&root, invalid),
            Err(SaveError::InvalidProfileId)
        ));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn wrong_profile_marker_fails_closed_without_changing_profile_data() {
    let root = temp_root("wrong-profile");
    let lib = SaveLibrary::for_profile(&root, "alice").unwrap();
    lib.store(DIGEST, "slot", b"keep").unwrap();
    let dir = root.join(DIGEST).join("profiles/alice");
    let marker = dir.join(".gba-native-profile-owner");
    fs::write(
        &marker,
        b"gba-native-save-profile-owner-v1\nrom-sha256=wrong\nprofile-id=bob\n",
    )
    .unwrap();
    for result in [
        lib.store(DIGEST, "slot", b"change"),
        lib.delete(DIGEST, "slot"),
        lib.set_default(DIGEST, "slot"),
    ] {
        assert!(matches!(result, Err(SaveError::UnownedDirectory)));
    }
    assert_eq!(fs::read(dir.join("slot.sav")).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn profile_namespace_symlinks_are_rejected_for_reads_and_writes() {
    let root = temp_root("profile-symlink");
    let lib = SaveLibrary::for_profile(&root, "alice").unwrap();
    lib.store(DIGEST, "slot", b"safe").unwrap();
    let outside = root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::remove_dir_all(root.join(DIGEST).join("profiles")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join(DIGEST).join("profiles")).unwrap();
    assert!(matches!(lib.list(DIGEST), Err(SaveError::UnownedDirectory)));
    assert!(matches!(
        lib.load(DIGEST, "slot"),
        Err(SaveError::UnownedDirectory)
    ));
    assert!(matches!(
        lib.store(DIGEST, "slot", b"changed"),
        Err(SaveError::UnownedDirectory)
    ));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_file(root.join(DIGEST).join("profiles")).unwrap();
    let profiles = root.join(DIGEST).join("profiles");
    fs::create_dir(&profiles).unwrap();
    std::os::unix::fs::symlink(&outside, profiles.join("alice")).unwrap();
    assert!(matches!(lib.list(DIGEST), Err(SaveError::UnownedDirectory)));
    assert!(matches!(
        lib.load(DIGEST, "slot"),
        Err(SaveError::UnownedDirectory)
    ));
    assert!(matches!(
        lib.store(DIGEST, "slot", b"changed"),
        Err(SaveError::UnownedDirectory)
    ));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_dir_all(root.join(DIGEST)).unwrap();
    std::os::unix::fs::symlink(&outside, root.join(DIGEST)).unwrap();
    assert!(matches!(lib.list(DIGEST), Err(SaveError::UnownedDirectory)));
    assert!(matches!(
        lib.load(DIGEST, "slot"),
        Err(SaveError::UnownedDirectory)
    ));
    assert!(matches!(
        lib.store(DIGEST, "slot", b"changed"),
        Err(SaveError::UnownedDirectory)
    ));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_file(root.join(DIGEST)).unwrap();
    fs::remove_dir_all(root).unwrap();
}

const DIGEST: &str = "8a7667d43d9ec0333a5a7c0a032837919e101ca1c1893bdf5f95f51b27555539";
const OTHER: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn multiple_slots_are_independent_and_default_is_tracked() {
    let root = temp_root("slots");
    let library = SaveLibrary::new(&root);
    assert_eq!(library.list(DIGEST).unwrap(), Vec::<String>::new());
    library.store(DIGEST, "first", &[0x47, 0x53]).unwrap();
    library.store(DIGEST, "second", &[0x41, 0x56]).unwrap();
    assert_eq!(library.list(DIGEST).unwrap(), ["first", "second"]);
    assert_eq!(library.load(DIGEST, "first").unwrap(), [0x47, 0x53]);
    assert_eq!(library.load(DIGEST, "second").unwrap(), [0x41, 0x56]);
    assert_eq!(
        library.default_slot(DIGEST).unwrap().as_deref(),
        Some("first")
    );
    library.set_default(DIGEST, "second").unwrap();
    assert_eq!(
        library.default_slot(DIGEST).unwrap().as_deref(),
        Some("second")
    );
    assert_eq!(
        library.resolve(DIGEST, None).unwrap().as_deref(),
        Some("second")
    );
    assert_eq!(
        library.resolve(DIGEST, Some("first")).unwrap().as_deref(),
        Some("first")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_the_default_falls_back_to_another_slot() {
    let root = temp_root("delete");
    let library = SaveLibrary::new(&root);
    library.store(DIGEST, "alpha", &[1]).unwrap();
    library.store(DIGEST, "beta", &[2]).unwrap();
    library.delete(DIGEST, "alpha").unwrap();
    assert_eq!(
        library.default_slot(DIGEST).unwrap().as_deref(),
        Some("beta")
    );
    assert!(library.load(DIGEST, "alpha").is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn digests_are_separate_and_names_cannot_escape_the_root() {
    let root = temp_root("names");
    let library = SaveLibrary::new(&root);
    library.store(DIGEST, "slot", &[1]).unwrap();
    library.store(OTHER, "slot", &[2]).unwrap();
    assert_eq!(library.load(DIGEST, "slot").unwrap(), [1]);
    assert_eq!(library.load(OTHER, "slot").unwrap(), [2]);
    for bad in ["", ".", "..", "../x", "a/b", "a\\b", &"x".repeat(33)] {
        assert!(matches!(
            library.store(DIGEST, bad, &[1]),
            Err(SaveError::InvalidSlotName)
        ));
    }
    assert!(matches!(
        library.list("../x"),
        Err(SaveError::InvalidRomDigest)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejected_images_do_not_create_a_slot() {
    let root = temp_root("length");
    let library = SaveLibrary::new(&root);
    assert!(matches!(
        library.store(DIGEST, "slot", &[]),
        Err(SaveError::InvalidSaveLength(0))
    ));
    let too_big = vec![0; MAX_SAVE_LEN + 1];
    assert!(matches!(
        library.store(DIGEST, "slot", &too_big),
        Err(SaveError::InvalidSaveLength(_))
    ));
    assert!(library.list(DIGEST).unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn loading_an_oversized_file_reads_only_the_rejection_byte() {
    let root = temp_root("oversized-load");
    let path = root.join(DIGEST).join("large.sav");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::File::create(&path)
        .unwrap()
        .set_len((MAX_SAVE_LEN + 1) as u64)
        .unwrap();

    assert!(matches!(
        SaveLibrary::new(&root).load(DIGEST, "large"),
        Err(SaveError::InvalidSaveLength(len)) if len == MAX_SAVE_LEN + 1
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn replacing_a_slot_keeps_the_new_image_and_cleans_temporary_files() {
    let root = temp_root("replace");
    let library = SaveLibrary::new(&root);
    library.store(DIGEST, "slot", &[1, 2, 3]).unwrap();
    library.store(DIGEST, "slot", &[4, 5, 6, 7]).unwrap();

    assert_eq!(library.load(DIGEST, "slot").unwrap(), [4, 5, 6, 7]);
    let mut files: Vec<_> = fs::read_dir(root.join(DIGEST))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(files, [".gba-native-owner", "default", "slot.sav"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_replacements_preserve_destinations_and_recover_cleanly() {
    let root = temp_root("replace-failure");
    let library = SaveLibrary::new(&root);
    library.store(DIGEST, "slot", &[1, 2, 3]).unwrap();
    let destination = root.join(DIGEST).join("slot.sav");
    assert!(matches!(
        write_atomic_with(&destination, &[4, 5], |temporary, target| {
            assert_eq!(fs::read(target).unwrap(), [1, 2, 3]);
            assert_eq!(fs::read(temporary).unwrap(), [4, 5]);
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected rename failure",
            ))
        }),
        Err(SaveError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied
    ));
    assert_eq!(library.load(DIGEST, "slot").unwrap(), [1, 2, 3]);

    let obstructing_destination = root.join(DIGEST).join("blocked.sav");
    fs::create_dir_all(&obstructing_destination).unwrap();
    let sentinel = obstructing_destination.join("unrelated");
    fs::write(&sentinel, b"preserve me").unwrap();
    assert!(matches!(
        library.store(DIGEST, "blocked", &[6, 7]),
        Err(SaveError::Io(_))
    ));
    assert_eq!(fs::read(&sentinel).unwrap(), b"preserve me");
    let mut entries: Vec<_> = fs::read_dir(destination.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        [".gba-native-owner", "blocked.sav", "default", "slot.sav"]
    );

    fs::remove_dir_all(&obstructing_destination).unwrap();
    library.store(DIGEST, "blocked", &[8, 9]).unwrap();
    assert_eq!(library.load(DIGEST, "blocked").unwrap(), [8, 9]);
    library.store(DIGEST, "slot", &[4, 5]).unwrap();
    assert_eq!(library.load(DIGEST, "slot").unwrap(), [4, 5]);

    let exhausted_target = root.join(DIGEST).join("exhausted.sav");
    for id in 0..MAX_TEMP_ATTEMPTS as u64 {
        fs::write(temporary_path(&exhausted_target, id).unwrap(), b"occupied").unwrap();
    }
    let mut next_id = 0;
    assert!(matches!(
        create_temporary_file_with(&exhausted_target, || {
            let id = next_id;
            next_id += 1;
            id
        })
        .as_ref()
        .map_err(|error| error.kind()),
        Err(io::ErrorKind::AlreadyExists)
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unowned_nonempty_directory_is_never_adopted_or_mutated() {
    let root = temp_root("unowned");
    let dir = root.join(DIGEST);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("slot.sav"), b"user save").unwrap();
    fs::write(dir.join("sentinel"), b"untouched").unwrap();
    let library = SaveLibrary::new(&root);
    assert_eq!(library.list(DIGEST).unwrap(), ["slot"]);
    assert_eq!(library.load(DIGEST, "slot").unwrap(), b"user save");
    assert!(matches!(
        library.store(DIGEST, "slot", b"new"),
        Err(SaveError::UnownedDirectory)
    ));
    assert!(matches!(
        library.set_default(DIGEST, "slot"),
        Err(SaveError::UnownedDirectory)
    ));
    assert!(matches!(
        library.delete(DIGEST, "slot"),
        Err(SaveError::UnownedDirectory)
    ));
    assert_eq!(fs::read(dir.join("slot.sav")).unwrap(), b"user save");
    assert_eq!(fs::read(dir.join("sentinel")).unwrap(), b"untouched");
    assert!(!dir.join(OWNER_FILE).exists());

    let marker = fs::File::create(dir.join(OWNER_FILE)).unwrap();
    marker.set_len(1024 * 1024 * 1024).unwrap();
    assert!(matches!(
        library.store(DIGEST, "slot", b"new"),
        Err(SaveError::UnownedDirectory)
    ));
    assert_eq!(fs::read(dir.join("slot.sav")).unwrap(), b"user save");
    fs::remove_dir_all(root).unwrap();

    #[cfg(unix)]
    {
        let root = temp_root("symlink");
        let target = root.join("outside");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, root.join(DIGEST)).unwrap();

        assert!(matches!(
            SaveLibrary::new(&root).store(DIGEST, "slot", b"save"),
            Err(SaveError::UnownedDirectory)
        ));
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn first_store_claims_new_directory_and_restart_can_replace_owned_slot() {
    let root = temp_root("owned-restart");
    let dir = root.join(DIGEST);
    let library = SaveLibrary::new(&root);

    let first_lock = library.mutation_lock(DIGEST).unwrap();
    let second_lock = library.open_mutation_lock_file(DIGEST).unwrap();
    assert!(matches!(
        second_lock.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    drop(first_lock);

    fs::create_dir(&dir).unwrap();
    assert!(matches!(
        library.store(DIGEST, "slot", b"not adopted"),
        Err(SaveError::UnownedDirectory)
    ));
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    fs::remove_dir(&dir).unwrap();

    let failed_claim = root.join("failed-claim");
    fs::create_dir(&failed_claim).unwrap();
    let marker = failed_claim.join(OWNER_FILE);
    assert!(matches!(
        write_owner_marker_with(&marker, b"complete marker", |temporary, target| {
            assert_eq!(fs::read(temporary).unwrap(), b"complete marker");
            assert_eq!(target, marker.as_path());
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected marker install failure",
            ))
        }),
        Err(SaveError::Io(error)) if error.kind() == io::ErrorKind::PermissionDenied
    ));
    assert!(!marker.exists());
    assert_eq!(fs::read_dir(&failed_claim).unwrap().count(), 0);

    fs::write(&marker, b"other process marker").unwrap();
    assert!(matches!(
        write_owner_marker_with(&marker, b"replacement", |temporary, target| {
            fs::hard_link(temporary, target)
        }),
        Err(SaveError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists
    ));
    assert_eq!(fs::read(&marker).unwrap(), b"other process marker");
    assert_eq!(fs::read_dir(&failed_claim).unwrap().count(), 1);
    fs::remove_file(&marker).unwrap();
    fs::remove_dir(&failed_claim).unwrap();

    let uppercase_digest = DIGEST.to_ascii_uppercase();
    library.store(&uppercase_digest, "slot", b"first").unwrap();
    assert_eq!(
        fs::read(dir.join(OWNER_FILE)).unwrap(),
        owner_marker(DIGEST).as_bytes()
    );
    SaveLibrary::new(&root)
        .store(DIGEST, "slot", b"replacement")
        .unwrap();
    assert_eq!(
        SaveLibrary::new(&root).load(DIGEST, "slot").unwrap(),
        b"replacement"
    );
    fs::remove_dir_all(root).unwrap();
}
