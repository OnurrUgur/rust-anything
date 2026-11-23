//! `doctor --json`: one entry per check, and the exit code follows the required checks.

use std::process::Command;

use serde_json::Value;

#[test]
fn doctor_json_lists_every_check_with_a_fix_for_each_problem() {
    let home = std::env::temp_dir().join(format!("rust-anything-doctor-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rust-anything"))
        .args(["doctor", "--json"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .unwrap();
    std::fs::remove_dir_all(&home).unwrap();
    let report: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!("{e}: {}", String::from_utf8_lossy(&out.stdout));
    });
    let checks = report["checks"].as_array().expect("a checks array");
    let mut all_required_ok = true;
    for c in checks {
        let status = c["status"].as_str().expect("a status");
        assert!(
            ["ok", "missing", "outdated", "foreign"].contains(&status),
            "{c}"
        );
        assert!(c["name"].is_string() && c["detail"].is_string(), "{c}");
        assert_eq!(c["fix"].is_string(), status != "ok", "{c}");
        if c["required"].as_bool().expect("required is a bool") && status != "ok" {
            all_required_ok = false;
        }
    }
    let names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    for name in [
        "cargo",
        "node",
        "python3",
        "go",
        "ruby",
        "java",
        "php",
        "mcp server",
    ] {
        assert!(names.contains(&name), "no {name} check in {names:?}");
    }
    let mcp = checks.iter().find(|c| c["name"] == "mcp server").unwrap();
    assert_eq!(mcp["status"], "ok", "{mcp}");
    assert_eq!(out.status.success(), all_required_ok, "{report:#}");
    assert!(matches!(out.status.code(), Some(0 | 1)));
}

#[cfg(unix)]
#[test]
fn doctor_reports_whether_php_has_the_ffi_extension() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("rust-anything-doctor-php-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let bin = dir.join("bin");
    let home = dir.join("home");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    let cargo = bin.join("cargo");
    std::fs::write(&cargo, "#!/bin/sh\necho 'cargo 1.97.0'\n").unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    let php = bin.join("php");
    let write_php = |modules: &str| {
        let _ = std::fs::remove_file(&php);
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo 'PHP 8.3.4 (cli) (built: Mar 12 2024)'\nelif [ \"$1\" = \"-m\" ]; then\n  printf '%s\\n' {modules}\nfi\n"
        );
        std::fs::write(&php, script).unwrap();
        std::fs::set_permissions(&php, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let run_doctor = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_rust-anything"))
            .args(args)
            .env("PATH", &bin)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .output()
            .unwrap()
    };

    write_php("'[PHP Modules]' Core json");
    let without_ffi = run_doctor(&["doctor", "--json"]);
    assert!(
        without_ffi.status.success(),
        "missing PHP FFI is optional: {}",
        String::from_utf8_lossy(&without_ffi.stdout)
    );
    let report: Value = serde_json::from_slice(&without_ffi.stdout).unwrap();
    let check = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "php")
        .unwrap();
    assert_eq!(check["status"], "missing", "{check}");
    assert_eq!(check["required"], false, "{check}");
    assert!(
        check["detail"]
            .as_str()
            .unwrap()
            .contains("FFI extension not loaded"),
        "{check}"
    );
    assert!(
        check["fix"].as_str().unwrap().contains("extension=ffi"),
        "{check}"
    );
    let human = String::from_utf8(run_doctor(&["doctor"]).stdout).unwrap();
    assert!(
        human.contains("FFI extension not loaded (optional)"),
        "{human}"
    );

    write_php("'[PHP Modules]' Core FFI json");
    let with_ffi = run_doctor(&["doctor", "--json"]);
    assert!(with_ffi.status.success());
    let report: Value = serde_json::from_slice(&with_ffi.stdout).unwrap();
    let check = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "php")
        .unwrap();
    assert_eq!(check["status"], "ok", "{check}");
    assert!(
        check["detail"].as_str().unwrap().ends_with("(FFI loaded)"),
        "{check}"
    );
    assert!(check["fix"].is_null(), "{check}");

    std::fs::remove_dir_all(&dir).unwrap();
}
