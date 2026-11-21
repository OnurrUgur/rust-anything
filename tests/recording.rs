//! `record` fingerprints the original, so a recording made before the original changed is
//! reported as stale instead of passing against old behavior.

mod common;

use std::process::Command;

use rust_anything::bridge::Manifest;
use rust_anything::recording;

#[test]
fn a_changed_original_makes_the_recording_stale() {
    if !common::have("python3") {
        return;
    }
    let root = std::env::temp_dir().join(format!("rust-anything-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"twice\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("twice.py"),
        "def twice(n: int) -> int:\n    return n * 2\n",
    )
    .unwrap();
    let cli = env!("CARGO_BIN_EXE_rust-anything");
    let crate_dir = root.join("port");
    let run = |args: &[&std::ffi::OsStr]| {
        let out = Command::new(cli).args(args).output().unwrap();
        assert!(out.status.success(), "{out:?}");
    };
    run(&[
        "bridge".as_ref(),
        root.as_os_str(),
        "twice.py".as_ref(),
        "--out".as_ref(),
        crate_dir.as_os_str(),
    ]);
    run(&["record".as_ref(), crate_dir.as_os_str()]);
    let manifest = Manifest::load(&crate_dir).unwrap();
    assert!(crate_dir.join(recording::RECORDED_FROM).is_file());
    let fresh = recording::stale(&crate_dir, &manifest);
    std::fs::write(
        root.join("twice.py"),
        "def twice(n: int) -> int:\n    return n + n\n",
    )
    .unwrap();
    let changed = recording::stale(&crate_dir, &manifest);
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(fresh, None);
    assert!(changed.is_some_and(|why| why.contains("twice.py changed")));
}

fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "rust-anything-from-tests-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

/// Bridges `unit`, records with `--from-tests -- <test>`, and returns what `record` printed and
/// the captured inputs.
fn from_tests(root: &std::path::Path, unit: &str, test: &[&str]) -> (String, String) {
    let cli = env!("CARGO_BIN_EXE_rust-anything");
    let crate_dir = root.join("port");
    let bridged = Command::new(cli)
        .arg("bridge")
        .arg(root)
        .arg(unit)
        .arg("--out")
        .arg(&crate_dir)
        .output()
        .unwrap();
    assert!(bridged.status.success(), "{bridged:?}");
    let recorded = Command::new(cli)
        .arg("record")
        .arg(&crate_dir)
        .args(["--count", "5", "--from-tests", "--"])
        .args(test)
        .output()
        .unwrap();
    assert!(recorded.status.success(), "{recorded:?}");
    (
        String::from_utf8_lossy(&recorded.stdout).into_owned(),
        std::fs::read_to_string(crate_dir.join("test-inputs.jsonl")).unwrap(),
    )
}

#[test]
fn records_the_calls_a_python_test_suite_makes() {
    if !common::have("python3") {
        return;
    }
    let root = project(
        "py",
        &[
            (
                "pyproject.toml",
                "[project]\nname = \"twice\"\nversion = \"0.1.0\"\n",
            ),
            (
                "twice.py",
                "def twice(n: float) -> float:\n    return n * 2\n",
            ),
            (
                "test_twice.py",
                "import math\nimport unittest\nfrom twice import twice\n\n\
                 class T(unittest.TestCase):\n    def test_it(self):\n        \
                 self.assertEqual(twice(21), 42)\n        self.assertEqual(twice(n=1.5), 3.0)\n        \
                 self.assertTrue(math.isnan(twice(float('nan'))))\n",
            ),
        ],
    );
    let (printed, inputs) = from_tests(&root, "twice.py", &["python3", "-m", "unittest"]);
    std::fs::remove_dir_all(&root).unwrap();
    assert!(printed.contains("Captured 3 call(s)"), "{printed}");
    assert!(
        inputs.contains(r#"{"fn": "twice", "args": [21]}"#),
        "{inputs}"
    );
    assert!(inputs.contains(r#""args": [1.5]"#), "{inputs}");
    assert!(inputs.contains(r#"{"$num": "NaN"}"#), "{inputs}");
}

#[test]
fn records_python_test_calls_that_pass_arguments_by_keyword() {
    if !common::have("python3") {
        return;
    }
    let root = project(
        "py-kw",
        &[
            (
                "pyproject.toml",
                "[project]\nname = \"shape\"\nversion = \"0.1.0\"\n",
            ),
            (
                "shape.py",
                "def scale(n: float, *, by: float = 2.0) -> float:\n    return n * by\n\n\n\
                 def pad(s: str, width: int = 4, fill: str = ' ') -> str:\n    return s.ljust(width, fill)\n",
            ),
            (
                "test_shape.py",
                "import unittest\nfrom shape import pad, scale\n\n\
                 class T(unittest.TestCase):\n    def test_it(self):\n        \
                 self.assertEqual(scale(3), 6)\n        self.assertEqual(scale(3, by=10), 30)\n        \
                 self.assertEqual(pad('a', fill='*'), 'a***')\n",
            ),
        ],
    );
    let (printed, inputs) = from_tests(&root, "shape.py", &["python3", "-m", "unittest"]);
    let cases = std::fs::read_to_string(root.join("port").join("cases.jsonl")).unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    // The original ran with the keyword arguments when the cases were recorded.
    assert!(
        cases.contains(r#""args": [3], "kwargs": {"by": 10}, "ok": 30"#),
        "{cases}"
    );
    assert!(
        cases.contains(r#""args": ["a"], "kwargs": {"fill": "*"}, "ok": "a***""#),
        "{cases}"
    );
    // A keyword-only argument, and one passed by name after a skipped default.
    assert!(printed.contains("Captured 3 call(s)"), "{printed}");
    assert!(
        inputs.contains(r#"{"fn": "scale", "args": [3], "kwargs": {"by": 10}}"#),
        "{inputs}"
    );
    assert!(
        inputs.contains(r#"{"fn": "pad", "args": ["a"], "kwargs": {"fill": "*"}}"#),
        "{inputs}"
    );
}

#[test]
fn diff_replays_python_keyword_arguments_by_their_binding_names() {
    if !common::have("python3") {
        return;
    }
    let root = project(
        "py-kw-diff",
        &[
            (
                "pyproject.toml",
                "[project]\nname = \"shape\"\nversion = \"0.1.0\"\n",
            ),
            (
                "shape.py",
                "def scale(n: float, *, scaleBy: float = 2.0) -> float:\n    return n * scaleBy\n\n\n\
                 def pad(s: str, width: int = 4, fill: str = ' ') -> str:\n    return s.ljust(width, fill)\n",
            ),
            (
                "test_shape.py",
                "import unittest\nfrom shape import pad, scale\n\n\
                 class T(unittest.TestCase):\n    def test_it(self):\n        \
                 self.assertEqual(scale(3, scaleBy=10), 30)\n        \
                 self.assertEqual(pad('a'), 'a   ')\n        self.assertEqual(pad('a', fill='*'), 'a***')\n",
            ),
        ],
    );
    let (printed, _) = from_tests(&root, "shape.py", &["python3", "-m", "unittest"]);
    assert!(printed.contains("Captured 3 call(s)"), "{printed}");
    let crate_dir = root.join("port");
    let lib = std::fs::read_to_string(crate_dir.join("src/lib.rs")).unwrap();
    // The bindings name the parameter in snake_case.
    assert!(lib.contains("signature = (n, *, scale_by="), "{lib}");
    // A pure-Python stand-in for the built package, with the bindings' parameter names: `scale`
    // is right, and `pad` ignores a `fill` of `*`.
    let manifest = Manifest::load(&crate_dir).unwrap();
    let package = crate_dir.join("python").join(manifest.lib_name());
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("__init__.py"),
        "def scale(n, *, scale_by=2.0):\n    return n * scale_by\n\n\n\
         def pad(s, width=4, fill=' '):\n    return s.ljust(width, ' ' if fill == '*' else fill)\n",
    )
    .unwrap();
    let outcome = rust_anything::harness::diff(&rust_anything::harness::DiffOptions {
        crate_dir: &crate_dir,
        module: None,
        ignore_key_order: false,
        float_tolerance: None,
        node_args: &[],
    })
    .unwrap();
    // A known difference for `pad('a')` does not explain `pad('a', fill='*')`.
    let known = [serde_json::json!({
        "fn": "pad", "args": ["a"], "actual": {"ok": "a   "}, "reason": "x"
    })];
    let (failed, mismatches) = (outcome.failed, outcome.mismatches.clone());
    let accepted = recording::apply_known(outcome, &known);
    std::fs::remove_dir_all(&root).unwrap();
    // `scaleBy=10` reached the bindings as `scale_by=10`.
    assert_eq!(failed, 1, "{mismatches:?}");
    // The mismatch shows the keyword argument, as Ruby's do.
    assert_eq!(mismatches[0]["fn"], "pad");
    assert_eq!(
        mismatches[0]["args"],
        serde_json::json!(["a", {"fill": "*"}])
    );
    assert_eq!((accepted.failed, accepted.accepted), (1, 0));
}

#[test]
fn records_the_calls_a_js_test_suite_makes_and_restores_the_source() {
    if !common::have("node") {
        return;
    }
    let source = "export function label(n: number): string {\n  return `${n}u`;\n}\n\nexport const unwrapped = (s: string): string => s;\n";
    let root = project(
        "ts",
        &[
            (
                "package.json",
                r#"{"name":"label","version":"0.1.0","type":"module"}"#,
            ),
            ("src/label.ts", source),
            (
                "test.ts",
                "import { label, unwrapped } from './src/label.ts'\nif (label(-0) !== '0u' || label(7) !== '7u' || unwrapped('a') !== 'a') process.exit(1)\n",
            ),
        ],
    );
    let (printed, inputs) = from_tests(&root, "src/label.ts", &["node", "test.ts"]);
    let after = std::fs::read_to_string(root.join("src/label.ts")).unwrap();
    let backups = std::fs::read_dir(root.join("src")).unwrap().count();
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(after, source);
    assert_eq!(backups, 1);
    assert!(
        printed.contains("Captured 2 call(s) from the tests:"),
        "{printed}"
    );
    assert!(printed.contains("not captured: unwrapped"), "{printed}");
    assert!(
        inputs.contains(r#"{"fn":"label","args":[{"$num":"-0"}]}"#),
        "{inputs}"
    );
    assert!(inputs.contains(r#"{"fn":"label","args":[7]}"#), "{inputs}");
}

#[test]
fn record_reports_lines_of_the_original_no_case_reached() {
    if !common::have("node") {
        return;
    }
    let root = project(
        "coverage",
        &[
            (
                "package.json",
                "{\"name\": \"sign\", \"type\": \"module\"}\n",
            ),
            (
                "sign.ts",
                "export function sign(n: number, word: string): string {\n  if (word === 'xyzzy-never-generated') {\n    return 'magic'\n  }\n  return n < 0 ? 'negative' : 'other'\n}\n",
            ),
        ],
    );
    let cli = env!("CARGO_BIN_EXE_rust-anything");
    let crate_dir = root.join("port");
    let run = |args: &[&std::ffi::OsStr]| {
        let out = Command::new(cli).args(args).output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    run(&[
        "bridge".as_ref(),
        root.as_os_str(),
        "sign.ts".as_ref(),
        "--out".as_ref(),
        crate_dir.as_os_str(),
    ]);
    let printed = run(&["record".as_ref(), crate_dir.as_os_str()]);
    let coverage = recording::saved_coverage(&crate_dir);
    std::fs::remove_dir_all(&root).unwrap();
    let coverage = coverage.expect("record saves coverage");
    assert_eq!(coverage["lines"], 6, "{coverage}");
    assert_eq!(coverage["reached"], 4, "{coverage}");
    assert_eq!(coverage["files"][0]["missed"], "3-4", "{coverage}");
    assert!(
        printed.contains("ran 4 of the original's 6 lines"),
        "{printed}"
    );
    assert!(printed.contains("sign.ts: 3-4"), "{printed}");
}
