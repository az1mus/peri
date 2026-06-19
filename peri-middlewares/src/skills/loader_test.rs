    fn write_skill(dir: &Path, name: &str, desc: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: '{}'\ndescription: '{}'\n---\n\n# {}\n\nContent here.\n",
            name, desc, name
        );
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    /// 在指定 path 直接写一个 SKILL.md（path 含完整文件名）
    fn write_skill_file(path: &Path, name: &str, desc: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let content = format!(
            "---\nname: '{}'\ndescription: '{}'\n---\n\n# {}\n\nContent here.\n",
            name, desc, name
        );
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn test_scan_skill_roots_nested() {
        let root = tempdir().unwrap();
        // 构造 6 层嵌套：root/a/b/c/d/e/f/SKILL.md（depth=6 在范围内）
        let deep = root
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("f");
        write_skill_file(&deep.join("SKILL.md"), "deep-skill", "deep nested");

        let roots = vec![SkillRoot {
            path: root.path().to_path_buf(),
            source: SkillSource::Project,
            plugin_name: None,
        }];
        let skills = scan_skill_roots(&roots);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "deep-skill");
        assert_eq!(skills[0].source, SkillSource::Project);
    }

    #[test]
    fn test_scan_skill_roots_depth_limit() {
        let root = tempdir().unwrap();
        // 构造 7 层嵌套：root/a/b/c/d/e/f/g/SKILL.md（depth=7 超出限制）
        let too_deep = root
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e")
            .join("f")
            .join("g");
        write_skill_file(&too_deep.join("SKILL.md"), "too-deep", "ignored");

        let roots = vec![SkillRoot {
            path: root.path().to_path_buf(),
            source: SkillSource::Project,
            plugin_name: None,
        }];
        let skills = scan_skill_roots(&roots);
        assert!(
            skills.is_empty(),
            "7 层深度的 SKILL.md 应被 MAX_SCAN_DEPTH=6 拒绝"
        );
    }

    #[test]
    fn test_scan_skill_roots_leaf_semantics() {
        let root = tempdir().unwrap();
        // dir 含 SKILL.md，且 dir/sub 也含 SKILL.md
        // 叶子语义：dir/SKILL.md 加载，dir/sub/SKILL.md 不应被扫描
        let dir = root.path().join("my-skill");
        write_skill_file(&dir.join("SKILL.md"), "parent", "parent skill");
        write_skill_file(&dir.join("sub").join("SKILL.md"), "child", "child skill");

        let roots = vec![SkillRoot {
            path: root.path().to_path_buf(),
            source: SkillSource::Project,
            plugin_name: None,
        }];
        let skills = scan_skill_roots(&roots);
        assert_eq!(
            skills.len(),
            1,
            "叶子语义应停止下钻，子目录 SKILL.md 不被扫描"
        );
        assert_eq!(skills[0].name, "parent");
    }

    #[test]
    fn test_scan_skill_roots_dir_count_limit() {
        let root = tempdir().unwrap();
        // 构造 5 个子目录，每个含 SKILL.md
        for i in 0..5 {
            let dir = root.path().join(format!("skill-{i}"));
            write_skill_file(&dir.join("SKILL.md"), &format!("s{i}"), "x");
        }

        let roots = vec![SkillRoot {
            path: root.path().to_path_buf(),
            source: SkillSource::Project,
            plugin_name: None,
        }];
        // 注入 max_dirs=3（root 自身算 1，最多再扫 2 个子目录）
        let skills = scan_skill_roots_with_limits(&roots, 6, 3);
        assert!(
            skills.len() <= 2,
            "max_dirs=3 时 root 自身占 1，剩余配额 2，扫描结果应 ≤ 2，实际 {}",
            skills.len()
        );
    }

    #[test]
    #[cfg(unix)] // symlink 在 Windows 需要管理员权限，仅在 unix 测试
    fn test_scan_skill_roots_symlink_followed() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        let real_target = tempdir().unwrap();
        // real_target/my-skill/SKILL.md
        write_skill_file(
            &real_target.path().join("my-skill").join("SKILL.md"),
            "linked",
            "via symlink",
        );
        // root/linked → real_target（symlink 应被跟随）
        symlink(real_target.path(), root.path().join("linked")).unwrap();

        let roots = vec![SkillRoot {
            path: root.path().to_path_buf(),
            source: SkillSource::User,
            plugin_name: None,
        }];
        let skills = scan_skill_roots(&roots);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "linked");
    }

    #[test]
    #[cfg(unix)]
    fn test_scan_skill_roots_symlink_loop() {
        use std::os::unix::fs::symlink;
        let root = tempdir().unwrap();
        // 构造环：root/a/loop → root/a（自指）
        let a_dir = root.path().join("a");
        std::fs::create_dir_all(&a_dir).unwrap();
        symlink(&a_dir, a_dir.join("loop")).unwrap();
        write_skill_file(&a_dir.join("SKILL.md"), "real", "real skill");

        let roots = vec![SkillRoot {
            path: root.path().to_path_buf(),
            source: SkillSource::Project,
            plugin_name: None,
        }];
        // 不应无限递归（防环 canonicalize 命中 visited 后退出）
        let skills = scan_skill_roots(&roots);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "real");
    }

    #[test]
    fn test_scan_skill_roots_dedup_across_roots() {
        let user_dir = tempdir().unwrap();
        let project_dir = tempdir().unwrap();
        // 两个 root 都有同名 skill "common"
        write_skill_file(
            &user_dir.path().join("common").join("SKILL.md"),
            "common",
            "from user",
        );
        write_skill_file(
            &project_dir.path().join("common").join("SKILL.md"),
            "common",
            "from project",
        );

        let roots = vec![
            SkillRoot {
                path: user_dir.path().to_path_buf(),
                source: SkillSource::User,
                plugin_name: None,
            },
            SkillRoot {
                path: project_dir.path().to_path_buf(),
                source: SkillSource::Project,
                plugin_name: None,
            },
        ];
        let skills = scan_skill_roots(&roots);
        assert_eq!(skills.len(), 1);
        assert_eq!(
            skills[0].description, "from user",
            "User 应先于 Project 胜出"
        );
        assert_eq!(skills[0].source, SkillSource::User);
    }

    #[test]
    fn test_scan_skill_roots_dedup_within_root() {
        let root = tempdir().unwrap();
        // 同一 root 下两个不同子目录都有 "dup" skill
        write_skill_file(&root.path().join("a").join("SKILL.md"), "dup", "from a");
        write_skill_file(&root.path().join("b").join("SKILL.md"), "dup", "from b");

        let roots = vec![SkillRoot {
            path: root.path().to_path_buf(),
            source: SkillSource::Project,
            plugin_name: None,
        }];
        let skills = scan_skill_roots(&roots);
        assert_eq!(skills.len(), 1);
        // subdirs.sort() 后 "a" 排在 "b" 前，应胜出
        assert_eq!(skills[0].description, "from a");
    }

    #[test]
    fn test_scan_skill_roots_source_tag() {
        let root = tempdir().unwrap();
        write_skill_file(&root.path().join("p").join("SKILL.md"), "x", "y");

        let roots = vec![SkillRoot {
            path: root.path().to_path_buf(),
            source: SkillSource::Plugin,
            plugin_name: Some("my-plugin".to_string()),
        }];
        let skills = scan_skill_roots(&roots);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source, SkillSource::Plugin);
        assert_eq!(skills[0].plugin_name.as_deref(), Some("my-plugin"));
    }

    #[test]
    fn test_load_skill_metadata() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "my-skill", "A test skill");
        let skill_file = dir.path().join("my-skill").join("SKILL.md");
        let meta = load_skill_metadata(&skill_file).unwrap();
        assert_eq!(meta.name, "my-skill");
        assert_eq!(meta.description, "A test skill");
    }

    #[test]
    fn test_list_skills_dedup() {
        let dir1 = tempdir().unwrap();
        let dir2 = tempdir().unwrap();
        write_skill(dir1.path(), "skill-a", "from dir1");
        write_skill(dir1.path(), "skill-b", "from dir1");
        write_skill(dir2.path(), "skill-a", "from dir2"); // 重复，应被忽略
        write_skill(dir2.path(), "skill-c", "from dir2");

        let skills = list_skills(&[dir1.path().to_path_buf(), dir2.path().to_path_buf()]);
        assert_eq!(skills.len(), 3);

        let skill_a = skills.iter().find(|s| s.name == "skill-a").unwrap();
        assert_eq!(skill_a.description, "from dir1"); // dir1 优先
    }

    #[test]
    fn test_resolve_skill_roots_returns_standard_paths() {
        let cwd = "/tmp/test-project";
        let roots = resolve_skill_roots(cwd, vec![]);
        assert!(
            roots
                .iter()
                .any(|r| r.path.ends_with(".claude/skills") && r.source == SkillSource::User),
            "应包含 ~/.claude/skills 作为 User source"
        );
        assert!(
            roots
                .iter()
                .any(|r| r.path == Path::new("/tmp/test-project/.claude/skills")
                    && r.source == SkillSource::Project),
            "应包含项目 .claude/skills 作为 Project source"
        );
    }

    #[test]
    fn test_resolve_skill_roots_includes_plugin_roots() {
        let extra = tempfile::tempdir().unwrap();
        let plugin_root = SkillRoot {
            path: extra.path().to_path_buf(),
            source: SkillSource::Plugin,
            plugin_name: Some("test-plugin".to_string()),
        };
        let roots = resolve_skill_roots("/tmp", vec![plugin_root]);
        assert!(
            roots.iter().any(|r| r.path == extra.path().to_path_buf()
                && r.source == SkillSource::Plugin
                && r.plugin_name.as_deref() == Some("test-plugin")),
            "应包含传入的 plugin root"
        );
    }

    #[test]
    fn test_resolve_skill_roots_skips_nonexistent_plugin_roots() {
        let nonexistent = SkillRoot {
            path: PathBuf::from("/nonexistent/path"),
            source: SkillSource::Plugin,
            plugin_name: None,
        };
        let roots = resolve_skill_roots("/tmp", vec![nonexistent]);
        assert!(
            !roots.iter().any(|r| r.path.to_str() == Some("/nonexistent/path")),
            "不存在的 plugin root 应被跳过"
        );
    }
