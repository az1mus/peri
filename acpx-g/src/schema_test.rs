use super::*;

#[test]
fn test_platform_files_resolve_matching() {
    let pf = PlatformFiles {
        linux: Some("./linux.sh".to_string()),
        macos: Some("./mac.sh".to_string()),
        windows: None,
        default: None,
    };
    assert_eq!(pf.resolve(Platform::Linux).unwrap(), "./linux.sh");
    assert_eq!(pf.resolve(Platform::MacOs).unwrap(), "./mac.sh");
}

#[test]
fn test_platform_files_resolve_default_fallback() {
    let pf = PlatformFiles {
        linux: None,
        macos: None,
        windows: None,
        default: Some("./default.sh".to_string()),
    };
    assert_eq!(pf.resolve(Platform::Linux).unwrap(), "./default.sh");
}

#[test]
fn test_platform_files_resolve_no_match_error() {
    let pf = PlatformFiles {
        linux: Some("./linux.sh".to_string()),
        macos: None,
        windows: None,
        default: None,
    };
    assert!(pf.resolve(Platform::Windows).is_err());
}

#[test]
fn test_script_source_inline() {
    let src = ScriptSource::Inline("echo hello".to_string());
    let resolved = src.resolve(Platform::Linux).unwrap();
    match resolved {
        ResolvedScript::Inline(s) => assert_eq!(s, "echo hello"),
        _ => panic!("expected Inline"),
    }
}

#[test]
fn test_script_source_file() {
    let src = ScriptSource::File(FileSource {
        file: "./script.sh".to_string(),
    });
    let resolved = src.resolve(Platform::MacOs).unwrap();
    match resolved {
        ResolvedScript::File(p) => assert_eq!(p, "./script.sh"),
        _ => panic!("expected File"),
    }
}

#[test]
fn test_parse_workflow_valid() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: greet
    type: shell
    run: echo hello
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.name, "test");
    assert_eq!(wf.version, "1.0");
    assert_eq!(wf.nodes.len(), 1);
}

#[test]
fn test_parse_workflow_empty_name() {
    let yaml = r#"
name: ""
version: "1.0"
nodes:
  - id: greet
    type: shell
    run: echo hello
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("name must not be empty"));
}

#[test]
fn test_parse_workflow_empty_version() {
    let yaml = r#"
name: test
version: ""
nodes:
  - id: greet
    type: shell
    run: echo hello
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("version must not be empty"));
}

#[test]
fn test_parse_workflow_no_nodes() {
    let yaml = r#"
name: test
version: "1.0"
nodes: []
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("at least one node"));
}

#[test]
fn test_parse_workflow_reference_missing_in_map() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: call
    type: reference
    ref: nonexistent
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err
        .to_string()
        .contains("not defined in the references section"));
}

#[test]
fn test_parse_workflow_reference_valid() {
    let yaml = r#"
name: test
version: "1.0"
references:
  sub: ./sub.yaml
nodes:
  - id: call
    type: reference
    ref: sub
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.name, "test");
    assert_eq!(wf.references["sub"], "./sub.yaml");
}

#[test]
fn test_parse_workflow_reference_with_params() {
    let yaml = r#"
name: test
version: "1.0"
references:
  sub: ./sub.yaml
nodes:
  - id: step-1
    type: shell
    run: echo hello
  - id: call
    type: reference
    ref: sub
    depends: [step-1]
    with:
      name: world
      count: 3
"#;
    let wf = parse_workflow(yaml).unwrap();
    let node = wf
        .nodes
        .iter()
        .find(|n| match n {
            NodeDef::Reference(r) => r.id == "call",
            _ => false,
        })
        .unwrap();
    if let NodeDef::Reference(r) = node {
        assert_eq!(r.r#ref, "sub");
        assert_eq!(r.depends, vec!["step-1"]);
        assert_eq!(r.with["name"].as_str(), Some("world"));
        assert_eq!(r.with["count"].as_i64(), Some(3));
    } else {
        panic!("expected Reference node");
    }
}

#[test]
fn test_parse_workflow_whitespace_name() {
    let yaml = r#"
name: "   "
version: "1.0"
nodes:
  - id: greet
    type: shell
    run: echo hello
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("name must not be empty"));
}

#[test]
fn test_parse_workflow_invalid_yaml() {
    let yaml = "not: valid: yaml: {{{";
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("failed to parse"));
}

