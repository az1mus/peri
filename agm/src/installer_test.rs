use crate::installer::remove_package_symlinks;

#[test]
fn test_remove_package_symlinks_removes_links_into_store() {
    let tmp = tempfile::TempDir::new().unwrap();
    let target_dir = tmp.path().join("target");
    let store_path = tmp.path().join("store").join("pkg@1.0.0");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::create_dir_all(&store_path).unwrap();

    let link_path = target_dir.join("skill-a");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&store_path, &link_path).unwrap();
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(&store_path, &link_path).unwrap();
    }

    remove_package_symlinks(&target_dir, store_path.parent().unwrap()).unwrap();

    assert!(!link_path.exists(), "指向 store 的 symlink 应被删除");
}

#[test]
fn test_remove_package_symlinks_keeps_unrelated_entries() {
    let tmp = tempfile::TempDir::new().unwrap();
    let target_dir = tmp.path().join("target");
    let store_path = tmp.path().join("store").join("pkg@1.0.0");
    let other_path = tmp.path().join("other");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::create_dir_all(&store_path).unwrap();
    std::fs::create_dir_all(&other_path).unwrap();

    let unrelated_link = target_dir.join("unrelated");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&other_path, &unrelated_link).unwrap();
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(&other_path, &unrelated_link).unwrap();
    }

    let regular_file = target_dir.join("regular.txt");
    std::fs::write(&regular_file, "hello").unwrap();

    remove_package_symlinks(&target_dir, store_path.parent().unwrap()).unwrap();

    assert!(
        unrelated_link.exists() || unrelated_link.read_link().is_ok(),
        "无关 symlink 不应被删除"
    );
    assert!(regular_file.exists(), "普通文件不应被删除");
}

#[test]
fn test_remove_package_symlinks_uses_canonical_paths() {
    let tmp = tempfile::TempDir::new().unwrap();
    let target_dir = tmp.path().join("target");
    let real_store = tmp.path().join("store").join("pkg@1.0.0");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::create_dir_all(&real_store).unwrap();

    // Create a symlink to the store directory, then refer to the store through a
    // different path (e.g. via the parent). The canonicalized comparison should
    // still identify the link as belonging to the store.
    let store_alias = tmp.path().join("store_alias");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(tmp.path().join("store"), &store_alias).unwrap();
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(tmp.path().join("store"), &store_alias).unwrap();
    }

    let link_path = target_dir.join("skill-a");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&real_store, &link_path).unwrap();
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(&real_store, &link_path).unwrap();
    }

    remove_package_symlinks(&target_dir, &store_alias.join("pkg@1.0.0")).unwrap();

    assert!(!link_path.exists(), "应通过 canonicalize 匹配到 store 并删除 symlink");
}

#[cfg(windows)]
#[test]
fn test_remove_package_symlinks_dir_symlink_does_not_follow_target() {
    let tmp = tempfile::TempDir::new().unwrap();
    let target_dir = tmp.path().join("target");
    let store_path = tmp.path().join("store").join("pkg@1.0.0");
    let protected_file = store_path.join("protected.txt");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::create_dir_all(&store_path).unwrap();
    std::fs::write(&protected_file, "keep me").unwrap();

    let link_path = target_dir.join("skill-a");
    std::os::windows::fs::symlink_dir(&store_path, &link_path).unwrap();

    remove_package_symlinks(&target_dir, store_path.parent().unwrap()).unwrap();

    assert!(!link_path.exists(), "目录 symlink 本身应被删除");
    assert!(
        protected_file.exists(),
        "不应跟随 symlink 删除目标目录内容"
    );
}