#[test]
fn test_prompt_source_inline() {
    let src = PromptSource::Inline("review code".to_string());
    let resolved = src.resolve(Platform::Linux).unwrap();
    match resolved {
        ResolvedPrompt::Inline(s) => assert_eq!(s, "review code"),
        _ => panic!("expected Inline"),
    }
}

#[test]
fn test_prompt_source_file() {
    let src = PromptSource::File(FileSource {
        file: "./prompt.txt".to_string(),
    });
    let resolved = src.resolve(Platform::Linux).unwrap();
    match resolved {
        ResolvedPrompt::File(p) => assert_eq!(p, "./prompt.txt"),
        _ => panic!("expected File"),
    }
}

#[test]
fn test_script_source_platform_resolve() {
    let src = ScriptSource::Platform(PlatformFiles {
        linux: Some("./linux.sh".to_string()),
        macos: None,
        windows: None,
        default: None,
    });
    let resolved = src.resolve(Platform::Linux).unwrap();
    match resolved {
        ResolvedScript::File(p) => assert_eq!(p, "./linux.sh"),
        _ => panic!("expected File"),
    }
}

#[test]
fn test_parse_workflow_self_dependency() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: loop
    type: shell
    depends: [loop]
    run: echo hello
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("cannot depend on itself"));
}

#[test]
fn test_parse_workflow_multiple_nodes_valid_deps() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: build
    type: shell
    run: echo build
  - id: test
    type: shell
    depends: [build]
    run: echo test
  - id: deploy
    type: shell
    depends: [test]
    run: echo deploy
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.nodes.len(), 3);
}

#[test]
fn test_parse_workflow_with_inputs() {
    let yaml = r#"
name: test
version: "1.0"
inputs:
  env:
    type: string
    default: staging
  count:
    type: number
    required: true
nodes:
  - id: greet
    type: shell
    run: echo {{ inputs.env }}
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert!(wf.inputs.contains_key("env"));
    assert!(wf.inputs.contains_key("count"));
    assert!(wf.inputs["count"].required);
    assert_eq!(wf.inputs["env"].default.as_deref(), Some("staging"));
}

#[test]
fn test_parse_workflow_with_env() {
    let yaml = r#"
name: test
version: "1.0"
env:
  FOO: bar
  BAZ: qux
nodes:
  - id: greet
    type: shell
    run: echo $FOO
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.env.get("FOO").unwrap(), "bar");
    assert_eq!(wf.env.get("BAZ").unwrap(), "qux");
}

#[test]
fn test_parse_workflow_defaults() {
    let yaml = r#"
name: test
version: "1.0"
defaults:
  retry: 3
  timeout: 600
nodes:
  - id: greet
    type: shell
    run: echo hello
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.defaults.retry, 3);
    assert_eq!(wf.defaults.timeout, 600);
}

#[test]
fn test_parse_workflow_partial_defaults() {
    let yaml = r#"
name: test
version: "1.0"
defaults:
  retry: 5
nodes:
  - id: greet
    type: shell
    run: echo hello
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.defaults.retry, 5);
    assert_eq!(wf.defaults.timeout, 300);
    assert_eq!(wf.defaults.shell, "bash -c");
}

#[test]
fn test_parse_workflow_per_node_exec_config() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: fast
    type: shell
    run: echo fast
  - id: slow
    type: shell
    run: echo slow
    timeout: 120
    retry: 3
    shell: "zsh -c"
    continue_on_error: true
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.nodes.len(), 2);
    match &wf.nodes[0] {
        NodeDef::Shell(n) => {
            assert!(n.exec.timeout.is_none());
            assert!(n.exec.retry.is_none());
            assert!(n.exec.shell.is_none());
            assert!(!n.continue_on_error);
        }
        _ => panic!("expected shell node"),
    }
    match &wf.nodes[1] {
        NodeDef::Shell(n) => {
            assert_eq!(n.exec.timeout, Some(120));
            assert_eq!(n.exec.retry, Some(3));
            assert_eq!(n.exec.shell.as_deref(), Some("zsh -c"));
            assert!(n.continue_on_error);
        }
        _ => panic!("expected shell node"),
    }
}

#[test]
fn test_parse_workflow_empty_node_id() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: ""
    type: shell
    run: echo hello
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("node id must not be empty"));
}

#[test]
fn test_parse_workflow_node_id_special_chars() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: "build@node"
    type: shell
    run: echo hello
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("invalid characters"));
}

#[test]
fn test_parse_workflow_node_id_with_spaces() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: "build step"
    type: shell
    run: echo hello
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("invalid characters"));
}

#[test]
fn test_parse_workflow_node_id_valid_chars() {
    // Hyphen, underscore, dot, and forward slash are allowed
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: build-step_1
    type: shell
    run: echo hello
  - id: build.step
    type: shell
    run: echo hello
  - id: deploy/prod
    type: shell
    depends: [build-step_1, build.step]
    run: echo deploy
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.nodes.len(), 3);
}

#[test]
fn test_parse_workflow_depends_string_instead_of_array() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: build
    type: shell
    run: echo build
  - id: deploy
    type: shell
    depends: build
    run: echo deploy
"#;
    let result = parse_workflow(yaml);
    assert!(result.is_err(), "depends as bare string should fail");
}

#[test]
fn test_parse_workflow_depends_invalid_chars() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: build
    type: shell
    run: echo hello
  - id: deploy
    type: shell
    depends: ["build@node"]
    run: echo deploy
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("invalid characters"));
}

#[test]
fn test_validate_workflow_long_name_accepted() {
    // Long names are currently allowed — verify this behavior is stable
    let yaml = format!(
        r#"
name: "{}"
version: "1.0"
nodes:
  - id: greet
    type: shell
    run: echo hello
"#,
        "x".repeat(256)
    );
    assert!(parse_workflow(&yaml).is_ok());
}

#[test]
fn test_parse_workflow_node_if_condition() {
    let yaml = r#"
name: test
version: "1.0"
inputs:
  deploy:
    type: string
    default: "false"
nodes:
  - id: build
    type: shell
    run: echo build
  - id: deploy
    type: shell
    if: "{{ inputs.deploy }} == true"
    depends: [build]
    run: echo deploy
"#;
    let wf = parse_workflow(yaml).unwrap();
    assert_eq!(wf.nodes.len(), 2);
    match &wf.nodes[1] {
        NodeDef::Shell(n) => {
            assert_eq!(n.exec.r#if.as_deref(), Some("{{ inputs.deploy }} == true"))
        }
        _ => panic!("expected shell node"),
    }
}

#[test]
fn test_parse_workflow_node_no_if() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: build
    type: shell
    run: echo build
"#;
    let wf = parse_workflow(yaml).unwrap();
    match &wf.nodes[0] {
        NodeDef::Shell(n) => assert!(n.exec.r#if.is_none()),
        _ => panic!("expected shell node"),
    }
}

#[test]
fn test_parse_workflow_agent_with_optional_fields() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: review
    type: agent
    prompt: review the code
    agent: claude
    model: sonnet
    cwd: /tmp/project
"#;
    let wf = parse_workflow(yaml).unwrap();
    match &wf.nodes[0] {
        NodeDef::Agent(n) => {
            assert_eq!(n.agent.as_deref(), Some("claude"));
            assert_eq!(n.model.as_deref(), Some("sonnet"));
            assert_eq!(n.cwd.as_deref(), Some("/tmp/project"));
        }
        _ => panic!("expected agent node"),
    }
}

#[test]
fn test_parse_workflow_agent_if_condition() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: review
    type: agent
    if: "{{ env.REVIEW_ENABLED }}"
    prompt: review the code
"#;
    let wf = parse_workflow(yaml).unwrap();
    match &wf.nodes[0] {
        NodeDef::Agent(n) => {
            assert_eq!(n.exec.r#if.as_deref(), Some("{{ env.REVIEW_ENABLED }}"))
        }
        _ => panic!("expected agent node"),
    }
}

#[test]
fn test_parse_workflow_duplicate_node_id() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: build
    type: shell
    run: echo first
  - id: build
    type: shell
    run: echo second
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("duplicate node id 'build'"));
}

#[test]
fn test_parse_workflow_depends_nonexistent_node() {
    let yaml = r#"
name: test
version: "1.0"
nodes:
  - id: build
    type: shell
    run: echo build
  - id: deploy
    type: shell
    depends: [build, test]
    run: echo deploy
"#;
    let err = parse_workflow(yaml).unwrap_err();
    assert!(err.to_string().contains("does not exist"));
    assert!(err.to_string().contains("'test'"));
}
